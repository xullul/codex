use std::path::PathBuf;

use codex_protocol::parse_command::ParsedCommand;

use crate::parse_command::shlex_join;
use crate::powershell_syntax::matching_delimiter;
use crate::powershell_syntax::powershell_literal as quoted_powershell_literal;
use crate::powershell_syntax::simple_variable_name;
use crate::powershell_syntax::split_command_tokens;
use crate::powershell_syntax::split_pipeline_parts;
use crate::powershell_syntax::split_top_level_statements;
use crate::powershell_syntax::strip_case_insensitive_keyword;
use crate::powershell_syntax::strip_wrapping_parens;

pub(crate) fn summarize_line_range_preview(script: &str) -> Option<ParsedCommand> {
    let mut statements = split_top_level_statements(script)?;
    let cwd = statements
        .first()
        .and_then(|statement| location_push_target(statement));
    if cwd.is_some() {
        statements.remove(0);
    }
    if statements
        .last()
        .is_some_and(|statement| is_pop_location(statement))
    {
        statements.pop();
    }

    if let Some(read) = summarize_streaming_line_range_preview(&statements, cwd.as_deref()) {
        return Some(read);
    }

    let mut remaining = statements.as_slice();
    let path_binding = match remaining {
        [path_assignment, lines_assignment, ..]
            if get_content_assignment(lines_assignment).is_some() =>
        {
            remaining = &remaining[1..];
            Some(literal_variable_assignment(path_assignment)?)
        }
        _ => None,
    };

    let [lines_assignment, rest @ ..] = remaining else {
        return None;
    };
    let (lines_var, read_target) = get_content_assignment(lines_assignment)?;

    if !all_line_range_preview_statements(rest, &lines_var) {
        return None;
    }
    let path = apply_cwd_to_path(
        cwd.as_deref(),
        &resolve_read_target(path_binding.as_ref(), read_target),
    );

    Some(ParsedCommand::Read {
        cmd: shlex_join(&["Get-Content".to_string(), path.clone()]),
        name: short_display_path(&path),
        path: PathBuf::from(path),
    })
}

fn all_line_range_preview_statements(statements: &[String], lines_var: &str) -> bool {
    let mut remaining = statements;
    let mut saw_preview = false;

    while let Some((statement, rest)) = remaining.split_first() {
        let mut range_binding = None;
        let mut loop_statement = statement.as_str();
        remaining = rest;

        if let Some(binding) = numeric_range_variable_assignment(statement) {
            let Some((next_statement, next_rest)) = remaining.split_first() else {
                return false;
            };
            range_binding = Some(binding);
            loop_statement = next_statement;
            remaining = next_rest;
        }

        if !is_line_range_preview_loop(loop_statement, lines_var, range_binding.as_deref())
            && !is_line_range_index_expression(loop_statement, lines_var)
            && !is_numeric_range_pipeline(loop_statement, lines_var)
        {
            return false;
        }
        saw_preview = true;
    }

    saw_preview
}

enum ReadTarget {
    Literal(String),
    Variable { name: String, display_token: String },
}

fn location_push_target(statement: &str) -> Option<String> {
    let tokens = split_command_tokens(strip_wrapping_parens(statement.trim()))?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "set-location" | "cd" | "chdir" | "sl" | "push-location" | "pushd"
    ) {
        return None;
    }
    location_path_operand(tail)
}

fn is_pop_location(statement: &str) -> bool {
    let Some(tokens) = split_command_tokens(strip_wrapping_parens(statement.trim())) else {
        return false;
    };
    let [head, tail @ ..] = tokens.as_slice() else {
        return false;
    };
    matches!(head.to_ascii_lowercase().as_str(), "pop-location" | "popd")
        && tail.iter().all(|arg| arg.starts_with('-'))
}

fn location_path_operand(args: &[String]) -> Option<String> {
    let mut path = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, value)) = arg.split_once('=')
            && matches!(name.to_ascii_lowercase().as_str(), "-path" | "-literalpath")
        {
            path = set_single_literal_path(path, value)?;
            i += 1;
            continue;
        }
        if matches!(lower.as_str(), "-path" | "-literalpath") {
            path = set_single_literal_path(path, args.get(i + 1)?.as_str())?;
            i += 2;
            continue;
        }
        if location_flag_consumes_next_value(&lower) {
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        path = set_single_literal_path(path, arg)?;
        i += 1;
    }
    path
}

fn set_single_literal_path(current: Option<String>, value: &str) -> Option<Option<String>> {
    if current.is_some() {
        return None;
    }
    Some(Some(powershell_literal(value)?))
}

fn location_flag_consumes_next_value(flag: &str) -> bool {
    matches!(flag, "-stackname")
}

fn literal_variable_assignment(statement: &str) -> Option<(String, String)> {
    let (lhs, rhs) = split_assignment(statement)?;
    Some((
        assignment_name(lhs.trim())?,
        powershell_literal(rhs.trim())?,
    ))
}

fn numeric_range_variable_assignment(statement: &str) -> Option<String> {
    let (lhs, rhs) = split_assignment(statement)?;
    let variable = assignment_name(lhs.trim())?;
    is_range_expression(rhs.trim()).then_some(variable)
}

fn numeric_variable_assignment(statement: &str) -> Option<String> {
    let (lhs, rhs) = split_assignment(statement)?;
    let variable = assignment_name(lhs.trim())?;
    is_numeric_literal(rhs.trim()).then_some(variable)
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

fn get_content_command(statement: &str) -> Option<ReadTarget> {
    let tokens = split_command_tokens(strip_wrapping_parens(statement.trim()))?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "get-content" | "gc" | "cat" | "type"
    ) {
        return None;
    }
    get_content_path_operand(tail)
}

fn summarize_streaming_line_range_preview(
    statements: &[String],
    cwd: Option<&str>,
) -> Option<ParsedCommand> {
    let (pipeline_statement, setup_statements) = statements.split_last()?;
    if setup_statements.len() > 2 {
        return None;
    }

    let pipeline_parts = split_pipeline_parts(pipeline_statement)?;
    let [get_content, tail @ ..] = pipeline_parts.as_slice() else {
        return None;
    };
    if tail.is_empty() {
        return None;
    }

    let read_target = get_content_command(get_content)?;
    let counter_var = streaming_line_range_tail_counter(tail)?;
    if let Some(counter_var) = counter_var.as_deref()
        && !setup_statements
            .iter()
            .any(|statement| numeric_variable_assignment(statement).as_deref() == Some(counter_var))
    {
        return None;
    }

    let path_binding = setup_statements.iter().find_map(|statement| {
        let binding = literal_variable_assignment(statement)?;
        match &read_target {
            ReadTarget::Variable { name, .. } if name == &binding.0 => Some(binding),
            _ => None,
        }
    });
    if !setup_statements.iter().all(|statement| {
        is_streaming_setup_statement(statement, &read_target, counter_var.as_deref())
    }) {
        return None;
    }

    let path = apply_cwd_to_path(
        cwd,
        &resolve_read_target(path_binding.as_ref(), read_target),
    );
    Some(ParsedCommand::Read {
        cmd: shlex_join(&["Get-Content".to_string(), path.clone()]),
        name: short_display_path(&path),
        path: PathBuf::from(path),
    })
}

fn is_streaming_setup_statement(
    statement: &str,
    read_target: &ReadTarget,
    counter_var: Option<&str>,
) -> bool {
    if let Some(counter_var) = counter_var
        && numeric_variable_assignment(statement).as_deref() == Some(counter_var)
    {
        return true;
    }
    if let Some((name, _)) = literal_variable_assignment(statement)
        && matches!(read_target, ReadTarget::Variable { name: read_name, .. } if read_name == &name)
    {
        return true;
    }
    false
}

fn streaming_line_range_tail_counter(parts: &[String]) -> Option<Option<String>> {
    let mut counter_var = None;
    for part in parts {
        if is_select_object_line_limiter(part) {
            continue;
        }
        if let Some(next_counter_var) = foreach_line_number_projection_counter(part) {
            if counter_var
                .as_ref()
                .is_some_and(|existing| existing != &next_counter_var)
            {
                return None;
            }
            counter_var = Some(next_counter_var);
            continue;
        }
        return None;
    }
    Some(counter_var)
}

fn is_select_object_line_limiter(statement: &str) -> bool {
    let tokens = match split_command_tokens(strip_wrapping_parens(statement.trim())) {
        Some(tokens) => tokens,
        None => return false,
    };
    let [head, args @ ..] = tokens.as_slice() else {
        return false;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "select-object" | "select"
    ) {
        return false;
    }
    let mut saw_limiter = false;
    let mut i = 0;
    while i < args.len() {
        let lower = args[i].to_ascii_lowercase();
        if let Some((flag, value)) = lower.split_once('=')
            && matches!(flag, "-skip" | "-first" | "-last" | "-index")
            && !value.is_empty()
        {
            saw_limiter = true;
            i += 1;
            continue;
        }
        if matches!(lower.as_str(), "-skip" | "-first" | "-last" | "-index") {
            if i + 1 >= args.len() {
                return false;
            }
            saw_limiter = true;
            i += 2;
            continue;
        }
        return false;
    }
    saw_limiter
}

fn foreach_line_number_projection_counter(statement: &str) -> Option<String> {
    let rest = strip_foreach_object_keyword(statement.trim())?.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let body_end = matching_delimiter(rest, 0, '{', '}')?;
    if !rest[body_end + 1..].trim().is_empty() {
        return None;
    }
    let body = &rest[1..body_end];
    let statements = split_top_level_statements(body)?;
    let [format_statement, increment_statement] = statements.as_slice() else {
        return None;
    };
    let counter_var = increment_variable(increment_statement)?;
    is_streaming_line_number_format_expression(format_statement, &counter_var)
        .then_some(counter_var)
}

fn strip_foreach_object_keyword(value: &str) -> Option<&str> {
    strip_case_insensitive_keyword(value, "ForEach-Object")
        .or_else(|| strip_case_insensitive_keyword(value, "foreach"))
        .or_else(|| value.strip_prefix('%'))
}

fn increment_variable(statement: &str) -> Option<String> {
    let compact = remove_ascii_whitespace(statement).to_ascii_lowercase();
    let variable = compact.strip_suffix("++")?;
    simple_variable_name(variable)
}

fn is_streaming_line_number_format_expression(expression: &str, counter_var: &str) -> bool {
    let Some((format, rest)) = parse_string_literal_prefix(expression.trim()) else {
        return false;
    };
    if !format_has_line_number_and_text_placeholders(&format) {
        return false;
    }
    let rest = remove_ascii_whitespace(rest).to_ascii_lowercase();
    rest == format!("-f${counter_var},$_")
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
    quoted_powershell_literal(value)
        .or_else(|| is_plain_path_literal(value).then(|| value.to_string()))
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

fn is_line_range_preview_loop(
    statement: &str,
    lines_var: &str,
    range_binding: Option<&str>,
) -> bool {
    if let Some((loop_var, range_var, body)) = parse_multi_range_loop(statement, lines_var) {
        return is_line_range_loop_body(&body, &loop_var, lines_var)
            && variable_references(statement)
                .into_iter()
                .all(|name| name == lines_var || name == loop_var || name == range_var);
    }

    let Some((loop_var, foreach_range_var, body)) =
        parse_line_range_loop(statement, lines_var, range_binding)
    else {
        return false;
    };
    is_line_range_loop_body(&body, &loop_var, lines_var)
        && variable_references(statement).into_iter().all(|name| {
            name == lines_var
                || name == loop_var
                || foreach_range_var
                    .as_deref()
                    .is_some_and(|range_var| name == range_var)
        })
}

fn parse_line_range_loop(
    statement: &str,
    lines_var: &str,
    range_binding: Option<&str>,
) -> Option<(String, Option<String>, String)> {
    parse_keyword_loop(statement, "foreach")
        .and_then(|(header, body)| {
            foreach_range_variable(&header, range_binding)
                .map(|(loop_var, range_var)| (loop_var, range_var, body))
        })
        .or_else(|| {
            parse_keyword_loop(statement, "for").and_then(|(header, body)| {
                for_range_variable(&header, lines_var).map(|var| (var, None, body))
            })
        })
}

fn is_line_range_index_expression(statement: &str, lines_var: &str) -> bool {
    let Some(parts) = split_pipeline_parts(statement) else {
        return false;
    };
    let [expression, tail @ ..] = parts.as_slice() else {
        return false;
    };
    if !tail
        .iter()
        .all(|part| is_select_object_line_limiter(part) || is_line_range_projection(part))
    {
        return false;
    }

    let compact = remove_ascii_whitespace(expression).to_ascii_lowercase();
    let Some(rest) = compact.strip_prefix(&format!("${lines_var}[")) else {
        return false;
    };
    let Some(range) = rest.strip_suffix(']') else {
        return false;
    };
    is_numeric_range(range)
}

fn is_line_range_projection(statement: &str) -> bool {
    let rest = match strip_foreach_object_keyword(statement.trim()) {
        Some(rest) => rest.trim_start(),
        None => return false,
    };
    if !rest.starts_with('{') {
        return false;
    }
    let Some(body_end) = matching_delimiter(rest, 0, '{', '}') else {
        return false;
    };
    if !rest[body_end + 1..].trim().is_empty() {
        return false;
    }
    matches!(remove_ascii_whitespace(&rest[1..body_end]).as_str(), "$_")
}

fn is_numeric_range_pipeline(statement: &str, lines_var: &str) -> bool {
    let Some(parts) = split_pipeline_parts(statement) else {
        return false;
    };
    let [range, foreach] = parts.as_slice() else {
        return false;
    };
    if !is_range_expression(range) {
        return false;
    }

    let rest = match strip_foreach_object_keyword(foreach.trim()) {
        Some(rest) => rest.trim_start(),
        None => return false,
    };
    if !rest.starts_with('{') {
        return false;
    }
    let Some(body_end) = matching_delimiter(rest, 0, '{', '}') else {
        return false;
    };
    rest[body_end + 1..].trim().is_empty()
        && is_line_range_loop_body(&rest[1..body_end], "_", lines_var)
}

fn parse_multi_range_loop(statement: &str, lines_var: &str) -> Option<(String, String, String)> {
    let (header, body) = parse_keyword_loop(statement, "foreach")?;
    let range_var = foreach_tuple_ranges_variable(&header)?;
    let (loop_var, inner_body) = parse_keyword_loop(&body, "for")?;
    let loop_var = for_range_variable_with_indexed_bounds(&loop_var, &range_var, lines_var)?;
    Some((loop_var, range_var, inner_body))
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

fn foreach_range_variable(
    header: &str,
    range_binding: Option<&str>,
) -> Option<(String, Option<String>)> {
    let mut parts = header.split_whitespace();
    let variable = parts.next().and_then(simple_variable_name)?;
    if !parts.next()?.eq_ignore_ascii_case("in") {
        return None;
    }
    let range = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if is_range_expression(range) {
        return Some((variable, None));
    }
    let range_var = simple_variable_name(range)?;
    (range_binding == Some(range_var.as_str())).then_some((variable, Some(range_var)))
}

fn foreach_tuple_ranges_variable(header: &str) -> Option<String> {
    let compact = remove_ascii_whitespace(header).to_ascii_lowercase();
    let (lhs, ranges) = compact.split_once("in")?;
    let variable = simple_variable_name(lhs)?;
    if !ranges.starts_with("@(") || !ranges.ends_with(')') {
        return None;
    }
    let mut rest = &ranges[2..ranges.len() - 1];
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        if !rest.starts_with("@(") {
            return None;
        }
        let close = matching_delimiter(rest, 1, '(', ')')?;
        let tuple = &rest[2..close];
        let (start, end) = tuple.split_once(',')?;
        if !is_numeric_literal(start) || !is_numeric_literal(end) {
            return None;
        }
        rest = &rest[close + 1..];
        if rest.is_empty() {
            break;
        }
        rest = rest.strip_prefix(',')?;
    }
    Some(variable)
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

fn for_range_variable_with_indexed_bounds(
    header: &str,
    range_var: &str,
    lines_var: &str,
) -> Option<String> {
    let compact = remove_ascii_whitespace(header).to_ascii_lowercase();
    let parts: Vec<&str> = compact.split(';').collect();
    let [initializer, condition, increment] = parts.as_slice() else {
        return None;
    };
    let (lhs, start) = initializer.split_once('=')?;
    let variable = simple_variable_name(lhs)?;
    if start != format!("${range_var}[0]") {
        return None;
    }

    let expected_increment = format!("${variable}++");
    if *increment != expected_increment {
        return None;
    }
    (is_supported_for_condition(condition, &variable, lines_var)
        || *condition == format!("${variable}-le${range_var}[1]"))
    .then_some(variable)
}

fn is_supported_for_condition(condition: &str, loop_var: &str, lines_var: &str) -> bool {
    [format!("${loop_var}-le"), format!("${loop_var}-lt")]
        .iter()
        .any(|prefix| {
            condition.strip_prefix(prefix).is_some_and(|limit| {
                is_numeric_literal(limit)
                    || is_count_property_reference(limit, lines_var)
                    || is_min_count_bound(limit, lines_var)
            })
        })
}

fn is_count_property_reference(value: &str, lines_var: &str) -> bool {
    matches!(
        value,
        count if count == format!("${lines_var}.count") || count == format!("${lines_var}.length")
    )
}

fn is_min_count_bound(value: &str, lines_var: &str) -> bool {
    let Some(args) = value
        .strip_prefix("[math]::min(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    let Some((left, right)) = args.split_once(',') else {
        return false;
    };
    (is_count_property_reference(left, lines_var) && is_numeric_literal(right))
        || (is_numeric_literal(left) && is_count_property_reference(right, lines_var))
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

fn is_range_expression(value: &str) -> bool {
    let compact = remove_ascii_whitespace(value);
    let range = compact
        .strip_prefix("@(")
        .and_then(|value| value.strip_suffix(')'))
        .or_else(|| {
            compact
                .strip_prefix('(')
                .and_then(|value| value.strip_suffix(')'))
        })
        .unwrap_or(&compact);
    is_numeric_range(range)
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
    if !format_has_line_number_and_text_placeholders(&format) {
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

fn format_has_line_number_and_text_placeholders(format: &str) -> bool {
    [("{0}", "{1}"), ("{0,", "{1}")]
        .into_iter()
        .any(|(line, text)| format.contains(line) && format.contains(text))
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

fn assignment_name(token: &str) -> Option<String> {
    simple_variable_name(token)
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

fn apply_cwd_to_path(cwd: Option<&str>, path: &str) -> String {
    let Some(cwd) = cwd else {
        return path.to_string();
    };
    if is_abs_like(path) {
        return path.to_string();
    }
    let mut buf = PathBuf::from(cwd);
    buf.push(path);
    buf.to_string_lossy().to_string()
}

fn is_abs_like(path: &str) -> bool {
    if std::path::Path::new(path).is_absolute() {
        return true;
    }
    let mut chars = path.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(d), Some(':'), Some('\\' | '/')) if d.is_ascii_alphabetic()
    ) || path.starts_with("\\\\")
}
