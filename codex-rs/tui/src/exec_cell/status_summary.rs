use std::time::Instant;

use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::ExecCommandSource;

use super::action_summary::summarize_call;
use super::model::ExecCall;
use crate::exec_command::strip_bash_lc_and_escape;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecStatusSummary {
    pub(crate) header: String,
    pub(crate) details: Option<String>,
}

pub(crate) fn combine_exec_status_summaries(
    mut summaries: Vec<ExecStatusSummary>,
) -> Option<ExecStatusSummary> {
    if summaries.len() <= 1 {
        return summaries.pop();
    }

    let primary = summaries.remove(0);
    let extra_count = summaries.len();
    let mut detail_lines = Vec::new();
    if let Some(detail) = primary
        .details
        .as_deref()
        .map(compact_status_text)
        .filter(|detail| !detail.is_empty())
    {
        detail_lines.push(detail);
    }
    if let Some(extra) = summaries.first().map(status_activity_line) {
        detail_lines.push(format!("Also active: {extra}"));
    }
    if extra_count > 1 {
        detail_lines.push(format!("+{} more active", extra_count - 1));
    }

    Some(ExecStatusSummary {
        header: primary.header,
        details: if detail_lines.is_empty() {
            None
        } else {
            Some(detail_lines.join("\n"))
        },
    })
}

pub(crate) fn exec_status_summary(
    command: &[String],
    parsed: &[ParsedCommand],
    source: ExecCommandSource,
    interaction_input: Option<&str>,
) -> ExecStatusSummary {
    let command_display = strip_bash_lc_and_escape(command);

    match source {
        ExecCommandSource::UnifiedExecStartup => {
            return ExecStatusSummary {
                header: "Starting background terminal".to_string(),
                details: non_empty(command_display),
            };
        }
        ExecCommandSource::UnifiedExecInteraction
            if interaction_input.is_some_and(str::is_empty) =>
        {
            return ExecStatusSummary {
                header: "Waiting for background terminal".to_string(),
                details: non_empty(command_display),
            };
        }
        ExecCommandSource::UnifiedExecInteraction if interaction_input.is_some() => {
            return ExecStatusSummary {
                header: "Interacting with terminal".to_string(),
                details: non_empty(command_display),
            };
        }
        ExecCommandSource::Agent
        | ExecCommandSource::UserShell
        | ExecCommandSource::UnifiedExecInteraction => {}
    }

    let call = ExecCall {
        call_id: String::new(),
        command: command.to_vec(),
        parsed: parsed.to_vec(),
        output: None,
        source,
        start_time: Some(Instant::now()),
        duration: None,
        interaction_input: interaction_input.map(str::to_string),
    };
    if let Some(summary) = summarize_call(&call, &command_display) {
        return ExecStatusSummary {
            header: summary.verb(/*active*/ true).to_string(),
            details: summary.detail,
        };
    }

    ExecStatusSummary {
        header: fallback_status_header(source).to_string(),
        details: non_empty(command_display),
    }
}

fn fallback_status_header(source: ExecCommandSource) -> &'static str {
    match source {
        ExecCommandSource::Agent => "Running",
        ExecCommandSource::UserShell => "Running command",
        ExecCommandSource::UnifiedExecStartup => "Starting background terminal",
        ExecCommandSource::UnifiedExecInteraction => "Interacting with terminal",
    }
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

fn status_activity_line(summary: &ExecStatusSummary) -> String {
    match summary.details.as_deref().map(compact_status_text) {
        Some(details) if !details.is_empty() => format!("{} {details}", summary.header),
        Some(_) | None => summary.header.clone(),
    }
}

fn compact_status_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use codex_protocol::parse_command::ParsedCommandActionKind;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn summarizes_parsed_read() {
        let parsed = vec![ParsedCommand::Read {
            cmd: "cat src/main.rs".to_string(),
            name: "src/main.rs".to_string(),
            path: PathBuf::from("src/main.rs"),
        }];

        assert_eq!(
            exec_status_summary(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "cat src/main.rs".to_string()
                ],
                &parsed,
                ExecCommandSource::Agent,
                None,
            ),
            ExecStatusSummary {
                header: "Read".to_string(),
                details: Some("src/main.rs".to_string()),
            }
        );
    }

    #[test]
    fn summarizes_parsed_test_action() {
        let parsed = vec![ParsedCommand::Action {
            cmd: "cargo test -p codex-tui".to_string(),
            kind: ParsedCommandActionKind::Test,
            detail: Some("cargo test -p codex-tui".to_string()),
        }];

        assert_eq!(
            exec_status_summary(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "cargo test -p codex-tui".to_string()
                ],
                &parsed,
                ExecCommandSource::Agent,
                None,
            ),
            ExecStatusSummary {
                header: "Testing".to_string(),
                details: Some("cargo test -p codex-tui".to_string()),
            }
        );
    }

    #[test]
    fn user_shell_keeps_command_language() {
        assert_eq!(
            exec_status_summary(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "echo hello".to_string()
                ],
                &[],
                ExecCommandSource::UserShell,
                None,
            ),
            ExecStatusSummary {
                header: "Running command".to_string(),
                details: Some("echo hello".to_string()),
            }
        );
    }

    #[test]
    fn unified_wait_uses_background_terminal_language() {
        assert_eq!(
            exec_status_summary(
                &[
                    "bash".to_string(),
                    "-lc".to_string(),
                    "npm run dev".to_string()
                ],
                &[],
                ExecCommandSource::UnifiedExecInteraction,
                Some(""),
            ),
            ExecStatusSummary {
                header: "Waiting for background terminal".to_string(),
                details: Some("npm run dev".to_string()),
            }
        );
    }

    #[test]
    fn combines_primary_with_secondary_activity() {
        assert_eq!(
            combine_exec_status_summaries(vec![
                ExecStatusSummary {
                    header: "Testing".to_string(),
                    details: Some("cargo test -p codex-tui".to_string()),
                },
                ExecStatusSummary {
                    header: "Read".to_string(),
                    details: Some("src/main.rs".to_string()),
                },
            ]),
            Some(ExecStatusSummary {
                header: "Testing".to_string(),
                details: Some("cargo test -p codex-tui\nAlso active: Read src/main.rs".to_string()),
            })
        );
    }

    #[test]
    fn combines_primary_with_more_active_count() {
        assert_eq!(
            combine_exec_status_summaries(vec![
                ExecStatusSummary {
                    header: "Testing".to_string(),
                    details: Some("cargo test -p codex-tui".to_string()),
                },
                ExecStatusSummary {
                    header: "Read".to_string(),
                    details: Some("src/main.rs".to_string()),
                },
                ExecStatusSummary {
                    header: "Search".to_string(),
                    details: Some("foo in src".to_string()),
                },
                ExecStatusSummary {
                    header: "Linting".to_string(),
                    details: Some("cargo clippy -p codex-tui".to_string()),
                },
            ]),
            Some(ExecStatusSummary {
                header: "Testing".to_string(),
                details: Some(
                    "cargo test -p codex-tui\nAlso active: Read src/main.rs\n+2 more active"
                        .to_string()
                ),
            })
        );
    }
}
