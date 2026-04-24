use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;

use super::model::ExecCall;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionKind {
    Read,
    Search,
    List,
    Inspect,
    Edit,
    Test,
    Build,
    Lint,
    Git,
    Wait,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActionSummary {
    pub(crate) kind: ActionKind,
    pub(crate) detail: Option<String>,
    pub(crate) suppress_success_output: bool,
}

impl ActionSummary {
    pub(crate) fn verb(&self, active: bool) -> &'static str {
        match (self.kind, active) {
            (ActionKind::Read, _) => "Read",
            (ActionKind::Search, _) => "Search",
            (ActionKind::List, _) => "List",
            (ActionKind::Inspect, true) => "Inspecting",
            (ActionKind::Inspect, false) => "Inspected",
            (ActionKind::Edit, true) => "Editing",
            (ActionKind::Edit, false) => "Edited",
            (ActionKind::Test, true) => "Testing",
            (ActionKind::Test, false) => "Tested",
            (ActionKind::Build, true) => "Building",
            (ActionKind::Build, false) => "Built",
            (ActionKind::Lint, true) => "Linting",
            (ActionKind::Lint, false) => "Linted",
            (ActionKind::Git, true) => "Checking git",
            (ActionKind::Git, false) => "Checked git",
            (ActionKind::Wait, true) => "Waiting",
            (ActionKind::Wait, false) => "Waited",
            (ActionKind::Run, true) => "Running",
            (ActionKind::Run, false) => "Ran",
        }
    }
}

pub(crate) fn summarize_call(call: &ExecCall, command_display: &str) -> Option<ActionSummary> {
    if call.is_user_shell_command() {
        return None;
    }
    if call.is_unified_exec_interaction() {
        return Some(ActionSummary {
            kind: ActionKind::Wait,
            detail: Some(command_display.to_string()),
            suppress_success_output: true,
        });
    }
    if let Some(summary) = summarize_parsed(&call.parsed) {
        return Some(summary);
    }
    summarize_raw(command_display)
}

fn summarize_parsed(parsed: &[ParsedCommand]) -> Option<ActionSummary> {
    let [single] = parsed else {
        return None;
    };
    match single {
        ParsedCommand::Read { name, .. } => Some(ActionSummary {
            kind: ActionKind::Read,
            detail: Some(name.clone()),
            suppress_success_output: true,
        }),
        ParsedCommand::ListFiles { cmd, path } => Some(ActionSummary {
            kind: ActionKind::List,
            detail: Some(path.clone().unwrap_or_else(|| cmd.clone())),
            suppress_success_output: true,
        }),
        ParsedCommand::Search { cmd, query, path } => {
            let detail = match (query, path) {
                (Some(query), Some(path)) => format!("{query} in {path}"),
                (Some(query), None) => query.clone(),
                _ => cmd.clone(),
            };
            Some(ActionSummary {
                kind: ActionKind::Search,
                detail: Some(detail),
                suppress_success_output: true,
            })
        }
        ParsedCommand::Action { kind, detail, cmd } => Some(ActionSummary {
            kind: action_kind_from_parsed(kind),
            detail: detail.clone().or_else(|| Some(cmd.clone())),
            suppress_success_output: *kind == ParsedCommandActionKind::Inspect,
        }),
        ParsedCommand::Unknown { .. } => None,
    }
}

fn action_kind_from_parsed(kind: &ParsedCommandActionKind) -> ActionKind {
    match kind {
        ParsedCommandActionKind::Inspect => ActionKind::Inspect,
        ParsedCommandActionKind::Edit => ActionKind::Edit,
        ParsedCommandActionKind::Test => ActionKind::Test,
        ParsedCommandActionKind::Build => ActionKind::Build,
        ParsedCommandActionKind::Lint => ActionKind::Lint,
        ParsedCommandActionKind::Git => ActionKind::Git,
        ParsedCommandActionKind::Wait => ActionKind::Wait,
        ParsedCommandActionKind::Run => ActionKind::Run,
    }
}

fn summarize_raw(command: &str) -> Option<ActionSummary> {
    let first_line = command.lines().next().unwrap_or(command).trim();
    if first_line.is_empty() {
        return None;
    }
    let lower = first_line.to_ascii_lowercase();
    let kind = if lower.starts_with("apply_patch")
        || lower.contains("apply_patch <<")
        || lower.starts_with("python") && lower.contains("write_text")
    {
        ActionKind::Edit
    } else if is_test_command(&lower) {
        ActionKind::Test
    } else if is_build_command(&lower) {
        ActionKind::Build
    } else if is_lint_command(&lower) {
        ActionKind::Lint
    } else if lower.starts_with("git ") {
        ActionKind::Git
    } else if lower.starts_with("sleep ") || lower == "sleep" || lower.starts_with("timeout ") {
        ActionKind::Wait
    } else if is_run_command(&lower) {
        ActionKind::Run
    } else {
        return None;
    };

    Some(ActionSummary {
        kind,
        detail: Some(first_line.to_string()),
        suppress_success_output: false,
    })
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
