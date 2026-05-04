use super::*;
use crate::JsonSchema;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;

#[test]
fn list_dir_tool_matches_expected_spec() {
    assert_eq!(
        create_list_dir_tool(),
        ToolSpec::Function(ResponsesApiTool {
            name: "list_dir".to_string(),
            description:
                "Lists entries in a local directory with 1-indexed entry numbers and simple type labels."
                    .to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::from([
                    (
                        "depth".to_string(),
                        JsonSchema::number(Some(
                            "The maximum directory depth to traverse. Must be 1 or greater."
                                .to_string(),
                        )),
                    ),
                    (
                        "dir_path".to_string(),
                        JsonSchema::string(Some(
                            "Absolute path to the directory to list.".to_string(),
                        )),
                    ),
                    (
                        "limit".to_string(),
                        JsonSchema::number(Some(
                            "The maximum number of entries to return.".to_string(),
                        )),
                    ),
                    (
                        "offset".to_string(),
                        JsonSchema::number(Some(
                            "The entry number to start listing from. Must be 1 or greater."
                                .to_string(),
                        )),
                    ),
                ]), Some(vec!["dir_path".to_string()]), Some(false.into())),
            output_schema: None,
        })
    );
}

#[test]
fn repo_search_tool_matches_expected_spec() {
    let ToolSpec::Function(tool) = create_repo_search_tool() else {
        panic!("repo_search should be a function tool");
    };

    assert_eq!(tool.name, "repo_search");
    assert!(tool.description.contains("Searches repository files"));
    let properties = tool.parameters.properties.expect("repo_search properties");
    assert_eq!(tool.parameters.required, None);
    assert!(properties.contains_key("query"));
    assert!(properties.contains_key("search_mode"));
    assert!(properties.contains_key("glob"));
    assert!(properties.contains_key("context_lines"));
    assert!(properties.contains_key("files_only"));
}

#[test]
fn repo_read_tool_matches_expected_spec() {
    let ToolSpec::Function(tool) = create_repo_read_tool() else {
        panic!("repo_read should be a function tool");
    };

    assert_eq!(tool.name, "repo_read");
    assert!(tool.description.contains("Reads a bounded range"));
    let properties = tool.parameters.properties.expect("repo_read properties");
    let required = tool.parameters.required;
    assert_eq!(required, Some(vec!["path".to_string()]));
    assert!(properties.contains_key("path"));
    assert!(properties.contains_key("offset"));
    assert!(properties.contains_key("limit"));
}

#[test]
fn test_sync_tool_matches_expected_spec() {
    assert_eq!(
        create_test_sync_tool(),
        ToolSpec::Function(ResponsesApiTool {
            name: "test_sync_tool".to_string(),
            description: "Internal synchronization helper used by Codex integration tests."
                .to_string(),
            strict: false,
            defer_loading: None,
            parameters: JsonSchema::object(BTreeMap::from([
                    (
                        "barrier".to_string(),
                        JsonSchema::object(
                            BTreeMap::from([
                                (
                                    "id".to_string(),
                                    JsonSchema::string(Some(
                                        "Identifier shared by concurrent calls that should rendezvous"
                                            .to_string(),
                                    )),
                                ),
                                (
                                    "participants".to_string(),
                                    JsonSchema::number(Some(
                                        "Number of tool calls that must arrive before the barrier opens"
                                            .to_string(),
                                    )),
                                ),
                                (
                                    "timeout_ms".to_string(),
                                    JsonSchema::number(Some(
                                        "Maximum time in milliseconds to wait at the barrier"
                                            .to_string(),
                                    )),
                                ),
                            ]),
                            Some(vec!["id".to_string(), "participants".to_string()]),
                            Some(false.into()),
                        ),
                    ),
                    (
                        "sleep_after_ms".to_string(),
                        JsonSchema::number(Some(
                            "Optional delay in milliseconds after completing the barrier"
                                .to_string(),
                        )),
                    ),
                    (
                        "sleep_before_ms".to_string(),
                        JsonSchema::number(Some(
                            "Optional delay in milliseconds before any other action".to_string(),
                        )),
                    ),
                ]), /*required*/ None, Some(false.into())),
            output_schema: None,
        })
    );
}
