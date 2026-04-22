use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const RATATUI_REFLOW_ACCUMULATOR_LIMIT: usize = u16::MAX as usize;
// ratatui's word reflow currently accumulates word widths in u16. Keep each preflight chunk
// comfortably below that limit so the next grapheme and surrounding whitespace cannot overflow
// the accumulator before ratatui decides to wrap.
const RATATUI_REFLOW_SAFE_LINE_WIDTH: usize = RATATUI_REFLOW_ACCUMULATOR_LIMIT / 4;

pub(crate) fn ratatui_reflow_safe_width(width: u16) -> u16 {
    width.min(RATATUI_REFLOW_SAFE_LINE_WIDTH as u16)
}

pub(crate) fn ratatui_reflow_safe_paragraph(
    lines: Vec<Line<'static>>,
    width: u16,
) -> Paragraph<'static> {
    let lines = if width > ratatui_reflow_safe_width(width) {
        ratatui_reflow_safe_lines(lines)
    } else {
        lines
    };
    Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false })
}

pub(crate) fn ratatui_reflow_safe_line_count(lines: Vec<Line<'static>>, width: u16) -> usize {
    Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .line_count(ratatui_reflow_safe_width(width))
}

fn ratatui_reflow_safe_lines(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        push_ratatui_reflow_safe_line(line, &mut out);
    }
    out
}

fn push_ratatui_reflow_safe_line(line: Line<'static>, out: &mut Vec<Line<'static>>) {
    if line.width() <= RATATUI_REFLOW_SAFE_LINE_WIDTH {
        out.push(line);
        return;
    }

    let style = line.style;
    let alignment = line.alignment;
    let mut current = Line {
        spans: Vec::new(),
        style,
        alignment,
    };
    let mut current_width = 0usize;

    for span in line.spans {
        let span_style = span.style;
        let mut pending = String::new();
        for grapheme in span.content.as_ref().graphemes(/*is_extended*/ true) {
            let grapheme_width = grapheme.width();
            if current_width > 0
                && grapheme_width > 0
                && current_width + grapheme_width > RATATUI_REFLOW_SAFE_LINE_WIDTH
            {
                push_pending_span(&mut current, &mut pending, span_style);
                out.push(current);
                current = Line {
                    spans: Vec::new(),
                    style,
                    alignment,
                };
                current_width = 0;
            }
            pending.push_str(grapheme);
            current_width += grapheme_width;
        }
        push_pending_span(&mut current, &mut pending, span_style);
    }

    out.push(current);
}

fn push_pending_span(line: &mut Line<'static>, pending: &mut String, style: Style) {
    if !pending.is_empty() {
        line.spans
            .push(Span::styled(std::mem::take(pending), style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools as _;
    use pretty_assertions::assert_eq;

    #[test]
    fn safe_lines_split_huge_tokens_below_accumulator_limit() {
        let line = Line::from("x".repeat(RATATUI_REFLOW_ACCUMULATOR_LIMIT + 1_000));
        let out = ratatui_reflow_safe_lines(vec![line]);
        let widths = out.iter().map(Line::width).collect_vec();

        assert_eq!(widths, vec![16_383, 16_383, 16_383, 16_383, 1_003]);
    }

    #[test]
    fn safe_line_count_handles_huge_wide_token_at_max_width() {
        let line = Line::from("界".repeat(40_000));

        assert_eq!(ratatui_reflow_safe_line_count(vec![line], u16::MAX), 5);
    }

    #[test]
    fn safe_line_count_preserves_normal_width_wrapping() {
        let line = Line::from("x".repeat(70_000));
        let width = 80;

        assert_eq!(
            ratatui_reflow_safe_paragraph(vec![line.clone()], width).line_count(width),
            875
        );
        assert_eq!(ratatui_reflow_safe_line_count(vec![line], width), 875);
    }
}
