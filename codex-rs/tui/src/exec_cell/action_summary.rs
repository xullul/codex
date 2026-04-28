use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;
use codex_shell_command::parse_command::classify_action_script;

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
    pub(crate) subactions: Vec<ActionSummary>,
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
            subactions: Vec::new(),
            suppress_success_output: true,
        });
    }
    if let Some(summary) = summarize_parsed(&call.parsed) {
        return Some(summary);
    }
    classify_action_script(command_display).and_then(|command| summarize_single_parsed(&command))
}

fn summarize_parsed(parsed: &[ParsedCommand]) -> Option<ActionSummary> {
    if parsed.len() > 1 && parsed.iter().all(is_non_exploration_action) {
        let subactions = parsed
            .iter()
            .map(summarize_single_parsed)
            .collect::<Option<Vec<_>>>()?;
        return Some(ActionSummary {
            kind: ActionKind::Run,
            detail: Some(format!("{} commands", subactions.len())),
            subactions,
            suppress_success_output: false,
        });
    }

    let single = primary_parsed_summary(parsed)?;
    summarize_single_parsed(single)
}

fn summarize_single_parsed(single: &ParsedCommand) -> Option<ActionSummary> {
    match single {
        ParsedCommand::Read { name, .. } => Some(ActionSummary {
            kind: ActionKind::Read,
            detail: Some(name.clone()),
            subactions: Vec::new(),
            suppress_success_output: true,
        }),
        ParsedCommand::ListFiles { cmd, path } => Some(ActionSummary {
            kind: ActionKind::List,
            detail: Some(path.clone().unwrap_or_else(|| cmd.clone())),
            subactions: Vec::new(),
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
                subactions: Vec::new(),
                suppress_success_output: true,
            })
        }
        ParsedCommand::Action { kind, detail, cmd } => Some(ActionSummary {
            kind: action_kind_from_parsed(kind),
            detail: detail.clone().or_else(|| Some(cmd.clone())),
            subactions: Vec::new(),
            suppress_success_output: *kind == ParsedCommandActionKind::Inspect,
        }),
        ParsedCommand::Unknown { .. } => None,
    }
}

fn primary_parsed_summary(parsed: &[ParsedCommand]) -> Option<&ParsedCommand> {
    if let [single] = parsed {
        return Some(single);
    }

    if !parsed.iter().all(is_non_exploration_action) {
        return None;
    }
    parsed.first()
}

fn is_non_exploration_action(parsed: &ParsedCommand) -> bool {
    matches!(
        parsed,
        ParsedCommand::Action {
            kind: ParsedCommandActionKind::Test
                | ParsedCommandActionKind::Build
                | ParsedCommandActionKind::Lint
                | ParsedCommandActionKind::Git
                | ParsedCommandActionKind::Wait
                | ParsedCommandActionKind::Run,
            ..
        }
    )
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
