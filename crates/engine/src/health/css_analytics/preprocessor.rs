pub(super) fn preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    let clean = strip_preprocessor_comments(source);
    let output = render_preprocessor_children(&clean, 0, clean.len(), 0);
    (!output.trim().is_empty()).then_some(output)
}

fn strip_preprocessor_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push_str(&source[cursor..i]);
            out.push_str("  ");
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            cursor = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&source[cursor..]);
    out
}

fn render_preprocessor_children(source: &str, start: usize, end: usize, indent: usize) -> String {
    let bytes = source.as_bytes();
    let mut output = String::new();
    let mut statement_start = start;
    let mut i = start;
    while i < end {
        if bytes[i] == b'{' {
            let prelude = source[statement_start..i].trim();
            let Some(close) = find_matching_brace(source, i, end) else {
                return output;
            };
            if let Some(block) = render_preprocessor_block(source, prelude, i + 1, close, indent) {
                output.push_str(&block);
            }
            i = close + 1;
            statement_start = i;
        } else if bytes[i] == b';' {
            i += 1;
            statement_start = i;
        } else {
            i += 1;
        }
    }
    output
}

fn render_preprocessor_block(
    source: &str,
    prelude: &str,
    body_start: usize,
    body_end: usize,
    indent: usize,
) -> Option<String> {
    let prelude = prelude.trim();
    if prelude.is_empty()
        || prelude.contains("#{")
        || prelude.starts_with("@mixin")
        || prelude.starts_with("@function")
        || prelude.starts_with("@for")
        || prelude.starts_with("@each")
        || prelude.starts_with("@if")
        || prelude.starts_with("@else")
        || prelude.starts_with("@while")
    {
        return None;
    }
    if prelude.starts_with("@media")
        || prelude.starts_with("@supports")
        || prelude.starts_with("@container")
        || prelude.starts_with("@layer")
    {
        let body = render_preprocessor_children(source, body_start, body_end, indent + 1);
        if body.trim().is_empty() {
            return None;
        }
        let mut output = String::new();
        push_indent(&mut output, indent);
        output.push_str(prelude);
        output.push_str(" {\n");
        output.push_str(&body);
        push_indent(&mut output, indent);
        output.push_str("}\n");
        return Some(output);
    }
    if prelude.starts_with('@') || prelude.ends_with(':') {
        return None;
    }

    let selectors = clean_preprocessor_selector_list(prelude)?;
    let (declarations, children) =
        render_preprocessor_body(source, body_start, body_end, indent + 1);
    if declarations.is_empty() && children.trim().is_empty() {
        return None;
    }
    let mut output = String::new();
    push_indent(&mut output, indent);
    output.push_str(&selectors);
    output.push_str(" {\n");
    for declaration in declarations {
        push_indent(&mut output, indent + 1);
        output.push_str(&declaration);
        output.push('\n');
    }
    output.push_str(&children);
    push_indent(&mut output, indent);
    output.push_str("}\n");
    Some(output)
}

fn render_preprocessor_body(
    source: &str,
    body_start: usize,
    body_end: usize,
    indent: usize,
) -> (Vec<String>, String) {
    let bytes = source.as_bytes();
    let mut declarations = Vec::new();
    let mut children = String::new();
    let mut statement_start = body_start;
    let mut i = body_start;
    while i < body_end {
        match bytes[i] {
            b'{' => {
                let prelude = source[statement_start..i].trim();
                let Some(close) = find_matching_brace(source, i, body_end) else {
                    break;
                };
                if let Some(block) =
                    render_preprocessor_block(source, prelude, i + 1, close, indent)
                {
                    children.push_str(&block);
                }
                i = close + 1;
                statement_start = i;
            }
            b';' => {
                let statement = source[statement_start..=i].trim();
                if let Some(declaration) = normalize_preprocessor_declaration(statement) {
                    declarations.push(declaration);
                }
                i += 1;
                statement_start = i;
            }
            _ => i += 1,
        }
    }
    (declarations, children)
}

fn clean_preprocessor_selector_list(prelude: &str) -> Option<String> {
    let children: Vec<&str> = prelude
        .split(',')
        .map(str::trim)
        .filter(|selector| {
            !selector.is_empty()
                && !selector.contains("#{")
                && !selector.starts_with('@')
                && !selector.ends_with(':')
        })
        .collect();
    if children.is_empty() {
        None
    } else {
        Some(children.join(", "))
    }
}

fn normalize_preprocessor_declaration(statement: &str) -> Option<String> {
    let statement = statement.trim().trim_end_matches(';').trim();
    if statement.is_empty()
        || statement.starts_with('$')
        || statement.starts_with("@include")
        || statement.starts_with("@extend")
        || statement.starts_with("@debug")
        || statement.starts_with("@warn")
        || statement.starts_with("@error")
        || statement.contains("#{")
    {
        return None;
    }
    let (property, value) = statement.split_once(':')?;
    let property = property.trim();
    let value = value.trim();
    if property.is_empty() || value.is_empty() || property.starts_with('@') {
        return None;
    }
    Some(format!(
        "{property}: {};",
        normalize_preprocessor_value(value)
    ))
}

fn normalize_preprocessor_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if (bytes[i] == b'$' || bytes[i] == b'@') && is_preprocessor_ident_start(bytes.get(i + 1)) {
            out.push_str(&value[cursor..i]);
            out.push_str("var(--fallow-preprocessor-var)");
            i += 2;
            while i < bytes.len() && is_preprocessor_ident_continue(bytes[i]) {
                i += 1;
            }
            cursor = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[cursor..]);
    out
}

fn is_preprocessor_ident_start(byte: Option<&u8>) -> bool {
    byte.is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b == b'-')
}

fn is_preprocessor_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn push_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("  ");
    }
}

fn find_matching_brace(source: &str, open: usize, limit: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < limit {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
