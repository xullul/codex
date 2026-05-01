//! Compact live activity list for background subagents.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::render::renderable::Renderable;

const MAX_VISIBLE_AGENTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentActivityState {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl SubagentActivityState {
    pub(crate) fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    fn prefix(self) -> Span<'static> {
        match self {
            Self::Pending => "  - ".dim(),
            Self::Running => "  › ".cyan().bold(),
            Self::Completed => "  ✓ ".green(),
            Self::Failed => "  ! ".red(),
            Self::Interrupted => "  ! ".dim(),
        }
    }

    fn summary_style(self, summary: &str) -> Span<'static> {
        match self {
            Self::Pending | Self::Running => summary.to_string().cyan(),
            Self::Completed => summary.to_string().green(),
            Self::Failed => summary.to_string().red(),
            Self::Interrupted => summary.to_string().dim(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubagentActivityRow {
    pub(crate) label: String,
    pub(crate) state: SubagentActivityState,
    pub(crate) summary: String,
    pub(crate) detail: Option<String>,
    pub(crate) token_summary: Option<String>,
}

pub(crate) struct SubagentActivity {
    rows: Vec<SubagentActivityRow>,
}

impl SubagentActivity {
    pub(crate) fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub(crate) fn set_rows(&mut self, rows: Vec<SubagentActivityRow>) {
        self.rows = rows;
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width < 8 || self.rows.is_empty() {
            return Vec::new();
        }

        let active_count = self.rows.iter().filter(|row| row.state.is_active()).count();
        let total = self.rows.len();
        let status_text = if active_count == 0 {
            format!(" {total} done")
        } else if active_count == total {
            format!(" {active_count} active")
        } else {
            format!(" {active_count} active, {} done", total - active_count)
        };

        let mut lines = vec![truncate_line_with_ellipsis_if_overflow(
            vec!["• ".dim(), "Subagents".bold(), status_text.dim()].into(),
            usize::from(width),
        )];

        for row in self.rows.iter().take(MAX_VISIBLE_AGENTS) {
            lines.push(render_row(row, width));
            if let Some(detail) = row
                .detail
                .as_ref()
                .filter(|detail| !detail.trim().is_empty())
            {
                lines.push(render_detail(detail, width));
            }
        }

        let hidden = self.rows.len().saturating_sub(MAX_VISIBLE_AGENTS);
        if hidden > 0 {
            lines.push(Line::from(format!("  +{hidden} more agents").dim()));
        }

        lines
    }
}

fn render_row(row: &SubagentActivityRow, width: u16) -> Line<'static> {
    let mut spans = vec![
        row.state.prefix(),
        Span::from(row.label.clone()).cyan().bold(),
        " · ".dim(),
        row.state.summary_style(&row.summary),
    ];
    if let Some(token_summary) = row
        .token_summary
        .as_ref()
        .filter(|token_summary| !token_summary.trim().is_empty())
    {
        spans.push(" · ".dim());
        spans.push(token_summary.clone().dim());
    }

    truncate_line_with_ellipsis_if_overflow(Line::from(spans), usize::from(width))
}

fn render_detail(detail: &str, width: u16) -> Line<'static> {
    truncate_line_with_ellipsis_if_overflow(
        vec!["    ".dim(), detail.trim().to_string().dim()].into(),
        usize::from(width),
    )
}

impl Renderable for SubagentActivity {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }

        Paragraph::new(self.render_lines(area.width)).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.render_lines(width).len() as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn render(activity: &SubagentActivity, width: u16) -> String {
        let height = activity.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        activity.render(Rect::new(0, 0, width, height), &mut buf);
        format!("{buf:?}")
    }

    #[test]
    fn hides_empty_activity() {
        let activity = SubagentActivity::new();
        assert_eq!(activity.desired_height(/*width*/ 60), 0);
    }

    #[test]
    fn renders_live_subagent_rows() {
        let mut activity = SubagentActivity::new();
        activity.set_rows(vec![
            SubagentActivityRow {
                label: "Huygens [explorer]".to_string(),
                state: SubagentActivityState::Running,
                summary: "Search".to_string(),
                detail: Some("subagent in codex-rs/tui/src".to_string()),
                token_summary: Some("844K tokens".to_string()),
            },
            SubagentActivityRow {
                label: "Volta [explorer]".to_string(),
                state: SubagentActivityState::Completed,
                summary: "finished".to_string(),
                detail: Some("reviewed Claude AgentProgressLine".to_string()),
                token_summary: Some("232K tokens".to_string()),
            },
        ]);

        assert_snapshot!(
            "renders_live_subagent_rows",
            render(&activity, /*width*/ 68)
        );
    }

    #[test]
    fn caps_long_agent_lists() {
        let mut activity = SubagentActivity::new();
        activity.set_rows(
            (1..=5)
                .map(|index| SubagentActivityRow {
                    label: format!("Agent {index} [explorer]"),
                    state: SubagentActivityState::Running,
                    summary: "running".to_string(),
                    detail: None,
                    token_summary: None,
                })
                .collect(),
        );

        assert_snapshot!("caps_long_agent_lists", render(&activity, /*width*/ 44));
    }
}
