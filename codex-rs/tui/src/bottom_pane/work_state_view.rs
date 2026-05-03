//! Inspectable snapshot of the current thread's work state.

use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::bottom_pane_view::BottomPaneView;
use crate::bottom_pane::subagent_activity::SubagentActivityRow;
use crate::key_hint;
use crate::render::renderable::Renderable;
use crate::wrapping::RtOptions;
use crate::wrapping::adaptive_wrap_lines;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkStateStepStatus {
    Pending,
    InProgress,
    Completed,
}

impl From<&StepStatus> for WorkStateStepStatus {
    fn from(status: &StepStatus) -> Self {
        match status {
            StepStatus::Pending => Self::Pending,
            StepStatus::InProgress => Self::InProgress,
            StepStatus::Completed => Self::Completed,
        }
    }
}

impl WorkStateStepStatus {
    fn to_step_status(&self) -> StepStatus {
        match self {
            Self::Pending => StepStatus::Pending,
            Self::InProgress => StepStatus::InProgress,
            Self::Completed => StepStatus::Completed,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "active",
            Self::Completed => "done",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkStatePlanItem {
    pub(crate) step: String,
    pub(crate) status: WorkStateStepStatus,
}

impl From<&PlanItemArg> for WorkStatePlanItem {
    fn from(item: &PlanItemArg) -> Self {
        Self {
            step: item.step.clone(),
            status: WorkStateStepStatus::from(&item.status),
        }
    }
}

impl WorkStatePlanItem {
    pub(crate) fn to_plan_item_arg(&self) -> PlanItemArg {
        PlanItemArg {
            step: self.step.clone(),
            status: self.status.to_step_status(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkProgressRow {
    pub(crate) label: String,
    pub(crate) detail: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkStateSnapshot {
    pub(crate) active_phase: Option<String>,
    pub(crate) active_tool_summary: Option<String>,
    pub(crate) pending_approval_summary: Option<String>,
    pub(crate) continuity_status: Option<String>,
    pub(crate) proposed_plan_markdown: Option<String>,
    pub(crate) checklist: Vec<WorkStatePlanItem>,
    pub(crate) progress: Vec<WorkProgressRow>,
    pub(crate) queued_messages: Vec<String>,
    pub(crate) pending_steers: Vec<String>,
    pub(crate) rejected_steers: Vec<String>,
    pub(crate) subagents: Vec<SubagentActivityRow>,
    pub(crate) background_summary: Option<String>,
}

pub(crate) struct WorkStateView {
    snapshot: WorkStateSnapshot,
    scroll_top: usize,
    complete: bool,
}

impl WorkStateView {
    pub(crate) const VIEW_ID: &'static str = "work-state";

    pub(crate) fn new(snapshot: WorkStateSnapshot) -> Self {
        Self {
            snapshot,
            scroll_top: 0,
            complete: false,
        }
    }

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width < 8 {
            return Vec::new();
        }

        let mut lines = vec![Line::from(vec![
            "> ".into(),
            "Work state".bold(),
            "  ".into(),
            self.thread_summary().dim(),
            "  ".into(),
            key_hint::plain(KeyCode::Esc).into(),
            " close".dim(),
        ])];

        let mut has_content = false;
        if self
            .snapshot
            .active_phase
            .as_deref()
            .is_some_and(|phase| !phase.trim().is_empty())
            || self
                .snapshot
                .active_tool_summary
                .as_deref()
                .is_some_and(|summary| !summary.trim().is_empty())
            || self
                .snapshot
                .pending_approval_summary
                .as_deref()
                .is_some_and(|summary| !summary.trim().is_empty())
            || self
                .snapshot
                .continuity_status
                .as_deref()
                .is_some_and(|status| !status.trim().is_empty())
        {
            has_content = true;
            push_gap(&mut lines);
            push_section_with_summary(&mut lines, "Current state", "live context");
            push_optional_state_row(
                &mut lines,
                width,
                "Phase",
                self.snapshot.active_phase.as_deref(),
            );
            push_optional_state_row(
                &mut lines,
                width,
                "Active tool",
                self.snapshot.active_tool_summary.as_deref(),
            );
            push_optional_state_row(
                &mut lines,
                width,
                "Approval",
                self.snapshot.pending_approval_summary.as_deref(),
            );
            push_optional_state_row(
                &mut lines,
                width,
                "Continuity",
                self.snapshot.continuity_status.as_deref(),
            );
        }

        if let Some(markdown) = self
            .snapshot
            .proposed_plan_markdown
            .as_deref()
            .filter(|markdown| !markdown.trim().is_empty())
        {
            has_content = true;
            push_gap(&mut lines);
            push_section_with_summary(&mut lines, "Proposed plan", "ready to implement");
            for line in markdown.lines().filter(|line| !line.trim().is_empty()) {
                lines.extend(wrap_dimmed(line, width, "  "));
            }
        }

        if !self.snapshot.checklist.is_empty() {
            has_content = true;
            push_gap(&mut lines);
            let completed = self
                .snapshot
                .checklist
                .iter()
                .filter(|item| matches!(item.status, WorkStateStepStatus::Completed))
                .count();
            let active = self
                .snapshot
                .checklist
                .iter()
                .position(|item| matches!(item.status, WorkStateStepStatus::InProgress))
                .map(|idx| idx + 1);
            let progress_summary = active.map_or_else(
                || format!("{completed}/{} complete", self.snapshot.checklist.len()),
                |active| {
                    format!(
                        "{completed}/{} complete, step {active} active",
                        self.snapshot.checklist.len()
                    )
                },
            );
            push_section_with_summary(&mut lines, "Task progress", &progress_summary);
            for item in &self.snapshot.checklist {
                lines.extend(wrap_checklist_item(item, width));
            }
        }

        if !self.snapshot.progress.is_empty() {
            has_content = true;
            push_gap(&mut lines);
            push_section_with_summary(
                &mut lines,
                "Evidence",
                &pluralize(self.snapshot.progress.len(), "event", "events"),
            );
            for row in &self.snapshot.progress {
                let line = if row.detail.trim().is_empty() {
                    Line::from(vec!["  - ".dim(), row.label.clone().into()])
                } else {
                    Line::from(vec![
                        "  - ".dim(),
                        row.label.clone().into(),
                        " · ".dim(),
                        row.detail.clone().dim(),
                    ])
                };
                lines.extend(adaptive_wrap_lines(
                    std::iter::once(line),
                    RtOptions::new(width as usize)
                        .initial_indent("".into())
                        .subsequent_indent("    ".into()),
                ));
            }
        }

        if !self.snapshot.pending_steers.is_empty()
            || !self.snapshot.rejected_steers.is_empty()
            || !self.snapshot.queued_messages.is_empty()
        {
            has_content = true;
            push_gap(&mut lines);
            let input_count = self.snapshot.pending_steers.len()
                + self.snapshot.rejected_steers.len()
                + self.snapshot.queued_messages.len();
            let input_summary = pluralize(input_count, "message", "messages");
            push_section_with_summary(&mut lines, "Queued input", &input_summary);
            push_input_group(
                &mut lines,
                width,
                "Current-turn",
                &self.snapshot.pending_steers,
            );
            push_input_group(
                &mut lines,
                width,
                "After this turn",
                &self.snapshot.rejected_steers,
            );
            push_input_group(
                &mut lines,
                width,
                "Next turn",
                &self.snapshot.queued_messages,
            );
        }

        if !self.snapshot.subagents.is_empty() {
            has_content = true;
            push_gap(&mut lines);
            let active_count = self
                .snapshot
                .subagents
                .iter()
                .filter(|row| row.state.is_active())
                .count();
            let done_count = self.snapshot.subagents.len().saturating_sub(active_count);
            let subagent_summary = match (active_count, done_count) {
                (0, done) => pluralize(done, "done", "done"),
                (active, 0) => pluralize(active, "active", "active"),
                (active, done) => format!("{active} active, {done} done"),
            };
            push_section_with_summary(&mut lines, "Subagents", &subagent_summary);
            for row in &self.snapshot.subagents {
                let detail = row
                    .detail
                    .as_deref()
                    .filter(|detail| !detail.trim().is_empty())
                    .map(|detail| format!(" · {detail}"))
                    .unwrap_or_default();
                let token_summary = row
                    .token_summary
                    .as_deref()
                    .filter(|tokens| !tokens.trim().is_empty())
                    .map(|tokens| format!(" · {tokens}"))
                    .unwrap_or_default();
                lines.extend(wrap_dimmed(
                    &format!(
                        "{} · {} · {}{detail}{token_summary}",
                        row.label,
                        row.state.label(),
                        row.summary
                    ),
                    width,
                    "  ",
                ));
            }
        }

        if let Some(summary) = self
            .snapshot
            .background_summary
            .as_deref()
            .filter(|summary| !summary.trim().is_empty())
        {
            has_content = true;
            push_gap(&mut lines);
            push_section_with_summary(&mut lines, "Background terminals", "running");
            lines.extend(wrap_dimmed(summary, width, "  "));
        }

        if !has_content {
            push_gap(&mut lines);
            lines.extend(wrap_dimmed(
                "No active plan, queued input, subagents, or background terminals.",
                width,
                "  ",
            ));
        }

        lines
    }

    fn thread_summary(&self) -> String {
        let mut parts = Vec::new();
        if self
            .snapshot
            .active_phase
            .as_deref()
            .is_some_and(|phase| !phase.trim().is_empty())
        {
            parts.push("live".to_string());
        }
        if self
            .snapshot
            .pending_approval_summary
            .as_deref()
            .is_some_and(|summary| !summary.trim().is_empty())
        {
            parts.push("approval".to_string());
        }
        if self
            .snapshot
            .proposed_plan_markdown
            .as_deref()
            .is_some_and(|markdown| !markdown.trim().is_empty())
        {
            parts.push("plan".to_string());
        }
        if !self.snapshot.checklist.is_empty() {
            let completed = self
                .snapshot
                .checklist
                .iter()
                .filter(|item| matches!(item.status, WorkStateStepStatus::Completed))
                .count();
            parts.push(format!(
                "steps {completed}/{}",
                self.snapshot.checklist.len()
            ));
        }
        if !self.snapshot.progress.is_empty() {
            parts.push(format!("evidence {}", self.snapshot.progress.len()));
        }
        let input_count = self.snapshot.pending_steers.len()
            + self.snapshot.rejected_steers.len()
            + self.snapshot.queued_messages.len();
        if input_count > 0 {
            parts.push(format!("input {input_count}"));
        }
        let active_subagents = self
            .snapshot
            .subagents
            .iter()
            .filter(|row| row.state.is_active())
            .count();
        if active_subagents > 0 {
            parts.push(format!("agents {active_subagents}"));
        }
        if self
            .snapshot
            .background_summary
            .as_deref()
            .is_some_and(|summary| !summary.trim().is_empty())
        {
            parts.push("bg".to_string());
        }

        if parts.is_empty() {
            "idle".to_string()
        } else {
            parts.join(" · ")
        }
    }

    fn scroll_down(&mut self, visible_rows: usize, line_count: usize) {
        self.scroll_top = self
            .scroll_top
            .saturating_add(1)
            .min(line_count.saturating_sub(visible_rows));
    }

    fn scroll_up(&mut self) {
        self.scroll_top = self.scroll_top.saturating_sub(1);
    }
}

fn push_gap(lines: &mut Vec<Line<'static>>) {
    if !lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.push(Line::from(""));
    }
}

fn push_section_with_summary(lines: &mut Vec<Line<'static>>, title: &str, summary: &str) {
    lines.push(Line::from(vec![
        "• ".dim(),
        title.to_string().bold(),
        format!(" · {summary}").dim(),
    ]));
}

fn pluralize(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn wrap_dimmed(text: &str, width: u16, indent: &'static str) -> Vec<Line<'static>> {
    adaptive_wrap_lines(
        std::iter::once(Line::from(text.to_string().dim())),
        RtOptions::new(width as usize)
            .initial_indent(Line::from(indent.dim()))
            .subsequent_indent(Line::from(indent)),
    )
}

fn push_optional_state_row(
    lines: &mut Vec<Line<'static>>,
    width: u16,
    label: &str,
    value: Option<&str>,
) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let line = Line::from(vec![
        "  - ".dim(),
        label.to_string().into(),
        ": ".dim(),
        value.to_string().dim(),
    ]);
    lines.extend(adaptive_wrap_lines(
        std::iter::once(line),
        RtOptions::new(width as usize)
            .initial_indent("".into())
            .subsequent_indent("    ".into()),
    ));
}

fn wrap_checklist_item(item: &WorkStatePlanItem, width: u16) -> Vec<Line<'static>> {
    let (prefix, style) = match item.status {
        WorkStateStepStatus::Completed => ("  ✓ ", "completed"),
        WorkStateStepStatus::InProgress => ("  › ", "active"),
        WorkStateStepStatus::Pending => ("  - ", "pending"),
    };
    let step_span = match style {
        "completed" => item.step.clone().dim(),
        "active" => item.step.clone().cyan().bold(),
        "pending" => item.step.clone().dim(),
        _ => item.step.clone().into(),
    };
    let line = Line::from(vec![format!("{} · ", item.status.label()).dim(), step_span]);
    adaptive_wrap_lines(
        std::iter::once(line),
        RtOptions::new(width as usize)
            .initial_indent(Line::from(prefix.dim()))
            .subsequent_indent(Line::from("    ")),
    )
}

fn push_input_group(lines: &mut Vec<Line<'static>>, width: u16, label: &str, messages: &[String]) {
    if messages.is_empty() {
        return;
    }
    lines.push(Line::from(format!("  {label}").dim()));
    for message in messages {
        lines.extend(wrap_dimmed(message, width, "    "));
    }
}

impl Renderable for WorkStateView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        let lines = self.render_lines(area.width);
        let scroll_top = self
            .scroll_top
            .min(lines.len().saturating_sub(area.height as usize));
        let visible = lines
            .into_iter()
            .skip(scroll_top)
            .take(area.height as usize)
            .collect::<Vec<_>>();
        Paragraph::new(visible).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.render_lines(width).len() as u16
    }
}

impl BottomPaneView for WorkStateView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }

        let line_count = self.render_lines(/*width*/ 80).len();
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(/*visible_rows*/ 12, line_count);
            }
            KeyCode::PageUp => {
                self.scroll_top = self.scroll_top.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.scroll_top = self.scroll_top.saturating_add(10);
            }
            KeyCode::Esc => {
                self.complete = true;
            }
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn view_id(&self) -> Option<&'static str> {
        Some(Self::VIEW_ID)
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn item(step: &str, status: WorkStateStepStatus) -> WorkStatePlanItem {
        WorkStatePlanItem {
            step: step.to_string(),
            status,
        }
    }

    fn render(view: &WorkStateView, width: u16) -> String {
        let height = view.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        view.render(Rect::new(0, 0, width, height), &mut buf);
        format!("{buf:?}")
    }

    #[test]
    fn renders_empty_state() {
        let view = WorkStateView::new(WorkStateSnapshot::default());
        assert_snapshot!("work_state_empty", render(&view, /*width*/ 60));
    }

    #[test]
    fn renders_complete_state() {
        let view = WorkStateView::new(WorkStateSnapshot {
            active_phase: Some("Working".to_string()),
            active_tool_summary: Some("reading TUI state surfaces".to_string()),
            pending_approval_summary: Some("1 command approval waiting".to_string()),
            continuity_status: Some(
                "Checklist, proposed plan, queued input, and subagent rows are retained for resume"
                    .to_string(),
            ),
            proposed_plan_markdown: Some(
                "# Plan\n\n- Inspect current surfaces\n- Add a unified work panel".to_string(),
            ),
            checklist: vec![
                item("Inspect current surfaces", WorkStateStepStatus::Completed),
                item("Add a unified work panel", WorkStateStepStatus::InProgress),
                item("Update snapshots", WorkStateStepStatus::Pending),
            ],
            progress: vec![
                WorkProgressRow {
                    label: "repo intel".to_string(),
                    detail: "Rust/Cargo; 420 files, 2 manifests".to_string(),
                },
                WorkProgressRow {
                    label: "command".to_string(),
                    detail: "cargo test -p codex-tui".to_string(),
                },
            ],
            queued_messages: vec!["Follow up once tests pass".to_string()],
            pending_steers: vec!["Use the existing bottom pane stack".to_string()],
            rejected_steers: Vec::new(),
            subagents: vec![SubagentActivityRow {
                label: "Curie [explorer]".to_string(),
                state: crate::bottom_pane::SubagentActivityState::Running,
                summary: "reading TUI state".to_string(),
                detail: Some("codex-rs/tui/src/chatwidget.rs".to_string()),
                token_summary: Some("12K tokens".to_string()),
            }],
            background_summary: Some(
                "1 bg terminal: cargo test -p codex-tui · /ps · /stop".to_string(),
            ),
        });

        assert_snapshot!("work_state_complete", render(&view, /*width*/ 72));
    }

    #[test]
    fn desired_height_tracks_wrapping() {
        let view = WorkStateView::new(WorkStateSnapshot {
            proposed_plan_markdown: Some(
                "A very long proposed plan line that wraps in a narrow panel".to_string(),
            ),
            ..Default::default()
        });

        assert!(view.desired_height(/*width*/ 24) > view.desired_height(/*width*/ 80));
        assert_eq!(WorkStateView::VIEW_ID, "work-state");
    }
}
