//! JSDoc visibility tags (`@public`, `@internal`, `@beta`, `@alpha`,
//! `@expected-unused`) on exports and re-exports.

use oxc_ast::Comment;

use crate::ExportInfo;
use fallow_types::extract::{ReExportInfo, VisibilityTag};

/// Apply JSDoc visibility tags (`@public`, `@internal`, `@alpha`, `@beta`) to exports and
/// re-exports by matching leading JSDoc comments.
///
/// A tag belongs to an export when it attaches to the export itself or to
/// the start of the export statement that holds it (see
/// [`crate::jsdoc_attach`]). A tag on one statement never reaches a later
/// statement, also in a file without semicolons. Re-exports
/// (`export { a } from './a'`, and an imported binding that a local
/// `export { a }` forwards) take their tag the same way, so the graph can
/// treat a tagged re-export as public API.
pub(super) fn apply_jsdoc_visibility_tags(
    exports: &mut [ExportInfo],
    re_exports: &mut [ReExportInfo],
    comments: &[Comment],
    source: &str,
    statements: &[oxc_span::Span],
) {
    if (exports.is_empty() && re_exports.is_empty()) || comments.is_empty() {
        return;
    }

    let mut tag_offsets = collect_jsdoc_tag_offsets(comments, source);
    if tag_offsets.is_empty() {
        return;
    }
    // Stable: comments stay in source order within one attachment offset.
    tag_offsets.sort_by_key(|&(offset, _, _)| offset);

    for export in exports.iter_mut() {
        apply_visibility_tag_to_export(export, &tag_offsets, statements);
    }
    for re_export in re_exports.iter_mut() {
        apply_visibility_tag_to_re_export(re_export, &tag_offsets, statements);
    }
}

/// Apply the visibility tag that belongs to a single re-export binding.
fn apply_visibility_tag_to_re_export(
    re_export: &mut ReExportInfo,
    tag_offsets: &[(u32, VisibilityTag, Option<String>)],
    statements: &[oxc_span::Span],
) {
    if re_export.span.is_empty() {
        return;
    }
    if let Some(idx) = crate::jsdoc_attach::tag_index_for_export(
        tag_offsets,
        |&(offset, _, _)| offset,
        re_export.span.start,
        statements,
    ) {
        re_export.visibility = tag_offsets[idx].1;
        re_export
            .expected_unused_reason
            .clone_from(&tag_offsets[idx].2);
    }
}

/// Classify a JSDoc comment body into a visibility tag (and optional reason),
/// or `None` when no recognized tag is present.
pub(super) fn classify_jsdoc_visibility_tag(text: &str) -> Option<(VisibilityTag, Option<String>)> {
    if super::has_public_tag(text) {
        Some((VisibilityTag::Public, None))
    } else if super::bare_jsdoc_tag_end(text, "@internal").is_some() {
        Some((VisibilityTag::Internal, None))
    } else if super::bare_jsdoc_tag_end(text, "@alpha").is_some() {
        Some((VisibilityTag::Alpha, None))
    } else if super::bare_jsdoc_tag_end(text, "@beta").is_some() {
        Some((VisibilityTag::Beta, None))
    } else {
        super::bare_jsdoc_tag_end(text, "@expected-unused").map(|after| {
            (
                VisibilityTag::ExpectedUnused,
                split_jsdoc_reason(&text[after..]),
            )
        })
    }
}

/// Collect `(attachment_offset, tag, reason)` triples for every JSDoc comment
/// that carries a recognized visibility tag.
fn collect_jsdoc_tag_offsets(
    comments: &[Comment],
    source: &str,
) -> Vec<(u32, VisibilityTag, Option<String>)> {
    let mut tag_offsets: Vec<(u32, VisibilityTag, Option<String>)> = Vec::new();
    for comment in comments {
        if !comment.is_jsdoc() {
            continue;
        }
        let content_span = comment.content_span();
        let start = content_span.start as usize;
        let end = (content_span.end as usize).min(source.len());
        if start >= end {
            continue;
        }
        if let Some((tag, reason)) = classify_jsdoc_visibility_tag(&source[start..end]) {
            tag_offsets.push((comment.attached_to, tag, reason));
        }
    }
    tag_offsets
}

/// Apply the visibility tag that belongs to a single export.
fn apply_visibility_tag_to_export(
    export: &mut ExportInfo,
    tag_offsets: &[(u32, VisibilityTag, Option<String>)],
    statements: &[oxc_span::Span],
) {
    if export.span.start == 0 && export.span.end == 0 {
        return;
    }
    let found = crate::jsdoc_attach::tag_index_for_export(
        tag_offsets,
        |&(offset, _, _)| offset,
        export.span.start,
        statements,
    );
    if let Some(idx) = found {
        export.visibility = tag_offsets[idx].1;
        export
            .expected_unused_reason
            .clone_from(&tag_offsets[idx].2);
    }
}

fn split_jsdoc_reason(rest: &str) -> Option<String> {
    for (idx, _) in rest.match_indices("--") {
        let before_ok = idx == 0
            || rest[..idx]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let after_idx = idx + 2;
        let after_ok = after_idx == rest.len()
            || rest[after_idx..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace);
        if before_ok && after_ok {
            let reason = rest[after_idx..].trim();
            return if reason.is_empty() {
                None
            } else {
                Some(reason.to_string())
            };
        }
    }

    None
}
