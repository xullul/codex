use std::collections::HashMap;
use std::path::PathBuf;

use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::parse_command::ParsedCommandActionKind;

use crate::action_classification::action_from_tokens;
use crate::parse_command::shlex_join;
use crate::powershell::UTF8_OUTPUT_PREFIX;
use crate::powershell_exploration::summarize_known_exploration_script;
use crate::powershell_file_read::summarize_get_child_item_file_read;
use crate::powershell_line_range::summarize_line_range_preview;
use crate::powershell_parser::PowershellParseOutcome;
use crate::powershell_parser::parse_with_powershell_ast;
use crate::powershell_projection::is_result_variable_projection_statement;
use crate::powershell_projection::top_level_assignment;
use crate::powershell_syntax::powershell_literal;
use crate::powershell_syntax::split_command_tokens;
use crate::powershell_syntax::split_pipeline_parts;
use crate::powershell_syntax::split_top_level_statements;
use crate::powershell_syntax::strip_wrapping_parens;

pub(crate) fn parse_powershell_script(
    executable: Option<&str>,
    script: &str,
) -> Vec<ParsedCommand> {
    let script = strip_utf8_prefix(script).trim();
    if script.is_empty() {
        return vec![unknown(script)];
    }

    if let Some(read) = summarize_line_range_preview(script) {
        return vec![read];
    }
    if let Some(read) = summarize_get_child_item_file_read(script) {
        return vec![read];
    }
    if let Some(command) = summarize_known_exploration_script(script) {
        return vec![command];
    }

    if let Some(executable) = executable
        && let PowershellParseOutcome::Commands(commands) =
            parse_with_powershell_ast(executable, script)
    {
        return summarize_parts(script, commands);
    }

    let Some(parts) = split_parts(script) else {
        return summarize_top_level_last_commands(script).unwrap_or_else(|| vec![unknown(script)]);
    };
    let commands = summarize_parts(script, parts.into_iter().map(|part| part.tokens).collect());
    if commands
        .iter()
        .any(|command| matches!(command, ParsedCommand::Unknown { .. }))
        && let Some(fallback) = summarize_top_level_last_commands(script)
    {
        return fallback;
    }
    commands
}

fn summarize_parts(script: &str, parts: Vec<Vec<String>>) -> Vec<ParsedCommand> {
    let mut out = Vec::new();
    let mut cwd: Option<String> = None;
    let mut cwd_stack: Vec<Option<String>> = Vec::new();
    let mut prior_list_path: Option<String> = None;
    let mut variables: HashMap<String, String> = HashMap::new();
    let mut command_result_variables: HashMap<String, ParsedCommand> = HashMap::new();
    let mut pending_wrapping_closes = 0;
    let mut last_location_action = None;

    for raw_tokens in parts {
        let tokens = normalize_part_tokens(raw_tokens, &variables, &mut pending_wrapping_closes);
        if tokens.is_empty() {
            continue;
        }
        if let Some((name, parsed)) = command_result_assignment(&tokens) {
            let parsed = apply_context_to_parsed(parsed, cwd.as_deref(), &mut prior_list_path);
            command_result_variables.insert(name, parsed.clone());
            out.push(parsed);
            continue;
        }
        if let Some((name, value)) = simple_variable_assignment(&tokens) {
            variables.insert(name, value);
            prior_list_path = None;
            continue;
        }
        let Some((head, tail)) = tokens.split_first() else {
            continue;
        };
        let head_lower = head.to_ascii_lowercase();
        if simple_variable_name(head)
            .is_some_and(|name| command_result_variables.contains_key(&name))
        {
            continue;
        }
        if is_set_location(&head_lower) {
            if let Some(path) = path_operand(tail) {
                cwd = Some(match cwd.as_deref() {
                    Some(base) => join_paths(base, &path),
                    None => path,
                });
            }
            last_location_action = Some(location_action(&tokens));
            prior_list_path = None;
            continue;
        }
        if is_push_location(&head_lower) {
            cwd_stack.push(cwd.clone());
            if let Some(path) = path_operand(tail) {
                cwd = Some(match cwd.as_deref() {
                    Some(base) => join_paths(base, &path),
                    None => path,
                });
            }
            last_location_action = Some(location_action(&tokens));
            prior_list_path = None;
            continue;
        }
        if is_pop_location(&head_lower, tail) {
            cwd = cwd_stack.pop().unwrap_or(None);
            last_location_action = Some(location_action(&tokens));
            prior_list_path = None;
            continue;
        }
        if is_formatting_helper(&head_lower, tail) {
            continue;
        }
        if is_safe_setup_helper(&head_lower, tail) {
            continue;
        }
        if is_mutating_or_ambiguous(&head_lower) {
            if is_mutating_command(&head_lower) {
                return vec![edit_action(script)];
            }
            return vec![unknown(script)];
        }

        let parsed = summarize_tokens(&tokens);
        let parsed = apply_context_to_parsed(parsed, cwd.as_deref(), &mut prior_list_path);
        if matches!(parsed, ParsedCommand::Unknown { .. }) {
            return vec![unknown(script)];
        }
        out.push(parsed);
    }

    if out.is_empty() {
        if let Some(action) = last_location_action {
            return vec![action];
        }
        return vec![unknown(script)];
    }

    simplify_powershell_commands(out)
}

fn summarize_top_level_last_commands(script: &str) -> Option<Vec<ParsedCommand>> {
    let statements = split_top_level_statements(script)?;
    let mut out = Vec::new();
    let mut cwd: Option<String> = None;
    let mut cwd_stack: Vec<Option<String>> = Vec::new();
    let mut prior_list_path: Option<String> = None;
    let mut variables: HashMap<String, String> = HashMap::new();
    let mut command_result_variables: HashMap<String, ParsedCommand> = HashMap::new();
    let mut last_location_action = None;

    for statement in statements {
        if let Some(tokens) = split_command_tokens(strip_wrapping_parens(statement.trim())) {
            let tokens = tokens
                .into_iter()
                .map(|token| resolved_variable_value(&token, &variables).unwrap_or(token))
                .collect::<Vec<_>>();
            let Some((head, tail)) = tokens.split_first() else {
                continue;
            };
            let head_lower = head.to_ascii_lowercase();
            if let Some((name, parsed)) = command_result_assignment(&tokens) {
                let parsed = apply_context_to_parsed(parsed, cwd.as_deref(), &mut prior_list_path);
                command_result_variables.insert(name, parsed.clone());
                out.push(parsed);
                continue;
            }
            if is_set_location(&head_lower) {
                if let Some(path) = path_operand(tail) {
                    cwd = Some(match cwd.as_deref() {
                        Some(base) => join_paths(base, &path),
                        None => path,
                    });
                }
                last_location_action = Some(location_action(&tokens));
                prior_list_path = None;
                continue;
            }
            if is_push_location(&head_lower) {
                cwd_stack.push(cwd.clone());
                if let Some(path) = path_operand(tail) {
                    cwd = Some(match cwd.as_deref() {
                        Some(base) => join_paths(base, &path),
                        None => path,
                    });
                }
                last_location_action = Some(location_action(&tokens));
                prior_list_path = None;
                continue;
            }
            if is_pop_location(&head_lower, tail) {
                cwd = cwd_stack.pop().unwrap_or(None);
                last_location_action = Some(location_action(&tokens));
                prior_list_path = None;
                continue;
            }
            if let Some((name, value)) = simple_variable_assignment(&tokens) {
                variables.insert(name, value);
                prior_list_path = None;
                continue;
            }
        } else if is_here_string_assignment(&statement) {
            prior_list_path = None;
            continue;
        }

        if let Some((name, parsed)) = command_result_statement_assignment(&statement, &variables) {
            let parsed = apply_context_to_parsed(parsed, cwd.as_deref(), &mut prior_list_path);
            command_result_variables.insert(name, parsed.clone());
            out.push(parsed);
            continue;
        }
        if is_result_variable_projection_statement(
            &statement,
            &command_result_variables,
            is_formatting_helper,
        ) {
            prior_list_path = None;
            continue;
        }

        let parsed = summarize_pipeline_last_command(&statement)?;
        let parsed = apply_context_to_parsed(parsed, cwd.as_deref(), &mut prior_list_path);
        if matches!(parsed, ParsedCommand::Unknown { .. }) {
            return None;
        }
        out.push(parsed);
    }

    if out.is_empty() {
        return last_location_action.map(|action| vec![action]);
    }

    Some(simplify_powershell_commands(out))
}

fn command_result_statement_assignment(
    statement: &str,
    variables: &HashMap<String, String>,
) -> Option<(String, ParsedCommand)> {
    let (lhs, rhs) = top_level_assignment(statement)?;
    let name = assignment_name(lhs.trim())?;
    let commands = summarize_pipeline_commands(rhs.trim(), variables)?;
    let parsed = commands.into_iter().last()?;
    if matches!(parsed, ParsedCommand::Unknown { .. }) {
        return None;
    }
    Some((name, parsed))
}

fn summarize_pipeline_commands(
    statement: &str,
    variables: &HashMap<String, String>,
) -> Option<Vec<ParsedCommand>> {
    let mut out = Vec::new();
    for part in split_pipeline_parts(statement)? {
        let trimmed = strip_wrapping_parens(part.trim());
        let Some(tokens) = split_command_tokens(trimmed) else {
            if is_pipeline_input_expression(trimmed) {
                continue;
            }
            return None;
        };
        let tokens = tokens
            .into_iter()
            .map(|token| resolved_variable_value(&token, variables).unwrap_or(token))
            .collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        let Some((head, tail)) = tokens.split_first() else {
            continue;
        };
        let head_lower = head.to_ascii_lowercase();
        if is_formatting_helper(&head_lower, tail) {
            continue;
        }
        if is_safe_setup_helper(&head_lower, tail) {
            continue;
        }
        if is_mutating_or_ambiguous(&head_lower) {
            if is_mutating_command(&head_lower) {
                return Some(vec![edit_action(&ps_join(&tokens))]);
            }
            return None;
        }
        let next = summarize_tokens(&tokens);
        if matches!(next, ParsedCommand::Unknown { .. }) {
            if is_pipeline_input_expression(trimmed) {
                continue;
            }
            return None;
        }
        out.push(next);
    }
    Some(simplify_powershell_commands(out))
}

fn summarize_pipeline_last_command(statement: &str) -> Option<ParsedCommand> {
    summarize_pipeline_commands(statement, &HashMap::new())?
        .into_iter()
        .last()
}

fn location_action(tokens: &[String]) -> ParsedCommand {
    let cmd = ps_join(tokens);
    ParsedCommand::Action {
        cmd: cmd.clone(),
        kind: ParsedCommandActionKind::Run,
        detail: Some(cmd),
    }
}

fn is_here_string_assignment(statement: &str) -> bool {
    let trimmed = statement.trim_start();
    trimmed.starts_with('$') && (trimmed.contains("=@'") || trimmed.contains("= @'"))
        || trimmed.starts_with('$') && (trimmed.contains("=@\"") || trimmed.contains("= @\""))
}

fn is_pipeline_input_expression(value: &str) -> bool {
    let trimmed = value.trim();
    is_here_string_literal(trimmed)
        || quoted_literal(trimmed)
        || simple_variable_name(trimmed).is_some()
}

fn is_here_string_literal(value: &str) -> bool {
    let trimmed = value.trim();
    (trimmed.starts_with("@'") && trimmed.ends_with("'@"))
        || (trimmed.starts_with("@\"") && trimmed.ends_with("\"@"))
}

fn quoted_literal(value: &str) -> bool {
    value.len() >= 2
        && (value.starts_with('\'') && value.ends_with('\'')
            || value.starts_with('"') && value.ends_with('"'))
}

fn apply_context_to_parsed(
    parsed: ParsedCommand,
    cwd: Option<&str>,
    prior_list_path: &mut Option<String>,
) -> ParsedCommand {
    match parsed {
        ParsedCommand::Read { cmd, name, path } => {
            let path = apply_cwd_to_path(cwd, path);
            *prior_list_path = None;
            ParsedCommand::Read { cmd, name, path }
        }
        ParsedCommand::ListFiles { cmd, path } => {
            let path = path.or_else(|| cwd.map(short_display_path));
            *prior_list_path = path.clone();
            ParsedCommand::ListFiles { cmd, path }
        }
        ParsedCommand::Search { cmd, query, path } => {
            let path = path.or_else(|| prior_list_path.clone());
            *prior_list_path = None;
            ParsedCommand::Search { cmd, query, path }
        }
        ParsedCommand::Action { cmd, kind, detail } => {
            *prior_list_path = None;
            ParsedCommand::Action { cmd, kind, detail }
        }
        ParsedCommand::Unknown { cmd } => ParsedCommand::Unknown { cmd },
    }
}

fn normalize_part_tokens(
    mut tokens: Vec<String>,
    variables: &HashMap<String, String>,
    pending_wrapping_closes: &mut usize,
) -> Vec<String> {
    trim_wrapping_parentheses(&mut tokens, pending_wrapping_closes);
    tokens.retain(|token| !token.is_empty());
    tokens
        .into_iter()
        .map(|token| resolved_variable_value(&token, variables).unwrap_or(token))
        .collect()
}

fn trim_wrapping_parentheses(tokens: &mut [String], pending_wrapping_closes: &mut usize) {
    if let Some(first) = tokens.first_mut() {
        let leading_opens = first.chars().take_while(|ch| *ch == '(').count();
        if leading_opens > 0 {
            let trimmed = first.trim_start_matches('(').to_string();
            if !trimmed.is_empty() && looks_like_wrapped_command_head(&trimmed) {
                *first = trimmed;
                *pending_wrapping_closes += leading_opens;
            }
        }
    }
    if *pending_wrapping_closes == 0 {
        return;
    }
    if let Some(last) = tokens.last_mut() {
        let removed = strip_trailing_closing_parens(last, *pending_wrapping_closes);
        *pending_wrapping_closes -= removed;
    }
}

fn looks_like_wrapped_command_head(head: &str) -> bool {
    let head_lower = head.to_ascii_lowercase();
    matches!(
        head_lower.as_str(),
        "get-content"
            | "gc"
            | "cat"
            | "type"
            | "get-childitem"
            | "gci"
            | "dir"
            | "ls"
            | "get-item"
            | "gi"
            | "select-string"
            | "sls"
            | "select-object"
            | "select"
            | "sort-object"
            | "sort"
            | "measure-object"
            | "measure"
            | "out-string"
            | "format-table"
            | "get-location"
            | "pwd"
            | "out-null"
            | "set-location"
            | "cd"
            | "chdir"
            | "sl"
            | "rg"
            | "git"
            | "ffmpeg"
            | "ffmpeg.exe"
            | "get-command"
            | "get-process"
            | "get-service"
            | "test-path"
            | "resolve-path"
            | "get-itemproperty"
            | "get-acl"
            | "get-filehash"
            | "import-csv"
            | "import-clixml"
            | "select-xml"
            | "more"
            | "findstr"
            | "where.exe"
            | "cmd"
            | "foreach-object"
            | "foreach"
            | "%"
            | "where-object"
            | "?"
    ) || simple_variable_name(head).is_some()
        || head.contains('=')
}

fn strip_trailing_closing_parens(token: &mut String, max_to_strip: usize) -> usize {
    let trailing = token.chars().rev().take_while(|ch| *ch == ')').count();
    let removed = trailing.min(max_to_strip);
    if removed > 0 {
        token.truncate(token.len() - removed);
    }
    removed
}

fn simplify_powershell_commands(mut commands: Vec<ParsedCommand>) -> Vec<ParsedCommand> {
    while let Some(next) = simplify_powershell_commands_once(&commands) {
        commands = next;
    }
    commands
}

fn simplify_powershell_commands_once(commands: &[ParsedCommand]) -> Option<Vec<ParsedCommand>> {
    for (idx, pair) in commands.windows(2).enumerate() {
        if let Some(merged_search) = simplify_search_source_pair(pair) {
            let mut merged = Vec::with_capacity(commands.len() - 1);
            merged.extend_from_slice(&commands[..idx]);
            merged.push(merged_search);
            merged.extend_from_slice(&commands[idx + 2..]);
            return Some(merged);
        }
    }
    None
}

fn simplify_search_source_pair(pair: &[ParsedCommand]) -> Option<ParsedCommand> {
    let [
        source,
        ParsedCommand::Search {
            cmd,
            query,
            path: search_path,
        },
    ] = pair
    else {
        return None;
    };

    let display_path = match source {
        ParsedCommand::Read {
            path: read_path, ..
        } => short_display_path(&read_path.to_string_lossy()),
        ParsedCommand::ListFiles {
            path: Some(list_path),
            ..
        } => list_path.clone(),
        _ => return None,
    };
    let should_merge = match search_path.as_deref() {
        None => true,
        Some(existing) => existing == display_path,
    };
    should_merge.then(|| ParsedCommand::Search {
        cmd: cmd.clone(),
        query: query.clone(),
        path: Some(display_path),
    })
}

fn strip_utf8_prefix(script: &str) -> &str {
    script.strip_prefix(UTF8_OUTPUT_PREFIX).unwrap_or(script)
}

#[derive(Debug)]
struct Part {
    tokens: Vec<String>,
}

fn split_parts(script: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut chars = script.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else if ch == '`' {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            } else {
                cur.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            '`' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            ' ' | '\t' | '\r' | '\n' => push_token(&mut tokens, &mut cur),
            '|' | ';' => {
                push_token(&mut tokens, &mut cur);
                push_part(&mut parts, &mut tokens);
            }
            '&' => {
                if chars.peek().is_some_and(|next| *next == '&') {
                    chars.next();
                    push_token(&mut tokens, &mut cur);
                    push_part(&mut parts, &mut tokens);
                } else {
                    return None;
                }
            }
            _ => cur.push(ch),
        }
    }

    if quote.is_some() {
        return None;
    }
    push_token(&mut tokens, &mut cur);
    push_part(&mut parts, &mut tokens);
    Some(parts)
}

fn push_token(tokens: &mut Vec<String>, cur: &mut String) {
    if !cur.is_empty() {
        tokens.push(std::mem::take(cur));
    }
}

fn push_part(parts: &mut Vec<Part>, tokens: &mut Vec<String>) {
    if !tokens.is_empty() {
        parts.push(Part {
            tokens: std::mem::take(tokens),
        });
    }
}

fn summarize_tokens(tokens: &[String]) -> ParsedCommand {
    let Some((head, tail)) = tokens.split_first() else {
        return ParsedCommand::Unknown { cmd: String::new() };
    };
    let head_lower = head.to_ascii_lowercase();
    match head_lower.as_str() {
        "get-content" | "gc" | "cat" | "type" => summarize_read(tokens, tail),
        "get-childitem" | "gci" | "dir" | "ls" => summarize_list(tokens, tail),
        "get-item" | "gi" => {
            action_from_tokens(tokens).unwrap_or_else(|| summarize_list(tokens, tail))
        }
        "import-csv" | "import-clixml" => summarize_read(tokens, tail),
        "select-xml" => summarize_select_xml(tokens, tail),
        "cmd" => summarize_cmd(tokens, tail),
        "more" => summarize_read(tokens, tail),
        "findstr" => summarize_findstr(tokens, tail),
        "select-string" | "sls" => summarize_select_string(tokens, tail),
        "rg" => summarize_rg(tokens, tail),
        "git" => summarize_git(tokens, tail),
        "ffmpeg" | "ffmpeg.exe" => summarize_ffmpeg(tokens, tail),
        "get-command" => summarize_get_command(tokens, tail),
        _ => summarize_file_method(tokens)
            .or_else(|| action_from_tokens(tokens))
            .unwrap_or_else(|| ParsedCommand::Unknown {
                cmd: ps_join(tokens),
            }),
    }
}

fn summarize_read(tokens: &[String], args: &[String]) -> ParsedCommand {
    match single_path_operand(args) {
        Some(path) => ParsedCommand::Read {
            cmd: ps_join(tokens),
            name: short_display_path(&path),
            path: PathBuf::from(path),
        },
        None => ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        },
    }
}

fn summarize_list(tokens: &[String], args: &[String]) -> ParsedCommand {
    ParsedCommand::ListFiles {
        cmd: ps_join(tokens),
        path: summarize_path_targets(path_operands(args)),
    }
}

fn summarize_select_string(tokens: &[String], args: &[String]) -> ParsedCommand {
    let operands = select_string_operands(args);
    let query = named_value(args, &["-pattern", "-simplematch", "-regex"])
        .or_else(|| operands.first().cloned());
    let path =
        summarize_path_targets(named_path_values(args, &["-path", "-literalpath"])).or_else(|| {
            let path_index = usize::from(query == operands.first().cloned());
            summarize_path_targets(operands.into_iter().skip(path_index).collect())
        });
    ParsedCommand::Search {
        cmd: ps_join(tokens),
        query,
        path,
    }
}

fn summarize_select_xml(tokens: &[String], args: &[String]) -> ParsedCommand {
    let path = named_value(args, &["-path", "-literalpath"])
        .or_else(|| path_operands(args).into_iter().next());
    match path {
        Some(path) => ParsedCommand::Read {
            cmd: ps_join(tokens),
            name: short_display_path(&path),
            path: PathBuf::from(path),
        },
        None => ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        },
    }
}

fn summarize_cmd(tokens: &[String], args: &[String]) -> ParsedCommand {
    match args {
        [flag, subcmd, path]
            if flag.eq_ignore_ascii_case("/c") && subcmd.eq_ignore_ascii_case("type") =>
        {
            ParsedCommand::Read {
                cmd: ps_join(tokens),
                name: short_display_path(path),
                path: PathBuf::from(path),
            }
        }
        _ => ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        },
    }
}

fn summarize_findstr(tokens: &[String], args: &[String]) -> ParsedCommand {
    let operands = positional_operands_skipping(args, &["/c:", "/g:", "/f:"]);
    let query = operands.first().cloned();
    let path = summarize_path_targets(operands.into_iter().skip(1).collect());
    ParsedCommand::Search {
        cmd: ps_join(tokens),
        query,
        path,
    }
}

fn summarize_file_method(tokens: &[String]) -> Option<ParsedCommand> {
    let [token] = tokens else {
        return None;
    };
    let lower = token.to_ascii_lowercase();
    let method = [
        "[io.file]::readalltext",
        "[io.file]::readalllines",
        "[io.file]::readlines",
        "[system.io.file]::readalltext",
        "[system.io.file]::readalllines",
        "[system.io.file]::readlines",
    ]
    .into_iter()
    .find(|method| lower.starts_with(method))?;
    let open = method.len();
    let args = token.get(open..)?.trim();
    let path = args
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let path = powershell_literal(&path).unwrap_or(path);
    Some(ParsedCommand::Read {
        cmd: ps_join(tokens),
        name: short_display_path(&path),
        path: PathBuf::from(path),
    })
}

fn summarize_rg(tokens: &[String], args: &[String]) -> ParsedCommand {
    let has_files_flag = args.iter().any(|arg| arg.eq_ignore_ascii_case("--files"));
    let operands = positional_operands_skipping(
        args,
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
    );
    if has_files_flag {
        ParsedCommand::ListFiles {
            cmd: ps_join(tokens),
            path: summarize_path_targets(operands.first().cloned().into_iter().collect()),
        }
    } else {
        ParsedCommand::Search {
            cmd: ps_join(tokens),
            query: operands.first().cloned(),
            path: summarize_path_targets(operands.into_iter().skip(1).collect()),
        }
    }
}

fn summarize_git(tokens: &[String], args: &[String]) -> ParsedCommand {
    match args.split_first() {
        Some((subcmd, sub_tail)) if subcmd == "grep" => {
            let operands = positional_operands_skipping(
                sub_tail,
                &[
                    "-e",
                    "--regexp",
                    "-f",
                    "--file",
                    "-m",
                    "--max-count",
                    "-a",
                    "-b",
                    "-c",
                    "--context",
                ],
            );
            let query = named_value(sub_tail, &["-e", "--regexp", "-f", "--file"])
                .or_else(|| operands.first().cloned());
            let path_index = usize::from(query == operands.first().cloned());
            ParsedCommand::Search {
                cmd: ps_join(tokens),
                query,
                path: summarize_path_targets(operands.into_iter().skip(path_index).collect()),
            }
        }
        Some((subcmd, sub_tail)) if subcmd == "ls-files" => ParsedCommand::ListFiles {
            cmd: ps_join(tokens),
            path: summarize_path_targets(
                positional_operands_skipping(
                    sub_tail,
                    &["--exclude", "--exclude-from", "--pathspec-from-file"],
                )
                .into_iter()
                .take(1)
                .collect(),
            ),
        },
        _ => action_from_tokens(tokens).unwrap_or_else(|| ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        }),
    }
}

fn summarize_ffmpeg(tokens: &[String], args: &[String]) -> ParsedCommand {
    if args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "-version" | "-v" | "--version"))
        && let Some(action) = action_from_tokens(tokens)
    {
        return action;
    }

    match ffmpeg_input_path(args) {
        Some(path) => ParsedCommand::Read {
            cmd: shlex_join(&["ffmpeg".to_string(), "-i".to_string(), path.clone()]),
            name: short_display_path(&path),
            path: PathBuf::from(path),
        },
        None => ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        },
    }
}

fn ffmpeg_input_path(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-i" {
            return args.get(i + 1).cloned();
        }
        if let Some((flag, value)) = arg.split_once('=')
            && flag == "-i"
        {
            return Some(value.to_string());
        }
        if ffmpeg_flag_consumes_next_value(arg) {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn summarize_get_command(tokens: &[String], args: &[String]) -> ParsedCommand {
    let operands = positional_operands_skipping(args, &["-erroraction"]);
    match operands.as_slice() {
        [tool] => ParsedCommand::Action {
            cmd: ps_join(tokens),
            kind: ParsedCommandActionKind::Inspect,
            detail: Some(format!("{tool} in PATH")),
        },
        _ => ParsedCommand::Unknown {
            cmd: ps_join(tokens),
        },
    }
}

fn simple_variable_assignment(tokens: &[String]) -> Option<(String, String)> {
    match tokens {
        [token] => assignment_parts(token),
        [lhs, eq, rhs] if eq == "=" => assignment_name(lhs).map(|name| (name, rhs.to_string())),
        _ => None,
    }
}

fn command_result_assignment(tokens: &[String]) -> Option<(String, ParsedCommand)> {
    let (name, rhs) = assignment_command_tokens(tokens)?;
    let parsed = summarize_tokens(&rhs);
    if matches!(parsed, ParsedCommand::Unknown { .. }) {
        return None;
    }
    Some((name, parsed))
}

fn assignment_command_tokens(tokens: &[String]) -> Option<(String, Vec<String>)> {
    match tokens {
        [lhs, eq, rhs @ ..] if eq == "=" && !rhs.is_empty() => {
            assignment_name(lhs).map(|name| (name, rhs.to_vec()))
        }
        [first, tail @ ..] => {
            let (lhs, rhs_head) = first.split_once('=')?;
            if rhs_head.is_empty() {
                return None;
            }
            let mut rhs = Vec::with_capacity(tail.len() + 1);
            rhs.push(rhs_head.to_string());
            rhs.extend_from_slice(tail);
            assignment_name(lhs).map(|name| (name, rhs))
        }
        _ => None,
    }
}

fn assignment_parts(token: &str) -> Option<(String, String)> {
    let (lhs, rhs) = token.split_once('=')?;
    assignment_name(lhs).map(|name| (name, rhs.to_string()))
}

fn assignment_name(token: &str) -> Option<String> {
    simple_variable_name(token)
}

fn resolved_variable_value(token: &str, variables: &HashMap<String, String>) -> Option<String> {
    let name = simple_variable_name(token)?;
    variables.get(&name).cloned()
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
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

fn single_path_operand(args: &[String]) -> Option<String> {
    let operands = path_operands(args);
    match operands.as_slice() {
        [path] => Some(path.clone()),
        _ => None,
    }
}

fn path_operand(args: &[String]) -> Option<String> {
    path_operands(args).into_iter().next()
}

fn path_operands(args: &[String]) -> Vec<String> {
    named_path_values(args, &["-path", "-literalpath"])
        .into_iter()
        .chain(
            positional_operands(args)
                .into_iter()
                .flat_map(|value| split_path_list(&value)),
        )
        .collect()
}

fn named_value(args: &[String], names: &[&str]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let lower = arg.to_ascii_lowercase();
        if let Some((name, _)) = lower.split_once('=')
            && names.contains(&name)
        {
            return arg.split_once('=').map(|(_, value)| value.to_string());
        }
        if names.contains(&lower.as_str()) {
            return args.get(i + 1).cloned();
        }
        i += 1;
    }
    None
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

fn summarize_path_targets(paths: Vec<String>) -> Option<String> {
    let display_paths: Vec<String> = paths
        .into_iter()
        .map(|path| short_display_path(&path))
        .collect();
    match display_paths.as_slice() {
        [] => None,
        [path] => Some(path.clone()),
        [first, rest @ ..] => Some(format!("{first} +{} more", rest.len())),
    }
}

fn split_path_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn select_string_operands(args: &[String]) -> Vec<String> {
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
            "-context",
        ],
    )
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

fn is_set_location(cmd: &str) -> bool {
    matches!(cmd, "set-location" | "cd" | "chdir" | "sl")
}

fn is_push_location(cmd: &str) -> bool {
    matches!(cmd, "push-location" | "pushd")
}

fn is_pop_location(cmd: &str, args: &[String]) -> bool {
    matches!(cmd, "pop-location" | "popd") && args.iter().all(|arg| arg.starts_with('-'))
}

fn is_formatting_helper(cmd: &str, args: &[String]) -> bool {
    matches!(
        cmd,
        "select-object"
            | "select"
            | "sort-object"
            | "sort"
            | "measure-object"
            | "measure"
            | "out-string"
            | "format-table"
            | "get-location"
            | "pwd"
            | "out-null"
    ) || is_simple_foreach_projection(cmd, args)
        || is_simple_where_filter(cmd, args)
}

fn is_safe_setup_helper(cmd: &str, args: &[String]) -> bool {
    matches!(cmd, "new-item" | "ni") && is_temp_directory_creation(args)
}

fn is_temp_directory_creation(args: &[String]) -> bool {
    let item_type = named_value(args, &["-itemtype", "-type"]);
    if !item_type.is_some_and(|value| value.eq_ignore_ascii_case("Directory")) {
        return false;
    }
    if !args.iter().any(|arg| arg.eq_ignore_ascii_case("-force")) {
        return false;
    }
    let Some(path) = path_operand(args) else {
        return false;
    };
    let normalized = path.replace('/', "\\").to_ascii_lowercase();
    normalized.contains("\\.tmp\\") || normalized.ends_with("\\.tmp")
}

fn ffmpeg_flag_consumes_next_value(flag: &str) -> bool {
    matches!(
        flag.to_ascii_lowercase().as_str(),
        "-filter"
            | "-filter_complex"
            | "-loglevel"
            | "-map"
            | "-pattern_type"
            | "-r"
            | "-s"
            | "-ss"
            | "-t"
            | "-vf"
            | "-vframes"
            | "-frames:v"
    )
}

fn is_simple_foreach_projection(cmd: &str, args: &[String]) -> bool {
    if !matches!(cmd, "foreach-object" | "foreach" | "%") {
        return false;
    }

    let normalized: Vec<String> = args
        .iter()
        .map(|arg| arg.trim().to_ascii_lowercase())
        .filter(|arg| !arg.is_empty())
        .collect();
    matches!(
        normalized.as_slice(),
        [open, expr, close]
            if open == "{"
                && close == "}"
                && matches!(
                    expr.as_str(),
                    "$_.fullname"
                        | "$_.full_name"
                        | "$_.name"
                        | "$_.path"
                        | "$_.line"
                        | "$_.tostring()"
                )
                || is_safe_select_string_match_projection(expr)
    )
}

fn is_safe_select_string_match_projection(expr: &str) -> bool {
    matches!(
        expr,
        "$($_.path):$($_.linenumber): $($_.line.trim())"
            | "$($_.path):$($_.linenumber):$($_.line.trim())"
    )
}

fn is_mutating_or_ambiguous(cmd: &str) -> bool {
    matches!(
        cmd,
        "set-content"
            | "sc"
            | "add-content"
            | "ac"
            | "remove-item"
            | "rm"
            | "rmdir"
            | "del"
            | "erase"
            | "new-item"
            | "ni"
            | "out-file"
            | "copy-item"
            | "cp"
            | "cpi"
            | "move-item"
            | "mv"
            | "mi"
            | "rename-item"
            | "ren"
            | "invoke-expression"
            | "iex"
            | "foreach-object"
            | "%"
            | "where-object"
            | "?"
    )
}

fn is_mutating_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "set-content"
            | "sc"
            | "add-content"
            | "ac"
            | "remove-item"
            | "rm"
            | "rmdir"
            | "del"
            | "erase"
            | "new-item"
            | "ni"
            | "out-file"
            | "copy-item"
            | "cp"
            | "cpi"
            | "move-item"
            | "mv"
            | "mi"
            | "rename-item"
            | "ren"
    )
}

fn is_simple_where_filter(cmd: &str, args: &[String]) -> bool {
    if !matches!(cmd, "where-object" | "?") {
        return false;
    }
    let normalized: Vec<String> = args
        .iter()
        .map(|arg| arg.trim().to_ascii_lowercase())
        .filter(|arg| !arg.is_empty())
        .collect();
    matches!(
        normalized.as_slice(),
        [open, field, op, _value, close]
            if open == "{"
                && close == "}"
                && field.starts_with("$_." )
                && matches!(
                    op.as_str(),
                    "-eq" | "-ne" | "-like" | "-notlike" | "-match" | "-notmatch"
                )
    )
}

fn unknown(script: &str) -> ParsedCommand {
    ParsedCommand::Unknown {
        cmd: script.to_string(),
    }
}

fn edit_action(script: &str) -> ParsedCommand {
    ParsedCommand::Action {
        cmd: script.to_string(),
        kind: ParsedCommandActionKind::Edit,
        detail: Some(script.to_string()),
    }
}

fn apply_cwd_to_path(cwd: Option<&str>, path: PathBuf) -> PathBuf {
    let Some(cwd) = cwd else {
        return path;
    };
    let path_str = path.to_string_lossy();
    PathBuf::from(join_paths(cwd, &path_str))
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

fn join_paths(base: &str, rel: &str) -> String {
    if is_abs_like(rel) {
        return rel.to_string();
    }
    if base.is_empty() {
        return rel.to_string();
    }
    let mut buf = PathBuf::from(base);
    buf.push(rel);
    buf.to_string_lossy().to_string()
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

fn ps_join(tokens: &[String]) -> String {
    shlex_join(tokens)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::powershell::try_find_powershell_executable_blocking;
    use pretty_assertions::assert_eq;

    #[test]
    fn ast_backed_parse_keeps_regex_like_pattern_and_simplifies_pipeline() {
        let Some(powershell) = try_find_powershell_executable_blocking() else {
            return;
        };

        let parsed = parse_powershell_script(
            powershell.as_path().to_str(),
            "Get-Content .\\EticketContext.cs | Select-String -Pattern 'Entity<Asset>|e => e.AssetTag' -Context 0,20",
        );

        assert_eq!(
            parsed,
            vec![ParsedCommand::Search {
                cmd: "Select-String -Pattern 'Entity<Asset>|e => e.AssetTag' -Context '0,20'"
                    .to_string(),
                query: Some("Entity<Asset>|e => e.AssetTag".to_string()),
                path: Some("EticketContext.cs".to_string()),
            }],
        );
    }
}
