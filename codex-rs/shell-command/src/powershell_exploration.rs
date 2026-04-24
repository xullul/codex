use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;

use crate::powershell_syntax::matching_delimiter;
use crate::powershell_syntax::powershell_literal;
use crate::powershell_syntax::simple_variable_name;
use crate::powershell_syntax::split_command_tokens;
use crate::powershell_syntax::split_pipeline_parts;
use crate::powershell_syntax::split_top_level_statements;
use crate::powershell_syntax::strip_case_insensitive_keyword;
use crate::powershell_syntax::strip_wrapping_parens;

pub(crate) fn summarize_known_exploration_script(script: &str) -> Option<ParsedCommand> {
    summarize_get_command_list_fallback(script).or_else(|| summarize_get_command_probe(script))
}

fn summarize_get_command_list_fallback(script: &str) -> Option<ParsedCommand> {
    let IfElseParts {
        condition,
        then_body,
        else_body,
    } = parse_if_else(script)?;
    parse_get_command_condition(&condition)?;

    let then_path = summarize_list_branch(&then_body)?;
    let else_path = summarize_list_branch(&else_body)?;
    if then_path != else_path {
        return None;
    }

    Some(ParsedCommand::ListFiles {
        cmd: script.to_string(),
        path: Some(short_display_path(&then_path)),
    })
}

fn summarize_get_command_probe(script: &str) -> Option<ParsedCommand> {
    let statements = split_top_level_statements(script)?;
    let [assignment, probe] = statements.as_slice() else {
        return None;
    };
    let (variable, tool) = parse_get_command_assignment(assignment)?;
    let IfElseParts {
        condition,
        then_body,
        else_body,
    } = parse_if_else(probe)?;

    if !condition
        .trim()
        .eq_ignore_ascii_case(&format!("${variable}"))
    {
        return None;
    }
    if !then_body
        .trim()
        .eq_ignore_ascii_case(&format!("${variable}.Source"))
    {
        return None;
    }
    if !matches!(powershell_literal(else_body.trim()).as_deref(), Some(value) if value.starts_with("NO_"))
    {
        return None;
    }

    Some(ParsedCommand::Action {
        cmd: script.to_string(),
        kind: ParsedCommandActionKind::Inspect,
        detail: Some(format!("{tool} in PATH")),
    })
}

fn parse_get_command_assignment(statement: &str) -> Option<(String, String)> {
    let (lhs, rhs) = statement.split_once('=')?;
    let variable = simple_variable_name(lhs.trim())?;
    let tool = parse_get_command_condition(rhs.trim())?;
    Some((variable, tool))
}

fn parse_get_command_condition(condition: &str) -> Option<String> {
    let tokens = split_command_tokens(strip_wrapping_parens(condition.trim()))?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !head.eq_ignore_ascii_case("Get-Command") {
        return None;
    }
    let mut tool = None;
    let mut i = 0;
    while i < tail.len() {
        let arg = &tail[i];
        let lower = arg.to_ascii_lowercase();
        if lower == "-erroraction" {
            if !tail
                .get(i + 1)
                .is_some_and(|value| value.eq_ignore_ascii_case("SilentlyContinue"))
            {
                return None;
            }
            i += 2;
            continue;
        }
        if lower.starts_with('-') || tool.is_some() {
            return None;
        }
        tool = Some(arg.clone());
        i += 1;
    }
    tool
}

fn summarize_list_branch(script: &str) -> Option<String> {
    summarize_rg_files_branch(script).or_else(|| summarize_get_child_item_branch(script))
}

fn summarize_rg_files_branch(script: &str) -> Option<String> {
    let tokens = split_command_tokens(script)?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !head.eq_ignore_ascii_case("rg") || !tail.iter().any(|arg| arg == "--files") {
        return None;
    }
    positional_operands_skipping(
        tail,
        &[
            "-g",
            "--glob",
            "--iglob",
            "-t",
            "--type",
            "--type-add",
            "--type-not",
            "-m",
            "--max-count",
            "-a",
            "-b",
            "-c",
            "--context",
            "--max-depth",
        ],
    )
    .into_iter()
    .next()
}

fn summarize_get_child_item_branch(script: &str) -> Option<String> {
    let parts = split_pipeline_parts(script)?;
    let [command, projection] = parts.as_slice() else {
        return None;
    };
    if !is_full_name_projection(projection) {
        return None;
    }

    let tokens = split_command_tokens(command)?;
    let [head, tail @ ..] = tokens.as_slice() else {
        return None;
    };
    if !matches!(
        head.to_ascii_lowercase().as_str(),
        "get-childitem" | "gci" | "dir" | "ls"
    ) {
        return None;
    }
    path_operand(tail)
}

fn is_full_name_projection(script: &str) -> bool {
    let tokens = split_command_tokens(script);
    matches!(
        tokens.as_deref(),
        Some([head, open, expr, close])
            if matches!(
                head.to_ascii_lowercase().as_str(),
                "foreach-object" | "foreach" | "%"
            ) && open == "{"
                && close == "}"
                && matches!(
                    expr.to_ascii_lowercase().as_str(),
                    "$_.fullname" | "$_.full_name" | "$_.name" | "$_.path"
                )
    )
}

struct IfElseParts {
    condition: String,
    then_body: String,
    else_body: String,
}

fn parse_if_else(script: &str) -> Option<IfElseParts> {
    let rest = strip_case_insensitive_keyword(script.trim(), "if")?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let condition_end = matching_delimiter(rest, 0, '(', ')')?;
    let condition = rest[1..condition_end].trim().to_string();
    let rest = rest[condition_end + 1..].trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let then_end = matching_delimiter(rest, 0, '{', '}')?;
    let then_body = rest[1..then_end].trim().to_string();
    let rest = rest[then_end + 1..].trim_start();
    let rest = strip_case_insensitive_keyword(rest, "else")?.trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let else_end = matching_delimiter(rest, 0, '{', '}')?;
    rest[else_end + 1..].trim().is_empty().then(|| IfElseParts {
        condition,
        then_body,
        else_body: rest[1..else_end].trim().to_string(),
    })
}

fn path_operand(args: &[String]) -> Option<String> {
    let named_paths = named_path_values(args, &["-path", "-literalpath"]);
    named_paths
        .into_iter()
        .next()
        .or_else(|| positional_operands(args).into_iter().next())
}

fn named_path_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, _)) = lower.split_once('=')
            && names.contains(&name)
        {
            if let Some((_, value)) = arg.split_once('=') {
                out.extend(split_path_list(value));
            }
            i += 1;
            continue;
        }
        if names.contains(&lower.as_str()) {
            if let Some(value) = args.get(i + 1) {
                out.extend(split_path_list(value));
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
    positional_operands_skipping(
        args,
        &[
            "-path",
            "-literalpath",
            "-filter",
            "-include",
            "-exclude",
            "-encoding",
            "-totalcount",
            "-first",
            "-last",
            "-skip",
            "-pattern",
            "-simplematch",
            "-regex",
        ],
    )
}

fn positional_operands_skipping(args: &[String], flags_with_values: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    let mut after_double_dash = false;
    for (idx, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if after_double_dash {
            out.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_double_dash = true;
            continue;
        }
        let lower = arg.to_ascii_lowercase();
        if lower.starts_with("--") && lower.contains('=') {
            continue;
        }
        if flags_with_values.contains(&lower.as_str()) {
            if idx + 1 < args.len() {
                skip_next = true;
            }
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        out.push(arg.clone());
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
