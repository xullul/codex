use super::*;
use codex_utils_elapsed::format_duration;

const AWAY_SUMMARY_MIN_UNFOCUSED: Duration = Duration::from_secs(5 * 60);
const IDLE_CONTEXT_NUDGE_MIN_UNFOCUSED: Duration = Duration::from_secs(75 * 60);
const IDLE_CONTEXT_NUDGE_MIN_INPUT_TOKENS: i64 = 100_000;
const IDLE_CONTEXT_NUDGE_MIN_USED_PERCENT: i64 = 80;
const LARGE_INSTRUCTION_SOURCE_BYTES: u64 = 15_000;

impl ChatWidget {
    pub(crate) fn handle_terminal_focus_changed(&mut self, focused: bool) {
        self.handle_terminal_focus_changed_at(focused, Instant::now());
    }

    pub(super) fn handle_terminal_focus_changed_at(&mut self, focused: bool, now: Instant) {
        self.cancel_delayed_completion_notification();
        if !focused {
            if self.terminal_unfocused_since.is_none() {
                self.terminal_unfocused_since = Some(now);
                self.away_snapshot = Some(self.current_away_snapshot());
            }
            return;
        }

        let Some(unfocused_since) = self.terminal_unfocused_since.take() else {
            return;
        };
        let unfocused_for = now.saturating_duration_since(unfocused_since);
        if unfocused_for >= AWAY_SUMMARY_MIN_UNFOCUSED {
            self.maybe_add_away_return_summary(unfocused_for);
        }
        if unfocused_for >= IDLE_CONTEXT_NUDGE_MIN_UNFOCUSED {
            self.maybe_show_idle_context_nudge(unfocused_for);
        }
    }

    fn maybe_add_away_return_summary(&mut self, unfocused_for: Duration) {
        let before = self.away_snapshot.take();
        let summary = match before.as_ref() {
            Some(before) => self.away_return_delta_summary(before),
            None => self.away_return_summary(),
        };
        let Some((key, detail)) = summary else {
            return;
        };
        if self.last_away_summary_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.last_away_summary_key = Some(key);

        let message = format!(
            "While you were away for {}: {detail}",
            format_duration(unfocused_for)
        );
        self.add_info_message(message, /*hint*/ None);
        self.add_work_progress(
            "return summary".to_string(),
            format!("away {}", format_duration(unfocused_for)),
        );
    }

    fn away_return_summary(&self) -> Option<(String, String)> {
        let snapshot = self.current_away_snapshot();

        let has_retained_work = snapshot.latest_progress.is_some()
            || !snapshot.subagents.is_empty()
            || snapshot.active_step.is_some()
            || snapshot.pending_approval.is_some()
            || snapshot.background_summary.is_some();
        if !has_retained_work {
            return None;
        }

        let key = format!(
            "retained:{}:{}:{}:{}:{}:{}",
            snapshot.progress_count,
            snapshot.subagents.len(),
            self.latest_work_state_checklist.len(),
            snapshot.pending_approval.as_deref().unwrap_or_default(),
            snapshot.active_step.as_deref().unwrap_or_default(),
            snapshot.background_summary.as_deref().unwrap_or_default()
        );

        let mut parts = Vec::new();
        if let Some(approval) = snapshot.pending_approval {
            parts.push(format!("needs attention: {approval}"));
        }
        if let Some(row) = snapshot.latest_progress {
            parts.push(format!("latest work: {} ({})", row.label, row.detail));
        }
        if let Some(step) = snapshot.active_step {
            parts.push(format!("active step: {step}"));
        }
        if !snapshot.subagents.is_empty() {
            let running = snapshot
                .subagents
                .iter()
                .filter(|row| row.state.is_active())
                .count();
            if running > 0 {
                parts.push(format!("{running} subagent(s) still active"));
            } else {
                parts.push("subagent work finished while you were away".to_string());
            }
        }
        if let Some(summary) = snapshot.background_summary {
            parts.push(format!("background terminal: {summary}"));
        }
        if parts.is_empty() {
            parts.push(self.current_status.header.clone());
        }

        Some((key, parts.join("; ")))
    }

    fn away_return_delta_summary(&self, before: &AwaySnapshot) -> Option<(String, String)> {
        let after = self.current_away_snapshot();
        if before == &after {
            return None;
        }

        let mut parts = Vec::new();
        if before.pending_approval != after.pending_approval
            && let Some(approval) = after.pending_approval.as_ref()
        {
            parts.push(format!("needs attention: {approval}"));
        }
        if (before.progress_count != after.progress_count
            || before.latest_progress != after.latest_progress)
            && let Some(row) = after.latest_progress.as_ref()
        {
            parts.push(format!("latest work: {} ({})", row.label, row.detail));
        }
        if before.active_step != after.active_step
            && let Some(step) = after.active_step.as_ref()
        {
            parts.push(format!("active step: {step}"));
        }
        if before.subagents != after.subagents && !after.subagents.is_empty() {
            let running = after
                .subagents
                .iter()
                .filter(|row| row.state.is_active())
                .count();
            if running > 0 {
                parts.push(format!("{running} subagent(s) still active"));
            } else {
                parts.push("subagent work finished while you were away".to_string());
            }
        }
        if before.background_summary != after.background_summary
            && let Some(summary) = after.background_summary.as_ref()
        {
            parts.push(format!("background terminal: {summary}"));
        }
        if parts.is_empty() {
            return None;
        }

        Some((format!("delta:{before:?}:{after:?}"), parts.join("; ")))
    }

    fn current_away_snapshot(&self) -> AwaySnapshot {
        AwaySnapshot {
            progress_count: self.latest_work_state_progress.len(),
            latest_progress: self.latest_work_state_progress.last().cloned(),
            active_step: self
                .latest_work_state_checklist
                .iter()
                .find(|item| matches!(item.status, WorkStateStepStatus::InProgress))
                .map(|item| item.step.clone()),
            pending_approval: self.bottom_pane.pending_approval_summary(),
            subagents: self.latest_work_state_subagents.clone(),
            background_summary: self.bottom_pane.background_terminal_summary(),
        }
    }

    fn maybe_show_idle_context_nudge(&mut self, unfocused_for: Duration) {
        let Some((key, detail)) = self.idle_context_nudge_detail(unfocused_for) else {
            return;
        };
        if self.last_idle_context_nudge_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.last_idle_context_nudge_key = Some(key);

        if self.bottom_pane.has_active_view() {
            self.add_info_message(detail, /*hint*/ None);
            return;
        }

        let side_actions: Vec<SelectionAction> = if let Some(parent_thread_id) = self.thread_id {
            vec![Box::new(move |tx| {
                tx.send(AppEvent::StartSide {
                    parent_thread_id,
                    user_message: /*user_message*/ None,
                });
            })]
        } else {
            Vec::new()
        };

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("High-context return".to_string()),
            subtitle: Some(detail),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![
                SelectionItem {
                    name: "Continue current thread".to_string(),
                    description: Some(
                        "Use this when you are continuing the same task.".to_string(),
                    ),
                    dismiss_on_select: true,
                    ..Default::default()
                },
                SelectionItem {
                    name: "Start side conversation".to_string(),
                    description: Some(
                        "Use this for a separate question without disturbing this thread."
                            .to_string(),
                    ),
                    is_disabled: self.thread_id.is_none(),
                    disabled_reason: self
                        .thread_id
                        .is_none()
                        .then(|| "Session has not started yet.".to_string()),
                    actions: side_actions,
                    dismiss_on_select: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        self.request_redraw();
    }

    fn idle_context_nudge_detail(&self, unfocused_for: Duration) -> Option<(String, String)> {
        let token_info = self.token_info.as_ref()?;
        let context_window = self.status_line_context_window_size();
        let used_percent = context_window
            .map(|window| {
                100 - token_info
                    .last_token_usage
                    .percent_of_context_window_remaining(window)
            })
            .unwrap_or(0)
            .clamp(0, 100);
        let input_tokens = token_info.total_token_usage.input_tokens;
        if input_tokens < IDLE_CONTEXT_NUDGE_MIN_INPUT_TOKENS
            && used_percent < IDLE_CONTEXT_NUDGE_MIN_USED_PERCENT
        {
            return None;
        }

        let key = format!("{input_tokens}:{used_percent}");
        let mut context_parts = vec![format!(
            "{} input tokens",
            format_tokens_compact(input_tokens)
        )];
        if used_percent > 0 {
            context_parts.push(format!("{used_percent}% context used"));
        }
        Some((
            key,
            format!(
                "You were away {} and this thread has high context ({})",
                format_duration(unfocused_for),
                context_parts.join(", ")
            ),
        ))
    }

    pub(crate) fn maybe_record_instruction_size_warning(&mut self) {
        if self.instruction_size_warning_emitted {
            return;
        }
        let Some((path, size)) = self.large_instruction_source() else {
            return;
        };
        self.instruction_size_warning_emitted = true;

        let detail = format!("{} is {}", path.display(), format_bytes(size));
        self.add_work_progress("instruction warning".to_string(), detail.clone());
        self.on_warning(format!(
            "Large instruction source may consume extra context: {detail}."
        ));
    }

    fn large_instruction_source(&self) -> Option<(PathBuf, u64)> {
        self.instruction_source_paths
            .iter()
            .filter_map(|path| {
                let metadata = std::fs::metadata(path).ok()?;
                let size = metadata.len();
                (size >= LARGE_INSTRUCTION_SOURCE_BYTES).then(|| (path.to_path_buf(), size))
            })
            .max_by_key(|(_, size)| *size)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}
