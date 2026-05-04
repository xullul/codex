use crate::JsonSchema;
use crate::ResponsesApiTool;
use crate::ToolSpec;
use serde_json::json;
use std::collections::BTreeMap;

pub fn create_list_dir_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "dir_path".to_string(),
            JsonSchema::string(Some("Absolute path to the directory to list.".to_string())),
        ),
        (
            "offset".to_string(),
            JsonSchema::number(Some(
                "The entry number to start listing from. Must be 1 or greater.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some("The maximum number of entries to return.".to_string())),
        ),
        (
            "depth".to_string(),
            JsonSchema::number(Some(
                "The maximum directory depth to traverse. Must be 1 or greater.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "list_dir".to_string(),
        description:
            "Lists entries in a local directory with 1-indexed entry numbers and simple type labels."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["dir_path".to_string()]), Some(false.into())),
        output_schema: None,
    })
}

pub fn create_repo_search_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "query".to_string(),
            JsonSchema::string(Some(
                "Pattern to search for in content mode. Treated as a ripgrep regex; required when search_mode is `content`.".to_string(),
            )),
        ),
        (
            "search_mode".to_string(),
            JsonSchema::string_enum(
                vec![json!("content"), json!("paths")],
                Some(
                    "Search content with a ripgrep regex, or list file paths matching `glob`. Defaults to `content`."
                        .to_string(),
                ),
            ),
        ),
        (
            "path".to_string(),
            JsonSchema::string(Some(
                "Optional file or directory path to search, relative to the turn cwd or absolute."
                    .to_string(),
            )),
        ),
        (
            "glob".to_string(),
            JsonSchema::string(Some(
                "Optional ripgrep glob such as `*.rs` or `src/**/*.ts`; required when search_mode is `paths`.".to_string(),
            )),
        ),
        (
            "context_lines".to_string(),
            JsonSchema::number(Some(
                "Optional number of context lines before and after each match.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some(
                "Maximum number of matching lines or paths to return.".to_string(),
            )),
        ),
        (
            "offset".to_string(),
            JsonSchema::number(Some(
                "Number of matching lines to skip before returning results.".to_string(),
            )),
        ),
        (
            "files_only".to_string(),
            JsonSchema::boolean(Some(
                "When true in content mode, return only file paths with matching content.".to_string(),
            )),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "repo_search".to_string(),
        description:
            "Searches repository files with ripgrep-style matching. Read-only, line-numbered, bounded, and policy-aware."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(
            properties,
            /*required*/ None,
            Some(false.into()),
        ),
        output_schema: None,
    })
}

pub fn create_repo_read_tool() -> ToolSpec {
    let properties = BTreeMap::from([
        (
            "path".to_string(),
            JsonSchema::string(Some(
                "File path to read, relative to the turn cwd or absolute.".to_string(),
            )),
        ),
        (
            "offset".to_string(),
            JsonSchema::number(Some(
                "1-indexed line number to start reading from.".to_string(),
            )),
        ),
        (
            "limit".to_string(),
            JsonSchema::number(Some("Maximum number of lines to return.".to_string())),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "repo_read".to_string(),
        description:
            "Reads a bounded range of a text file with line numbers after filesystem deny-read checks."
                .to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, Some(vec!["path".to_string()]), Some(false.into())),
        output_schema: None,
    })
}

pub fn create_test_sync_tool() -> ToolSpec {
    let barrier_properties = BTreeMap::from([
        (
            "id".to_string(),
            JsonSchema::string(Some(
                "Identifier shared by concurrent calls that should rendezvous".to_string(),
            )),
        ),
        (
            "participants".to_string(),
            JsonSchema::number(Some(
                "Number of tool calls that must arrive before the barrier opens".to_string(),
            )),
        ),
        (
            "timeout_ms".to_string(),
            JsonSchema::number(Some(
                "Maximum time in milliseconds to wait at the barrier".to_string(),
            )),
        ),
    ]);

    let properties = BTreeMap::from([
        (
            "sleep_before_ms".to_string(),
            JsonSchema::number(Some(
                "Optional delay in milliseconds before any other action".to_string(),
            )),
        ),
        (
            "sleep_after_ms".to_string(),
            JsonSchema::number(Some(
                "Optional delay in milliseconds after completing the barrier".to_string(),
            )),
        ),
        (
            "barrier".to_string(),
            JsonSchema::object(
                barrier_properties,
                Some(vec!["id".to_string(), "participants".to_string()]),
                Some(false.into()),
            ),
        ),
    ]);

    ToolSpec::Function(ResponsesApiTool {
        name: "test_sync_tool".to_string(),
        description: "Internal synchronization helper used by Codex integration tests.".to_string(),
        strict: false,
        defer_loading: None,
        parameters: JsonSchema::object(properties, /*required*/ None, Some(false.into())),
        output_schema: None,
    })
}

#[cfg(test)]
#[path = "utility_tool_tests.rs"]
mod tests;
