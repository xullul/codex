use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ParsedCommandActionKind {
    Inspect,
    Edit,
    Test,
    Build,
    Lint,
    Git,
    Wait,
    Run,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParsedCommand {
    Read {
        cmd: String,
        name: String,
        /// (Best effort) Path to the file being read by the command. When
        /// possible, this is an absolute path, though when relative, it should
        /// be resolved against the `cwd`` that will be used to run the command
        /// to derive the absolute path.
        path: PathBuf,
    },
    ListFiles {
        cmd: String,
        path: Option<String>,
    },
    Search {
        cmd: String,
        query: Option<String>,
        path: Option<String>,
    },
    Action {
        cmd: String,
        kind: ParsedCommandActionKind,
        detail: Option<String>,
    },
    Unknown {
        cmd: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn action_round_trips() {
        let command = ParsedCommand::Action {
            cmd: "cargo test -p codex-shell-command".to_string(),
            kind: ParsedCommandActionKind::Test,
            detail: Some("cargo test -p codex-shell-command".to_string()),
        };

        let json = serde_json::to_string(&command).expect("serialize parsed command action");
        assert_eq!(
            json,
            r#"{"type":"action","cmd":"cargo test -p codex-shell-command","kind":"test","detail":"cargo test -p codex-shell-command"}"#
        );
        assert_eq!(
            serde_json::from_str::<ParsedCommand>(&json)
                .expect("deserialize parsed command action"),
            command
        );
    }
}
