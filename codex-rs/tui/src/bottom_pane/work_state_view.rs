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

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkStateSnapshot {
    pub(crate) proposed_plan_markdown: Option<String>,
    pub(crate) checklist: Vec<WorkStatePlanItem>,
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
            key_hint::plain(KeyCode::Esc).into(),
            " close".dim(),
        ])];

        let mut has_content = false;
        if let Some(markdown) = self
            .snapshot
            .proposed_plan_markdown
            .as_deref()
            .filter(|markdown| !markdown.trim().is_empty())
        {
            has_content = true;
            push_gap(&mut lines);
            push_section(&mut lines, "Proposed plan");
            for line in markdown.lines().filter(|line| !line.trim().is_empty()) {
                lines.extend(wrap_dimmed(line, width, "  "));
            }
        }

        if !self.snapshot.checklist.is_empty() {
            has_content = true;
            push_gap(&mut lines);
            push_section(&mut lines, "Task progress");
            let completed = self
                .snapshot
                .checklist
                .iter()
                .filter(|item| matches!(item.status, WorkStateStepStatus::Completed))
                .count();
            lines.push(Line::from(
                format!("  {completed}/{} complete", self.snapshot.checklist.len()).dim(),
            ));
            for item in &self.snapshot.checklist {
                lines.extend(wrap_checklist_item(item, width));
            }
        }

        if !self.snapshot.pending_steers.is_empty()
            || !self.snapshot.rejected_steers.is_empty()
            || !self.snapshot.queued_messages.is_empty()
        {
            has_content = true;
            push_gap(&mut lines);
            push_section(&mut lines, "Queued input");
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
            push_section(&mut lines, "Subagents");
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
                        "{} · {:?} · {}{detail}{token_summary}",
                        row.label, row.state, row.summary
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
            push_section(&mut lines, "Background terminals");
            lines.extend(wrap_dimmed(summary, width, "  "));
        }

        if !has_content {
            push_gap(&mut lines);
            lines.push(
                "  No active or recent work state for this thread."
                    .dim()
                    .into(),
            );
        }

        lines
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

fn push_section(lines: &mut Vec<Line<'static>>, title: &str) {
    lines.push(Line::from(vec!["• ".dim(), title.to_string().bold()]));
}

fn wrap_dimmed(text: &str, width: u16, indent: &'static str) -> Vec<Line<'static>> {
    adaptive_wrap_lines(
        std::iter::once(Line::from(text.to_string().dim())),
        RtOptions::new(width as usize)
            .initial_indent(Line::from(indent.dim()))
            .subsequent_indent(Line::from(indent)),
    )
}

fn wrap_checklist_item(item: &WorkStatePlanItem, width: u16) -> Vec<Line<'static>> {
    let (prefix, style) = match item.status {
        WorkStateStepStatus::Completed => ("  ✓ ", "completed"),
        WorkStateStepStatus::InProgress => ("  › ", "active"),
        WorkStateStepStatus::Pending => ("  - ", "pending"),
    };
    let line = match style {
        "completed" => Line::from(item.step.clone().dim()),
        "active" => Line::from(item.step.clone().cyan().bold()),
        "pending" => Line::from(item.step.clone().dim()),
        _ => Line::from(item.step.clone()),
    };
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
            proposed_plan_markdown: Some(
                "# Plan\n\n- Inspect current surfaces\n- Add a unified work panel".to_string(),
            ),
            checklist: vec![
                item("Inspect current surfaces", WorkStateStepStatus::Completed),
                item("Add a unified work panel", WorkStateStepStatus::InProgress),
                item("Update snapshots", WorkStateStepStatus::Pending),
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
