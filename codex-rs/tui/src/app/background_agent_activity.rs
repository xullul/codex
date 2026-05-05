use std::collections::HashMap;
use std::collections::HashSet;

use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnStatus;
use codex_protocol::ThreadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BackgroundAgentActivityStatus {
    Starting,
    Running,
    EditingFiles,
    Finished,
    Failed,
}

#[derive(Debug, Clone)]
struct BackgroundAgentActivity {
    status: BackgroundAgentActivityStatus,
}

#[derive(Debug, Default)]
pub(super) struct BackgroundAgentActivityState {
    activity: HashMap<ThreadId, BackgroundAgentActivity>,
    mirrored_file_changes: HashSet<(ThreadId, String, String)>,
}

impl BackgroundAgentActivityStatus {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::EditingFiles => "editing files",
            Self::Finished => "finished",
            Self::Failed => "failed",
        }
    }

    pub(super) fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::EditingFiles)
    }
}

impl BackgroundAgentActivityState {
    pub(super) fn clear(&mut self) {
        self.activity.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        self.activity.is_empty()
    }

    pub(super) fn iter(
        &self,
    ) -> impl Iterator<Item = (&ThreadId, BackgroundAgentActivityStatus)> + '_ {
        self.activity
            .iter()
            .map(|(thread_id, activity)| (thread_id, activity.status))
    }

    pub(super) fn note_notification(
        &mut self,
        thread_id: ThreadId,
        notification: &ServerNotification,
    ) {
        if let Some(status) = background_agent_status_for_notification(notification) {
            self.activity
                .insert(thread_id, BackgroundAgentActivity { status });
        }
    }

    pub(super) fn file_change_summary_to_mirror(
        &mut self,
        thread_id: ThreadId,
        notification: &ServerNotification,
    ) -> Option<Vec<FileUpdateChange>> {
        let ServerNotification::ItemCompleted(notification) = notification else {
            return None;
        };
        let (item_id, changes) = file_change_item_summary(&notification.item)?;
        self.mirrored_file_changes
            .insert((thread_id, notification.turn_id.clone(), item_id.to_string()))
            .then(|| changes.to_vec())
    }
}

fn file_change_item_summary(item: &ThreadItem) -> Option<(&str, &[FileUpdateChange])> {
    match item {
        ThreadItem::FileChange { id, changes, .. } if !changes.is_empty() => {
            Some((id.as_str(), changes.as_slice()))
        }
        _ => None,
    }
}

fn background_agent_status_for_notification(
    notification: &ServerNotification,
) -> Option<BackgroundAgentActivityStatus> {
    match notification {
        ServerNotification::ThreadStarted(_) => Some(BackgroundAgentActivityStatus::Starting),
        ServerNotification::TurnStarted(_) => Some(BackgroundAgentActivityStatus::Running),
        ServerNotification::ItemStarted(notification) => {
            if matches!(notification.item, ThreadItem::FileChange { .. }) {
                Some(BackgroundAgentActivityStatus::EditingFiles)
            } else {
                Some(BackgroundAgentActivityStatus::Running)
            }
        }
        ServerNotification::ItemCompleted(notification) => {
            if matches!(notification.item, ThreadItem::FileChange { .. }) {
                Some(BackgroundAgentActivityStatus::EditingFiles)
            } else {
                Some(BackgroundAgentActivityStatus::Running)
            }
        }
        ServerNotification::TurnCompleted(notification) => match notification.turn.status {
            TurnStatus::Failed => Some(BackgroundAgentActivityStatus::Failed),
            TurnStatus::Completed | TurnStatus::Interrupted => {
                Some(BackgroundAgentActivityStatus::Finished)
            }
            TurnStatus::InProgress => Some(BackgroundAgentActivityStatus::Running),
        },
        ServerNotification::ThreadClosed(_) => Some(BackgroundAgentActivityStatus::Finished),
        _ => None,
    }
}
