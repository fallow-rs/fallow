//! Markdown helpers for LSP hover bodies.
//!
//! Hover content in `crates/lsp/src/hover.rs` is published as
//! `MarkupContent { kind: MarkupKind::Markdown }`. Every user-controlled
//! identifier (export name, member name, import specifier, file basename)
//! is interpolated into a CommonMark inline code span. CommonMark
//! deliberately does NOT process backslash escapes inside code spans, so a
//! prose-style `\.` escape would render as a literal `\.` and corrupt the
//! display. The correct primitive for embedding user-controlled values
//! inside an inline code span is to vary the fence length, not to escape
//! individual characters.
//!
//! `format_inline_code` picks a backtick-fence length one greater than the
//! longest backtick run in the value, padding with spaces when the value
//! starts or ends with a backtick (per the CommonMark spec for code spans).
//! This guarantees the rendered span contains the value verbatim and
//! cannot be broken by a crafted identifier (e.g. an export named
//! `` `evil`](command:vscode.open?bad) ``).
//!
//! ## Sibling helpers in the workspace
//!
//! - `editors/vscode/src/statusBar-utils.ts::escapeMarkdownText` escapes
//!   prose-context markdown metacharacters for the VS Code extension's
//!   trusted-markdown status-bar tooltip. Different context (prose, not
//!   code span); intentionally different shape.
//! - `crates/cli/src/report/markdown.rs::escape_backticks` escapes only
//!   backticks for the CLI markdown report's prose context. Narrower than
//!   either of the two surfaces above. If a future LSP renderer needs a
//!   prose-context escape, add it here rather than introducing a fourth
//!   helper.

/// Format `value` as a CommonMark inline code span that contains the value
/// verbatim.
///
/// Picks a backtick-fence length one greater than the longest backtick run
/// in `value`, and pads with a single space on each side when the value
/// starts or ends with a backtick. Inside a code span CommonMark suppresses
/// every inline construct (links, emphasis, images, command URIs), so this
/// is the right primitive for embedding user-controlled identifiers without
/// leaking markdown syntax into the rendered output.
///
/// Iterates by `char` so multibyte identifiers (CJK, Cyrillic, emoji in
/// JavaScript identifier names) pass through unchanged.
pub fn format_inline_code(value: &str) -> String {
    let max_run = max_backtick_run(value);
    let fence_len = max_run + 1;
    let mut out = String::with_capacity(value.len() + 2 * fence_len + 2);
    for _ in 0..fence_len {
        out.push('`');
    }
    let needs_pad = value.starts_with('`') || value.ends_with('`');
    if needs_pad {
        out.push(' ');
    }
    out.push_str(value);
    if needs_pad {
        out.push(' ');
    }
    for _ in 0..fence_len {
        out.push('`');
    }
    out
}

/// Length of the longest contiguous run of backticks in `s`.
fn max_backtick_run(s: &str) -> usize {
    let mut max = 0usize;
    let mut cur = 0usize;
    for ch in s.chars() {
        if ch == '`' {
            cur += 1;
            if cur > max {
                max = cur;
            }
        } else {
            cur = 0;
        }
    }
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_value_uses_single_backtick_fence() {
        assert_eq!(format_inline_code("foo"), "`foo`");
        assert_eq!(format_inline_code(""), "``");
        assert_eq!(format_inline_code("app.ts"), "`app.ts`");
        assert_eq!(format_inline_code("my-component"), "`my-component`");
        assert_eq!(format_inline_code("HelloWorld123"), "`HelloWorld123`");
    }

    #[test]
    fn single_backtick_escalates_to_double_fence() {
        assert_eq!(format_inline_code("a`b"), "``a`b``");
    }

    #[test]
    fn double_backtick_escalates_to_triple_fence() {
        assert_eq!(format_inline_code("a``b"), "```a``b```");
    }

    #[test]
    fn leading_or_trailing_backtick_pads_with_spaces() {
        // value starts with `: pad on left
        assert_eq!(format_inline_code("`a"), "`` `a ``");
        // value ends with `: pad on right
        assert_eq!(format_inline_code("a`"), "`` a` ``");
        // both ends
        assert_eq!(format_inline_code("`a`"), "`` `a` ``");
        // pure backticks
        assert_eq!(format_inline_code("`"), "`` ` ``");
    }

    #[test]
    fn longest_run_wins_for_fence_length() {
        // mixed runs; the longest is what determines the fence
        assert_eq!(format_inline_code("`a```b`"), "```` `a```b` ````");
    }

    #[test]
    fn command_link_injection_renders_as_inert_text() {
        // The canonical injection probe from issue #480. The crafted
        // export name renders verbatim inside a single-backtick code span
        // because it contains no backticks; CommonMark does not interpret
        // link syntax inside code spans.
        let crafted = "[click](command:vscode.open?evil)";
        let rendered = format_inline_code(crafted);
        assert_eq!(rendered, "`[click](command:vscode.open?evil)`");
        // The `](` boundary that would close a link label and start a URL
        // is now bracketed by backticks; no markdown renderer treats it
        // as a link.
        assert!(rendered.starts_with('`') && rendered.ends_with('`'));
    }

    #[test]
    fn backtick_injection_via_breakout_is_neutralized() {
        // Hostile identifier that contains a backtick: a naive
        // `format!("`{}`", value)` would close the span and let the
        // trailing `](command:foo)` render as a link. The escalating
        // fence prevents this.
        let crafted = "evil`](command:foo)";
        let rendered = format_inline_code(crafted);
        // Outer fence is two backticks (longest run inside is 1).
        assert_eq!(rendered, "``evil`](command:foo)``");
        // The single inner backtick cannot close the surrounding fence,
        // so the `](command:foo)` substring stays inside the code span.
        assert!(!rendered.contains("``](command:"));
    }

    #[test]
    fn multibyte_utf8_passes_through() {
        // CJK identifier (legal in JS/TS).
        assert_eq!(format_inline_code("日本語"), "`日本語`");
        // Cyrillic identifier (legal in JS/TS).
        assert_eq!(format_inline_code("Привет"), "`Привет`");
        // Combining accents.
        assert_eq!(format_inline_code("café"), "`café`");
        // Mixed: multibyte + backtick.
        assert_eq!(format_inline_code("日本`語"), "``日本`語``");
    }
}
