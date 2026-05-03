use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use codex_protocol::permissions::ReadDenyMatcher;
use serde::Deserialize;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;

use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct RepoSearchHandler;
pub struct RepoReadHandler;

const DENY_READ_POLICY_MESSAGE: &str =
    "access denied: reading this path is blocked by filesystem deny_read policy";
const DEFAULT_SEARCH_LIMIT: usize = 50;
const MAX_SEARCH_LIMIT: usize = 200;
const DEFAULT_READ_LIMIT: usize = 120;
const MAX_READ_LIMIT: usize = 400;
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FALLBACK_FILES: usize = 5_000;
const RG_TIMEOUT: Duration = Duration::from_secs(8);

fn default_offset() -> usize {
    0
}

fn default_search_limit() -> usize {
    DEFAULT_SEARCH_LIMIT
}

fn default_read_offset() -> usize {
    1
}

fn default_read_limit() -> usize {
    DEFAULT_READ_LIMIT
}

#[derive(Deserialize)]
struct RepoSearchArgs {
    query: String,
    path: Option<String>,
    glob: Option<String>,
    #[serde(default)]
    context_lines: usize,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(default = "default_offset")]
    offset: usize,
    #[serde(default)]
    files_only: bool,
}

#[derive(Deserialize)]
struct RepoReadArgs {
    path: String,
    #[serde(default = "default_read_offset")]
    offset: usize,
    #[serde(default = "default_read_limit")]
    limit: usize,
}

impl ToolHandler for RepoSearchHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;
        let arguments = function_arguments(payload, "repo_search")?;
        let args: RepoSearchArgs = parse_arguments(&arguments)?;
        let base_path = resolve_path(&turn.cwd, args.path.as_deref());
        validate_search_args(&args)?;

        let file_system_sandbox_policy = turn.file_system_sandbox_policy();
        let read_deny_matcher = ReadDenyMatcher::new(&file_system_sandbox_policy, &turn.cwd);
        ensure_path_read_allowed(&base_path, read_deny_matcher.as_ref())?;

        let result = if read_deny_matcher.is_none() {
            rg_search(&base_path, &args).await
        } else {
            fallback_search(&base_path, &args, read_deny_matcher.as_ref()).await
        }?;

        Ok(FunctionToolOutput::from_text(result, Some(true)))
    }
}

impl ToolHandler for RepoReadHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;
        let arguments = function_arguments(payload, "repo_read")?;
        let args: RepoReadArgs = parse_arguments(&arguments)?;
        let path = resolve_path(&turn.cwd, Some(&args.path));
        validate_read_args(&args)?;

        let file_system_sandbox_policy = turn.file_system_sandbox_policy();
        let read_deny_matcher = ReadDenyMatcher::new(&file_system_sandbox_policy, &turn.cwd);
        ensure_path_read_allowed(&path, read_deny_matcher.as_ref())?;

        let lines = read_text_lines(&path).await?;
        let output = format_read_output(&path, &lines, args.offset, args.limit.min(MAX_READ_LIMIT));
        Ok(FunctionToolOutput::from_text(output, Some(true)))
    }
}

fn function_arguments(payload: ToolPayload, name: &str) -> Result<String, FunctionCallError> {
    match payload {
        ToolPayload::Function { arguments } => Ok(arguments),
        _ => Err(FunctionCallError::RespondToModel(format!(
            "{name} handler received unsupported payload"
        ))),
    }
}

fn validate_search_args(args: &RepoSearchArgs) -> Result<(), FunctionCallError> {
    if args.query.is_empty() {
        return Err(FunctionCallError::RespondToModel(
            "query must not be empty".to_string(),
        ));
    }
    if args.limit == 0 {
        return Err(FunctionCallError::RespondToModel(
            "limit must be greater than zero".to_string(),
        ));
    }
    if args.context_lines > 5 {
        return Err(FunctionCallError::RespondToModel(
            "context_lines must be 5 or less".to_string(),
        ));
    }
    Ok(())
}

fn validate_read_args(args: &RepoReadArgs) -> Result<(), FunctionCallError> {
    if args.offset == 0 {
        return Err(FunctionCallError::RespondToModel(
            "offset must be a 1-indexed line number".to_string(),
        ));
    }
    if args.limit == 0 {
        return Err(FunctionCallError::RespondToModel(
            "limit must be greater than zero".to_string(),
        ));
    }
    Ok(())
}

fn resolve_path(cwd: &Path, path: Option<&str>) -> PathBuf {
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return cwd.to_path_buf();
    };
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn ensure_path_read_allowed(
    path: &Path,
    read_deny_matcher: Option<&ReadDenyMatcher>,
) -> Result<(), FunctionCallError> {
    if read_deny_matcher.is_some_and(|matcher| matcher.is_read_denied(path)) {
        return Err(FunctionCallError::RespondToModel(format!(
            "{DENY_READ_POLICY_MESSAGE}: `{}`",
            path.display()
        )));
    }
    Ok(())
}

async fn rg_search(path: &Path, args: &RepoSearchArgs) -> Result<String, FunctionCallError> {
    let mut command = Command::new("rg");
    command
        .arg("--color")
        .arg("never")
        .arg("--line-number")
        .arg("--with-filename")
        .arg("--no-heading");
    if args.files_only {
        command.arg("--files-with-matches");
    }
    if args.context_lines > 0 {
        command.arg("--context").arg(args.context_lines.to_string());
    }
    if let Some(glob) = args.glob.as_deref().filter(|glob| !glob.is_empty()) {
        command.arg("--glob").arg(glob);
    }
    command.arg(&args.query).arg(path);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = timeout(RG_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            FunctionCallError::RespondToModel(format!(
                "repo_search timed out after {} seconds",
                RG_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|err| FunctionCallError::RespondToModel(format!("failed to execute rg: {err}")))?;

    match output.status.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(format_search_output(path, &text, args.offset, args.limit))
        }
        Some(1) => Ok(format!("No matches found under {}", path.display())),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(FunctionCallError::RespondToModel(if stderr.is_empty() {
                "repo_search failed to run rg".to_string()
            } else {
                format!("repo_search failed: {stderr}")
            }))
        }
    }
}

async fn fallback_search(
    path: &Path,
    args: &RepoSearchArgs,
    read_deny_matcher: Option<&ReadDenyMatcher>,
) -> Result<String, FunctionCallError> {
    let files = collect_search_files(path, args.glob.as_deref(), read_deny_matcher).await?;
    let mut matches = Vec::new();
    for file in files {
        let Ok(lines) = read_text_lines(&file).await else {
            continue;
        };
        let matching_indexes = lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| line.contains(&args.query).then_some(idx))
            .collect::<Vec<_>>();
        if args.files_only {
            if !matching_indexes.is_empty() {
                matches.push(file.display().to_string());
            }
            continue;
        }
        for idx in matching_indexes {
            let start = idx.saturating_sub(args.context_lines);
            let end = (idx + args.context_lines + 1).min(lines.len());
            for (line_idx, line) in lines.iter().enumerate().take(end).skip(start) {
                matches.push(format!("{}:{}:{}", file.display(), line_idx + 1, line));
            }
        }
    }

    if matches.is_empty() {
        return Ok(format!("No matches found under {}", path.display()));
    }
    Ok(format_search_output(
        path,
        &matches.join("\n"),
        args.offset,
        args.limit,
    ))
}

async fn collect_search_files(
    path: &Path,
    glob: Option<&str>,
    read_deny_matcher: Option<&ReadDenyMatcher>,
) -> Result<Vec<PathBuf>, FunctionCallError> {
    let metadata = fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to inspect search path: {err}"))
    })?;
    if metadata.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !metadata.is_dir() {
        return Err(FunctionCallError::RespondToModel(
            "search path must be a file or directory".to_string(),
        ));
    }

    let mut files = Vec::new();
    let mut queue = VecDeque::from([path.to_path_buf()]);
    while let Some(dir) = queue.pop_front() {
        let mut entries = fs::read_dir(&dir).await.map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to read directory: {err}"))
        })?;
        while let Some(entry) = entries.next_entry().await.map_err(|err| {
            FunctionCallError::RespondToModel(format!("failed to read directory: {err}"))
        })? {
            let entry_path = entry.path();
            if read_deny_matcher.is_some_and(|matcher| matcher.is_read_denied(&entry_path)) {
                continue;
            }
            let file_type = entry.file_type().await.map_err(|err| {
                FunctionCallError::RespondToModel(format!("failed to inspect entry: {err}"))
            })?;
            if file_type.is_dir() {
                queue.push_back(entry_path);
            } else if file_type.is_file()
                && glob_matches(glob, &entry_path)
                && files.len() < MAX_FALLBACK_FILES
            {
                files.push(entry_path);
            }
        }
    }
    Ok(files)
}

async fn read_text_lines(path: &Path) -> Result<Vec<String>, FunctionCallError> {
    let metadata = fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("failed to inspect file: {err}"))
    })?;
    if !metadata.is_file() {
        return Err(FunctionCallError::RespondToModel(
            "path must be a file".to_string(),
        ));
    }
    if metadata.len() > MAX_READ_BYTES {
        return Err(FunctionCallError::RespondToModel(format!(
            "file is too large to read directly ({} bytes, limit {MAX_READ_BYTES})",
            metadata.len()
        )));
    }
    let bytes = fs::read(path)
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("failed to read file: {err}")))?;
    if bytes.contains(&0) {
        return Err(FunctionCallError::RespondToModel(
            "binary files cannot be read by repo_read/repo_search".to_string(),
        ));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        FunctionCallError::RespondToModel(
            "file is not valid UTF-8 text; binary or encoded files are not supported".to_string(),
        )
    })?;
    Ok(text.lines().map(str::to_string).collect())
}

fn format_read_output(path: &Path, lines: &[String], offset: usize, limit: usize) -> String {
    let start = offset - 1;
    if start >= lines.len() {
        return format!(
            "Absolute path: {}\nNo lines at offset {offset}; file has {} lines",
            path.display(),
            lines.len()
        );
    }
    let end = (start + limit).min(lines.len());
    let mut output = Vec::with_capacity(end - start + 2);
    output.push(format!("Absolute path: {}", path.display()));
    for (idx, line) in lines.iter().enumerate().take(end).skip(start) {
        output.push(format!("{:>6}: {}", idx + 1, line));
    }
    if end < lines.len() {
        output.push(format!(
            "More lines available. Next offset: {}",
            end.saturating_add(1)
        ));
    }
    output.join("\n")
}

fn format_search_output(path: &Path, text: &str, offset: usize, limit: usize) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return format!("No matches found under {}", path.display());
    }
    let capped_limit = limit.min(MAX_SEARCH_LIMIT);
    let start = offset.min(lines.len());
    let end = (start + capped_limit).min(lines.len());
    let mut output = Vec::with_capacity(end - start + 3);
    output.push(format!("Search root: {}", path.display()));
    output.extend(lines[start..end].iter().map(|line| (*line).to_string()));
    if end < lines.len() {
        output.push(format!("More matches available. Next offset: {end}"));
    }
    output.join("\n")
}

fn glob_matches(glob: Option<&str>, path: &Path) -> bool {
    let Some(glob) = glob.filter(|glob| !glob.is_empty()) else {
        return true;
    };
    wildcard_match(glob, &path.to_string_lossy().replace('\\', "/"))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut p_idx, mut v_idx, mut star_idx, mut match_idx) = (0, 0, None, 0);
    while v_idx < value.len() {
        if p_idx < pattern.len() && (pattern[p_idx] == b'?' || pattern[p_idx] == value[v_idx]) {
            p_idx += 1;
            v_idx += 1;
        } else if p_idx < pattern.len() && pattern[p_idx] == b'*' {
            star_idx = Some(p_idx);
            match_idx = v_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            v_idx = match_idx;
        } else {
            return false;
        }
    }
    while p_idx < pattern.len() && pattern[p_idx] == b'*' {
        p_idx += 1;
    }
    p_idx == pattern.len()
}

#[cfg(test)]
#[path = "repo_explore_tests.rs"]
mod tests;
