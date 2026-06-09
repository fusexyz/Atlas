use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum PpToken {
    Identifier(String),
    Number(String),
    StringLit(String),
    CharLit(String),
    Punct(char),
    DoubleHash,
    Space(String),
    Newline,
}

#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub args: Option<Vec<String>>,
    pub body: Vec<PpToken>,
}

#[derive(Debug, Clone)]
struct ConditionalState {
    cond_met: bool,
    is_active: bool,
}

pub fn preprocess(
    text: &str,
    filepath: &Path,
    include_dirs: &[PathBuf],
    defines: &mut HashMap<String, MacroDef>,
    active_includes: &mut HashSet<PathBuf>,
    skip_headers: &HashSet<String>,
) -> Result<String, String> {
    if active_includes.contains(filepath) {
        return Err(format!(
            "infinite include recursion detected: {:?}",
            filepath
        ));
    }
    active_includes.insert(filepath.to_path_buf());

    let merged_text = merge_backslash_lines(text);
    let tokens = tokenize(&merged_text);

    let mut result = String::new();
    let mut cond_stack = Vec::new();
    let mut i = 0;
    let len = tokens.len();

    while i < len {
        let mut line_tokens = Vec::new();
        while i < len {
            let tok = &tokens[i];
            line_tokens.push(tok.clone());
            i += 1;
            if *tok == PpToken::Newline {
                break;
            }
        }

        let mut directive_idx = 0;
        while directive_idx < line_tokens.len() {
            if let PpToken::Space(_) = &line_tokens[directive_idx] {
                directive_idx += 1;
            } else {
                break;
            }
        }

        let is_directive =
            directive_idx < line_tokens.len() && line_tokens[directive_idx] == PpToken::Punct('#');

        if is_directive {
            let mut cmd_idx = directive_idx + 1;
            while cmd_idx < line_tokens.len() {
                if let PpToken::Space(_) = &line_tokens[cmd_idx] {
                    cmd_idx += 1;
                } else {
                    break;
                }
            }

            if cmd_idx < line_tokens.len() {
                if let PpToken::Identifier(ref cmd) = line_tokens[cmd_idx] {
                    let rest = &line_tokens[cmd_idx + 1..];
                    match cmd.as_str() {
                        "define" => {
                            if should_emit(&cond_stack) {
                                parse_define(rest, defines)?;
                            }
                        }
                        "undef" => {
                            if should_emit(&cond_stack) {
                                parse_undef(rest, defines)?;
                            }
                        }
                        "include" => {
                            if should_emit(&cond_stack) {
                                let included_content = resolve_include(
                                    rest,
                                    filepath,
                                    include_dirs,
                                    defines,
                                    active_includes,
                                    skip_headers,
                                )?;
                                result.push_str(&included_content);
                            }
                        }
                        "ifdef" => {
                            let parent_active = should_emit(&cond_stack);
                            let macro_name = get_first_identifier(rest)?;
                            let exists = defines.contains_key(&macro_name);
                            cond_stack.push(ConditionalState {
                                cond_met: exists,
                                is_active: parent_active && exists,
                            });
                        }
                        "ifndef" => {
                            let parent_active = should_emit(&cond_stack);
                            let macro_name = get_first_identifier(rest)?;
                            let exists = defines.contains_key(&macro_name);
                            cond_stack.push(ConditionalState {
                                cond_met: !exists,
                                is_active: parent_active && !exists,
                            });
                        }
                        "if" => {
                            let parent_active = should_emit(&cond_stack);
                            let cond_val = evaluate_expression(rest, defines);
                            let met = cond_val != 0;
                            cond_stack.push(ConditionalState {
                                cond_met: met,
                                is_active: parent_active && met,
                            });
                        }
                        "elif" => {
                            let parent_active = if cond_stack.len() <= 1 {
                                true
                            } else {
                                cond_stack[cond_stack.len() - 2].is_active
                            };
                            if let Some(state) = cond_stack.last_mut() {
                                if state.cond_met {
                                    state.is_active = false;
                                } else {
                                    let cond_val = evaluate_expression(rest, defines);
                                    let met = cond_val != 0;
                                    state.is_active = parent_active && met;
                                    if met {
                                        state.cond_met = true;
                                    }
                                }
                            } else {
                                return Err("unmatched #elif".to_string());
                            }
                        }
                        "else" => {
                            let parent_active = if cond_stack.len() <= 1 {
                                true
                            } else {
                                cond_stack[cond_stack.len() - 2].is_active
                            };
                            if let Some(state) = cond_stack.last_mut() {
                                state.is_active = parent_active && !state.cond_met;
                                state.cond_met = true;
                            } else {
                                return Err("unmatched #else".to_string());
                            }
                        }
                        "endif" => {
                            if cond_stack.pop().is_none() {
                                return Err("unmatched #endif".to_string());
                            }
                        }
                        "line" | "error" => {}
                        "pragma" => {
                            if should_emit(&cond_stack) {
                                if let Some(pragma_str) = handle_pragma(rest) {
                                    result.push_str(&pragma_str);
                                }
                            }
                        }
                        _ => {
                            return Err(format!("unknown preprocessor directive: {}", cmd));
                        }
                    }
                }
            }
            result.push('\n');
        } else {
            if should_emit(&cond_stack) {
                let mut idx = 0;
                while idx < line_tokens.len() {
                    if let PpToken::Identifier(ref name) = line_tokens[idx] {
                        if let Some(mdef) = defines.get(name) {
                            if mdef.args.is_some() {
                                let mut next = idx + 1;
                                while next < line_tokens.len()
                                    && matches!(line_tokens[next], PpToken::Space(_))
                                {
                                    next += 1;
                                }
                                if next < line_tokens.len()
                                    && line_tokens[next] == PpToken::Punct('(')
                                {
                                    let mut depth = 1;
                                    let mut scan = next + 1;
                                    while scan < line_tokens.len() {
                                        if line_tokens[scan] == PpToken::Punct('(') {
                                            depth += 1;
                                        } else if line_tokens[scan] == PpToken::Punct(')') {
                                            depth -= 1;
                                            if depth == 0 {
                                                break;
                                            }
                                        }
                                        scan += 1;
                                    }

                                    if depth > 0 {
                                        while i < len && depth > 0 {
                                            let tok = &tokens[i];
                                            line_tokens.push(tok.clone());
                                            i += 1;
                                            if *tok == PpToken::Punct('(') {
                                                depth += 1;
                                            } else if *tok == PpToken::Punct(')') {
                                                depth -= 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    idx += 1;
                }

                let expanded = expand_macros(&line_tokens, defines, &mut HashSet::new());
                result.push_str(&stringify_tokens(&expanded));
            } else {
                result.push('\n');
            }
        }
    }

    if !cond_stack.is_empty() {
        return Err("unterminated conditional compilation block (missing #endif)".to_string());
    }

    active_includes.remove(filepath);
    Ok(result)
}

fn merge_backslash_lines(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some('\n') = chars.peek() {
                chars.next();
            } else if let Some('\r') = chars.peek() {
                chars.next();
                if let Some('\n') = chars.peek() {
                    chars.next();
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn tokenize(text: &str) -> Vec<PpToken> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let len = chars.len();
    let mut tokens = Vec::new();

    while i < len {
        let c = chars[i];
        if c == ' ' || c == '\t' {
            let mut s = String::new();
            while i < len && (chars[i] == ' ' || chars[i] == '\t') {
                s.push(chars[i]);
                i += 1;
            }
            tokens.push(PpToken::Space(s));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && i + 1 < len && chars[i + 1] == '\n' {
                i += 1;
            }
            tokens.push(PpToken::Newline);
            i += 1;
        } else if c == '/' && i + 1 < len && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' && chars[i] != '\r' {
                i += 1;
            }
        } else if c == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i < len {
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            tokens.push(PpToken::Space(" ".to_string()));
        } else if c == '"' {
            let mut s = String::new();
            s.push('"');
            i += 1;
            while i < len {
                if chars[i] == '\\' {
                    s.push('\\');
                    if i + 1 < len {
                        s.push(chars[i + 1]);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if chars[i] == '"' {
                    s.push('"');
                    i += 1;
                    break;
                } else {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            tokens.push(PpToken::StringLit(s));
        } else if c == '\'' {
            let mut s = String::new();
            s.push('\'');
            i += 1;
            while i < len {
                if chars[i] == '\\' {
                    s.push('\\');
                    if i + 1 < len {
                        s.push(chars[i + 1]);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else if chars[i] == '\'' {
                    s.push('\'');
                    i += 1;
                    break;
                } else {
                    s.push(chars[i]);
                    i += 1;
                }
            }
            tokens.push(PpToken::CharLit(s));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                s.push(chars[i]);
                i += 1;
            }
            tokens.push(PpToken::Identifier(s));
        } else if c.is_ascii_digit() {
            let mut s = String::new();
            while i < len && chars[i].is_ascii_alphanumeric() {
                s.push(chars[i]);
                i += 1;
            }
            tokens.push(PpToken::Number(s));
        } else if c == '#' && i + 1 < len && chars[i + 1] == '#' {
            tokens.push(PpToken::DoubleHash);
            i += 2;
        } else {
            tokens.push(PpToken::Punct(c));
            i += 1;
        }
    }
    tokens
}

fn should_emit(cond_stack: &[ConditionalState]) -> bool {
    cond_stack.iter().all(|s| s.is_active)
}

fn get_first_identifier(tokens: &[PpToken]) -> Result<String, String> {
    for t in tokens {
        match t {
            PpToken::Space(_) => {}
            PpToken::Identifier(name) => return Ok(name.clone()),
            _ => return Err("expected macro name after directive".to_string()),
        }
    }
    Err("expected macro name".to_string())
}

fn parse_define(tokens: &[PpToken], defines: &mut HashMap<String, MacroDef>) -> Result<(), String> {
    let mut i = 0;
    let len = tokens.len();

    while i < len && matches!(tokens[i], PpToken::Space(_)) {
        i += 1;
    }
    if i >= len {
        return Err("expected macro name".to_string());
    }

    let macro_name = match &tokens[i] {
        PpToken::Identifier(name) => name.clone(),
        _ => return Err("expected identifier as macro name".to_string()),
    };
    i += 1;

    let is_fn_like = i < len && tokens[i] == PpToken::Punct('(');

    let mut macro_args = None;
    if is_fn_like {
        i += 1;
        let mut args = Vec::new();
        let mut expecting_arg = true;
        while i < len {
            match &tokens[i] {
                PpToken::Space(_) => {
                    i += 1;
                }
                PpToken::Punct(')') => {
                    i += 1;
                    break;
                }
                PpToken::Identifier(arg_name) => {
                    if !expecting_arg {
                        return Err(
                            "expected comma or closing parenthesis in macro arguments".to_string()
                        );
                    }
                    args.push(arg_name.clone());
                    expecting_arg = false;
                    i += 1;
                }
                PpToken::Punct(',') => {
                    if expecting_arg {
                        return Err("unexpected comma in macro arguments".to_string());
                    }
                    expecting_arg = true;
                    i += 1;
                }
                PpToken::Punct('.') => {
                    if i + 2 < len
                        && tokens[i + 1] == PpToken::Punct('.')
                        && tokens[i + 2] == PpToken::Punct('.')
                    {
                        if !expecting_arg {
                            return Err(
                                "expected comma before ellipsis in macro arguments".to_string()
                            );
                        }
                        args.push("...".to_string());
                        expecting_arg = false;
                        i += 3;
                    } else {
                        return Err("invalid character in macro arguments".to_string());
                    }
                }
                _ => return Err("invalid character in macro arguments".to_string()),
            }
        }
        macro_args = Some(args);
    }

    let mut body = Vec::new();
    while i < len {
        if tokens[i] == PpToken::Newline {
            break;
        }
        body.push(tokens[i].clone());
        i += 1;
    }

    while let Some(PpToken::Space(_)) = body.last() {
        body.pop();
    }

    defines.insert(
        macro_name.clone(),
        MacroDef {
            name: macro_name,
            args: macro_args,
            body,
        },
    );

    Ok(())
}

fn parse_undef(tokens: &[PpToken], defines: &mut HashMap<String, MacroDef>) -> Result<(), String> {
    let name = get_first_identifier(tokens)?;
    defines.remove(&name);
    Ok(())
}

fn resolve_include(
    tokens: &[PpToken],
    current_filepath: &Path,
    include_dirs: &[PathBuf],
    defines: &mut HashMap<String, MacroDef>,
    active_includes: &mut HashSet<PathBuf>,
    skip_headers: &HashSet<String>,
) -> Result<String, String> {
    let mut include_str = String::new();
    let mut is_system = false;

    for t in tokens {
        match t {
            PpToken::Space(_) | PpToken::Newline => {}
            PpToken::StringLit(s) => {
                if s.starts_with('"') && s.ends_with('"') {
                    include_str = s[1..s.len() - 1].to_string();
                }
            }
            PpToken::Punct('<') => {
                is_system = true;
            }
            PpToken::Punct('>') => {}
            _ => {
                if is_system {
                    include_str.push_str(&stringify_token_raw(t));
                }
            }
        }
    }

    if is_system {
        include_str = include_str.trim_end_matches('>').to_string();
    }

    if skip_headers.contains(&include_str) {
        return Ok(String::new());
    }

    let mut found_path = None;
    if !is_system {
        if let Some(parent) = current_filepath.parent() {
            let rel_path = parent.join(&include_str);
            if rel_path.exists() {
                found_path = Some(rel_path);
            }
        }
    }

    if found_path.is_none() {
        for dir in include_dirs {
            let p = dir.join(&include_str);
            if p.exists() {
                found_path = Some(p);
                break;
            }
        }
    }

    if let Some(path) = found_path {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read include file {:?}: {}", path, e))?;
        preprocess(
            &content,
            &path,
            include_dirs,
            defines,
            active_includes,
            skip_headers,
        )
    } else {
        Err(format!("could not resolve include file: {}", include_str))
    }
}

fn stringify_tokens(tokens: &[PpToken]) -> String {
    let mut s = String::new();
    for t in tokens {
        match t {
            PpToken::Identifier(x) => s.push_str(x),
            PpToken::Number(x) => s.push_str(x),
            PpToken::StringLit(x) => s.push_str(x),
            PpToken::CharLit(x) => s.push_str(x),
            PpToken::Punct(x) => s.push(*x),
            PpToken::DoubleHash => s.push_str("##"),
            PpToken::Space(x) => s.push_str(x),
            PpToken::Newline => s.push('\n'),
        }
    }
    s
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn process_token_concatenation(tokens: Vec<PpToken>) -> Vec<PpToken> {
    let mut result = Vec::new();
    let mut i = 0;
    let len = tokens.len();

    while i < len {
        if i + 1 < len && tokens[i + 1] == PpToken::DoubleHash {
            let lhs = &tokens[i];
            let mut rhs_idx = i + 2;
            while rhs_idx < len {
                if let PpToken::Space(_) = &tokens[rhs_idx] {
                    rhs_idx += 1;
                } else {
                    break;
                }
            }
            if rhs_idx < len {
                let rhs = &tokens[rhs_idx];
                let new_str = format!("{}{}", stringify_token_raw(lhs), stringify_token_raw(rhs));
                let mut lexed = tokenize(&new_str);
                lexed.retain(|t| !matches!(t, PpToken::Newline | PpToken::Space(_)));
                if !lexed.is_empty() {
                    result.push(lexed[0].clone());
                }
                i = rhs_idx + 1;
            } else {
                result.push(tokens[i].clone());
                i += 2;
            }
        } else {
            result.push(tokens[i].clone());
            i += 1;
        }
    }
    result
}

fn stringify_token_raw(t: &PpToken) -> String {
    match t {
        PpToken::Identifier(x) => x.clone(),
        PpToken::Number(x) => x.clone(),
        PpToken::StringLit(x) => x.clone(),
        PpToken::CharLit(x) => x.clone(),
        PpToken::Punct(x) => x.to_string(),
        PpToken::DoubleHash => "##".to_string(),
        PpToken::Space(x) => x.clone(),
        PpToken::Newline => "\n".to_string(),
    }
}

fn expand_macros(
    tokens: &[PpToken],
    defines: &HashMap<String, MacroDef>,
    expanding: &mut HashSet<String>,
) -> Vec<PpToken> {
    let mut result = Vec::new();
    let mut i = 0;
    let len = tokens.len();

    while i < len {
        match &tokens[i] {
            PpToken::Identifier(name) => {
                if defines.contains_key(name) && !expanding.contains(name) {
                    let mdef = &defines[name];
                    expanding.insert(name.clone());

                    if let Some(args) = &mdef.args {
                        let mut next_idx = i + 1;
                        while next_idx < len {
                            if let PpToken::Space(_) = &tokens[next_idx] {
                                next_idx += 1;
                            } else {
                                break;
                            }
                        }

                        if next_idx < len && tokens[next_idx] == PpToken::Punct('(') {
                            let mut args_tokens = Vec::new();
                            let mut current_arg = Vec::new();
                            let mut paren_depth = 0;
                            let mut arg_idx = next_idx + 1;

                            while arg_idx < len {
                                let tok = &tokens[arg_idx];
                                if *tok == PpToken::Punct('(') {
                                    paren_depth += 1;
                                    current_arg.push(tok.clone());
                                } else if *tok == PpToken::Punct(')') {
                                    if paren_depth == 0 {
                                        args_tokens.push(current_arg);
                                        arg_idx += 1;
                                        break;
                                    } else {
                                        paren_depth -= 1;
                                        current_arg.push(tok.clone());
                                    }
                                } else if *tok == PpToken::Punct(',') && paren_depth == 0 {
                                    args_tokens.push(current_arg);
                                    current_arg = Vec::new();
                                } else {
                                    current_arg.push(tok.clone());
                                }
                                arg_idx += 1;
                            }

                            let mut expanded_body = Vec::new();
                            let mut bi = 0;
                            let body_len = mdef.body.len();

                            while bi < body_len {
                                match &mdef.body[bi] {
                                    PpToken::Identifier(param_name) => {
                                        if param_name == "__VA_ARGS__" {
                                            if let Some(var_arg_pos) =
                                                args.iter().position(|x| x == "...")
                                            {
                                                let mut first = true;
                                                for arg in &args_tokens[var_arg_pos..] {
                                                    if !first {
                                                        expanded_body.push(PpToken::Punct(','));
                                                        expanded_body
                                                            .push(PpToken::Space(" ".to_string()));
                                                    }
                                                    first = false;
                                                    let expanded_arg =
                                                        expand_macros(arg, defines, expanding);
                                                    expanded_body.extend(expanded_arg);
                                                }
                                            } else {
                                                expanded_body.push(mdef.body[bi].clone());
                                            }
                                        } else if let Some(arg_pos) =
                                            args.iter().position(|x| x == param_name)
                                        {
                                            let actual_arg = args_tokens
                                                .get(arg_pos)
                                                .cloned()
                                                .unwrap_or_default();
                                            let expanded_arg =
                                                expand_macros(&actual_arg, defines, expanding);
                                            expanded_body.extend(expanded_arg);
                                        } else {
                                            expanded_body.push(mdef.body[bi].clone());
                                        }
                                    }
                                    PpToken::DoubleHash => {
                                        expanded_body.push(PpToken::DoubleHash);
                                    }
                                    PpToken::Punct('#') if bi + 1 < body_len => {
                                        if let PpToken::Identifier(param_name) = &mdef.body[bi + 1]
                                        {
                                            if param_name == "__VA_ARGS__" {
                                                if let Some(var_arg_pos) =
                                                    args.iter().position(|x| x == "...")
                                                {
                                                    let mut first = true;
                                                    let mut joined_tokens = Vec::new();
                                                    for arg in &args_tokens[var_arg_pos..] {
                                                        if !first {
                                                            joined_tokens.push(PpToken::Punct(','));
                                                            joined_tokens.push(PpToken::Space(
                                                                " ".to_string(),
                                                            ));
                                                        }
                                                        first = false;
                                                        joined_tokens.extend(arg.clone());
                                                    }
                                                    let arg_str = stringify_tokens(&joined_tokens);
                                                    expanded_body.push(PpToken::StringLit(
                                                        format!("\"{}\"", escape_string(&arg_str)),
                                                    ));
                                                    bi += 2;
                                                    continue;
                                                }
                                            } else if let Some(arg_pos) =
                                                args.iter().position(|x| x == param_name)
                                            {
                                                let actual_arg = args_tokens
                                                    .get(arg_pos)
                                                    .cloned()
                                                    .unwrap_or_default();
                                                let arg_str = stringify_tokens(&actual_arg);
                                                expanded_body.push(PpToken::StringLit(format!(
                                                    "\"{}\"",
                                                    escape_string(&arg_str)
                                                )));
                                                bi += 2;
                                                continue;
                                            }
                                        }
                                        expanded_body.push(mdef.body[bi].clone());
                                    }
                                    _ => {
                                        expanded_body.push(mdef.body[bi].clone());
                                    }
                                }
                                bi += 1;
                            }

                            let final_expanded_body = process_token_concatenation(expanded_body);
                            let final_expanded =
                                expand_macros(&final_expanded_body, defines, expanding);
                            result.extend(final_expanded);
                            i = arg_idx;
                        } else {
                            result.push(tokens[i].clone());
                            i += 1;
                        }
                    } else {
                        let expanded_body = expand_macros(&mdef.body, defines, expanding);
                        result.extend(expanded_body);
                        i += 1;
                    }

                    expanding.remove(name);
                } else {
                    result.push(tokens[i].clone());
                    i += 1;
                }
            }
            _ => {
                result.push(tokens[i].clone());
                i += 1;
            }
        }
    }

    result
}

fn evaluate_expression(tokens: &[PpToken], defines: &HashMap<String, MacroDef>) -> i64 {
    let mut resolved = Vec::new();
    let mut i = 0;
    let len = tokens.len();
    while i < len {
        if let PpToken::Identifier(ref name) = tokens[i] {
            if name == "defined" {
                let mut next = i + 1;
                while next < len && matches!(tokens[next], PpToken::Space(_)) {
                    next += 1;
                }
                if next < len && tokens[next] == PpToken::Punct('(') {
                    let mut ident_idx = next + 1;
                    while ident_idx < len && matches!(tokens[ident_idx], PpToken::Space(_)) {
                        ident_idx += 1;
                    }
                    if ident_idx < len {
                        if let PpToken::Identifier(ref mac) = tokens[ident_idx] {
                            let mut close_idx = ident_idx + 1;
                            while close_idx < len && matches!(tokens[close_idx], PpToken::Space(_))
                            {
                                close_idx += 1;
                            }
                            if close_idx < len && tokens[close_idx] == PpToken::Punct(')') {
                                let val = if defines.contains_key(mac) { "1" } else { "0" };
                                resolved.push(PpToken::Number(val.to_string()));
                                i = close_idx + 1;
                                continue;
                            }
                        }
                    }
                }
                let mut ident_idx = i + 1;
                while ident_idx < len && matches!(tokens[ident_idx], PpToken::Space(_)) {
                    ident_idx += 1;
                }
                if ident_idx < len {
                    if let PpToken::Identifier(ref mac) = tokens[ident_idx] {
                        let val = if defines.contains_key(mac) { "1" } else { "0" };
                        resolved.push(PpToken::Number(val.to_string()));
                        i = ident_idx + 1;
                        continue;
                    }
                }
            }
        }
        resolved.push(tokens[i].clone());
        i += 1;
    }

    let expanded = expand_macros(&resolved, defines, &mut HashSet::new());

    let mut clean_tokens = Vec::new();
    for t in expanded {
        match t {
            PpToken::Space(_) | PpToken::Newline => {}
            PpToken::Identifier(_) => {
                clean_tokens.push(PpToken::Number("0".to_string()));
            }
            _ => {
                clean_tokens.push(t);
            }
        }
    }

    let mut parser = PpExprParser::new(clean_tokens);
    parser.parse().unwrap_or(0)
}

struct PpExprParser {
    tokens: Vec<PpToken>,
    pos: usize,
}

impl PpExprParser {
    fn new(tokens: Vec<PpToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&PpToken> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&PpToken> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn parse(&mut self) -> Result<i64, String> {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_logical_and()?;
        while let Some(PpToken::Punct('|')) = self.peek() {
            if self.tokens.get(self.pos + 1) == Some(&PpToken::Punct('|')) {
                self.advance();
                self.advance();
                let rhs = self.parse_logical_and()?;
                lhs = if lhs != 0 || rhs != 0 { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_logical_and(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_equality()?;
        while let Some(PpToken::Punct('&')) = self.peek() {
            if self.tokens.get(self.pos + 1) == Some(&PpToken::Punct('&')) {
                self.advance();
                self.advance();
                let rhs = self.parse_equality()?;
                lhs = if lhs != 0 && rhs != 0 { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_relational()?;
        while let Some(PpToken::Punct(op)) = self.peek() {
            if *op == '=' && self.tokens.get(self.pos + 1) == Some(&PpToken::Punct('=')) {
                self.advance();
                self.advance();
                let rhs = self.parse_relational()?;
                lhs = if lhs == rhs { 1 } else { 0 };
            } else if *op == '!' && self.tokens.get(self.pos + 1) == Some(&PpToken::Punct('=')) {
                self.advance();
                self.advance();
                let rhs = self.parse_relational()?;
                lhs = if lhs != rhs { 1 } else { 0 };
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_additive()?;
        while let Some(PpToken::Punct(op)) = self.peek().cloned() {
            if op == '<' {
                self.advance();
                if self.peek() == Some(&PpToken::Punct('=')) {
                    self.advance();
                    let rhs = self.parse_additive()?;
                    lhs = if lhs <= rhs { 1 } else { 0 };
                } else {
                    let rhs = self.parse_additive()?;
                    lhs = if lhs < rhs { 1 } else { 0 };
                }
            } else if op == '>' {
                self.advance();
                if self.peek() == Some(&PpToken::Punct('=')) {
                    self.advance();
                    let rhs = self.parse_additive()?;
                    lhs = if lhs >= rhs { 1 } else { 0 };
                } else {
                    let rhs = self.parse_additive()?;
                    lhs = if lhs > rhs { 1 } else { 0 };
                }
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_multiplicative()?;
        while let Some(PpToken::Punct(op)) = self.peek().cloned() {
            if op == '+' {
                self.advance();
                let rhs = self.parse_multiplicative()?;
                lhs += rhs;
            } else if op == '-' {
                self.advance();
                let rhs = self.parse_multiplicative()?;
                lhs -= rhs;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_multiplicative(&mut self) -> Result<i64, String> {
        let mut lhs = self.parse_unary()?;
        while let Some(PpToken::Punct(op)) = self.peek().cloned() {
            if op == '*' {
                self.advance();
                let rhs = self.parse_unary()?;
                lhs *= rhs;
            } else if op == '/' {
                self.advance();
                let rhs = self.parse_unary()?;
                if rhs == 0 {
                    return Err("division by zero in preprocessor expression".to_string());
                }
                lhs /= rhs;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<i64, String> {
        match self.peek().cloned() {
            Some(PpToken::Punct('!')) => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(if val == 0 { 1 } else { 0 })
            }
            Some(PpToken::Punct('-')) => {
                self.advance();
                let val = self.parse_unary()?;
                Ok(-val)
            }
            Some(PpToken::Punct('+')) => {
                self.advance();
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<i64, String> {
        match self.advance().cloned() {
            Some(PpToken::Number(val_str)) => {
                let val = val_str.parse::<i64>().unwrap_or(0);
                Ok(val)
            }
            Some(PpToken::Punct('(')) => {
                let val = self.parse()?;
                if self.advance() != Some(&PpToken::Punct(')')) {
                    return Err("expected closing parenthesis".to_string());
                }
                Ok(val)
            }
            t => Err(format!("expected primary expression, got {:?}", t)),
        }
    }
}

fn handle_pragma(tokens: &[PpToken]) -> Option<String> {
    let mut i = 0;
    while i < tokens.len() && matches!(tokens[i], PpToken::Space(_)) {
        i += 1;
    }
    if i >= tokens.len() {
        return None;
    }
    if let PpToken::Identifier(ref name) = tokens[i] {
        if name == "pack" {
            i += 1;
            while i < tokens.len() && matches!(tokens[i], PpToken::Space(_)) {
                i += 1;
            }
            if i < tokens.len() && tokens[i] == PpToken::Punct('(') {
                i += 1;
                let mut arg_tokens = Vec::new();
                while i < tokens.len() && tokens[i] != PpToken::Punct(')') {
                    if !matches!(tokens[i], PpToken::Space(_)) {
                        arg_tokens.push(tokens[i].clone());
                    }
                    i += 1;
                }
                if arg_tokens.is_empty() {
                    return Some("__pragma_pack_default".to_string());
                }
                match &arg_tokens[0] {
                    PpToken::Identifier(arg) if arg == "push" => {
                        if arg_tokens.len() >= 3 && arg_tokens[1] == PpToken::Punct(',') {
                            if let PpToken::Number(ref n) = arg_tokens[2] {
                                return Some(format!("__pragma_pack_push_{}", n));
                            }
                        }
                        return Some("__pragma_pack_push_default".to_string());
                    }
                    PpToken::Identifier(arg) if arg == "pop" => {
                        return Some("__pragma_pack_pop".to_string());
                    }
                    PpToken::Number(n) => {
                        return Some(format!("__pragma_pack_{}", n));
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

pub fn predefine_system_macros(defines: &mut HashMap<String, MacroDef>) {
    let macros = vec![
        ("_WIN32", "1"),
        ("_WIN64", "1"),
        ("_MSC_VER", "1930"),
        ("_AMD64_", "1"),
        ("_M_AMD64", "1"),
        ("_M_X64", "1"),
        ("__stdcall", ""),
        ("__cdecl", ""),
        ("__fastcall", ""),
        ("__thiscall", ""),
        ("__vectorcall", ""),
        ("__unaligned", ""),
        ("__inline", "inline"),
        ("__forceinline", "inline"),
        ("__declspec(x)", ""),
        ("NULL", "((void*)0)"),
        ("_INTEGRAL_MAX_BITS", "64"),
        ("_MSVC_LANG", "201402L"),
        ("__STDC__", "1"),
        ("__STDC_VERSION__", "201112L"),
    ];

    for (name, val) in macros {
        let name_str = name.to_string();
        let (macro_name, args) = if name_str.contains('(') {
            let open_p = name_str.find('(').unwrap();
            let close_p = name_str.find(')').unwrap();
            let arg_name = name_str[open_p + 1..close_p].to_string();
            let m_name = name_str[..open_p].to_string();
            (m_name, Some(vec![arg_name]))
        } else {
            (name_str, None)
        };

        let body = tokenize(val);
        defines.insert(
            macro_name.clone(),
            MacroDef {
                name: macro_name,
                args,
                body,
            },
        );
    }
}
pub fn discover_system_includes() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    if let Ok(include_env) = std::env::var("INCLUDE") {
        for p in std::env::split_paths(&include_env) {
            if p.exists() {
                paths.push(p);
            }
        }
    }

    let sdk_base = std::path::Path::new(r"C:\Program Files (x86)\\Windows Kits\\10\\Include");
    if sdk_base.exists() {
        if let Ok(entries) = std::fs::read_dir(sdk_base) {
            let mut versions: Vec<std::path::PathBuf> = entries
                .filter_map(|e| e.ok().map(|x| x.path()))
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .map_or(false, |s| s.starts_with("10."))
                })
                .collect();
            versions.sort();
            if let Some(latest) = versions.last() {
                for sub in &["ucrt", "shared", "um", "winrt"] {
                    let subpath = latest.join(sub);
                    if subpath.exists() {
                        paths.push(subpath);
                    }
                }
            }
        }
    }

    let vs_base = std::path::Path::new(r"C:\Program Files\\Microsoft Visual Studio");
    if vs_base.exists() {
        let mut msvc_includes = Vec::new();
        if let Ok(years) = std::fs::read_dir(vs_base) {
            for year in years.filter_map(|e| e.ok()) {
                let year_path = year.path();
                if year_path.is_dir() {
                    if let Ok(editions) = std::fs::read_dir(&year_path) {
                        for edition in editions.filter_map(|e| e.ok()) {
                            let tools_path = edition.path().join("VC").join("Tools").join("MSVC");
                            if tools_path.exists() {
                                if let Ok(versions) = std::fs::read_dir(&tools_path) {
                                    for ver in versions.filter_map(|e| e.ok()) {
                                        let inc_path = ver.path().join("include");
                                        if inc_path.exists() {
                                            msvc_includes.push(inc_path);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        msvc_includes.sort();
        if let Some(latest_msvc) = msvc_includes.last() {
            paths.push(latest_msvc.clone());
        }
    }

    paths
}
