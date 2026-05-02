use std::path::Path;
use std::path::PathBuf;

use codex_app_server_protocol::CommandAction;
use codex_protocol::parse_command::ParsedCommand;
use codex_shell_command::parse_command::extract_shell_command;
use codex_shell_command::parse_command::parse_command;
use dirs::home_dir;
use shlex::try_join;

pub(crate) fn escape_command(command: &[String]) -> String {
    try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

pub(crate) fn strip_bash_lc_and_escape(command: &[String]) -> String {
    if let Some((_, script)) = extract_shell_command(command) {
        return script.to_string();
    }
    escape_command(command)
}

pub(crate) fn split_command_string(command: &str) -> Vec<String> {
    let Some(parts) = shlex::split(command) else {
        return vec![command.to_string()];
    };
    match shlex::try_join(parts.iter().map(String::as_str)) {
        Ok(round_trip)
            if round_trip == command
                || (!command.contains(":\\")
                    && shlex::split(&round_trip).as_ref() == Some(&parts)) =>
        {
            parts
        }
        _ => vec![command.to_string()],
    }
}

pub(crate) fn parsed_command_actions_from_item(
    command: &str,
    command_actions: Vec<CommandAction>,
) -> Vec<ParsedCommand> {
    let parsed = command_actions
        .into_iter()
        .map(CommandAction::into_core)
        .collect::<Vec<_>>();
    if parsed
        .iter()
        .any(|command| !matches!(command, ParsedCommand::Unknown { .. }))
    {
        return parsed;
    }

    let reparsed = parse_command(&split_command_string(command));
    if reparsed
        .iter()
        .any(|command| !matches!(command, ParsedCommand::Unknown { .. }))
    {
        reparsed
    } else {
        parsed
    }
}

/// If `path` is absolute and inside $HOME, return the part *after* the home
/// directory; otherwise, return the path as-is. Note if `path` is the homedir,
/// this will return and empty path.
pub(crate) fn relativize_to_home<P>(path: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    if !path.is_absolute() {
        // If the path is not absolute, we can’t do anything with it.
        return None;
    }

    let home_dir = home_dir()?;
    let rel = path.strip_prefix(&home_dir).ok()?;
    Some(rel.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_command() {
        let args = vec!["foo".into(), "bar baz".into(), "weird&stuff".into()];
        let cmdline = escape_command(&args);
        assert_eq!(cmdline, "foo 'bar baz' 'weird&stuff'");
    }

    #[test]
    fn test_strip_bash_lc_and_escape() {
        // Test bash
        let args = vec!["bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test zsh
        let args = vec!["zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to zsh
        let args = vec!["/usr/bin/zsh".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");

        // Test absolute path to bash
        let args = vec!["/bin/bash".into(), "-lc".into(), "echo hello".into()];
        let cmdline = strip_bash_lc_and_escape(&args);
        assert_eq!(cmdline, "echo hello");
    }

    #[test]
    fn split_command_string_round_trips_shell_wrappers() {
        let command =
            shlex::try_join(["/bin/zsh", "-lc", r#"python3 -c 'print("Hello, world!")'"#])
                .expect("round-trippable command");
        assert_eq!(
            split_command_string(&command),
            vec![
                "/bin/zsh".to_string(),
                "-lc".to_string(),
                r#"python3 -c 'print("Hello, world!")'"#.to_string(),
            ]
        );
    }

    #[test]
    fn split_command_string_preserves_non_roundtrippable_windows_commands() {
        let command = r#"C:\Program Files\Git\bin\bash.exe -lc "echo hi""#;
        assert_eq!(split_command_string(command), vec![command.to_string()]);
    }

    #[test]
    fn parsed_command_actions_falls_back_to_command_string_for_unknown_powershell() {
        let command = shlex::try_join([
            r#"C:\Program Files\PowerShell\7\pwsh.exe"#,
            "-Command",
            "Get-Content src/main.rs",
        ])
        .expect("round-trippable command");

        assert_eq!(
            parsed_command_actions_from_item(
                &command,
                vec![CommandAction::Unknown {
                    command: command.clone()
                }]
            ),
            vec![ParsedCommand::Read {
                cmd: "Get-Content src/main.rs".to_string(),
                name: "main.rs".to_string(),
                path: PathBuf::from("src/main.rs"),
            }]
        );
    }
}
