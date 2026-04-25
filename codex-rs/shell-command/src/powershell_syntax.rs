pub(crate) fn split_top_level_statements(script: &str) -> Option<Vec<String>> {
    split_top_level(script, StatementSeparator::Statement)
}

pub(crate) fn split_pipeline_parts(script: &str) -> Option<Vec<String>> {
    split_top_level(script, StatementSeparator::Char('|'))
}

enum StatementSeparator {
    Statement,
    Char(char),
}

fn split_top_level(script: &str, separator: StatementSeparator) -> Option<Vec<String>> {
    let mut parts = Vec::new();
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
            ch if is_separator(ch, &separator)
                && paren_depth == 0
                && brace_depth == 0
                && bracket_depth == 0 =>
            {
                push_part(script, start, idx, &mut parts);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if quote.is_some() || paren_depth != 0 || brace_depth != 0 || bracket_depth != 0 {
        return None;
    }
    push_part(script, start, script.len(), &mut parts);
    Some(parts)
}

fn is_separator(ch: char, separator: &StatementSeparator) -> bool {
    match separator {
        StatementSeparator::Statement => ch == ';' || ch == '\n',
        StatementSeparator::Char(separator) => ch == *separator,
    }
}

fn push_part(script: &str, start: usize, end: usize, parts: &mut Vec<String>) {
    let part = script[start..end].trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }
}

pub(crate) fn strip_wrapping_parens(value: &str) -> &str {
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

pub(crate) fn split_command_tokens(command: &str) -> Option<Vec<String>> {
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
            '|' | ';' | '&' | '<' | '>' => return None,
            '{' | '}' => {
                push_token(&mut tokens, &mut current);
                tokens.push(ch.to_string());
            }
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

pub(crate) fn powershell_literal(value: &str) -> Option<String> {
    if let Some(inner) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Some(inner.replace("''", "'"));
    }
    if let Some(inner) = value.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return unescape_double_quoted_literal(inner);
    }
    None
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

pub(crate) fn matching_delimiter(
    value: &str,
    open_idx: usize,
    open: char,
    close: char,
) -> Option<usize> {
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

pub(crate) fn strip_case_insensitive_keyword<'a>(value: &'a str, keyword: &str) -> Option<&'a str> {
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

pub(crate) fn simple_variable_name(token: &str) -> Option<String> {
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
