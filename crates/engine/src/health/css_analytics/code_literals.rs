//! Class names that project code holds in string literals.
//!
//! Code often keeps a class name in a constant (`const MARKER = "drag-marker"`)
//! or pushes it into a list (`classes.push("cell-border-top")`), and applies it
//! later through a variable. The reference surface cannot follow the variable,
//! so it credits the literal itself when its statement names a class: the text
//! from the start of the statement to the literal contains `class` in any case
//! (`ROW_MARKER_CLASS = "..."`, `classes.push("...")`, `cellClass = ...`).
//!
//! - a string literal that holds only class-shaped tokens credits each token
//!   that has a `-` or `_`;
//! - a template literal credits the static head before its first `${` as a
//!   class prefix, when the head ends in `-` or `_` (`cell-edge-${side}`).
//!
//! A single plain word (`hidden`, `active`), and a literal in a statement that
//! does not name a class (an element id, a test id, a cache key), stay
//! uncredited, so a generic string does not credit a class by coincidence.

/// The class evidence of the string literals in one source file.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct CodeLiteralClasses {
    /// Class tokens of string literals that hold only class names.
    pub tokens: Vec<String>,
    /// Static heads of template literals, for example `cell-edge-`.
    pub prefixes: Vec<String>,
}

/// The longest statement head that [`names_a_class`] reads before a literal.
const MAX_STATEMENT_HEAD: usize = 240;

/// The shortest template head stem (the head without its trailing `-` or `_`)
/// that credits a prefix. A shorter stem (`a-`, `x_`) matches too many classes.
const MIN_PREFIX_STEM: usize = 3;

/// Scan the string and template literals of a JavaScript-family source.
pub(super) fn scan_code_literal_classes(source: &str) -> CodeLiteralClasses {
    let bytes = source.as_bytes();
    let mut out = CodeLiteralClasses::default();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = source[i + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |n| i + n + 4);
            }
            quote @ (b'\'' | b'"') => {
                let end = string_end(bytes, i + 1, quote);
                let tokens = source
                    .get(i + 1..end)
                    .map(literal_tokens)
                    .unwrap_or_default();
                if !tokens.is_empty() && names_a_class(bytes, i) {
                    out.tokens.extend(tokens.into_iter().map(str::to_owned));
                }
                i = end + 1;
            }
            b'`' => {
                let (head_end, end) = template_bounds(bytes, i + 1);
                let head = source.get(i + 1..head_end).unwrap_or_default();
                let (tokens, prefix) = if head_end == end {
                    (literal_tokens(head), None)
                } else {
                    template_head(head)
                };
                if (!tokens.is_empty() || prefix.is_some()) && names_a_class(bytes, i) {
                    out.tokens.extend(tokens.into_iter().map(str::to_owned));
                    out.prefixes.extend(prefix.map(str::to_owned));
                }
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// True when the statement text before the literal at `quote` contains
/// `class` in any case.
///
/// The head starts after the nearest `;`, `{`, `}`, line break or earlier
/// literal. When only whitespace and commas separate the literal from a line
/// break or an earlier literal, the head continues before them, so every item
/// of a multi-line class list (`const CLASSES = [\n  'a',\n  'b',\n]`) or of a
/// call (`classes.push('a', 'b')`) sees the class name. Another JSX attribute
/// (`data-testid="x"`) or another statement on an earlier line ends the head.
/// The scan reads at most [`MAX_STATEMENT_HEAD`] bytes back.
fn names_a_class(bytes: &[u8], quote: usize) -> bool {
    let floor = quote.saturating_sub(MAX_STATEMENT_HEAD);
    let mut end = quote;
    while end > floor {
        let boundary = bytes[floor..end]
            .iter()
            .rposition(|b| matches!(b, b';' | b'{' | b'}' | b'\n' | b'\'' | b'"' | b'`'))
            .map(|pos| floor + pos);
        let start = boundary.map_or(floor, |pos| pos + 1);
        let head = &bytes[start..end];
        if head
            .windows("class".len())
            .any(|window| window.eq_ignore_ascii_case(b"class"))
        {
            return true;
        }
        if !head.iter().all(|b| b.is_ascii_whitespace() || *b == b',') {
            return false;
        }
        end = match boundary.map(|pos| (pos, bytes[pos])) {
            Some((pos, b'\n')) => pos,
            Some((pos, quote @ (b'\'' | b'"' | b'`'))) => {
                match bytes[floor..pos].iter().rposition(|b| *b == quote) {
                    Some(open) => floor + open,
                    None => return false,
                }
            }
            _ => return false,
        };
    }
    false
}

/// The offset of the closing quote, or the end of the line for an
/// unterminated literal.
fn string_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut j = start;
    while j < bytes.len() && bytes[j] != quote && bytes[j] != b'\n' {
        j += if bytes[j] == b'\\' { 2 } else { 1 };
    }
    j.min(bytes.len())
}

/// The end of the static head (the first `${`, or the closing backtick) and
/// the offset of the closing backtick of a template literal.
fn template_bounds(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut head_end = None;
    let mut depth = 0usize;
    let mut j = start;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' => j += 1,
            b'$' if depth == 0 && bytes.get(j + 1) == Some(&b'{') => {
                head_end.get_or_insert(j);
                depth = 1;
                j += 1;
            }
            b'{' if depth > 0 => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            b'`' if depth == 0 => break,
            _ => {}
        }
        j += 1;
    }
    let end = j.min(bytes.len());
    (head_end.unwrap_or(end), end)
}

/// The compound class tokens of a literal that holds only class names.
fn literal_tokens(content: &str) -> Vec<&str> {
    let parts: Vec<&str> = content.split_ascii_whitespace().collect();
    if parts.is_empty() || !parts.iter().all(|part| is_class_token(part)) {
        return Vec::new();
    }
    parts.into_iter().filter(|part| is_compound(part)).collect()
}

/// The compound class tokens and the class prefix of a template head.
fn template_head(head: &str) -> (Vec<&str>, Option<&str>) {
    if head.ends_with(|c: char| c.is_ascii_whitespace()) {
        return (literal_tokens(head), None);
    }
    let mut parts: Vec<&str> = head.split_ascii_whitespace().collect();
    let Some(prefix) = parts.pop() else {
        return (Vec::new(), None);
    };
    let stem = prefix.trim_end_matches(['-', '_']);
    if !parts.iter().all(|part| is_class_token(part))
        || stem.len() == prefix.len()
        || stem.len() < MIN_PREFIX_STEM
        || !is_class_token(stem)
    {
        return (Vec::new(), None);
    }
    parts.retain(|part| is_compound(part));
    (parts, Some(prefix))
}

/// A CSS class name as code writes it: a letter or `_` first, then letters,
/// digits, `-` and `_`, and no trailing `-`.
fn is_class_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && !token.ends_with('-')
}

fn is_compound(token: &str) -> bool {
    token.contains(['-', '_'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> (Vec<String>, Vec<String>) {
        let out = scan_code_literal_classes(source);
        (out.tokens, out.prefixes)
    }

    #[test]
    fn class_constants_and_pushed_literals_credit_compound_tokens() {
        let (tokens, _) = scan(
            "export const ROW_MARKER_CLASS = \"table-row-drag-marker\";\nif (top) {\n  classes.push('selectedCell-border-top');\n}\nconst CLASSES = [\n  'drop-marker hidden',\n];\n",
        );
        assert_eq!(
            tokens,
            [
                "table-row-drag-marker",
                "selectedCell-border-top",
                "drop-marker"
            ]
        );
    }

    #[test]
    fn literals_outside_a_class_statement_credit_nothing() {
        let (tokens, prefixes) = scan(
            "export const helper = 'helper-only';\ndocument.getElementById('app-root');\nconst key = `user-${id}`;\n// reuse the class below\nconst id = 'user-card';\nconst cls = classNames(a)\nconst other = 'row-item';\n<div className=\"card\" data-testid=\"user-card\" aria-label=\"close-dialog\">\nconst shortClass = `ab-${b}`;\n",
        );
        assert!(tokens.is_empty(), "{tokens:?}");
        assert!(prefixes.is_empty(), "{prefixes:?}");
    }

    #[test]
    fn plain_words_prose_and_paths_credit_nothing() {
        let (tokens, prefixes) = scan(
            "const aClass = 'hidden';\nconst bClass = \"Could not load the user-profile.\";\nconst cClass = '/api/user-list';\nconst dClass = 'trailing-';\n",
        );
        assert!(tokens.is_empty(), "{tokens:?}");
        assert!(prefixes.is_empty(), "{prefixes:?}");
    }

    #[test]
    fn template_heads_credit_a_prefix() {
        let (tokens, prefixes) = scan(
            "const aClass = `cell-edge-${side}`;\nconst bClass = `grid-wrap col_${n} x`;\nconst cClass = `/users-${id}`;\nconst dClass = `${base} is-open`;\nconst eClass = `plain-class`;\n",
        );
        assert_eq!(tokens, ["grid-wrap", "plain-class"]);
        assert_eq!(prefixes, ["cell-edge-", "col_"]);
    }

    #[test]
    fn comments_and_nested_templates_do_not_desync_the_scan() {
        let (tokens, prefixes) = scan(
            "// don't read 'comment-class'\n/* it's 'block-class' */\nconst rowClass = `row-${cond ? `inner-${x}` : 'b'}`;\nconst kClass = 'after-nested';\n",
        );
        assert_eq!(tokens, ["after-nested"]);
        assert_eq!(prefixes, ["row-"]);
    }
}
