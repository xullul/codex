use std::collections::HashMap;

use codex_protocol::parse_command::ParsedCommand;

use crate::powershell_syntax::matching_delimiter;
use crate::powershell_syntax::simple_variable_name;
use crate::powershell_syntax::split_command_tokens;
use crate::powershell_syntax::split_pipeline_parts;
use crate::powershell_syntax::split_top_level_statements;
use crate::powershell_syntax::strip_case_insensitive_keyword;
use crate::powershell_syntax::strip_wrapping_parens;

pub(crate) fn top_level_assignment(statement: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut chars = statement.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        if let Some(q) = quote {
            if ch == '`' {
                chars.next();
            } else if ch == q {
                if q == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.checked_sub(1)?,
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.checked_sub(1)?,
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.checked_sub(1)?,
            '=' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                return Some((&statement[..idx], &statement[idx + ch.len_utf8()..]));
            }
            _ => {}
        }
    }

    None
}

pub(crate) fn is_result_variable_projection_statement(
    statement: &str,
    command_result_variables: &HashMap<String, ParsedCommand>,
    is_formatting_helper: impl Fn(&str, &[String]) -> bool,
) -> bool {
    if command_result_variables.is_empty() {
        return false;
    }
    is_result_variable_pipeline_projection(
        statement,
        command_result_variables,
        &is_formatting_helper,
    ) || is_result_variable_foreach_projection(statement, command_result_variables)
        || is_result_variable_for_projection(statement, command_result_variables)
}

fn is_result_variable_pipeline_projection(
    statement: &str,
    command_result_variables: &HashMap<String, ParsedCommand>,
    is_formatting_helper: &impl Fn(&str, &[String]) -> bool,
) -> bool {
    let Some(parts) = split_pipeline_parts(statement) else {
        return false;
    };
    let [source, tail @ ..] = parts.as_slice() else {
        return false;
    };
    if !is_result_variable_expression(source, command_result_variables) {
        return false;
    }
    tail.iter().all(|part| {
        let Some(tokens) = split_command_tokens(strip_wrapping_parens(part.trim())) else {
            return false;
        };
        let Some((head, args)) = tokens.split_first() else {
            return false;
        };
        is_formatting_helper(&head.to_ascii_lowercase(), args)
    })
}

fn is_result_variable_expression(
    expression: &str,
    command_result_variables: &HashMap<String, ParsedCommand>,
) -> bool {
    let compact = expression
        .trim()
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    if simple_variable_name(&compact)
        .is_some_and(|name| command_result_variables.contains_key(&name))
    {
        return true;
    }
    let Some((variable, index)) = compact.split_once('[') else {
        return false;
    };
    index.ends_with(']')
        && simple_variable_name(variable)
            .is_some_and(|name| command_result_variables.contains_key(&name))
}

fn is_result_variable_foreach_projection(
    statement: &str,
    command_result_variables: &HashMap<String, ParsedCommand>,
) -> bool {
    let Some((header, body)) = parse_keyword_block(statement, "foreach") else {
        return false;
    };
    let mut parts = header.split_whitespace();
    let Some(loop_var) = parts.next().and_then(simple_variable_name) else {
        return false;
    };
    if !parts
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("in"))
    {
        return false;
    }
    let Some(source_var) = parts.next().and_then(simple_variable_name) else {
        return false;
    };
    if parts.next().is_some() || !command_result_variables.contains_key(&source_var) {
        return false;
    }
    is_projection_loop_body(&body, &[loop_var, source_var])
}

fn is_result_variable_for_projection(
    statement: &str,
    command_result_variables: &HashMap<String, ParsedCommand>,
) -> bool {
    let Some((header, body)) = parse_keyword_block(statement, "for") else {
        return false;
    };
    let compact = remove_ascii_whitespace(&header).to_ascii_lowercase();
    let parts = compact.split(';').collect::<Vec<_>>();
    let [initializer, condition, increment] = parts.as_slice() else {
        return false;
    };
    let Some((lhs, start)) = initializer.split_once('=') else {
        return false;
    };
    let Some(loop_var) = simple_variable_name(lhs) else {
        return false;
    };
    if start != "0" && start != "1" {
        return false;
    }
    if *increment != format!("${loop_var}++") {
        return false;
    }
    let Some(source_var) = for_loop_collection_var(condition, &loop_var) else {
        return false;
    };
    if !command_result_variables.contains_key(&source_var) {
        return false;
    }
    is_projection_loop_body(&body, &[loop_var, source_var])
}

fn parse_keyword_block(statement: &str, keyword: &str) -> Option<(String, String)> {
    let trimmed = statement.trim();
    let rest = strip_case_insensitive_keyword(trimmed, keyword)?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let header_end = matching_delimiter(rest, 0, '(', ')')?;
    let header = rest[1..header_end].trim().to_string();
    let rest = rest[header_end + 1..].trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let body_end = matching_delimiter(rest, 0, '{', '}')?;
    rest[body_end + 1..]
        .trim()
        .is_empty()
        .then(|| (header, rest[1..body_end].trim().to_string()))
}

fn for_loop_collection_var(condition: &str, loop_var: &str) -> Option<String> {
    [format!("${loop_var}-lt"), format!("${loop_var}-le")]
        .into_iter()
        .find_map(|prefix| {
            let rest = condition.strip_prefix(&prefix)?;
            let (variable, member) = rest.split_once('.')?;
            let variable = simple_variable_name(variable)?;
            matches!(member, "count" | "length").then_some(variable)
        })
}

fn is_projection_loop_body(body: &str, allowed_vars: &[String]) -> bool {
    let Some(statements) = split_top_level_statements(body) else {
        return false;
    };
    !statements.is_empty()
        && statements
            .iter()
            .all(|statement| is_projection_expression(statement, allowed_vars))
}

fn is_projection_expression(statement: &str, allowed_vars: &[String]) -> bool {
    let compact = remove_ascii_whitespace(statement).to_ascii_lowercase();
    allowed_vars.iter().any(|variable| {
        compact == format!("${variable}")
            || compact.starts_with(&format!("${variable}."))
            || compact.starts_with(&format!("${variable}["))
            || compact == format!("write-output${variable}")
    })
}

fn remove_ascii_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}
