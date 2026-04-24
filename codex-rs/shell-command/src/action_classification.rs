use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;

use crate::parse_command::shlex_join;

pub(crate) fn action_from_tokens(tokens: &[String]) -> Option<ParsedCommand> {
    let (head, tail) = tokens.split_first()?;
    let command = shlex_join(tokens);
    let head_lower = head.to_ascii_lowercase();
    if is_simple_version_probe(&head_lower, tail) {
        return Some(ParsedCommand::Action {
            cmd: command.clone(),
            kind: ParsedCommandActionKind::Inspect,
            detail: Some(command),
        });
    }
    let kind = match head_lower.as_str() {
        "cargo" => classify_cargo(tail),
        "npm" => classify_npm(tail),
        "pnpm" => classify_pnpm(tail),
        "yarn" => classify_yarn(tail),
        "go" => classify_go(tail),
        "bazel" => classify_bazel(tail),
        "git" => classify_git(tail),
        "node" | "node.exe" => Some(ParsedCommandActionKind::Run),
        "python" | "python.exe" | "python3" | "python3.exe" => Some(classify_python(tail)),
        "sleep" | "start-sleep" => Some(ParsedCommandActionKind::Wait),
        "timeout" => Some(ParsedCommandActionKind::Wait),
        "test-path" | "resolve-path" | "get-item" | "gi" | "get-itemproperty" | "get-acl"
        | "get-filehash" | "get-process" | "get-service" | "get-command" | "where.exe" => {
            Some(ParsedCommandActionKind::Inspect)
        }
        _ => None,
    }?;

    Some(ParsedCommand::Action {
        cmd: command.clone(),
        kind,
        detail: Some(command),
    })
}

pub(crate) fn action_from_script(script: &str) -> Option<ParsedCommand> {
    let first_line = script.lines().next().unwrap_or(script).trim();
    if first_line.is_empty() {
        return None;
    }
    let lower_script = script.to_ascii_lowercase();
    let lower = first_line.to_ascii_lowercase();
    let kind = if is_obvious_edit_script(&lower_script) {
        ParsedCommandActionKind::Edit
    } else if !is_single_simple_script(script, first_line)
        || lower.contains("where-object")
        || lower.contains("foreach-object")
        || lower.contains("| ?")
        || lower.contains("| %")
    {
        return None;
    } else if is_test_command(&lower) {
        ParsedCommandActionKind::Test
    } else if is_build_command(&lower) {
        ParsedCommandActionKind::Build
    } else if is_lint_command(&lower) {
        ParsedCommandActionKind::Lint
    } else if lower.starts_with("git ") && is_read_only_git_line(&lower) {
        ParsedCommandActionKind::Git
    } else if lower.starts_with("sleep ")
        || lower == "sleep"
        || lower.starts_with("start-sleep")
        || lower.starts_with("timeout ")
    {
        ParsedCommandActionKind::Wait
    } else if is_run_command(&lower) {
        ParsedCommandActionKind::Run
    } else if is_inspect_script(&lower) {
        ParsedCommandActionKind::Inspect
    } else {
        return None;
    };

    Some(ParsedCommand::Action {
        cmd: script.to_string(),
        kind,
        detail: Some(first_line.to_string()),
    })
}

fn is_single_simple_script(script: &str, first_line: &str) -> bool {
    let mut non_empty_lines = script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if non_empty_lines.next() != Some(first_line) || non_empty_lines.next().is_some() {
        return false;
    }
    !first_line.contains(';')
        && !first_line.contains('|')
        && !first_line.contains("&&")
        && !first_line.contains("||")
}

fn classify_cargo(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "test" | "nextest" => Some(ParsedCommandActionKind::Test),
        "build" | "check" => Some(ParsedCommandActionKind::Build),
        "clippy" | "fmt" | "fix" => Some(ParsedCommandActionKind::Lint),
        "run" => Some(ParsedCommandActionKind::Run),
        _ => None,
    }
}

fn classify_npm(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args {
        [cmd, ..] if cmd == "test" || cmd == "start" => Some(if cmd == "test" {
            ParsedCommandActionKind::Test
        } else {
            ParsedCommandActionKind::Run
        }),
        [run, script, ..] if run == "run" => classify_package_script(script),
        _ => None,
    }
}

fn classify_pnpm(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args {
        [cmd, ..] if matches!(cmd.as_str(), "test" | "build" | "lint" | "start") => {
            classify_package_script(cmd)
        }
        [run, script, ..] if run == "run" => classify_package_script(script),
        _ => None,
    }
}

fn classify_yarn(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args {
        [script, ..] => classify_package_script(script),
        _ => None,
    }
}

fn classify_package_script(script: &str) -> Option<ParsedCommandActionKind> {
    let script = script.to_ascii_lowercase();
    if script.contains("test") {
        Some(ParsedCommandActionKind::Test)
    } else if script.contains("build") {
        Some(ParsedCommandActionKind::Build)
    } else if script.contains("lint") || script.contains("fmt") || script.contains("format") {
        Some(ParsedCommandActionKind::Lint)
    } else if script == "start" || script.contains("dev") {
        Some(ParsedCommandActionKind::Run)
    } else {
        None
    }
}

fn classify_go(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "test" => Some(ParsedCommandActionKind::Test),
        "build" => Some(ParsedCommandActionKind::Build),
        "fmt" | "vet" => Some(ParsedCommandActionKind::Lint),
        "run" => Some(ParsedCommandActionKind::Run),
        _ => None,
    }
}

fn classify_bazel(args: &[String]) -> Option<ParsedCommandActionKind> {
    match args.first().map(String::as_str)? {
        "test" => Some(ParsedCommandActionKind::Test),
        "build" => Some(ParsedCommandActionKind::Build),
        "run" => Some(ParsedCommandActionKind::Run),
        _ => None,
    }
}

fn classify_git(args: &[String]) -> Option<ParsedCommandActionKind> {
    let subcommand = args.first()?.to_ascii_lowercase();
    if matches!(
        subcommand.as_str(),
        "status" | "log" | "diff" | "show" | "branch"
    ) {
        Some(ParsedCommandActionKind::Git)
    } else {
        None
    }
}

fn classify_python(args: &[String]) -> ParsedCommandActionKind {
    if args
        .iter()
        .any(|arg| arg.contains("write_text") || arg.contains("Path(") && arg.contains(".write"))
    {
        ParsedCommandActionKind::Edit
    } else {
        ParsedCommandActionKind::Run
    }
}

fn is_simple_version_probe(head: &str, tail: &[String]) -> bool {
    let version_flag = matches!(
        tail.first().map(String::as_str),
        Some("-v" | "--version" | "-version" | "--info")
    );
    version_flag
        && matches!(
            head,
            "node"
                | "node.exe"
                | "python"
                | "python.exe"
                | "python3"
                | "python3.exe"
                | "dotnet"
                | "cargo"
                | "ffmpeg"
                | "ffmpeg.exe"
                | "go"
                | "npm"
                | "pnpm"
                | "yarn"
                | "rg"
        )
}

fn is_obvious_edit_script(command: &str) -> bool {
    command.starts_with("apply_patch")
        || command.contains("apply_patch <<")
        || command.starts_with("python") && command.contains("write_text")
        || command.contains(" > ")
        || command.contains(" >> ")
        || command.contains(" 1> ")
        || command.contains(" 2> ")
        || command.starts_with("set-content ")
        || command.starts_with("add-content ")
        || command.starts_with("out-file ")
        || command.starts_with("new-item ")
        || command.starts_with("remove-item ")
        || command.starts_with("copy-item ")
        || command.starts_with("move-item ")
        || command.starts_with("rename-item ")
        || command.contains(" set-content ")
        || command.contains(" add-content ")
        || command.contains(" out-file ")
        || command.contains(" new-item ")
        || command.contains(" remove-item ")
        || command.contains(" copy-item ")
        || command.contains(" move-item ")
        || command.contains(" rename-item ")
        || command.contains("; set-content ")
        || command.contains("; add-content ")
        || command.contains("; out-file ")
        || command.contains("; new-item ")
        || command.contains("; remove-item ")
        || command.contains("; copy-item ")
        || command.contains("; move-item ")
        || command.contains("; rename-item ")
}

fn is_test_command(command: &str) -> bool {
    command.starts_with("cargo test")
        || command.starts_with("cargo nextest")
        || command.starts_with("just test")
        || command.starts_with("npm test")
        || command.starts_with("npm run test")
        || command.starts_with("pnpm test")
        || command.starts_with("pnpm run test")
        || command.starts_with("yarn test")
        || command.starts_with("pytest")
        || command.starts_with("go test")
        || command.starts_with("bazel test")
}

fn is_build_command(command: &str) -> bool {
    command.starts_with("cargo build")
        || command.starts_with("cargo check")
        || command.starts_with("just build")
        || command.starts_with("npm run build")
        || command.starts_with("pnpm build")
        || command.starts_with("pnpm run build")
        || command.starts_with("yarn build")
        || command.starts_with("go build")
        || command.starts_with("bazel build")
}

fn is_lint_command(command: &str) -> bool {
    command.starts_with("cargo clippy")
        || command.starts_with("just fix")
        || command.starts_with("just fmt")
        || command.starts_with("cargo fmt")
        || command.starts_with("npm run lint")
        || command.starts_with("pnpm lint")
        || command.starts_with("pnpm run lint")
        || command.starts_with("yarn lint")
        || command.starts_with("go fmt")
        || command.starts_with("go vet")
}

fn is_read_only_git_line(command: &str) -> bool {
    matches!(
        command.split_whitespace().nth(1),
        Some("status" | "log" | "diff" | "show" | "branch")
    )
}

fn is_run_command(command: &str) -> bool {
    command.starts_with("cargo run")
        || command.starts_with("npm start")
        || command.starts_with("npm run start")
        || command.starts_with("pnpm start")
        || command.starts_with("pnpm run start")
        || command.starts_with("yarn start")
        || command.starts_with("node ")
        || command.starts_with("python ")
        || command.starts_with("python3 ")
}

fn is_inspect_script(command: &str) -> bool {
    command.starts_with("test-path ")
        || command.starts_with("resolve-path ")
        || command.starts_with("get-item ")
        || command.starts_with("get-itemproperty ")
        || command.starts_with("get-acl ")
        || command.starts_with("get-filehash ")
        || command.starts_with("get-process")
        || command.starts_with("get-service")
        || command.starts_with("get-command ")
        || command.starts_with("where.exe ")
        || command.starts_with("$env:")
        || command.starts_with("$psversiontable")
}
