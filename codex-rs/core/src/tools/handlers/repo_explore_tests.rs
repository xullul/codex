use super::*;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

#[tokio::test]
async fn repo_read_formats_line_numbered_ranges() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let file = temp.path().join("sample.rs");
    tokio::fs::write(&file, "alpha\nbeta\ngamma\ndelta\n").await?;

    let lines = read_text_lines(&file).await?;
    let output = format_read_output(&file, &lines, /*offset*/ 2, /*limit*/ 2);

    assert_eq!(
        output,
        format!(
            "Absolute path: {}\n     2: beta\n     3: gamma\nMore lines available. Next offset: 4",
            file.display()
        )
    );
    Ok(())
}

#[tokio::test]
async fn repo_read_rejects_binary_files() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let file = temp.path().join("binary.bin");
    tokio::fs::write(&file, b"alpha\0beta").await?;

    let err = read_text_lines(&file)
        .await
        .expect_err("binary file should be rejected");

    assert_eq!(
        err,
        FunctionCallError::RespondToModel(
            "binary files cannot be read by repo_read/repo_search".to_string()
        )
    );
    Ok(())
}

#[tokio::test]
async fn fallback_search_hides_denied_files() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let dir = temp.path();
    let public = dir.join("public.txt");
    let private = dir.join("private.txt");
    tokio::fs::write(&public, "needle public\n").await?;
    tokio::fs::write(&private, "needle secret\n").await?;

    let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::Path {
            path: private.clone().try_into().expect("absolute denied path"),
        },
        access: FileSystemAccessMode::None,
    }]);
    let read_deny_matcher = ReadDenyMatcher::new(&policy, dir);
    let args = RepoSearchArgs {
        query: Some("needle".to_string()),
        search_mode: RepoSearchMode::Content,
        path: None,
        glob: None,
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };

    let output = fallback_search(dir, &args, read_deny_matcher.as_ref()).await?;

    assert!(output.contains("public.txt"));
    assert!(output.contains("needle public"));
    assert!(!output.contains("private.txt"));
    assert!(!output.contains("secret"));
    Ok(())
}

#[tokio::test]
async fn fallback_search_uses_regex_matching() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let dir = temp.path();
    tokio::fs::write(dir.join("sample.txt"), "alpha\nbravo\nbeta\n").await?;
    let args = RepoSearchArgs {
        query: Some("b.t.".to_string()),
        search_mode: RepoSearchMode::Content,
        path: None,
        glob: None,
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };

    let output = fallback_search(dir, &args, None).await?;

    assert!(output.contains("beta"));
    assert!(!output.contains("alpha"));
    Ok(())
}

#[tokio::test]
async fn fallback_path_search_lists_globbed_files_without_query() -> anyhow::Result<()> {
    let temp = tempdir()?;
    let dir = temp.path();
    tokio::fs::create_dir_all(dir.join("src")).await?;
    tokio::fs::write(dir.join("src/main.rs"), "fn main() {}\n").await?;
    tokio::fs::write(dir.join("README.md"), "# sample\n").await?;
    let args = RepoSearchArgs {
        query: None,
        search_mode: RepoSearchMode::Paths,
        path: None,
        glob: Some("*.rs".to_string()),
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };

    let output = fallback_search(dir, &args, None).await?;

    assert!(output.contains("main.rs"));
    assert!(!output.contains("README.md"));
    Ok(())
}

#[tokio::test]
async fn rg_search_handles_dash_prefixed_patterns() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir()?;
    let dir = temp.path();
    tokio::fs::write(dir.join("flags.txt"), "--hidden\n").await?;
    let args = RepoSearchArgs {
        query: Some("--hidden".to_string()),
        search_mode: RepoSearchMode::Content,
        path: None,
        glob: None,
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };

    let output = rg_search(dir, &args).await?;

    assert!(output.contains("flags.txt"));
    assert!(output.contains("--hidden"));
    Ok(())
}

#[tokio::test]
async fn rg_path_search_includes_hidden_files_and_excludes_vcs_dirs() -> anyhow::Result<()> {
    if !rg_available() {
        return Ok(());
    }
    let temp = tempdir()?;
    let dir = temp.path();
    tokio::fs::create_dir_all(dir.join(".github/workflows")).await?;
    tokio::fs::create_dir_all(dir.join(".git")).await?;
    tokio::fs::write(dir.join(".github/workflows/ci.yml"), "name: ci\n").await?;
    tokio::fs::write(dir.join(".git/ignored.yml"), "ignored: true\n").await?;
    let args = RepoSearchArgs {
        query: None,
        search_mode: RepoSearchMode::Paths,
        path: None,
        glob: Some("*.yml".to_string()),
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };

    let output = rg_path_search(dir, &args).await?;

    assert!(output.contains(".github"));
    assert!(output.contains("ci.yml"));
    assert!(!output.contains("ignored.yml"));
    Ok(())
}

#[test]
fn validates_mode_specific_required_fields() {
    let content_without_query = RepoSearchArgs {
        query: None,
        search_mode: RepoSearchMode::Content,
        path: None,
        glob: None,
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };
    assert!(validate_search_args(&content_without_query).is_err());

    let paths_without_glob = RepoSearchArgs {
        query: None,
        search_mode: RepoSearchMode::Paths,
        path: None,
        glob: None,
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };
    assert!(validate_search_args(&paths_without_glob).is_err());

    let paths_without_query = RepoSearchArgs {
        query: None,
        search_mode: RepoSearchMode::Paths,
        path: None,
        glob: Some("*.rs".to_string()),
        context_lines: 0,
        limit: 20,
        offset: 0,
        files_only: false,
    };
    assert!(validate_search_args(&paths_without_query).is_ok());
}

#[test]
fn wildcard_glob_matches_common_repo_patterns() {
    assert!(glob_matches(Some("*.rs"), Path::new("src/main.rs")));
    assert!(glob_matches(
        Some("*src/*.ts"),
        Path::new("web/src/index.ts")
    ));
    assert!(!glob_matches(Some("*.rs"), Path::new("src/main.ts")));
}

fn rg_available() -> bool {
    std::process::Command::new("rg")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
