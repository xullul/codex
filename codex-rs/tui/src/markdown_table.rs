use itertools::Itertools;
use pulldown_cmark::Alignment;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

#[derive(Debug)]
pub(crate) struct TableState {
    alignments: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    after_table_lines: Vec<String>,
    current_row: Vec<String>,
    current_cell: String,
    pub(crate) in_header: bool,
    in_cell: bool,
}

impl TableState {
    pub(crate) fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
            after_table_lines: Vec::new(),
            current_row: Vec::new(),
            current_cell: String::new(),
            in_header: false,
            in_cell: false,
        }
    }

    pub(crate) fn start_row(&mut self) {
        self.current_row.clear();
    }

    pub(crate) fn start_cell(&mut self) {
        self.in_cell = true;
        self.current_cell.clear();
    }

    pub(crate) fn push_text(&mut self, text: &str) {
        if !self.in_cell {
            return;
        }
        self.current_cell.push_str(text);
    }

    pub(crate) fn end_cell(&mut self) {
        self.in_cell = false;
        self.current_row
            .push(normalize_table_cell(&self.current_cell));
        self.current_cell.clear();
    }

    pub(crate) fn end_row(&mut self) {
        let row = std::mem::take(&mut self.current_row);
        if self.in_header {
            self.header = row;
        } else if self.header.len() > 1
            && row.len() <= self.header.len()
            && row.iter().skip(1).all(String::is_empty)
            && row.first().is_some_and(|cell| !cell.is_empty())
        {
            self.after_table_lines.push(row[0].clone());
        } else if !row.iter().all(String::is_empty) {
            self.rows.push(row);
        }
    }
}

pub(crate) fn render_table(
    table: TableState,
    wrap_width: Option<usize>,
    code_style: Style,
) -> Vec<Line<'static>> {
    let column_count = std::iter::once(table.header.len())
        .chain(table.rows.iter().map(Vec::len))
        .max()
        .unwrap_or(0);
    if column_count == 0 {
        return Vec::new();
    }

    let mut rows = Vec::new();
    if !table.header.is_empty() {
        rows.push(padded_table_row(table.header.clone(), column_count));
    }
    rows.extend(
        table
            .rows
            .iter()
            .cloned()
            .map(|row| padded_table_row(row, column_count)),
    );

    let widths = table_column_widths(&rows, column_count);
    let aligned_width = widths.iter().sum::<usize>() + column_count.saturating_sub(1) * 3;
    let mut lines = if wrap_width.is_none_or(|width| aligned_width <= width) {
        render_aligned_table(&rows, &widths, &table.alignments, code_style)
    } else {
        render_vertical_table(&rows, code_style)
    };
    lines.extend(table.after_table_lines.into_iter().map(Line::from));
    lines
}

fn normalize_table_cell(cell: &str) -> String {
    cell.split_whitespace().join(" ")
}

fn padded_table_row(mut row: Vec<String>, column_count: usize) -> Vec<String> {
    row.resize_with(column_count, String::new);
    row
}

fn table_column_widths(rows: &[Vec<String>], column_count: usize) -> Vec<usize> {
    let mut widths = vec![3; column_count];
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }
    widths
}

fn render_aligned_table(
    rows: &[Vec<String>],
    widths: &[usize],
    alignments: &[Alignment],
    code_style: Style,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for (row_idx, row) in rows.iter().enumerate() {
        if row_idx == 1 {
            out.push(render_table_rule(widths, alignments));
        }
        let mut spans = Vec::new();
        for (cell_idx, cell) in row.iter().enumerate() {
            if cell_idx > 0 {
                spans.push(" | ".dim());
            }
            let aligned = align_table_cell(
                cell,
                widths[cell_idx],
                alignments.get(cell_idx).copied().unwrap_or(Alignment::None),
            );
            let style = if row_idx == 0 {
                code_style.bold()
            } else {
                code_style
            };
            spans.push(Span::styled(aligned, style));
        }
        out.push(Line::from(spans));
    }
    out
}

fn render_table_rule(widths: &[usize], alignments: &[Alignment]) -> Line<'static> {
    let mut spans = Vec::new();
    for (idx, width) in widths.iter().enumerate() {
        if idx > 0 {
            spans.push("-+-".dim());
        }
        let rule = match alignments.get(idx).copied().unwrap_or(Alignment::None) {
            Alignment::Left => format!(":{:-<width$}", "", width = width.saturating_sub(1)),
            Alignment::Center if *width >= 2 => {
                format!(":{:-<width$}:", "", width = width.saturating_sub(2))
            }
            Alignment::Right => format!("{:->width$}:", "", width = width.saturating_sub(1)),
            Alignment::Center | Alignment::None => "-".repeat(*width),
        };
        spans.push(rule.dim());
    }
    Line::from(spans)
}

fn align_table_cell(cell: &str, width: usize, alignment: Alignment) -> String {
    let cell_width = UnicodeWidthStr::width(cell);
    if cell_width >= width {
        return cell.to_string();
    }
    let padding = width - cell_width;
    match alignment {
        Alignment::Right => format!("{}{}", " ".repeat(padding), cell),
        Alignment::Center => {
            let left = padding / 2;
            let right = padding - left;
            format!("{}{}{}", " ".repeat(left), cell, " ".repeat(right))
        }
        Alignment::Left | Alignment::None => format!("{}{}", cell, " ".repeat(padding)),
    }
}

fn render_vertical_table(rows: &[Vec<String>], code_style: Style) -> Vec<Line<'static>> {
    let Some((headers, body_rows)) = rows.split_first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (row_idx, row) in body_rows.iter().enumerate() {
        if row_idx > 0 {
            out.push(Line::default());
        }
        for (idx, cell) in row.iter().enumerate() {
            let header = headers
                .get(idx)
                .filter(|header| !header.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("Column {}", idx + 1));
            out.push(Line::from(vec![
                Span::styled(header, code_style.bold()),
                ": ".dim(),
                Span::styled(cell.clone(), code_style),
            ]));
        }
    }
    if out.is_empty() {
        out.push(Line::from(
            Itertools::intersperse(
                headers
                    .iter()
                    .cloned()
                    .map(|header| Span::styled(header, code_style.bold())),
                " | ".dim(),
            )
            .collect::<Vec<_>>(),
        ));
    }
    out
}
