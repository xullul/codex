use codex_protocol::protocol::McpInvocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpToolStatusSummary {
    pub(crate) header: String,
    pub(crate) details: Option<String>,
}

pub(crate) fn mcp_tool_status_summary(invocation: &McpInvocation) -> McpToolStatusSummary {
    McpToolStatusSummary {
        header: format!("Using {}", format_mcp_tool_name(invocation)),
        details: format_mcp_arguments(invocation),
    }
}

pub(crate) fn combine_mcp_tool_status_summaries(
    mut summaries: Vec<McpToolStatusSummary>,
) -> Option<McpToolStatusSummary> {
    if summaries.len() <= 1 {
        return summaries.pop();
    }

    let primary = summaries.remove(0);
    let extra_count = summaries.len();
    let mut detail_lines = Vec::new();
    if let Some(detail) = primary.details {
        detail_lines.push(detail);
    }
    if let Some(extra) = summaries
        .first()
        .map(format_mcp_summary_detail)
        .filter(|detail| !detail.is_empty())
    {
        detail_lines.push(format!("Also active: {extra}"));
    }
    if extra_count > 1 {
        detail_lines.push(format!("+{} more active", extra_count - 1));
    }

    Some(McpToolStatusSummary {
        header: primary.header,
        details: if detail_lines.is_empty() {
            None
        } else {
            Some(detail_lines.join("\n"))
        },
    })
}

fn format_mcp_tool_name(invocation: &McpInvocation) -> String {
    format!("{}.{}", invocation.server, invocation.tool)
}

fn format_mcp_arguments(invocation: &McpInvocation) -> Option<String> {
    invocation
        .arguments
        .as_ref()
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| value.to_string()))
}

fn format_mcp_summary_detail(summary: &McpToolStatusSummary) -> String {
    let tool_name = summary
        .header
        .strip_prefix("Using ")
        .unwrap_or(&summary.header);
    match summary.details.as_deref() {
        Some(details) => format!("{tool_name} {details}"),
        None => tool_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn invocation(server: &str, tool: &str, arguments: Option<serde_json::Value>) -> McpInvocation {
        McpInvocation {
            server: server.to_string(),
            tool: tool.to_string(),
            arguments,
        }
    }

    #[test]
    fn renders_compact_invocation_detail() {
        assert_eq!(
            mcp_tool_status_summary(&invocation(
                "github",
                "search",
                Some(json!({"query":"status row"})),
            )),
            McpToolStatusSummary {
                header: "Using github.search".to_string(),
                details: Some(r#"{"query":"status row"}"#.to_string()),
            }
        );
    }

    #[test]
    fn combines_parallel_mcp_activity() {
        assert_eq!(
            combine_mcp_tool_status_summaries(vec![
                mcp_tool_status_summary(&invocation("github", "search", None)),
                mcp_tool_status_summary(&invocation("figma", "inspect", None)),
                mcp_tool_status_summary(&invocation("drive", "read", None)),
            ]),
            Some(McpToolStatusSummary {
                header: "Using github.search".to_string(),
                details: Some("Also active: figma.inspect\n+1 more active".to_string()),
            })
        );
    }
}
