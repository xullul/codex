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
        query: "needle".to_string(),
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

#[test]
fn wildcard_glob_matches_common_repo_patterns() {
    assert!(glob_matches(Some("*.rs"), Path::new("src/main.rs")));
    assert!(glob_matches(
        Some("*src/*.ts"),
        Path::new("web/src/index.ts")
    ));
    assert!(!glob_matches(Some("*.rs"), Path::new("src/main.ts")));
}
