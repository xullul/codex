use std::borrow::Cow;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExecOutputTail {
    partial_line: String,
    recent_lines: Vec<String>,
    total_lines: usize,
}

impl ExecOutputTail {
    pub(crate) fn append(&mut self, chunk: &[u8]) -> bool {
        if chunk.is_empty() {
            return false;
        }

        let before = self.clone();
        let text = String::from_utf8_lossy(chunk).replace('\r', "\n");
        for segment in text.split_inclusive('\n') {
            if let Some(line) = segment.strip_suffix('\n') {
                self.partial_line.push_str(line);
                self.push_line();
            } else {
                self.partial_line.push_str(segment);
            }
        }

        *self != before
    }

    pub(crate) fn append_to_details(
        &self,
        details: Option<String>,
        max_lines: usize,
    ) -> Option<String> {
        let mut lines: Vec<String> = details
            .as_deref()
            .map(|details| details.lines().map(str::to_string).collect())
            .unwrap_or_default();
        let remaining = max_lines.saturating_sub(lines.len());
        if remaining > 0 {
            lines.extend(self.status_lines(remaining));
        }

        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    fn status_lines(&self, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }

        let mut lines = self.recent_lines.clone();
        let partial = sanitize_output_line(&self.partial_line);
        if !partial.is_empty() {
            lines.push(partial);
        }
        if lines.is_empty() {
            return Vec::new();
        }

        let display_count = self.display_line_count();
        let tail_start = lines.len().saturating_sub(limit);
        lines[tail_start..]
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                if idx == 0 {
                    let hidden_count = display_count.saturating_sub(limit);
                    if hidden_count > 0 {
                        format!("Output (+{hidden_count} lines): {line}")
                    } else {
                        format!("Output: {line}")
                    }
                } else {
                    format!("        {line}")
                }
            })
            .collect()
    }

    fn display_line_count(&self) -> usize {
        self.total_lines + usize::from(!self.partial_line.trim().is_empty())
    }

    fn push_line(&mut self) {
        self.total_lines = self.total_lines.saturating_add(1);
        let line = sanitize_output_line(&self.partial_line);
        self.partial_line.clear();
        if line.is_empty() {
            return;
        }

        self.recent_lines.push(truncate_line(line));
        const MAX_RECENT_LINES: usize = 5;
        if self.recent_lines.len() > MAX_RECENT_LINES {
            let drop_count = self.recent_lines.len() - MAX_RECENT_LINES;
            self.recent_lines.drain(0..drop_count);
        }
    }
}

fn sanitize_output_line(line: &str) -> String {
    let stripped = strip_ansi_sequences(line);
    stripped
        .chars()
        .filter(|ch| *ch == '\t' || !ch.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

fn strip_ansi_sequences(line: &str) -> Cow<'_, str> {
    if !line.contains('\u{1b}') {
        return Cow::Borrowed(line);
    }

    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for seq_ch in chars.by_ref() {
                    if ('@'..='~').contains(&seq_ch) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(seq_ch) = chars.next() {
                    if seq_ch == '\u{7}' {
                        break;
                    }
                    if seq_ch == '\u{1b}' && chars.peek().copied() == Some('\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    Cow::Owned(out)
}

fn truncate_line(line: String) -> String {
    const MAX_CHARS: usize = 160;
    let mut chars = line.chars();
    let truncated: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn appends_tail_lines_to_existing_details() {
        let mut tail = ExecOutputTail::default();
        assert!(tail.append(b"Compiling codex-tui\nrunning 1819 tests\n"));

        assert_eq!(
            tail.append_to_details(Some("cargo test -p codex-tui".to_string()), 3),
            Some(
                "cargo test -p codex-tui\nOutput: Compiling codex-tui\n        running 1819 tests"
                    .to_string()
            )
        );
    }

    #[test]
    fn reports_hidden_output_lines_when_tail_is_capped() {
        let mut tail = ExecOutputTail::default();
        assert!(tail.append(b"one\ntwo\nthree\nfour\n"));

        assert_eq!(
            tail.append_to_details(Some("cargo test".to_string()), 2),
            Some("cargo test\nOutput (+3 lines): four".to_string())
        );
    }

    #[test]
    fn preserves_partial_line_for_live_progress() {
        let mut tail = ExecOutputTail::default();
        assert!(tail.append(b"Downloading crate"));

        assert_eq!(
            tail.append_to_details(None, 2),
            Some("Output: Downloading crate".to_string())
        );

        assert!(tail.append(b"s\nFinished\n"));
        assert_eq!(
            tail.append_to_details(None, 2),
            Some("Output: Downloading crates\n        Finished".to_string())
        );
    }

    #[test]
    fn strips_ansi_sequences_from_progress_lines() {
        let mut tail = ExecOutputTail::default();
        assert!(tail.append(b"\x1b[32mok\x1b[0m\n"));

        assert_eq!(
            tail.append_to_details(None, 2),
            Some("Output: ok".to_string())
        );
    }
}
