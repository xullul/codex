//! Terminal progress output helpers for the TUI.
//!
//! This module owns the low-level OSC 9;4 write path and terminal capability
//! detection for ambient progress. Callers provide the lifecycle state and cache
//! the last emitted value so redraws do not repeatedly write escape sequences.

use std::fmt;
use std::io;
use std::io::IsTerminal;
use std::io::stdout;

use codex_terminal_detection::Multiplexer;
use codex_terminal_detection::TerminalInfo;
use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use crossterm::Command;
use ratatui::crossterm::execute;

const ITERM2_MIN_PROGRESS_VERSION: &[u64] = &[3, 6, 6];
const GHOSTTY_MIN_PROGRESS_VERSION: &[u64] = &[1, 2, 0];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalProgressState {
    Indeterminate,
    Clear,
}

pub(crate) fn supports_terminal_progress_for_current_terminal() -> bool {
    supports_terminal_progress(
        &terminal_info(),
        std::env::var_os("ConEmuANSI").is_some() || std::env::var_os("ConEmuPID").is_some(),
    )
}

pub(crate) fn terminal_progress_uses_tmux_passthrough() -> bool {
    matches!(terminal_info().multiplexer, Some(Multiplexer::Tmux { .. }))
}

pub(crate) fn emit_terminal_progress(
    state: TerminalProgressState,
    dcs_passthrough: bool,
) -> io::Result<()> {
    if !stdout().is_terminal() {
        return Ok(());
    }

    execute!(
        stdout(),
        SetTerminalProgress {
            state,
            dcs_passthrough,
        }
    )
}

pub(crate) fn clear_terminal_progress_if_supported() -> io::Result<()> {
    if supports_terminal_progress_for_current_terminal() {
        emit_terminal_progress(
            TerminalProgressState::Clear,
            terminal_progress_uses_tmux_passthrough(),
        )?;
    }
    Ok(())
}

fn supports_terminal_progress(terminal: &TerminalInfo, conemu_detected: bool) -> bool {
    if terminal.name == TerminalName::WindowsTerminal {
        return false;
    }

    match terminal.name {
        TerminalName::Ghostty => terminal
            .version
            .as_deref()
            .is_some_and(|version| version_at_least(version, GHOSTTY_MIN_PROGRESS_VERSION)),
        TerminalName::Iterm2 => terminal
            .version
            .as_deref()
            .is_some_and(|version| version_at_least(version, ITERM2_MIN_PROGRESS_VERSION)),
        TerminalName::AppleTerminal
        | TerminalName::WarpTerminal
        | TerminalName::VsCode
        | TerminalName::WezTerm
        | TerminalName::Kitty
        | TerminalName::Alacritty
        | TerminalName::Konsole
        | TerminalName::GnomeTerminal
        | TerminalName::Vte
        | TerminalName::Dumb
        | TerminalName::Unknown => conemu_detected,
        TerminalName::WindowsTerminal => false,
    }
}

fn version_at_least(version: &str, minimum: &[u64]) -> bool {
    let parsed = numeric_version_components(version);
    for (index, required) in minimum.iter().copied().enumerate() {
        let actual = parsed.get(index).copied().unwrap_or(0);
        if actual != required {
            return actual > required;
        }
    }
    true
}

fn numeric_version_components(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|segment| {
            segment
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .take_while(|digits| !digits.is_empty())
        .filter_map(|digits| digits.parse::<u64>().ok())
        .collect()
}

#[derive(Debug, Clone)]
struct SetTerminalProgress {
    state: TerminalProgressState,
    dcs_passthrough: bool,
}

impl Command for SetTerminalProgress {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        let sequence = match self.state {
            TerminalProgressState::Indeterminate => "\x1b]9;4;3;0\x07",
            TerminalProgressState::Clear => "\x1b]9;4;0;0\x07",
        };

        if self.dcs_passthrough {
            write!(
                f,
                "\x1bPtmux;{}\x1b\\",
                escape_tmux_dcs_passthrough_payload(sequence)
            )
        } else {
            f.write_str(sequence)
        }
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Err(std::io::Error::other(
            "tried to execute SetTerminalProgress using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

fn escape_tmux_dcs_passthrough_payload(payload: &str) -> String {
    payload.replace('\u{1b}', "\u{1b}\u{1b}")
}

#[cfg(test)]
mod tests {
    use codex_terminal_detection::TerminalInfo;
    use codex_terminal_detection::TerminalName;
    use crossterm::Command;
    use pretty_assertions::assert_eq;

    use super::SetTerminalProgress;
    use super::TerminalProgressState;
    use super::supports_terminal_progress;
    use super::version_at_least;

    fn test_terminal(name: TerminalName, version: Option<&str>) -> TerminalInfo {
        TerminalInfo {
            name,
            term_program: None,
            version: version.map(str::to_string),
            term: None,
            multiplexer: None,
        }
    }

    #[test]
    fn terminal_progress_indeterminate_writes_osc9_4_sequence() {
        let mut ansi = String::new();
        let command = SetTerminalProgress {
            state: TerminalProgressState::Indeterminate,
            dcs_passthrough: false,
        };

        command
            .write_ansi(&mut ansi)
            .expect("terminal progress command should format");

        assert_eq!(ansi, "\u{1b}]9;4;3;0\u{7}");
    }

    #[test]
    fn terminal_progress_clear_writes_osc9_4_sequence() {
        let mut ansi = String::new();
        let command = SetTerminalProgress {
            state: TerminalProgressState::Clear,
            dcs_passthrough: false,
        };

        command
            .write_ansi(&mut ansi)
            .expect("terminal progress command should format");

        assert_eq!(ansi, "\u{1b}]9;4;0;0\u{7}");
    }

    #[test]
    fn terminal_progress_wraps_tmux_passthrough() {
        let mut ansi = String::new();
        let command = SetTerminalProgress {
            state: TerminalProgressState::Indeterminate,
            dcs_passthrough: true,
        };

        command
            .write_ansi(&mut ansi)
            .expect("terminal progress command should format");

        assert_eq!(ansi, "\u{1b}Ptmux;\u{1b}\u{1b}]9;4;3;0\u{7}\u{1b}\\");
    }

    #[test]
    fn terminal_progress_supports_minimum_versions() {
        assert!(supports_terminal_progress(
            &test_terminal(TerminalName::Iterm2, Some("3.6.6")),
            /*conemu_detected*/ false,
        ));
        assert!(supports_terminal_progress(
            &test_terminal(TerminalName::Iterm2, Some("3.7.0")),
            /*conemu_detected*/ false,
        ));
        assert!(supports_terminal_progress(
            &test_terminal(TerminalName::Ghostty, Some("1.2.0")),
            /*conemu_detected*/ false,
        ));
        assert!(supports_terminal_progress(
            &test_terminal(TerminalName::Ghostty, Some("1.2.1")),
            /*conemu_detected*/ false,
        ));
    }

    #[test]
    fn terminal_progress_rejects_old_or_unknown_versions() {
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::Iterm2, Some("3.6.5")),
            /*conemu_detected*/ false,
        ));
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::Iterm2, None),
            /*conemu_detected*/ false,
        ));
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::Ghostty, Some("1.1.9")),
            /*conemu_detected*/ false,
        ));
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::Ghostty, None),
            /*conemu_detected*/ false,
        ));
    }

    #[test]
    fn terminal_progress_excludes_windows_terminal() {
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::WindowsTerminal, None),
            /*conemu_detected*/ true,
        ));
    }

    #[test]
    fn terminal_progress_supports_conemu_envs() {
        assert!(supports_terminal_progress(
            &test_terminal(TerminalName::Unknown, None),
            /*conemu_detected*/ true,
        ));
        assert!(!supports_terminal_progress(
            &test_terminal(TerminalName::Unknown, None),
            /*conemu_detected*/ false,
        ));
    }

    #[test]
    fn terminal_progress_rejects_unsupported_terminals() {
        for name in [
            TerminalName::AppleTerminal,
            TerminalName::WarpTerminal,
            TerminalName::VsCode,
            TerminalName::WezTerm,
            TerminalName::Kitty,
            TerminalName::Alacritty,
            TerminalName::Konsole,
            TerminalName::GnomeTerminal,
            TerminalName::Vte,
            TerminalName::Dumb,
        ] {
            assert!(!supports_terminal_progress(
                &test_terminal(name, None),
                /*conemu_detected*/ false,
            ));
        }
    }

    #[test]
    fn terminal_progress_version_comparison_uses_numeric_prefixes() {
        assert!(version_at_least("3.6.6beta", &[3, 6, 6]));
        assert!(version_at_least("3.10", &[3, 6, 6]));
        assert!(!version_at_least("3.6a", &[3, 6, 6]));
    }
}
