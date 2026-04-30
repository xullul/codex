//! Compact live checklist for `update_plan` progress.

use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::render::renderable::Renderable;
use crate::wrapping::RtOptions;
use crate::wrapping::adaptive_wrap_lines;

const MAX_VISIBLE_STEPS: usize = 4;

pub(crate) struct PlanChecklist {
    plan: Vec<PlanItemArg>,
}

impl PlanChecklist {
    pub(crate) fn new() -> Self {
        Self { plan: Vec::new() }
    }

    pub(crate) fn set_plan(&mut self, plan: Vec<PlanItemArg>) {
        self.plan = plan;
    }

    pub(crate) fn clear(&mut self) {
        self.plan.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.visible_plan_items().is_none()
    }

    fn visible_plan_items(&self) -> Option<(usize, usize, usize, Vec<&PlanItemArg>)> {
        let total = self.plan.len();
        if total == 0 {
            return None;
        }

        let completed = self
            .plan
            .iter()
            .filter(|item| matches!(item.status, StepStatus::Completed))
            .count();
        let focus_index = self
            .plan
            .iter()
            .position(|item| matches!(item.status, StepStatus::InProgress))
            .or_else(|| {
                self.plan
                    .iter()
                    .position(|item| !matches!(item.status, StepStatus::Completed))
            })
            .or_else(|| total.checked_sub(1))
            .unwrap_or(0);
        let start = if total.saturating_sub(focus_index) < MAX_VISIBLE_STEPS {
            total.saturating_sub(MAX_VISIBLE_STEPS)
        } else {
            focus_index.saturating_sub(1)
        };
        let items = self
            .plan
            .iter()
            .skip(start)
            .take(MAX_VISIBLE_STEPS)
            .collect();

        Some((completed, total, start, items))
    }

    fn render_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width < 8 {
            return Vec::new();
        }
        let Some((completed, total, start, items)) = self.visible_plan_items() else {
            return Vec::new();
        };

        let mut lines = vec![Line::from(vec![
            "• ".dim(),
            "Plan".bold(),
            format!(" {completed}/{total} complete").dim(),
        ])];
        for item in &items {
            lines.extend(wrap_step(item, width));
        }

        let hidden_before = start;
        let hidden_after = total.saturating_sub(start + items.len());
        let hidden_summary = match (hidden_before, hidden_after) {
            (0, 0) => None,
            (0, count) => Some(format!("  +{count} more steps")),
            (count, 0) => Some(format!("  +{count} earlier steps")),
            (before, after) => Some(format!("  +{before} earlier, +{after} later")),
        };
        if let Some(summary) = hidden_summary {
            lines.push(Line::from(summary.dim()));
        }

        lines
    }
}

fn wrap_step(item: &PlanItemArg, width: u16) -> Vec<Line<'static>> {
    let (initial_indent, subsequent_indent) = match item.status {
        StepStatus::Completed => (Line::from("  ✓ ".dim()), Line::from("    ")),
        StepStatus::InProgress => (Line::from("  › ".cyan().bold()), Line::from("    ")),
        StepStatus::Pending => (Line::from("  - ".dim()), Line::from("    ")),
    };
    let opts = RtOptions::new(width as usize)
        .initial_indent(initial_indent)
        .subsequent_indent(subsequent_indent)
        .break_words(/*break_words*/ true);

    adaptive_wrap_lines(
        item.step.lines().map(|line| match item.status {
            StepStatus::Completed => Line::from(line.to_string().dim()),
            StepStatus::InProgress => Line::from(line.to_string().cyan().bold()),
            StepStatus::Pending => Line::from(line.to_string().dim()),
        }),
        opts,
    )
}

impl Renderable for PlanChecklist {
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

    fn item(step: &str, status: StepStatus) -> PlanItemArg {
        PlanItemArg {
            step: step.to_string(),
            status,
        }
    }

    fn render(checklist: &PlanChecklist, width: u16) -> String {
        let height = checklist.desired_height(width);
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        checklist.render(Rect::new(0, 0, width, height), &mut buf);
        format!("{buf:?}")
    }

    #[test]
    fn hides_empty_plan() {
        let checklist = PlanChecklist::new();
        assert_eq!(checklist.desired_height(/*width*/ 60), 0);
    }

    #[test]
    fn renders_completed_plan_until_cleared() {
        let mut checklist = PlanChecklist::new();

        checklist.set_plan(vec![
            item("Inspect Claude task rendering", StepStatus::Completed),
            item("Mirror completed task state", StepStatus::Completed),
        ]);

        assert_snapshot!(
            "renders_completed_plan_until_cleared",
            render(&checklist, /*width*/ 54)
        );

        checklist.clear();
        assert_eq!(checklist.desired_height(/*width*/ 60), 0);
    }

    #[test]
    fn renders_active_plan() {
        let mut checklist = PlanChecklist::new();
        checklist.set_plan(vec![
            item(
                "Explore the current TUI progress surfaces",
                StepStatus::Completed,
            ),
            item(
                "Surface the live checklist above the composer",
                StepStatus::InProgress,
            ),
            item("Update snapshots", StepStatus::Pending),
        ]);

        assert_snapshot!("renders_active_plan", render(&checklist, /*width*/ 64));
    }

    #[test]
    fn caps_long_plan_near_active_step() {
        let mut checklist = PlanChecklist::new();
        checklist.set_plan(vec![
            item("Step 1", StepStatus::Completed),
            item("Step 2", StepStatus::Completed),
            item("Step 3", StepStatus::Completed),
            item("Step 4", StepStatus::InProgress),
            item("Step 5", StepStatus::Pending),
            item("Step 6", StepStatus::Pending),
        ]);

        assert_snapshot!(
            "caps_long_plan_near_active_step",
            render(&checklist, /*width*/ 44)
        );
    }
}
