use std::path::PathBuf;

use codex_protocol::parse_command::ParsedCommand;

use crate::parse_command::shlex_join;

pub(crate) fn summarize_line_range_preview(script: &str) -> Option<ParsedCommand> {
    let statements = split_top_level_statements(script)?;
    let (path_binding, lines_assignment, loop_statement) = match statements.as_slice() {
        [lines_assignment, loop_statement] => {
            (None, lines_assignment.as_str(), loop_statement.as_str())
        }
        [path_assignment, lines_assignment, loop_statement] => (
            Some(literal_variable_assignment(path_assignment)?),
            lines_assignment.as_str(),
            loop_statement.as_str(),
        ),
        _ => return None,
    };

    let (lines_var, read_target) = get_content_assignment(lines_assignment)?;
    if !is_line_range_preview_loop(loop_statement, &lines_var) {
        return None;
    }
    let path = resolve_read_target(path_binding.as_ref(), read_target);

    Some(ParsedCommand::Read {
        cmd: shlex_join(&["Get-Content".to_string(), path.clone()]),
        name: short_display_path(&path),
        path: PathBuf::from(path),
    })
}

enum ReadTarget {
    Literal(String),
    Variable { name: String, display_token: String },
}

fn split_top_level_statements(script: &str) -> Option<Vec<String>> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote: Option<char> = None;
    let mut chars = script.char_indices().peekable();

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
            ';' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                push_statement(script, start, idx, &mut statements);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if quote.is_some() || paren_depth != 0 || brace_depth != 0 || bracket_depth != 0 {
        return None;
    }
    push_statement(script, start, script.len(), &mut statements);
    Some(statements)
}

fn push_statement(script: &str, start: usize, end: usize, statements: &mut Vec<String>) {
    let statement = script[start..end].trim();
    if !statement.is_empty() {
        statements.push(statement.to_string());
    }
}

fn literal_variable_assignment(statement: &str) -> Option<(String, String)> {
    let (lhs, rhs) = split_assignment(statement)?;
    Some((
        assignment_name(lhs.trim())?,
        powershell_literal(rhs.trim())?,
    ))
}

fn get_content_assignment(statement: &str) -> Option<(String, ReadTarget)> {
    let (lhs, rhs) = split_assignment(statement)?;
    let target_var = assignment_name(lhs.trim())?;
    let tokens = split_command_tokens(strip_wrapping_parens(rhs.trim()))?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "get-content" | "gc" | "cat" | "type"
    ) {
        return None;
    }
    Some((target_var, get_content_path_operand(tail)?))
}

fn resolve_read_target(path_binding: Option<&(String, String)>, read_target: ReadTarget) -> String {
    match read_target {
        ReadTarget::Literal(path) => path,
        ReadTarget::Variable {
            name,
            display_token,
        } => {
            if let Some((bound_name, bound_path)) = path_binding
                && bound_name == &name
            {
                return bound_path.clone();
            }
            display_token
        }
    }
}

fn split_assignment(statement: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
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
            '=' => return Some((&statement[..idx], &statement[idx + ch.len_utf8()..])),
            _ => {}
        }
    }

    None
}

fn powershell_literal(value: &str) -> Option<String> {
    if let Some(inner) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Some(inner.replace("''", "'"));
    }
    if let Some(inner) = value.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return unescape_double_quoted_literal(inner);
    }
    is_plain_path_literal(value).then(|| value.to_string())
}

fn unescape_double_quoted_literal(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            return None;
        }
        if ch == '`' {
            out.push(chars.next()?);
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn is_plain_path_literal(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '$' | ';' | '|' | '&' | '<' | '>' | '(' | ')' | '{' | '}' | '[' | ']'
                )
        })
}

fn strip_wrapping_parens(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.starts_with('(')
        && matching_delimiter(trimmed, 0, '(', ')')
            .is_some_and(|idx| idx + ')'.len_utf8() == trimmed.len())
    {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    }
}

fn split_command_tokens(command: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == '`' {
                current.push(chars.next()?);
            } else if ch == q {
                if q == '\'' && chars.peek().is_some_and(|next| *next == '\'') {
                    chars.next();
                    current.push('\'');
                } else {
                    quote = None;
                }
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_ascii_whitespace() => push_token(&mut tokens, &mut current),
            '|' | ';' | '&' | '<' | '>' | '{' | '}' => return None,
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return None;
    }
    push_token(&mut tokens, &mut current);
    Some(tokens)
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

fn get_content_path_operand(args: &[String]) -> Option<ReadTarget> {
    let mut path_target = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, value)) = arg.split_once('=')
            && matches!(name.to_ascii_lowercase().as_str(), "-path" | "-literalpath")
        {
            path_target = set_single_path_target(path_target, value)?;
            i += 1;
            continue;
        }
        if matches!(lower.as_str(), "-path" | "-literalpath") {
            path_target = set_single_path_target(path_target, args.get(i + 1)?.as_str())?;
            i += 2;
            continue;
        }
        if flag_consumes_next_value(&lower) {
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        path_target = set_single_path_target(path_target, arg)?;
        i += 1;
    }
    path_target
}

fn set_single_path_target(current: Option<ReadTarget>, value: &str) -> Option<Option<ReadTarget>> {
    if current.is_some() {
        return None;
    }
    Some(Some(parse_read_target(value)?))
}

fn parse_read_target(value: &str) -> Option<ReadTarget> {
    if let Some(path) = powershell_literal(value) {
        return Some(ReadTarget::Literal(path));
    }
    let name = simple_variable_name(value)?;
    Some(ReadTarget::Variable {
        name,
        display_token: variable_display_token(value)?,
    })
}

fn variable_display_token(value: &str) -> Option<String> {
    if let Some(name) = value
        .strip_prefix("${")
        .and_then(|token| token.strip_suffix('}'))
    {
        normalize_variable_name(name)?;
        return Some(format!("${name}"));
    }
    value
        .strip_prefix('$')
        .filter(|name| normalize_variable_name(name).is_some())?;
    Some(value.to_string())
}

fn flag_consumes_next_value(flag: &str) -> bool {
    matches!(
        flag,
        "-encoding"
            | "-filter"
            | "-include"
            | "-exclude"
            | "-totalcount"
            | "-first"
            | "-last"
            | "-skip"
    )
}

fn is_line_range_preview_loop(statement: &str, lines_var: &str) -> bool {
    let Some((loop_var, body)) = parse_line_range_loop(statement, lines_var) else {
        return false;
    };
    is_line_range_loop_body(&body, &loop_var, lines_var)
        && variable_references(statement)
            .into_iter()
            .all(|name| name == lines_var || name == loop_var)
}

fn parse_line_range_loop(statement: &str, lines_var: &str) -> Option<(String, String)> {
    parse_keyword_loop(statement, "foreach")
        .and_then(|(header, body)| foreach_range_variable(&header).map(|var| (var, body)))
        .or_else(|| {
            parse_keyword_loop(statement, "for").and_then(|(header, body)| {
                for_range_variable(&header, lines_var).map(|var| (var, body))
            })
        })
}

fn parse_keyword_loop(statement: &str, keyword: &str) -> Option<(String, String)> {
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

fn strip_case_insensitive_keyword<'a>(value: &'a str, keyword: &str) -> Option<&'a str> {
    let prefix = value.get(..keyword.len())?;
    if !prefix.eq_ignore_ascii_case(keyword) {
        return None;
    }
    let rest = &value[keyword.len()..];
    if rest
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }
    Some(rest)
}

fn foreach_range_variable(header: &str) -> Option<String> {
    let mut parts = header.split_whitespace();
    let variable = parts.next().and_then(simple_variable_name)?;
    if !parts.next()?.eq_ignore_ascii_case("in") {
        return None;
    }
    let range = parts.next()?;
    (parts.next().is_none() && is_numeric_range(range)).then_some(variable)
}

fn for_range_variable(header: &str, lines_var: &str) -> Option<String> {
    let compact = remove_ascii_whitespace(header).to_ascii_lowercase();
    let parts: Vec<&str> = compact.split(';').collect();
    let [initializer, condition, increment] = parts.as_slice() else {
        return None;
    };
    let (lhs, start) = initializer.split_once('=')?;
    let variable = simple_variable_name(lhs)?;
    if start.is_empty() || !start.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let expected_increment = format!("${variable}++");
    (*increment == expected_increment
        && is_supported_for_condition(condition, &variable, lines_var))
    .then_some(variable)
}

fn is_supported_for_condition(condition: &str, loop_var: &str, lines_var: &str) -> bool {
    [format!("${loop_var}-le"), format!("${loop_var}-lt")]
        .iter()
        .any(|prefix| {
            condition.strip_prefix(prefix).is_some_and(|limit| {
                is_numeric_literal(limit) || is_count_property_reference(limit, lines_var)
            })
        })
}

fn is_count_property_reference(value: &str, lines_var: &str) -> bool {
    matches!(
        value,
        count if count == format!("${lines_var}.count") || count == format!("${lines_var}.length")
    )
}

fn is_numeric_literal(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn is_numeric_range(value: &str) -> bool {
    let Some((start, end)) = value.split_once("..") else {
        return false;
    };
    !start.is_empty()
        && !end.is_empty()
        && start.chars().all(|ch| ch.is_ascii_digit())
        && end.chars().all(|ch| ch.is_ascii_digit())
}

fn is_line_range_loop_body(body: &str, loop_var: &str, lines_var: &str) -> bool {
    let expression = strip_optional_count_guard(body, loop_var, lines_var).unwrap_or(body.trim());
    is_line_range_format_expression(expression, loop_var, lines_var)
}

fn strip_optional_count_guard<'a>(
    body: &'a str,
    loop_var: &str,
    lines_var: &str,
) -> Option<&'a str> {
    let rest = strip_case_insensitive_keyword(body.trim(), "if")?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let condition_end = matching_delimiter(rest, 0, '(', ')')?;
    let condition = remove_ascii_whitespace(&rest[1..condition_end]).to_ascii_lowercase();
    if !is_supported_count_guard(&condition, loop_var, lines_var) {
        return None;
    }

    let rest = rest[condition_end + 1..].trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let body_end = matching_delimiter(rest, 0, '{', '}')?;
    rest[body_end + 1..]
        .trim()
        .is_empty()
        .then(|| rest[1..body_end].trim())
}

fn is_supported_count_guard(condition: &str, loop_var: &str, lines_var: &str) -> bool {
    let count_property = format!("${lines_var}.count");
    let length_property = format!("${lines_var}.length");
    let one_based_guards = [
        format!("${loop_var}-le{count_property}"),
        format!("${loop_var}-le{length_property}"),
        format!("(${loop_var}+1)-le{count_property}"),
        format!("(${loop_var}+1)-le{length_property}"),
        format!("${loop_var}+1-le{count_property}"),
        format!("${loop_var}+1-le{length_property}"),
    ];
    let zero_based_guards = [
        format!("${loop_var}-lt{count_property}"),
        format!("${loop_var}-lt{length_property}"),
    ];
    one_based_guards
        .iter()
        .chain(zero_based_guards.iter())
        .any(|guard| condition == guard)
}

fn is_line_range_format_expression(expression: &str, loop_var: &str, lines_var: &str) -> bool {
    let Some((format, rest)) = parse_string_literal_prefix(expression.trim()) else {
        return false;
    };
    if !(format.contains("{0}") && format.contains("{1}")) {
        return false;
    }
    let rest = remove_ascii_whitespace(rest).to_ascii_lowercase();
    [
        format!("-f${loop_var},${lines_var}[${loop_var}-1]"),
        format!("-f(${loop_var}+1),${lines_var}[${loop_var}]"),
        format!("-f${loop_var}+1,${lines_var}[${loop_var}]"),
    ]
    .contains(&rest)
}

fn parse_string_literal_prefix(value: &str) -> Option<(String, &str)> {
    let quote = value.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }

    let mut out = String::new();
    let mut chars = value.char_indices().skip(1).peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '`' && quote == '"' {
            out.push(chars.next()?.1);
            continue;
        }
        if ch == quote {
            if quote == '\'' && chars.peek().is_some_and(|(_, next)| *next == '\'') {
                chars.next();
                out.push('\'');
                continue;
            }
            return Some((out, &value[idx + ch.len_utf8()..]));
        }
        out.push(ch);
    }
    None
}

fn variable_references(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut chars = value.char_indices().peekable();
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
            '$' => {
                if let Some(name) = variable_reference_at(value, idx) {
                    out.push(name);
                }
            }
            _ => {}
        }
    }
    out
}

fn variable_reference_at(value: &str, dollar_idx: usize) -> Option<String> {
    let after = &value[dollar_idx + 1..];
    if let Some(after_brace) = after.strip_prefix('{') {
        return normalize_variable_name(&after_brace[..after_brace.find('}')?]);
    }

    let mut len = 0;
    for ch in after.chars() {
        if len == 0 {
            if !(ch.is_ascii_alphabetic() || ch == '_') {
                return None;
            }
        } else if !(ch.is_ascii_alphanumeric() || ch == '_') {
            break;
        }
        len += ch.len_utf8();
    }

    (len > 0).then(|| normalize_variable_name(&after[..len]))?
}

fn matching_delimiter(value: &str, open_idx: usize, open: char, close: char) -> Option<usize> {
    if !value[open_idx..].starts_with(open) {
        return None;
    }

    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut chars = value[open_idx..].char_indices().peekable();
    while let Some((relative_idx, ch)) = chars.next() {
        let idx = open_idx + relative_idx;
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
            ch if ch == open => depth += 1,
            ch if ch == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }

    None
}

fn assignment_name(token: &str) -> Option<String> {
    simple_variable_name(token)
}

fn simple_variable_name(token: &str) -> Option<String> {
    if let Some(name) = token
        .strip_prefix("${")
        .and_then(|value| value.strip_suffix('}'))
    {
        return normalize_variable_name(name);
    }
    normalize_variable_name(token.strip_prefix('$')?)
}

fn normalize_variable_name(name: &str) -> Option<String> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    chars
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        .then(|| name.to_ascii_lowercase())
}

fn remove_ascii_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}

fn short_display_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    let mut parts = trimmed.split('/').rev().filter(|part| {
        !part.is_empty()
            && *part != "build"
            && *part != "dist"
            && *part != "node_modules"
            && *part != "src"
    });
    parts
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| trimmed.to_string())
}
