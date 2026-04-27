use std::collections::HashMap;
use std::path::PathBuf;

use codex_protocol::parse_command::ParsedCommand;

use crate::parse_command::shlex_join;
use crate::powershell_projection::top_level_assignment;
use crate::powershell_syntax::matching_delimiter;
use crate::powershell_syntax::powershell_literal;
use crate::powershell_syntax::simple_variable_name;
use crate::powershell_syntax::split_command_tokens;
use crate::powershell_syntax::split_pipeline_parts;
use crate::powershell_syntax::split_top_level_statements;
use crate::powershell_syntax::strip_case_insensitive_keyword;
use crate::powershell_syntax::strip_wrapping_parens;

pub(crate) fn summarize_get_child_item_file_read(script: &str) -> Option<ParsedCommand> {
    let statements = split_top_level_statements(script)?;
    let mut cwd: Option<String> = None;
    let mut cwd_stack: Vec<Option<String>> = Vec::new();
    let mut variables: HashMap<String, String> = HashMap::new();
    let mut read_pipeline = None;

    for statement in statements {
        if let Some((name, value)) = literal_variable_assignment(&statement) {
            variables.insert(name, value);
            continue;
        }
        if let Some(location) = location_target(
            &statement,
            cwd.as_deref(),
            &variables,
            &["set-location", "cd", "chdir", "sl"],
        ) {
            cwd = Some(location);
            continue;
        }
        if let Some(location) = location_target(
            &statement,
            cwd.as_deref(),
            &variables,
            &["push-location", "pushd"],
        ) {
            cwd_stack.push(cwd.clone());
            cwd = Some(location);
            continue;
        }
        if is_pop_location(&statement) {
            cwd = cwd_stack.pop().unwrap_or(None);
            continue;
        }
        if read_pipeline
            .replace((statement, cwd.clone(), variables.clone()))
            .is_some()
        {
            return None;
        }
    }

    let (read_pipeline, read_cwd, read_variables) = read_pipeline?;
    let read_path = summarize_get_child_item_pipeline_read(
        &read_pipeline,
        read_cwd.as_deref(),
        &read_variables,
    )?;
    Some(ParsedCommand::Read {
        cmd: shlex_join(&["Get-Content".to_string(), read_path.clone()]),
        name: short_display_path(&read_path),
        path: PathBuf::from(read_path),
    })
}

fn summarize_get_child_item_pipeline_read(
    statement: &str,
    cwd: Option<&str>,
    variables: &HashMap<String, String>,
) -> Option<String> {
    let parts = split_pipeline_parts(statement)?;
    let [source, tail @ ..] = parts.as_slice() else {
        return None;
    };
    if tail.is_empty() {
        return None;
    }

    let source_tokens = command_tokens(source, variables)?;
    let [head, args @ ..] = source_tokens.as_slice() else {
        return None;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "get-childitem" | "gci" | "dir" | "ls"
    ) {
        return None;
    }

    let read_target = get_child_item_read_target(args)?;
    let read_count = tail
        .iter()
        .filter(|part| is_get_content_foreach_read(part))
        .count();
    if read_count != 1 {
        return None;
    }
    if !tail.iter().enumerate().all(|(idx, part)| {
        is_get_content_foreach_read(part) || idx > 0 && is_select_object_line_limiter(part)
    }) {
        return None;
    }

    Some(apply_cwd_to_path(cwd, &read_target))
}

fn command_tokens(statement: &str, variables: &HashMap<String, String>) -> Option<Vec<String>> {
    split_command_tokens(strip_wrapping_parens(statement.trim())).map(|tokens| {
        tokens
            .into_iter()
            .map(|token| resolved_variable_value(&token, variables).unwrap_or(token))
            .collect()
    })
}

fn literal_variable_assignment(statement: &str) -> Option<(String, String)> {
    let (lhs, rhs) = top_level_assignment(statement)?;
    let name = simple_variable_name(lhs.trim())?;
    let rhs = rhs.trim();
    let value =
        powershell_literal(rhs).or_else(|| is_plain_path_literal(rhs).then(|| rhs.to_string()))?;
    Some((name, value))
}

fn resolved_variable_value(token: &str, variables: &HashMap<String, String>) -> Option<String> {
    let name = simple_variable_name(token)?;
    variables.get(&name).cloned()
}

fn get_child_item_read_target(args: &[String]) -> Option<String> {
    let base_path = single_path_value(args, &["-path", "-literalpath"]).or_else(|| {
        positional_operands(args)
            .into_iter()
            .next()
            .filter(|path| path != ".")
    });
    let selector = single_path_value(args, &["-filter", "-include"]);

    if let Some(selector) = selector
        && is_exact_path_component(&selector)
    {
        return Some(match base_path {
            Some(base_path) if !path_has_wildcards(&base_path) => join_paths(&base_path, &selector),
            _ => selector,
        });
    }

    let base_path = base_path?;
    (!path_has_wildcards(&base_path) && looks_like_file_path(&base_path)).then_some(base_path)
}

fn is_get_content_foreach_read(statement: &str) -> bool {
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

    let body = &rest[1..body_end];
    let parts = match split_pipeline_parts(body) {
        Some(parts) => parts,
        None => return false,
    };
    let [get_content, tail @ ..] = parts.as_slice() else {
        return false;
    };
    get_content_reads_pipeline_item(get_content)
        && tail.iter().all(|part| is_select_object_line_limiter(part))
}

fn get_content_reads_pipeline_item(statement: &str) -> bool {
    let Some(tokens) = split_command_tokens(strip_wrapping_parens(statement.trim())) else {
        return false;
    };
    let [head, args @ ..] = tokens.as_slice() else {
        return false;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "get-content" | "gc" | "cat" | "type"
    ) {
        return false;
    }

    let Some(target) = single_get_content_target(args) else {
        return false;
    };
    matches!(
        remove_ascii_whitespace(&target)
            .to_ascii_lowercase()
            .as_str(),
        "$_" | "$_.fullname" | "$psitem" | "$psitem.fullname"
    )
}

fn single_get_content_target(args: &[String]) -> Option<String> {
    let mut target = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, value)) = arg.split_once('=')
            && matches!(name.to_ascii_lowercase().as_str(), "-path" | "-literalpath")
        {
            target = set_single_value(target, value)?;
            i += 1;
            continue;
        }
        if matches!(lower.as_str(), "-path" | "-literalpath") {
            target = set_single_value(target, args.get(i + 1)?.as_str())?;
            i += 2;
            continue;
        }
        if read_flag_consumes_next_value(&lower) {
            i += 2;
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        target = set_single_value(target, arg)?;
        i += 1;
    }
    target
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

fn single_path_value(args: &[String], names: &[&str]) -> Option<String> {
    let values = named_values(args, names)
        .into_iter()
        .flat_map(|value| split_path_list(&value))
        .collect::<Vec<_>>();
    match values.as_slice() {
        [value] => Some(value.clone()),
        _ => None,
    }
}

fn named_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, _)) = lower.split_once('=')
            && names.contains(&name)
        {
            if let Some((_, value)) = arg.split_once('=') {
                out.push(value.to_string());
            }
            i += 1;
            continue;
        }
        if names.contains(&lower.as_str()) {
            if let Some(value) = args.get(i + 1) {
                out.push(value.clone());
                i += 2;
                continue;
            }
            break;
        }
        i += 1;
    }
    out
}

fn positional_operands(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for (idx, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        let lower = arg.to_ascii_lowercase();
        if gci_flag_consumes_next_value(&lower) {
            if idx + 1 < args.len() {
                skip_next = true;
            }
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        out.extend(split_path_list(arg));
    }
    out
}

fn split_path_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn location_target(
    statement: &str,
    cwd: Option<&str>,
    variables: &HashMap<String, String>,
    commands: &[&str],
) -> Option<String> {
    let tokens = command_tokens(statement, variables)?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !commands.contains(&head.to_ascii_lowercase().as_str()) {
        return None;
    }
    location_path_operand(tail).map(|path| apply_cwd_to_path(cwd, &path))
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
    let value = single_path_value(args, &["-path", "-literalpath"])
        .or_else(|| positional_operands(args).into_iter().next())?;
    (!path_has_wildcards(&value)).then_some(value)
}

fn strip_foreach_object_keyword(value: &str) -> Option<&str> {
    strip_case_insensitive_keyword(value, "ForEach-Object")
        .or_else(|| strip_case_insensitive_keyword(value, "foreach"))
        .or_else(|| value.strip_prefix('%'))
}

fn set_single_value(current: Option<String>, value: &str) -> Option<Option<String>> {
    if current.is_some() {
        return None;
    }
    Some(Some(value.to_string()))
}

fn gci_flag_consumes_next_value(flag: &str) -> bool {
    matches!(
        flag,
        "-path"
            | "-literalpath"
            | "-filter"
            | "-include"
            | "-exclude"
            | "-name"
            | "-attributes"
            | "-depth"
    )
}

fn read_flag_consumes_next_value(flag: &str) -> bool {
    matches!(
        flag,
        "-encoding" | "-filter" | "-include" | "-exclude" | "-totalcount" | "-first" | "-last"
    )
}

fn is_exact_path_component(value: &str) -> bool {
    !value.is_empty() && !path_has_wildcards(value) && !value.contains('/') && !value.contains('\\')
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

fn looks_like_file_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let filename = normalized.rsplit('/').next().unwrap_or(&normalized);
    filename.contains('.') && !filename.ends_with('.')
}

fn path_has_wildcards(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[') || path.contains(']')
}

fn apply_cwd_to_path(cwd: Option<&str>, path: &str) -> String {
    let Some(cwd) = cwd else {
        return path.to_string();
    };
    join_paths(cwd, path)
}

fn join_paths(base: &str, rel: &str) -> String {
    if is_abs_like(rel) {
        return rel.to_string();
    }
    if base.is_empty() || base == "." {
        return rel.to_string();
    }
    let mut buf = PathBuf::from(base);
    buf.push(rel);
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

fn short_display_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    let ignored = ["build", "dist", "node_modules", "src"];
    let mut parts = trimmed
        .split('/')
        .rev()
        .filter(|part| !part.is_empty() && !ignored.contains(part));
    parts
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| trimmed.to_string())
}

fn remove_ascii_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect()
}
