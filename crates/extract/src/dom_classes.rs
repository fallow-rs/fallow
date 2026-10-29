//! CSS-class references in plain JS/TS DOM code.
//!
//! A plain `.js` / `.ts` module applies classes through DOM class APIs, not
//! through a markup `class` attribute. [`scan_dom_class_values`] finds the
//! values that these APIs receive:
//!
//! - `el.className = '...'` and `el.className += '...'`
//! - `el.classList.add / remove / toggle / replace / contains(...)`
//! - `el.setAttribute('class', ...)`
//! - a `class:` or `className:` object property (`{ class: 'x' }`)
//!
//! Only these call sites count. Every other string in the file stays
//! uncredited, so a generic string never credits a class by coincidence.
//!
//! The scanner is byte-based and conservative. It never evaluates an
//! expression. A value that is not fully static goes back as a dynamic value,
//! so the caller can credit its prefixes and interpolants.

/// The class values that one plain JS/TS source passes to DOM class APIs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DomClassScan {
    /// Class tokens from fully static string literals.
    pub static_tokens: Vec<String>,
    /// The source text of each value that is not fully static, for example
    /// `` `panel-${kind}` `` or `open ? 'is-open' : base`.
    pub dynamic_values: Vec<String>,
}

const CLASS_LIST_METHODS: &[&str] = &["add", "remove", "toggle", "replace", "contains"];

/// Scan a plain JS/TS source for the values that DOM class APIs receive.
#[must_use]
pub fn scan_dom_class_values(source: &str) -> DomClassScan {
    let mut values = Vec::new();
    collect_class_keyword_values(source, &mut values);
    collect_set_attribute_values(source, &mut values);
    let mut scan = DomClassScan::default();
    for value in values {
        classify_value(value, &mut scan);
    }
    scan
}

fn collect_class_keyword_values<'a>(source: &'a str, out: &mut Vec<&'a str>) {
    let bytes = source.as_bytes();
    for (start, _) in source.match_indices("class") {
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        let after_class = start + "class".len();
        if source[after_class..].starts_with("List") {
            let end = after_class + "List".len();
            if !bytes.get(end).copied().is_some_and(is_ident_byte) {
                collect_class_list_args(source, end, out);
            }
            continue;
        }
        let key_end = if source[after_class..].starts_with("Name") {
            after_class + "Name".len()
        } else {
            after_class
        };
        if bytes.get(key_end).copied().is_some_and(is_ident_byte) {
            continue;
        }
        if let Some(value) = class_key_value(source, start, key_end) {
            out.push(value);
        }
    }
}

/// The value of a `class` / `className` key that starts at `key_start`: an
/// assignment (`=` or `+=`) or an object property (`:`).
fn class_key_value(source: &str, key_start: usize, key_end: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let quote_before = key_start
        .checked_sub(1)
        .map(|i| bytes[i])
        .filter(|b| matches!(b, b'"' | b'\''));
    let mut cursor = key_end;
    let mut key_open = key_start;
    if let Some(quote) = quote_before {
        if bytes.get(key_end) != Some(&quote) {
            return None;
        }
        cursor += 1;
        key_open -= 1;
    }
    cursor = skip_whitespace(bytes, cursor);
    let next = bytes.get(cursor + 1).copied();
    let value_start = match bytes.get(cursor) {
        Some(b':') if follows_object_open(bytes, key_open) => cursor + 1,
        Some(b'=') if quote_before.is_none() && !matches!(next, Some(b'=' | b'>')) => cursor + 1,
        Some(b'+') if quote_before.is_none() && next == Some(b'=') => cursor + 2,
        _ => return None,
    };
    let value_end = expression_end(bytes, value_start);
    Some(source[value_start..value_end].trim())
}

/// True when the nearest non-whitespace byte before `pos` opens an object
/// property: `{` or `,`.
fn follows_object_open(bytes: &[u8], pos: usize) -> bool {
    let mut i = pos;
    while i > 0 {
        i -= 1;
        if !bytes[i].is_ascii_whitespace() {
            return matches!(bytes[i], b'{' | b',');
        }
    }
    false
}

/// Collect the arguments of a `classList.<method>(...)` call. `list_end` is
/// the byte after `classList`.
fn collect_class_list_args<'a>(source: &'a str, list_end: usize, out: &mut Vec<&'a str>) {
    let bytes = source.as_bytes();
    let mut cursor = skip_whitespace(bytes, list_end);
    if bytes.get(cursor) == Some(&b'?') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'.') {
        return;
    }
    cursor = skip_whitespace(bytes, cursor + 1);
    let method_start = cursor;
    while bytes.get(cursor).copied().is_some_and(is_ident_byte) {
        cursor += 1;
    }
    let method = &source[method_start..cursor];
    if !CLASS_LIST_METHODS.contains(&method) {
        return;
    }
    let Some(args) = call_args(source, cursor) else {
        return;
    };
    let args = split_top_level_args(args);
    // The second `toggle` argument is the force flag, not a class.
    let take = if method == "toggle" { 1 } else { args.len() };
    out.extend(args.into_iter().take(take));
}

fn collect_set_attribute_values<'a>(source: &'a str, out: &mut Vec<&'a str>) {
    let bytes = source.as_bytes();
    for (start, name) in source.match_indices("setAttribute") {
        if start > 0 && is_ident_byte(bytes[start - 1]) {
            continue;
        }
        let Some(args) = call_args(source, start + name.len()) else {
            continue;
        };
        let mut args = split_top_level_args(args).into_iter();
        let is_class_attribute = args
            .next()
            .is_some_and(|name| matches!(name, "'class'" | "\"class\"" | "`class`"));
        if is_class_attribute && let Some(value) = args.next() {
            out.push(value);
        }
    }
}

/// The text between the parentheses of a call whose `(` follows `cursor`
/// after optional whitespace.
fn call_args(source: &str, cursor: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let open = skip_whitespace(bytes, cursor);
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                i = string_end(bytes, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[open + 1..i]);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split call arguments at top-level commas and trim each argument.
fn split_top_level_args(args: &str) -> Vec<&str> {
    let bytes = args.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' | b'`' => {
                i = string_end(bytes, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                out.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let last = args[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// The end of the expression that starts at `start`. The expression stops at
/// a top-level `;`, `,`, line break, `>`, `//`, an unmatched closing bracket,
/// or a new JSX attribute (an identifier after whitespace that follows an
/// operand).
fn expression_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut last_significant: Option<u8> = None;
    let mut i = skip_whitespace(bytes, start);
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'"' | b'\'' | b'`' => {
                i = string_end(bytes, i);
                last_significant = Some(byte);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b';' | b',' | b'\n' | b'>' if depth == 0 => return i,
            b'/' if depth == 0 && bytes.get(i + 1) == Some(&b'/') => return i,
            b' ' | b'\t' | b'\r' if depth == 0 => {
                let next = skip_whitespace(bytes, i);
                let follows_operand = last_significant.is_some_and(|b| {
                    is_ident_byte(b) || matches!(b, b'"' | b'\'' | b'`' | b')' | b']' | b'}')
                });
                if follows_operand
                    && bytes.get(next).copied().is_some_and(is_ident_start)
                    && !starts_with_keyword_operator(&bytes[next..])
                {
                    return i;
                }
                i = next;
                continue;
            }
            _ => {}
        }
        last_significant = Some(byte);
        i += 1;
    }
    bytes.len()
}

fn starts_with_keyword_operator(bytes: &[u8]) -> bool {
    ["in", "instanceof", "as", "satisfies"].iter().any(|word| {
        bytes.starts_with(word.as_bytes())
            && !bytes.get(word.len()).copied().is_some_and(is_ident_byte)
    })
}

/// The byte after the string literal that opens at `open`. A backslash escapes
/// the next byte. An unclosed literal runs to the end of the source.
fn string_end(bytes: &[u8], open: usize) -> usize {
    let quote = bytes[open];
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            b'\n' if quote != b'`' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Sort one value into static tokens or a dynamic value. A value without any
/// string literal carries no class name and is dropped.
fn classify_value(value: &str, scan: &mut DomClassScan) {
    let bytes = value.as_bytes();
    if !bytes.iter().any(|b| matches!(b, b'"' | b'\'' | b'`')) {
        return;
    }
    let mut dynamic = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if matches!(byte, b'"' | b'\'' | b'`') {
            let end = string_end(bytes, i);
            let content_end = if end > i + 1 && bytes[end - 1] == byte {
                end - 1
            } else {
                end
            };
            let content = &value[i + 1..content_end];
            if byte == b'`' && content.contains("${") {
                dynamic = true;
            } else {
                scan.static_tokens.extend(
                    content
                        .split_whitespace()
                        .filter(|token| is_plausible_class_token(token))
                        .map(str::to_owned),
                );
            }
            i = end;
            continue;
        }
        if !byte.is_ascii_whitespace() && byte != b',' {
            dynamic = true;
        }
        i += 1;
    }
    if dynamic {
        scan.dynamic_values.push(value.to_owned());
    }
}

fn is_plausible_class_token(token: &str) -> bool {
    !token.contains(['{', '}', '$', '`', '"', '\'', '(', ')', '<', '>', '=', '\\'])
}

fn skip_whitespace(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_ident_byte(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_tokens(source: &str) -> Vec<String> {
        scan_dom_class_values(source).static_tokens
    }

    #[test]
    fn credits_class_name_assignment() {
        assert_eq!(
            static_tokens("el.className = 'panel-a panel-b';"),
            ["panel-a", "panel-b"]
        );
        assert_eq!(static_tokens("el.className += \" is-open\";"), ["is-open"]);
    }

    #[test]
    fn credits_class_list_methods() {
        assert_eq!(
            static_tokens(
                "el.classList.add('a-one', \"a-two\");\nel.classList?.remove('a-three');"
            ),
            ["a-one", "a-two", "a-three"]
        );
        assert_eq!(
            static_tokens("el.classList.replace('old-x', 'new-x'); el.classList.contains('has-x')"),
            ["old-x", "new-x", "has-x"]
        );
    }

    #[test]
    fn toggle_force_argument_is_not_a_class() {
        assert_eq!(
            static_tokens("el.classList.toggle('is-active', mode === 'other-mode');"),
            ["is-active"]
        );
    }

    #[test]
    fn credits_set_attribute_class() {
        assert_eq!(
            static_tokens("el.setAttribute('class', 'attr-a attr-b');"),
            ["attr-a", "attr-b"]
        );
        assert!(static_tokens("el.setAttribute('title', 'not-a-class');").is_empty());
    }

    #[test]
    fn credits_class_object_properties() {
        assert_eq!(
            static_tokens("const attrs = { class: 'obj-a', id: 'not-a-class' };"),
            ["obj-a"]
        );
        assert_eq!(
            static_tokens("return ['div', { \"class\": 'obj-b' }, 0];"),
            ["obj-b"]
        );
        assert_eq!(
            static_tokens("const props = {\n  role: 'note',\n  className: 'obj-c',\n};"),
            ["obj-c"]
        );
    }

    #[test]
    fn ignores_strings_outside_class_apis() {
        for source in [
            "export const helper = 'panel-helper';",
            "if (el.className === 'cmp-a') {}",
            "if (el.className == 'cmp-b') {}",
            "const x = cond ? 'tern-a' : 'tern-b';",
            "class Foo { name = 'field-a' }",
            "const classes = 'plural-a';",
            "el.classList.item('item-a');",
            "type P = { className: string };",
        ] {
            let scan = scan_dom_class_values(source);
            assert!(scan.static_tokens.is_empty(), "{source:?} -> {scan:?}");
            assert!(scan.dynamic_values.is_empty(), "{source:?} -> {scan:?}");
        }
    }

    #[test]
    fn reports_dynamic_values() {
        let scan = scan_dom_class_values(
            "el.className = `panel-${kind}`;\nel.classList.add(open ? 'is-open' : 'is-closed');",
        );
        assert_eq!(scan.static_tokens, ["is-open", "is-closed"]);
        assert_eq!(
            scan.dynamic_values,
            ["`panel-${kind}`", "open ? 'is-open' : 'is-closed'"]
        );
    }

    #[test]
    fn jsx_attribute_value_stops_at_next_attribute() {
        assert_eq!(
            static_tokens("<div className=\"jsx-a\" id=\"not-a-class\">x</div>"),
            ["jsx-a"]
        );
        assert_eq!(
            static_tokens("const html = '<div class=\"html-a\">x</div>';"),
            ["html-a"]
        );
    }
}
