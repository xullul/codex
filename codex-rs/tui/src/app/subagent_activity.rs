//! Tracks live activity from inactive subagent threads for the parent footer.

use super::*;
use crate::bottom_pane::SubagentActivityRow;
use crate::bottom_pane::SubagentActivityState;
use crate::status::format_tokens_compact;
use codex_app_server_protocol::CollabAgentTool;
use codex_app_server_protocol::CollabAgentToolCallStatus;
use codex_app_server_protocol::CommandExecutionStatus;
use codex_app_server_protocol::DynamicToolCallStatus;
use codex_app_server_protocol::McpToolCallStatus;
use codex_app_server_protocol::PatchApplyStatus;

#[derive(Default)]
pub(super) struct SubagentActivityTracker {
    records: HashMap<ThreadId, SubagentActivityRecord>,
    next_order: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SubagentActivityRecord {
    label: String,
    state: SubagentActivityState,
    summary: String,
    detail: Option<String>,
    token_summary: Option<String>,
    order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivityUpdate {
    state: SubagentActivityState,
    summary: String,
    detail: Option<String>,
}

impl SubagentActivityTracker {
    pub(super) fn clear(&mut self) -> bool {
        let was_empty = self.records.is_empty();
        self.records.clear();
        !was_empty
    }

    pub(super) fn remove(&mut self, thread_id: ThreadId) -> bool {
        self.records.remove(&thread_id).is_some()
    }

    pub(super) fn note_notification(
        &mut self,
        thread_id: ThreadId,
        label: String,
        notification: &ServerNotification,
    ) -> bool {
        match notification {
            ServerNotification::ThreadStarted(_) => self.upsert(
                thread_id,
                label,
                ActivityUpdate {
                    state: SubagentActivityState::Pending,
                    summary: "starting".to_string(),
                    detail: None,
                },
            ),
            ServerNotification::TurnStarted(_) => self.upsert(
                thread_id,
                label,
                ActivityUpdate {
                    state: SubagentActivityState::Running,
                    summary: "running".to_string(),
                    detail: None,
                },
            ),
            ServerNotification::ThreadTokenUsageUpdated(notification) => {
                let token_summary =
                    format_token_summary(notification.token_usage.total.total_tokens);
                self.update_token_summary(thread_id, label, token_summary)
            }
            ServerNotification::ItemStarted(notification) => {
                if let Some(update) = item_started_update(&notification.item) {
                    self.upsert(thread_id, label, update)
                } else {
                    false
                }
            }
            ServerNotification::ItemCompleted(notification) => {
                if let Some(update) = item_completed_update(&notification.item) {
                    self.upsert(thread_id, label, update)
                } else {
                    false
                }
            }
            ServerNotification::TurnCompleted(notification) => self.upsert(
                thread_id,
                label,
                turn_completed_update(
                    notification.turn.status.clone(),
                    notification.turn.error.as_ref(),
                ),
            ),
            ServerNotification::ThreadClosed(_) => self.remove(thread_id),
            _ => false,
        }
    }

    pub(super) fn rows(
        &self,
        active_thread_id: Option<ThreadId>,
        primary_thread_id: Option<ThreadId>,
    ) -> Vec<SubagentActivityRow> {
        let mut records = self
            .records
            .iter()
            .filter(|(thread_id, _)| {
                Some(**thread_id) != primary_thread_id && Some(**thread_id) != active_thread_id
            })
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        records.sort_by_key(|record| (!record.state.is_active(), record.order));
        records
            .into_iter()
            .map(|record| SubagentActivityRow {
                label: record.label.clone(),
                state: record.state,
                summary: record.summary.clone(),
                detail: record.detail.clone(),
                token_summary: record.token_summary.clone(),
            })
            .collect()
    }

    fn upsert(&mut self, thread_id: ThreadId, label: String, update: ActivityUpdate) -> bool {
        if !self.records.contains_key(&thread_id) {
            let order = self.next_order;
            self.next_order += 1;
            self.records.insert(
                thread_id,
                SubagentActivityRecord {
                    label: label.clone(),
                    state: update.state,
                    summary: update.summary.clone(),
                    detail: update.detail.clone(),
                    token_summary: None,
                    order,
                },
            );
        }
        let Some(record) = self.records.get_mut(&thread_id) else {
            return false;
        };

        let changed = record.label != label
            || record.state != update.state
            || record.summary != update.summary
            || record.detail != update.detail;
        record.label = label;
        record.state = update.state;
        record.summary = update.summary;
        record.detail = update.detail;
        changed
    }

    fn update_token_summary(
        &mut self,
        thread_id: ThreadId,
        label: String,
        token_summary: Option<String>,
    ) -> bool {
        if !self.records.contains_key(&thread_id) {
            let order = self.next_order;
            self.next_order += 1;
            self.records.insert(
                thread_id,
                SubagentActivityRecord {
                    label: label.clone(),
                    state: SubagentActivityState::Running,
                    summary: "running".to_string(),
                    detail: None,
                    token_summary: None,
                    order,
                },
            );
        }
        let Some(record) = self.records.get_mut(&thread_id) else {
            return false;
        };

        let changed = record.label != label || record.token_summary != token_summary;
        record.label = label;
        record.token_summary = token_summary;
        changed
    }
}

fn format_token_summary(total_tokens: i64) -> Option<String> {
    (total_tokens > 0).then(|| format!("{} tokens", format_tokens_compact(total_tokens)))
}

fn item_started_update(item: &ThreadItem) -> Option<ActivityUpdate> {
    match item {
        ThreadItem::CommandExecution { command, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "started command".to_string(),
            detail: Some(command.clone()),
        }),
        ThreadItem::FileChange { changes, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "editing files".to_string(),
            detail: Some(format!("{} changed", pluralize(changes.len(), "file"))),
        }),
        ThreadItem::McpToolCall { server, tool, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: format!("using {server}.{tool}"),
            detail: None,
        }),
        ThreadItem::DynamicToolCall {
            namespace, tool, ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: format!("using {}", dynamic_tool_name(namespace.as_deref(), tool)),
            detail: None,
        }),
        ThreadItem::CollabAgentToolCall { tool, status, .. } => {
            Some(collab_tool_update(tool, status, /*completed*/ false))
        }
        ThreadItem::WebSearch { query, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "searching web".to_string(),
            detail: Some(query.clone()),
        }),
        ThreadItem::ImageView { path, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "viewing image".to_string(),
            detail: Some(path.display().to_string()),
        }),
        ThreadItem::ImageGeneration { revised_prompt, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "generating image".to_string(),
            detail: revised_prompt.clone(),
        }),
        ThreadItem::ContextCompaction { .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "compacting context".to_string(),
            detail: None,
        }),
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. } => None,
    }
}

fn item_completed_update(item: &ThreadItem) -> Option<ActivityUpdate> {
    match item {
        ThreadItem::CommandExecution {
            command,
            status,
            exit_code,
            ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: command_status_summary(status, *exit_code),
            detail: Some(command.clone()),
        }),
        ThreadItem::FileChange {
            changes, status, ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: patch_status_summary(status),
            detail: Some(format!("{} changed", pluralize(changes.len(), "file"))),
        }),
        ThreadItem::McpToolCall {
            server,
            tool,
            status,
            error,
            ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: mcp_status_summary(server, tool, status),
            detail: error.as_ref().map(|error| error.message.clone()),
        }),
        ThreadItem::DynamicToolCall {
            namespace,
            tool,
            status,
            ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: dynamic_status_summary(namespace.as_deref(), tool, status),
            detail: None,
        }),
        ThreadItem::CollabAgentToolCall { tool, status, .. } => {
            Some(collab_tool_update(tool, status, /*completed*/ true))
        }
        ThreadItem::WebSearch { query, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "searched web".to_string(),
            detail: Some(query.clone()),
        }),
        ThreadItem::ImageView { path, .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "viewed image".to_string(),
            detail: Some(path.display().to_string()),
        }),
        ThreadItem::ImageGeneration {
            status,
            revised_prompt,
            ..
        } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: format!("image generation {status}"),
            detail: revised_prompt.clone(),
        }),
        ThreadItem::ContextCompaction { .. } => Some(ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "compacted context".to_string(),
            detail: None,
        }),
        ThreadItem::UserMessage { .. }
        | ThreadItem::HookPrompt { .. }
        | ThreadItem::AgentMessage { .. }
        | ThreadItem::Plan { .. }
        | ThreadItem::Reasoning { .. }
        | ThreadItem::EnteredReviewMode { .. }
        | ThreadItem::ExitedReviewMode { .. } => None,
    }
}

fn turn_completed_update(
    status: TurnStatus,
    error: Option<&codex_app_server_protocol::TurnError>,
) -> ActivityUpdate {
    match status {
        TurnStatus::Completed => ActivityUpdate {
            state: SubagentActivityState::Completed,
            summary: "finished".to_string(),
            detail: None,
        },
        TurnStatus::Interrupted => ActivityUpdate {
            state: SubagentActivityState::Interrupted,
            summary: "interrupted".to_string(),
            detail: None,
        },
        TurnStatus::Failed => ActivityUpdate {
            state: SubagentActivityState::Failed,
            summary: "failed".to_string(),
            detail: error.map(|error| error.message.clone()),
        },
        TurnStatus::InProgress => ActivityUpdate {
            state: SubagentActivityState::Running,
            summary: "running".to_string(),
            detail: None,
        },
    }
}

fn command_status_summary(status: &CommandExecutionStatus, exit_code: Option<i32>) -> String {
    match (status, exit_code) {
        (CommandExecutionStatus::Completed, Some(code)) => format!("finished command ({code})"),
        (CommandExecutionStatus::Completed, None) => "finished command".to_string(),
        (CommandExecutionStatus::Failed, Some(code)) => format!("command failed ({code})"),
        (CommandExecutionStatus::Failed, None) => "command failed".to_string(),
        (CommandExecutionStatus::Declined, _) => "command declined".to_string(),
        (CommandExecutionStatus::InProgress, _) => "running command".to_string(),
    }
}

fn patch_status_summary(status: &PatchApplyStatus) -> String {
    match status {
        PatchApplyStatus::InProgress => "editing files".to_string(),
        PatchApplyStatus::Completed => "edited files".to_string(),
        PatchApplyStatus::Failed => "file edit failed".to_string(),
        PatchApplyStatus::Declined => "file edit declined".to_string(),
    }
}

fn mcp_status_summary(server: &str, tool: &str, status: &McpToolCallStatus) -> String {
    let tool_name = format!("{server}.{tool}");
    match status {
        McpToolCallStatus::InProgress => format!("using {tool_name}"),
        McpToolCallStatus::Completed => format!("finished {tool_name}"),
        McpToolCallStatus::Failed => format!("failed {tool_name}"),
    }
}

fn dynamic_status_summary(
    namespace: Option<&str>,
    tool: &str,
    status: &DynamicToolCallStatus,
) -> String {
    let tool_name = dynamic_tool_name(namespace, tool);
    match status {
        DynamicToolCallStatus::InProgress => format!("using {tool_name}"),
        DynamicToolCallStatus::Completed => format!("finished {tool_name}"),
        DynamicToolCallStatus::Failed => format!("failed {tool_name}"),
    }
}

fn collab_tool_update(
    tool: &CollabAgentTool,
    status: &CollabAgentToolCallStatus,
    completed: bool,
) -> ActivityUpdate {
    let summary = match (tool, status, completed) {
        (CollabAgentTool::SpawnAgent, CollabAgentToolCallStatus::InProgress, _) => {
            "spawning subagent"
        }
        (CollabAgentTool::SpawnAgent, CollabAgentToolCallStatus::Completed, _) => {
            "spawned subagent"
        }
        (CollabAgentTool::SpawnAgent, CollabAgentToolCallStatus::Failed, _) => {
            "subagent spawn failed"
        }
        (CollabAgentTool::SendInput, CollabAgentToolCallStatus::InProgress, _) => {
            "sending subagent input"
        }
        (CollabAgentTool::SendInput, CollabAgentToolCallStatus::Completed, _) => {
            "sent subagent input"
        }
        (CollabAgentTool::SendInput, CollabAgentToolCallStatus::Failed, _) => {
            "subagent input failed"
        }
        (CollabAgentTool::ResumeAgent, CollabAgentToolCallStatus::InProgress, _) => {
            "resuming subagent"
        }
        (CollabAgentTool::ResumeAgent, CollabAgentToolCallStatus::Completed, _) => {
            "resumed subagent"
        }
        (CollabAgentTool::ResumeAgent, CollabAgentToolCallStatus::Failed, _) => {
            "subagent resume failed"
        }
        (CollabAgentTool::Wait, CollabAgentToolCallStatus::InProgress, _) => "waiting on subagent",
        (CollabAgentTool::Wait, CollabAgentToolCallStatus::Completed, _) => "finished waiting",
        (CollabAgentTool::Wait, CollabAgentToolCallStatus::Failed, _) => "subagent wait failed",
        (CollabAgentTool::CloseAgent, CollabAgentToolCallStatus::InProgress, _) => {
            "closing subagent"
        }
        (CollabAgentTool::CloseAgent, CollabAgentToolCallStatus::Completed, _) => "closed subagent",
        (CollabAgentTool::CloseAgent, CollabAgentToolCallStatus::Failed, _) => {
            "subagent close failed"
        }
    };
    ActivityUpdate {
        state: SubagentActivityState::Running,
        summary: summary.to_string(),
        detail: None,
    }
}

fn dynamic_tool_name(namespace: Option<&str>, tool: &str) -> String {
    namespace
        .filter(|namespace| !namespace.trim().is_empty())
        .map(|namespace| format!("{namespace}.{tool}"))
        .unwrap_or_else(|| tool.to_string())
}

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {singular}s")
    }
}

impl App {
    pub(super) fn refresh_subagent_activity_panel(&mut self) {
        let rows = self
            .subagent_activity
            .rows(self.active_thread_id, self.primary_thread_id);
        self.chat_widget.set_subagent_activity(rows);
    }

    pub(super) fn clear_subagent_activity_panel(&mut self) {
        if self.subagent_activity.clear() {
            self.chat_widget.clear_subagent_activity();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_server_protocol::ThreadTokenUsage;
    use codex_app_server_protocol::ThreadTokenUsageUpdatedNotification;
    use codex_app_server_protocol::TokenUsageBreakdown;
    use pretty_assertions::assert_eq;

    fn thread_id(suffix: u128) -> ThreadId {
        ThreadId::from_string(&format!("00000000-0000-0000-0000-{suffix:012}"))
            .expect("valid thread id")
    }

    fn command_item(command: &str, status: CommandExecutionStatus) -> ThreadItem {
        ThreadItem::CommandExecution {
            id: "cmd-1".to_string(),
            command: command.to_string(),
            cwd: test_path_buf("/tmp/project").abs(),
            process_id: None,
            source: codex_app_server_protocol::CommandExecutionSource::Agent,
            status,
            command_actions: Vec::new(),
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        }
    }

    fn token_notification(thread_id: ThreadId, total_tokens: i64) -> ServerNotification {
        ServerNotification::ThreadTokenUsageUpdated(ThreadTokenUsageUpdatedNotification {
            thread_id: thread_id.to_string(),
            turn_id: "turn-1".to_string(),
            token_usage: ThreadTokenUsage {
                total: TokenUsageBreakdown {
                    total_tokens,
                    input_tokens: total_tokens,
                    cached_input_tokens: 0,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                },
                last: TokenUsageBreakdown {
                    total_tokens,
                    input_tokens: total_tokens,
                    cached_input_tokens: 0,
                    output_tokens: 0,
                    reasoning_output_tokens: 0,
                },
                model_context_window: None,
            },
        })
    }

    #[test]
    fn tracks_one_row_per_inactive_subagent() {
        let mut tracker = SubagentActivityTracker::default();
        let primary_id = thread_id(1);
        let agent_id = thread_id(2);

        tracker.note_notification(
            agent_id,
            "Huygens [explorer]".to_string(),
            &ServerNotification::ItemStarted(codex_app_server_protocol::ItemStartedNotification {
                thread_id: agent_id.to_string(),
                turn_id: "turn-1".to_string(),
                item: command_item(
                    "rg -n \"subagent\" codex-rs/tui/src",
                    CommandExecutionStatus::InProgress,
                ),
            }),
        );
        tracker.note_notification(
            agent_id,
            "Huygens [explorer]".to_string(),
            &token_notification(agent_id, 844_000),
        );
        tracker.note_notification(
            agent_id,
            "Huygens [explorer]".to_string(),
            &ServerNotification::ItemCompleted(
                codex_app_server_protocol::ItemCompletedNotification {
                    thread_id: agent_id.to_string(),
                    turn_id: "turn-1".to_string(),
                    item: command_item(
                        "rg -n \"subagent\" codex-rs/tui/src",
                        CommandExecutionStatus::Completed,
                    ),
                },
            ),
        );

        assert_eq!(
            tracker.rows(Some(primary_id), Some(primary_id)),
            vec![SubagentActivityRow {
                label: "Huygens [explorer]".to_string(),
                state: SubagentActivityState::Running,
                summary: "finished command".to_string(),
                detail: Some("rg -n \"subagent\" codex-rs/tui/src".to_string()),
                token_summary: Some("844K tokens".to_string()),
            }]
        );
    }

    #[test]
    fn hides_currently_viewed_subagent() {
        let mut tracker = SubagentActivityTracker::default();
        let primary_id = thread_id(1);
        let agent_id = thread_id(2);

        tracker.note_notification(
            agent_id,
            "Huygens [explorer]".to_string(),
            &ServerNotification::TurnStarted(codex_app_server_protocol::TurnStartedNotification {
                thread_id: agent_id.to_string(),
                turn: Turn {
                    id: "turn-1".to_string(),
                    items: Vec::new(),
                    status: TurnStatus::InProgress,
                    error: None,
                    started_at: None,
                    completed_at: None,
                    duration_ms: None,
                },
            }),
        );

        assert_eq!(tracker.rows(Some(agent_id), Some(primary_id)), Vec::new());
    }
}
