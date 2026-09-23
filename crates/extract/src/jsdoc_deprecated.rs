//! `@deprecated` JSDoc tag detection for exports.
//!
//! Deprecation is orthogonal to the visibility tags in `parse.rs`
//! (`@public @deprecated` is a normal combination), so it has its own pass
//! over the same comments. The tag belongs to an export under the same
//! attachment rule the visibility tags use (see [`crate::jsdoc_attach`]).

use fallow_types::extract::ExportInfo;
use fallow_types::results::DEPRECATED_REASON_MAX_CHARS;
use oxc_ast::Comment;

const DEPRECATED_TAG: &str = "@deprecated";
const ELLIPSIS: char = '\u{2026}';

/// One JSDoc comment that carries a `@deprecated` tag.
struct DeprecatedComment {
    /// Byte offset of the node the comment attaches to.
    attached_to: u32,
    /// Plain-text message, `None` for a bare tag.
    reason: Option<Box<str>>,
}

/// Mark every export whose leading JSDoc carries `@deprecated`.
pub fn apply_jsdoc_deprecated_tags(
    exports: &mut [ExportInfo],
    comments: &[Comment],
    source: &str,
    statements: &[oxc_span::Span],
) {
    if exports.is_empty() || comments.is_empty() {
        return;
    }
    let mut tags = collect_deprecated_comments(comments, source);
    if tags.is_empty() {
        return;
    }
    // Stable: comments stay in source order within one attachment offset.
    tags.sort_by_key(|tag| tag.attached_to);

    for export in exports.iter_mut() {
        if export.span.start == 0 && export.span.end == 0 {
            continue;
        }
        if let Some(tag) = leading_deprecated_comment(export.span.start, &tags, statements) {
            export.deprecated = true;
            export.deprecated_reason.clone_from(&tag.reason);
        }
    }
}

fn collect_deprecated_comments(comments: &[Comment], source: &str) -> Vec<DeprecatedComment> {
    comments
        .iter()
        .filter(|comment| comment.is_jsdoc())
        .filter_map(|comment| {
            let content = comment.content_span();
            let start = content.start as usize;
            let end = (content.end as usize).min(source.len());
            if start >= end || is_inside_export_braces(source, comment.span.start as usize) {
                return None;
            }
            let tag = deprecated_tag(&source[start..end])?;
            Some(DeprecatedComment {
                attached_to: comment.attached_to,
                reason: tag.reason.map(String::into_boxed_str),
            })
        })
        .collect()
}

/// The comment that belongs to the export starting at `export_start`.
fn leading_deprecated_comment<'a>(
    export_start: u32,
    tags: &'a [DeprecatedComment],
    statements: &[oxc_span::Span],
) -> Option<&'a DeprecatedComment> {
    crate::jsdoc_attach::tag_index_for_export(tags, |tag| tag.attached_to, export_start, statements)
        .map(|idx| &tags[idx])
}

/// A JSDoc comment inside `export { ... }` belongs to one specifier, not to a
/// declaration. TypeScript does not read it as a deprecation of the export.
fn is_inside_export_braces(source: &str, comment_start: usize) -> bool {
    source[..comment_start.min(source.len())]
        .trim_end()
        .ends_with(['{', ','])
}

/// A `@deprecated` tag found in a JSDoc body.
#[derive(Debug, PartialEq, Eq)]
struct DeprecatedTag {
    /// Plain-text message, `None` for a bare tag.
    reason: Option<String>,
}

/// The `@deprecated` tag in a JSDoc body, or `None` when the body has none.
fn deprecated_tag(body: &str) -> Option<DeprecatedTag> {
    let after = deprecated_tag_end(body)?;
    Some(DeprecatedTag {
        reason: plain_text_message(&body[after..]),
    })
}

/// Byte offset just after the first `@deprecated` that starts a tag: it
/// follows the comment start, whitespace, or a line-leading `*`, and no
/// identifier character follows it. This rejects `@deprecatedFoo`,
/// `` `@deprecated` `` and the inline `{@deprecated}` form.
fn deprecated_tag_end(body: &str) -> Option<usize> {
    body.match_indices(DEPRECATED_TAG).find_map(|(idx, _)| {
        let before_ok = body[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || c == '*');
        let after = idx + DEPRECATED_TAG.len();
        let after_ok = body[after..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '-'));
        (before_ok && after_ok).then_some(after)
    })
}

/// Turn the text after the tag into one plain-text line: stop at the next
/// block tag, drop the `*` that starts a continuation line, replace inline tags such as
/// `{@link Foo}` with their text, collapse whitespace, and cap the length.
fn plain_text_message(rest: &str) -> Option<String> {
    let mut text = String::new();
    for (index, line) in rest.lines().enumerate() {
        let line = line.trim_start();
        // A leading `*` is JSDoc decoration only on a continuation line; on
        // the tag line it is part of the message.
        let line = if index == 0 {
            line
        } else {
            line.strip_prefix('*').unwrap_or(line)
        };
        text.push_str(line);
        text.push(' ');
    }
    let text = cut_at_next_block_tag(&text);
    let text = replace_inline_tags(text);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(cap_message(collapsed))
}

/// Cut the text at the first `@tag` outside an inline `{...}` group that
/// starts a new tag (preceded by whitespace or the text start).
fn cut_at_next_block_tag(text: &str) -> &str {
    let mut depth = 0usize;
    let mut prev_is_space = true;
    for (idx, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '@' if depth == 0 && prev_is_space => {
                let next = text[idx + 1..].chars().next();
                if next.is_some_and(|n| n.is_ascii_alphabetic()) {
                    return &text[..idx];
                }
            }
            _ => {}
        }
        prev_is_space = c.is_whitespace();
    }
    text
}

/// Replace `{@link target}`, `{@link target text}`, `{@link target | text}`
/// and other inline tags with their display text, so the message stays plain
/// text in SARIF, markdown and CI annotations.
fn replace_inline_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{@") {
        out.push_str(&rest[..open]);
        let inner_start = open + 2;
        let Some(close_rel) = rest[inner_start..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &rest[inner_start..inner_start + close_rel];
        out.push_str(inline_tag_text(inner));
        rest = &rest[inner_start + close_rel + 1..];
    }
    out.push_str(rest);
    out
}

/// Display text of one inline tag body such as `link Foo | the foo`.
fn inline_tag_text(inner: &str) -> &str {
    let content = inner
        .split_once(char::is_whitespace)
        .map_or("", |(_, content)| content.trim());
    if let Some((_, label)) = content.split_once('|') {
        return label.trim();
    }
    match content.split_once(char::is_whitespace) {
        Some((_, label)) if !label.trim().is_empty() => label.trim(),
        _ => content,
    }
}

/// Cap `message` at [`DEPRECATED_REASON_MAX_CHARS`] characters, cutting at a
/// character boundary and ending a cut message with an ellipsis.
fn cap_message(message: String) -> String {
    if message.chars().count() <= DEPRECATED_REASON_MAX_CHARS {
        return message;
    }
    let mut capped: String = message
        .chars()
        .take(DEPRECATED_REASON_MAX_CHARS - 1)
        .collect::<String>()
        .trim_end()
        .to_string();
    capped.push(ELLIPSIS);
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARE: DeprecatedTag = DeprecatedTag { reason: None };

    fn with_reason(reason: &str) -> DeprecatedTag {
        DeprecatedTag {
            reason: Some(reason.to_string()),
        }
    }

    #[test]
    fn bare_tag_has_no_message() {
        assert_eq!(deprecated_tag("*\n * @deprecated\n "), Some(BARE));
        assert_eq!(deprecated_tag(" @deprecated "), Some(BARE));
    }

    #[test]
    fn message_stops_at_the_next_block_tag() {
        let body = "*\n * Old helper.\n * @deprecated Use `newHelper` instead.\n *   It goes away in v4.\n * @see newHelper\n ";
        assert_eq!(
            deprecated_tag(body),
            Some(with_reason("Use `newHelper` instead. It goes away in v4."))
        );
    }

    #[test]
    fn single_line_message_stops_at_an_inline_following_tag() {
        assert_eq!(
            deprecated_tag(" @deprecated use b @see b "),
            Some(with_reason("use b"))
        );
    }

    #[test]
    fn link_tags_become_plain_text() {
        assert_eq!(
            deprecated_tag(" @deprecated Use {@link newApi} or {@link other | the other one}. "),
            Some(with_reason("Use newApi or the other one."))
        );
        assert_eq!(
            deprecated_tag(" @deprecated See {@linkcode https://x.dev/a docs page}. "),
            Some(with_reason("See docs page."))
        );
    }

    #[test]
    fn a_leading_star_on_the_tag_line_is_message_text() {
        assert_eq!(
            deprecated_tag(" @deprecated *Use* foo "),
            Some(with_reason("*Use* foo"))
        );
        assert_eq!(
            deprecated_tag("*\n * @deprecated *Use* foo\n * and *bar*\n "),
            Some(with_reason("*Use* foo and *bar*"))
        );
    }

    #[test]
    fn email_like_text_is_not_a_tag() {
        assert_eq!(
            deprecated_tag(" @deprecated ask team@example.com "),
            Some(with_reason("ask team@example.com"))
        );
    }

    #[test]
    fn guards_reject_tags_that_are_not_block_tags() {
        assert_eq!(deprecated_tag(" @deprecatedFoo "), None);
        assert_eq!(deprecated_tag(" see `@deprecated` docs "), None);
        assert_eq!(deprecated_tag(" {@deprecated} "), None);
        assert_eq!(deprecated_tag(" \"@deprecated\" "), None);
    }

    #[test]
    fn long_message_is_capped_with_an_ellipsis_at_a_char_boundary() {
        let body = format!(" @deprecated {} ", "\u{e9}".repeat(300));
        let message = deprecated_tag(&body)
            .and_then(|tag| tag.reason)
            .expect("message");
        assert_eq!(message.chars().count(), DEPRECATED_REASON_MAX_CHARS);
        assert!(message.ends_with(ELLIPSIS));
    }

    #[test]
    fn message_at_the_cap_is_kept_whole() {
        let text = "a".repeat(DEPRECATED_REASON_MAX_CHARS);
        let message = deprecated_tag(&format!(" @deprecated {text} "))
            .and_then(|tag| tag.reason)
            .expect("message");
        assert_eq!(message, text);
    }
}
