//! Compact transcript rows for activity mirrored from subagent threads.

use crate::history_cell::PlainHistoryCell;
use crate::multi_agents::format_agent_picker_item_name;
use crate::multi_agents::format_agent_tokens;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubagentActivityEvent {
    pub(crate) thread_id: ThreadId,
    pub(crate) agent_nickname: Option<String>,
    pub(crate) agent_role: Option<String>,
    pub(crate) kind: SubagentActivityKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SubagentActivityKind {
    Started,
    Finished { status: TurnStatus },
    ItemStarted { item: SubagentActivityItem },
    ItemFinished { item: SubagentActivityItem },
    TokenUsage { total_tokens: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubagentActivityItem {
    Command { command: String },
    FileChange,
    McpTool { name: String },
    DynamicTool { name: String },
    WebSearch { query: String },
    ImageGeneration,
}

pub(crate) fn event_from_notification(
    thread_id: ThreadId,
    agent_nickname: Option<String>,
    agent_role: Option<String>,
    notification: &ServerNotification,
) -> Option<SubagentActivityEvent> {
    let kind = match notification {
        ServerNotification::TurnStarted(_) => SubagentActivityKind::Started,
        ServerNotification::TurnCompleted(notification) => SubagentActivityKind::Finished {
            status: notification.turn.status.clone(),
        },
        ServerNotification::ItemStarted(notification) => SubagentActivityKind::ItemStarted {
            item: item_activity(&notification.item)?,
        },
        ServerNotification::ItemCompleted(notification) => SubagentActivityKind::ItemFinished {
            item: item_activity(&notification.item)?,
        },
        ServerNotification::ThreadTokenUsageUpdated(notification) => {
            SubagentActivityKind::TokenUsage {
                total_tokens: notification.token_usage.total.total_tokens,
            }
        }
        _ => return None,
    };

    Some(SubagentActivityEvent {
        thread_id,
        agent_nickname,
        agent_role,
        kind,
    })
}

pub(crate) fn render(event: SubagentActivityEvent) -> PlainHistoryCell {
    PlainHistoryCell::new(vec![line_for_event(event)])
}

fn item_activity(item: &ThreadItem) -> Option<SubagentActivityItem> {
    match item {
        ThreadItem::CommandExecution { command, .. } => Some(SubagentActivityItem::Command {
            command: command.clone(),
        }),
        ThreadItem::FileChange { .. } => Some(SubagentActivityItem::FileChange),
        ThreadItem::McpToolCall { server, tool, .. } => Some(SubagentActivityItem::McpTool {
            name: format!("{server}.{tool}"),
        }),
        ThreadItem::DynamicToolCall {
            namespace, tool, ..
        } => Some(SubagentActivityItem::DynamicTool {
            name: format_dynamic_tool_name(namespace.as_deref(), tool),
        }),
        ThreadItem::WebSearch { query, .. } => Some(SubagentActivityItem::WebSearch {
            query: query.clone(),
        }),
        ThreadItem::ImageGeneration { .. } => Some(SubagentActivityItem::ImageGeneration),
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::CollabAgentToolCall { .. }
        | ThreadItem::ImageView { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. }
        | ThreadItem::ContextCompaction { .. } => None,
    }
}

fn format_dynamic_tool_name(namespace: Option<&str>, tool: &str) -> String {
    match namespace
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
    {
        Some(namespace) => format!("{namespace}.{tool}"),
        None => tool.to_string(),
    }
}

fn line_for_event(event: SubagentActivityEvent) -> Line<'static> {
    let agent_label = format_agent_picker_item_name(
        event.agent_nickname.as_deref(),
        event.agent_role.as_deref(),
        /*is_primary*/ false,
    );
    let mut spans = vec!["• ".dim(), agent_label.cyan()];
    match event.kind {
        SubagentActivityKind::Started => spans.push(" started".into()),
        SubagentActivityKind::Finished { status } => {
            spans.push(" finished".into());
            if !matches!(status, TurnStatus::Completed) {
                spans.push(format!(" ({})", status_label(status)).dim());
            }
        }
        SubagentActivityKind::ItemStarted { item } => {
            spans.extend(item_spans(" started", item));
        }
        SubagentActivityKind::ItemFinished { item } => {
            spans.extend(item_spans(" finished", item));
        }
        SubagentActivityKind::TokenUsage { total_tokens } => {
            spans.push(" token usage: ".into());
            spans.push(format_agent_tokens(total_tokens).magenta());
            spans.push(" total".dim());
        }
    }
    spans.into()
}

fn item_spans(prefix: &str, item: SubagentActivityItem) -> Vec<Span<'static>> {
    let mut spans = vec![Span::from(prefix.to_string())];
    match item {
        SubagentActivityItem::Command { command } => {
            spans.push(" command".into());
            if !command.trim().is_empty() {
                spans.push(": ".dim());
                spans.push(command.trim().to_string().dim());
            }
        }
        SubagentActivityItem::FileChange => spans.push(" file change".into()),
        SubagentActivityItem::McpTool { name } | SubagentActivityItem::DynamicTool { name } => {
            spans.push(" tool".into());
            if !name.trim().is_empty() {
                spans.push(": ".dim());
                spans.push(name.dim());
            }
        }
        SubagentActivityItem::WebSearch { query } => {
            spans.push(" search".into());
            if !query.trim().is_empty() {
                spans.push(": ".dim());
                spans.push(query.trim().to_string().dim());
            }
        }
        SubagentActivityItem::ImageGeneration => spans.push(" image generation".into()),
    }
    spans
}

fn status_label(status: TurnStatus) -> &'static str {
    match status {
        TurnStatus::Completed => "completed",
        TurnStatus::Interrupted => "interrupted",
        TurnStatus::Failed => "failed",
        TurnStatus::InProgress => "in progress",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::HistoryCell;
    use insta::assert_snapshot;

    #[test]
    fn activity_rows_snapshot() {
        let thread_id =
            ThreadId::from_string("00000000-0000-0000-0000-000000000002").expect("valid thread");
        let events = vec![
            SubagentActivityEvent {
                thread_id,
                agent_nickname: Some("Scout".to_string()),
                agent_role: Some("explorer".to_string()),
                kind: SubagentActivityKind::Started,
            },
            SubagentActivityEvent {
                thread_id,
                agent_nickname: Some("Scout".to_string()),
                agent_role: Some("explorer".to_string()),
                kind: SubagentActivityKind::ItemStarted {
                    item: SubagentActivityItem::Command {
                        command: "rg TODO src".to_string(),
                    },
                },
            },
            SubagentActivityEvent {
                thread_id,
                agent_nickname: Some("Scout".to_string()),
                agent_role: Some("explorer".to_string()),
                kind: SubagentActivityKind::ItemFinished {
                    item: SubagentActivityItem::WebSearch {
                        query: "rust clippy collapsible_if".to_string(),
                    },
                },
            },
            SubagentActivityEvent {
                thread_id,
                agent_nickname: Some("Scout".to_string()),
                agent_role: Some("explorer".to_string()),
                kind: SubagentActivityKind::Finished {
                    status: TurnStatus::Completed,
                },
            },
        ];

        assert_snapshot!("subagent_activity_rows", render_events(events));
    }

    #[test]
    fn token_usage_row_snapshot() {
        let thread_id =
            ThreadId::from_string("00000000-0000-0000-0000-000000000002").expect("valid thread");
        assert_snapshot!(
            "subagent_activity_token_usage",
            render_events(vec![SubagentActivityEvent {
                thread_id,
                agent_nickname: Some("Scout".to_string()),
                agent_role: Some("explorer".to_string()),
                kind: SubagentActivityKind::TokenUsage {
                    total_tokens: 8_400
                },
            }])
        );
    }

    fn render_events(events: Vec<SubagentActivityEvent>) -> String {
        events
            .into_iter()
            .map(render)
            .flat_map(|cell| cell.display_lines(/*width*/ 200))
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
