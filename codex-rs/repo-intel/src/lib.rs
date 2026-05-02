use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;

const MAX_GIT_FILES: usize = 8_000;
const MAX_WALK_FILES: usize = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoIntelRequest {
    pub cwd: PathBuf,
    pub user_prompt: String,
    pub budget: RepoIntelBudget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoIntelBudget {
    pub max_files: usize,
    pub max_excerpt_bytes: usize,
    pub max_manifest_count: usize,
    pub max_doc_count: usize,
}

impl Default for RepoIntelBudget {
    fn default() -> Self {
        Self {
            max_files: MAX_GIT_FILES,
            max_excerpt_bytes: 1_200,
            max_manifest_count: 20,
            max_doc_count: 12,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntelSnapshot {
    pub cwd: PathBuf,
    pub root: PathBuf,
    pub git: RepoIntelGit,
    pub project_kinds: Vec<String>,
    pub languages: Vec<RepoIntelLanguage>,
    pub manifests: Vec<RepoIntelFile>,
    pub docs: Vec<RepoIntelFile>,
    pub commands: Vec<String>,
    pub warnings: Vec<String>,
    pub files_seen: usize,
}

impl RepoIntelSnapshot {
    pub fn render_for_model(&self) -> String {
        let mut out = String::new();
        out.push_str("<repo_intel>\n");
        out.push_str("Fresh repository intelligence for this turn. Treat this as orientation only; verify target files before editing.\n");
        out.push_str(&format!("Root: {}\n", self.root.display()));
        out.push_str(&format!("CWD: {}\n", self.cwd.display()));
        out.push_str(&format!(
            "Git: branch={} head={} dirty={}\n",
            self.git.branch.as_deref().unwrap_or("unknown"),
            self.git.head.as_deref().unwrap_or("unknown"),
            self.git.dirty
        ));
        if !self.project_kinds.is_empty() {
            out.push_str(&format!(
                "Project signals: {}\n",
                self.project_kinds.join(", ")
            ));
        }
        if !self.languages.is_empty() {
            out.push_str("Languages by file count:\n");
            for language in &self.languages {
                out.push_str(&format!("- {}: {}\n", language.name, language.files));
            }
        }
        if !self.manifests.is_empty() {
            out.push_str("High-signal manifests:\n");
            for manifest in &self.manifests {
                out.push_str(&format!("- {}\n", manifest.path));
                if let Some(excerpt) = manifest
                    .excerpt
                    .as_deref()
                    .filter(|excerpt| !excerpt.is_empty())
                {
                    out.push_str(&indent_excerpt(excerpt));
                }
            }
        }
        if !self.docs.is_empty() {
            out.push_str("High-signal docs:\n");
            for doc in &self.docs {
                out.push_str(&format!("- {}\n", doc.path));
                if let Some(excerpt) = doc.excerpt.as_deref().filter(|excerpt| !excerpt.is_empty())
                {
                    out.push_str(&indent_excerpt(excerpt));
                }
            }
        }
        if !self.commands.is_empty() {
            out.push_str("Likely local commands:\n");
            for command in &self.commands {
                out.push_str(&format!("- {command}\n"));
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("Scan notes:\n");
            for warning in &self.warnings {
                out.push_str(&format!("- {warning}\n"));
            }
        }
        out.push_str("</repo_intel>");
        out
    }

    pub fn progress_summary(&self) -> String {
        let project = if self.project_kinds.is_empty() {
            "project".to_string()
        } else {
            self.project_kinds.join(", ")
        };
        format!(
            "repo intel ready: {project}; {} files, {} manifests, {} docs",
            self.files_seen,
            self.manifests.len(),
            self.docs.len()
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntelGit {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub source: RepoIntelGitSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoIntelGitSource {
    Git,
    #[default]
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntelLanguage {
    pub name: String,
    pub files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIntelFile {
    pub path: String,
    pub excerpt: Option<String>,
}

pub fn should_collect_repo_intel(user_prompt: &str) -> bool {
    let prompt = user_prompt.to_ascii_lowercase();
    if prompt.trim().is_empty() {
        return false;
    }

    const POSITIVE: &[&str] = &[
        "codebase",
        "repo",
        "repository",
        "project",
        "architecture",
        "implement",
        "fix",
        "debug",
        "review",
        "refactor",
        "tests",
        "build",
        "where is",
        "how does",
        "understand",
        "large",
    ];
    const NEGATIVE_PREFIXES: &[&str] = &[
        "hi",
        "hello",
        "thanks",
        "thank you",
        "what time",
        "translate",
        "rewrite this sentence",
    ];

    if NEGATIVE_PREFIXES
        .iter()
        .any(|prefix| prompt.trim_start().starts_with(prefix))
    {
        return false;
    }

    POSITIVE.iter().any(|needle| prompt.contains(needle))
}

pub fn collect_repo_intel(request: &RepoIntelRequest) -> anyhow::Result<RepoIntelSnapshot> {
    let cwd = dunce::canonicalize(&request.cwd)
        .with_context(|| format!("canonicalizing cwd {}", request.cwd.display()))?;
    let mut warnings = Vec::new();
    let root = git_root(&cwd)
        .or_else(|| find_marker_root(&cwd))
        .unwrap_or_else(|| cwd.clone());
    let files = tracked_files(&root, request.budget.max_files, &mut warnings);
    let files_seen = files.len();
    let manifests = collect_high_signal_files(&root, &files, true, &request.budget);
    let docs = collect_high_signal_files(&root, &files, false, &request.budget);
    let project_kinds = project_kinds(&files, &manifests);
    let languages = language_counts(&files);
    let commands = infer_commands(&root, &manifests);
    let git = collect_git(&root);
    if files_seen >= request.budget.max_files {
        warnings.push(format!(
            "file scan hit the {} file budget",
            request.budget.max_files
        ));
    }

    Ok(RepoIntelSnapshot {
        cwd,
        root,
        git,
        project_kinds,
        languages,
        manifests,
        docs,
        commands,
        warnings,
        files_seen,
    })
}

fn indent_excerpt(excerpt: &str) -> String {
    excerpt
        .lines()
        .take(8)
        .map(|line| format!("  {line}\n"))
        .collect()
}

fn git_root(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

fn find_marker_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = Some(cwd);
    while let Some(dir) = current {
        if ["Cargo.toml", "package.json", "go.mod", "pyproject.toml"]
            .iter()
            .any(|name| dir.join(name).is_file())
            || dir
                .read_dir()
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension() == Some(OsStr::new("sln")))
        {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn tracked_files(root: &Path, max_files: usize, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    if let Some(files) = git_files(root, max_files) {
        return files;
    }
    warnings.push("git ls-files was unavailable; used ignore-aware filesystem walk".to_string());
    walk_files(root, max_files.min(MAX_WALK_FILES))
}

fn git_files(root: &Path, max_files: usize) -> Option<Vec<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("ls-files")
        .arg("--recurse-submodules")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(max_files)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    Some(files)
}

fn walk_files(root: &Path, max_files: usize) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .filter_map(|entry| entry.path().strip_prefix(root).ok().map(Path::to_path_buf))
        .take(max_files)
        .collect()
}

fn collect_git(root: &Path) -> RepoIntelGit {
    if !git_command_succeeds(root, &["rev-parse", "--is-inside-work-tree"]) {
        return RepoIntelGit {
            source: RepoIntelGitSource::None,
            ..RepoIntelGit::default()
        };
    }

    let branch = git_stdout(root, &["branch", "--show-current"]);
    let head = git_stdout(root, &["rev-parse", "--short", "HEAD"]);
    let dirty = git_stdout(root, &["status", "--porcelain", "-uno"])
        .is_some_and(|status| !status.trim().is_empty());
    RepoIntelGit {
        branch,
        head,
        dirty,
        source: RepoIntelGitSource::Git,
    }
}

fn git_command_succeeds(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn collect_high_signal_files(
    root: &Path,
    files: &[PathBuf],
    manifests: bool,
    budget: &RepoIntelBudget,
) -> Vec<RepoIntelFile> {
    let limit = if manifests {
        budget.max_manifest_count
    } else {
        budget.max_doc_count
    };
    files
        .iter()
        .filter(|path| {
            if manifests {
                is_manifest(path)
            } else {
                is_high_signal_doc(path)
            }
        })
        .take(limit)
        .map(|path| RepoIntelFile {
            path: normalize_path(path),
            excerpt: read_excerpt(&root.join(path), budget.max_excerpt_bytes),
        })
        .collect()
}

fn is_manifest(path: &Path) -> bool {
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "pnpm-workspace.yaml"
            | "go.mod"
            | "pyproject.toml"
            | "pom.xml"
            | "build.gradle"
            | "settings.gradle"
            | "composer.json"
    ) || path.extension() == Some(OsStr::new("sln"))
        || path.extension() == Some(OsStr::new("csproj"))
}

fn is_high_signal_doc(path: &Path) -> bool {
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "agents.md" | "agents.override.md" | "readme.md" | "contributing.md" | "architecture.md"
    ) || path.starts_with("docs") && lower.ends_with(".md")
}

fn read_excerpt(path: &Path, max_bytes: usize) -> Option<String> {
    let mut text = std::fs::read_to_string(path).ok()?;
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("\n...");
    }
    Some(text.trim().to_string()).filter(|text| !text.is_empty())
}

fn project_kinds(files: &[PathBuf], manifests: &[RepoIntelFile]) -> Vec<String> {
    let has = |name: &str| manifests.iter().any(|file| file.path.ends_with(name));
    let mut kinds = Vec::new();
    if has("Cargo.toml") {
        kinds.push("Rust/Cargo".to_string());
    }
    if has("package.json") || has("pnpm-workspace.yaml") {
        kinds.push("Node/JavaScript".to_string());
    }
    if files
        .iter()
        .any(|path| path.extension() == Some(OsStr::new("sln")))
        || files
            .iter()
            .any(|path| path.extension() == Some(OsStr::new("csproj")))
    {
        kinds.push(".NET".to_string());
    }
    if has("go.mod") {
        kinds.push("Go".to_string());
    }
    if has("pyproject.toml") {
        kinds.push("Python".to_string());
    }
    kinds
}

fn language_counts(files: &[PathBuf]) -> Vec<RepoIntelLanguage> {
    let mut counts = BTreeMap::<String, usize>::new();
    for file in files {
        let Some(ext) = file.extension().and_then(OsStr::to_str) else {
            continue;
        };
        let Some(language) = language_for_extension(ext) else {
            continue;
        };
        *counts.entry(language.to_string()).or_default() += 1;
    }
    let mut languages = counts
        .into_iter()
        .map(|(name, files)| RepoIntelLanguage { name, files })
        .collect::<Vec<_>>();
    languages.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    languages.truncate(8);
    languages
}

fn language_for_extension(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "rs" => Some("Rust"),
        "ts" | "tsx" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "cs" => Some("C#"),
        "py" => Some("Python"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "kt" | "kts" => Some("Kotlin"),
        "swift" => Some("Swift"),
        "cpp" | "cc" | "cxx" | "hpp" | "h" | "c" => Some("C/C++"),
        "md" => Some("Markdown"),
        "toml" => Some("TOML"),
        "json" => Some("JSON"),
        "yaml" | "yml" => Some("YAML"),
        _ => None,
    }
}

fn infer_commands(root: &Path, manifests: &[RepoIntelFile]) -> Vec<String> {
    let mut commands = Vec::new();
    if manifests
        .iter()
        .any(|file| file.path.ends_with("Cargo.toml"))
    {
        commands.extend(["cargo test".to_string(), "cargo build".to_string()]);
    }
    if manifests
        .iter()
        .any(|file| file.path.ends_with(".sln") || file.path.ends_with(".csproj"))
    {
        commands.extend(["dotnet test".to_string(), "dotnet build".to_string()]);
    }
    if manifests.iter().any(|file| file.path.ends_with("go.mod")) {
        commands.push("go test ./...".to_string());
    }
    for manifest in manifests
        .iter()
        .filter(|file| file.path.ends_with("package.json"))
    {
        if let Some(scripts) = package_scripts(&root.join(&manifest.path)) {
            for script in ["test", "build", "typecheck", "lint"] {
                if scripts.contains_key(script) {
                    commands.push(format!("npm run {script}"));
                }
            }
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn package_scripts(path: &Path) -> Option<BTreeMap<String, serde_json::Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    serde_json::from_value(json.get("scripts")?.clone()).ok()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn prompt_trigger_ignores_simple_chat() {
        assert!(!should_collect_repo_intel("hello"));
        assert!(should_collect_repo_intel("fix the build in this repo"));
    }

    #[test]
    fn collects_rust_workspace_signals() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\"]\n",
        )
        .unwrap();
        std::fs::create_dir(tmp.path().join("app")).unwrap();
        std::fs::write(tmp.path().join("app").join("lib.rs"), "pub fn demo() {}\n").unwrap();
        std::fs::write(tmp.path().join("README.md"), "# Demo\n\nA repo.\n").unwrap();

        let snapshot = collect_repo_intel(&RepoIntelRequest {
            cwd: tmp.path().to_path_buf(),
            user_prompt: "understand this project".to_string(),
            budget: RepoIntelBudget::default(),
        })
        .unwrap();

        assert_eq!(snapshot.project_kinds, vec!["Rust/Cargo"]);
        assert_eq!(snapshot.commands, vec!["cargo build", "cargo test"]);
        assert!(
            snapshot
                .manifests
                .iter()
                .any(|file| file.path == "Cargo.toml")
        );
        assert!(snapshot.docs.iter().any(|file| file.path == "README.md"));
        assert!(snapshot.render_for_model().contains("<repo_intel>"));
    }

    #[test]
    fn infers_package_scripts() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts":{"build":"vite build","test":"vitest","dev":"vite"}}"#,
        )
        .unwrap();
        std::fs::write(tmp.path().join("index.ts"), "export {}\n").unwrap();

        let snapshot = collect_repo_intel(&RepoIntelRequest {
            cwd: tmp.path().to_path_buf(),
            user_prompt: "review this project".to_string(),
            budget: RepoIntelBudget::default(),
        })
        .unwrap();

        assert_eq!(snapshot.project_kinds, vec!["Node/JavaScript"]);
        assert_eq!(snapshot.commands, vec!["npm run build", "npm run test"]);
        assert!(
            snapshot
                .languages
                .iter()
                .any(|language| language.name == "TypeScript")
        );
    }
}
