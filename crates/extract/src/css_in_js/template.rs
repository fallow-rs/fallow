//! CSS-in-JS template-literal lifter for the styling-health analytics pipeline
//! (CSS program Phase 3b).
//!
//! Styled-components / emotion / linaria write their CSS as the body of a tagged
//! template (`` styled.div`...` ``, `` css`...` ``, `` keyframes`...` ``). That CSS
//! never reaches the structural analytics that `.css` files and Vue/Svelte SFC
//! `<style>` blocks flow through, so a styled-components app gets `null` styling
//! analytics. This module is the tagged-template analogue of
//! [`crate::sfc_css::sfc_virtual_stylesheet`]: it lexically scans JS/TS source for
//! CSS-in-JS tagged templates, lifts each template body into a blank-line-padded
//! virtual stylesheet (so metric line numbers map back onto the real source line),
//! masks every `${...}` interpolation to a CSS-valid placeholder, and returns
//! `None` when the source has no CSS-in-JS template.
//!
//! It is health-time-only: it runs over file SOURCE in the engine's CSS walk, like
//! `sfc_virtual_stylesheet` and `compute_css_analytics`, and persists nothing to
//! the extraction cache (no `CACHE_VERSION` bump).
//!
//! Scope (first cut): TEMPLATE-LITERAL form only. The object form
//! (`css({ color: 'red' })`, `styled.div({...})`) is JS-object-to-CSS
//! serialization, a heavier and separate problem, and is deferred. `styled.div`,
//! `styled(Component)`, bare `css` / `keyframes` / `createGlobalStyle` /
//! `injectGlobal`, and `styled.div.attrs(...)` chains whose backtick does NOT
//! immediately follow the tag are out of scope for the regex anchor (the
//! `.attrs(...)` chain is a documented deferral).

use std::sync::LazyLock;

use super::shared::{WRAPPER, count_newlines};

/// A CSS-valid identifier placeholder substituted for every `${...}`
/// interpolation. Chosen so that a value-position interpolation
/// (`color: ${x}` -> `color: fallowinterp`) parses as an identifier rather than a
/// number / hex / color keyword, so it can never be mistaken for a design-token
/// color and an interpolation `compute_css_analytics` cannot make valid is simply
/// dropped by its `error_recovery: true` parse.
const INTERP_PLACEHOLDER: &str = "fallowinterp";

/// Maximum template-literal nesting depth the body scanner will descend before
/// bailing (issue #1843 follow-up). `scan_template_body` and `scan_interpolation`
/// are mutually recursive (a nested `` ${`...`} `` re-enters `scan_template_body`
/// one level deeper), so an adversarial `` ${`${`${...}`}`} `` overflows the stack
/// (SIGABRT under `panic = "abort"`). Past the cap the scan returns `None`
/// (unterminated-equivalent) and the lift is dropped gracefully. Real
/// styled-components / emotion / linaria CSS nests only a handful of levels, so
/// output is byte-identical for any genuine input. Deliberately a constant, not a
/// config knob, mirroring `MAX_TAINT_BINDING_HOPS` / `MAX_BINDING_PATH_DEPTH`.
const MAX_CSS_IN_JS_TEMPLATE_DEPTH: usize = 64;

/// Matches the opening of a CSS-in-JS tagged template: a recognized tag
/// (`styled.div`, `styled(Component)`, bare `css` / `keyframes` /
/// `createGlobalStyle` / `injectGlobal`) immediately followed by a backtick. The
/// match END is positioned at the opening backtick so the byte scanner takes over
/// from there to find the matching close (handling `${}` and nested templates).
static CSS_IN_JS_TAG_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r"(?:\bstyled\.[A-Za-z_$][A-Za-z0-9_$]*|\bstyled\([^()`]*\)|\bcss|\bkeyframes|\bcreateGlobalStyle|\binjectGlobal)\s*`",
    )
});

/// Build a virtual stylesheet from the CSS-in-JS tagged templates in a JS/TS
/// source. Each template body is placed at its real line in the source via
/// blank-line padding, so CSS metric line numbers from
/// [`crate::compute_css_analytics`] map straight back onto the source. Every
/// `${...}` interpolation is masked to a CSS-valid placeholder (newline count
/// preserved so lines after a multi-line interpolation stay aligned). Returns
/// `None` when the source has no CSS-in-JS tagged template, so callers skip the
/// file entirely (no `files_analyzed` inflation).
#[must_use]
pub fn css_in_js_virtual_stylesheet(source: &str) -> Option<String> {
    // Cheap pre-filter: no backtick means no tagged template at all.
    if !source.contains('`') {
        return None;
    }

    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut current_line: usize = 1;
    let mut found = false;
    let mut search_from = 0;
    // Incremental source-line cursor (issue #1843 follow-up). `body_start`
    // advances monotonically across matches (the regex is scanned left-to-right
    // and `search_from` only moves forward), so the block line is the running
    // count plus the newline delta since the last block, instead of rescanning
    // `source[..body_start]` from the start every iteration (the old
    // O(matches * source-len) cost). Byte-identical: `last_line` always equals
    // `1 + count_newlines(&source[..last_offset])`.
    let mut last_offset: usize = 0;
    let mut last_line: usize = 1;

    while let Some(m) = CSS_IN_JS_TAG_RE.find_at(source, search_from) {
        // The regex match ends at the opening backtick (its last byte).
        let backtick = m.end() - 1;
        let Some((body, after)) = scan_template_body(bytes, backtick, 0) else {
            // Unterminated template; stop scanning (the rest is malformed).
            break;
        };

        // Blank-line-pad to the template body's real start line, so a metric on
        // line N of the lifted sheet maps to line N of the source.
        let body_start = backtick + 1;
        last_line += count_newlines(&source[last_offset..body_start]);
        last_offset = body_start;
        let block_line = last_line;
        while current_line < block_line {
            out.push('\n');
            current_line += 1;
        }
        // Each lifted block is its own rule context. Wrapping the body in a
        // synthetic selector keeps top-level declarations (the common
        // `` styled.div`color: red` `` shape) inside a rule so they are counted,
        // while a body that already contains full rules (`& { ... }`, `&:hover`)
        // still parses under nesting. The wrapper selector occupies the body's
        // start line; the body keeps its own lines.
        out.push_str(WRAPPER);
        out.push('{');
        out.push_str(&body);
        out.push('}');
        current_line += count_newlines(&body);
        found = true;

        search_from = after;
    }

    found.then_some(out)
}

/// Scan a template literal whose opening backtick is at `open`. Returns the body
/// text with every top-level `${...}` interpolation replaced by the placeholder
/// (newline count preserved), plus the index immediately after the closing
/// backtick. Returns `None` if the template is unterminated.
fn scan_template_body(bytes: &[u8], open: usize, depth: usize) -> Option<(String, usize)> {
    // Bounded-work depth cap (issue #1843 follow-up): a template nested past
    // `MAX_CSS_IN_JS_TEMPLATE_DEPTH` levels bails as if unterminated rather than
    // recursing further and overflowing the stack. Byte work stays linear.
    if depth > MAX_CSS_IN_JS_TEMPLATE_DEPTH {
        return None;
    }
    // The body is accumulated as raw bytes and converted to a `String` at the end.
    // Every static byte (including the continuation bytes of a multi-byte UTF-8
    // char) is copied verbatim and contiguously, and only ASCII bytes (the
    // placeholder and newlines) are inserted at ASCII-boundary positions, so the
    // accumulated buffer is always valid UTF-8. Pushing `byte as char` instead
    // would Latin-1-mangle every non-ASCII char (e.g. a `content:`/`font-family`
    // value), so byte accumulation is the correct mirror of `sfc_virtual_stylesheet`'s
    // `&str` slicing.
    let mut out: Vec<u8> = Vec::new();
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                // Escaped char: copy the backslash and the escaped byte verbatim so
                // an escaped backtick / `${` is not treated as a delimiter. A
                // multi-byte escaped char's continuation bytes are picked up by the
                // catch-all arm on the following iterations.
                out.push(b'\\');
                if i + 1 < bytes.len() {
                    out.push(bytes[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'`' => return Some((String::from_utf8(out).unwrap_or_default(), i + 1)),
            b'$' if i + 1 < bytes.len() && bytes[i + 1] == b'{' => {
                let interp_end = scan_interpolation(bytes, i + 2, depth)?;
                // The span is bounded by ASCII `$` and the byte after `}`, so the
                // sub-slice is always valid UTF-8. Count via the str helper to
                // preserve newlines that lived inside nested templates/strings too.
                let newlines =
                    count_newlines(std::str::from_utf8(&bytes[i..interp_end]).unwrap_or(""));
                out.extend_from_slice(INTERP_PLACEHOLDER.as_bytes());
                out.extend(std::iter::repeat_n(b'\n', newlines));
                i = interp_end;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

/// Scan a `${...}` interpolation whose body starts at `start` (just after `{`).
/// Returns the index immediately after the matching `}`. Handles nested braces,
/// nested template literals (which may carry their own `${}`), and string
/// literals so a `}` inside a string or nested template does not close early.
fn scan_interpolation(bytes: &[u8], start: usize, template_depth: usize) -> Option<usize> {
    let mut brace_depth: usize = 1;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                brace_depth += 1;
                i += 1;
            }
            b'}' => {
                brace_depth -= 1;
                i += 1;
                if brace_depth == 0 {
                    return Some(i);
                }
            }
            b'`' => {
                // Nested template literal: skip it wholesale (recurses for its
                // own interpolations) one template level deeper so the shared
                // depth cap in `scan_template_body` bounds the mutual recursion.
                let (_, after) = scan_template_body(bytes, i, template_depth + 1)?;
                i = after;
            }
            b'\'' | b'"' => {
                i = skip_string(bytes, i)?;
            }
            // Skip the escaped byte; `saturating_add` guards a trailing backslash
            // at end-of-input (the `while` guard then exits cleanly).
            b'\\' => i = i.saturating_add(2),
            _ => i += 1,
        }
    }
    None
}

/// Skip a single- or double-quoted string whose opening quote is at `open`.
/// Returns the index immediately after the closing quote.
fn skip_string(bytes: &[u8], open: usize) -> Option<usize> {
    let quote = bytes[open];
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i = i.saturating_add(2),
            b if b == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use crate::compute_css_analytics;

    #[test]
    fn preserves_multibyte_utf8_in_lifted_body() {
        // A non-ASCII `content:` value (2-byte, 3-byte, and 4-byte chars) must
        // survive the lift byte-for-byte (no Latin-1 mangling), and the result
        // stays valid UTF-8 / parseable CSS.
        let src = "const T = styled.div`\n\
                   content: \"café 日本 €\";\n\
                   font-family: \"Ñoño\";\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has a styled template");
        assert!(
            vcss.contains("café 日本 €"),
            "multibyte content preserved: {vcss:?}"
        );
        assert!(
            vcss.contains("Ñoño"),
            "multibyte font-family preserved: {vcss:?}"
        );
        // Still valid UTF-8 and parseable (no None, no panic).
        assert!(compute_css_analytics(&vcss).is_some(), "lifted CSS parses");
    }

    #[test]
    fn lifts_styled_component_body_to_parseable_css() {
        let src = "import styled from 'styled-components';\n\
                   export const Button = styled.button`\n\
                   color: white;\n\
                   padding: 8px 16px;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has a styled template");
        let analytics = compute_css_analytics(&vcss).expect("masked CSS must parse, not None");
        assert!(
            analytics.total_declarations >= 2,
            "styled body declarations should be counted: {analytics:?}"
        );
    }

    #[test]
    fn none_without_any_css_in_js_template() {
        assert!(css_in_js_virtual_stylesheet("const x = 1; function f() {}").is_none());
        // A plain (non-CSS-in-JS) template literal is not lifted.
        assert!(css_in_js_virtual_stylesheet("const s = `hello ${name}`;").is_none());
    }

    #[test]
    fn interpolation_heavy_template_does_not_return_none_or_garble() {
        // Every value is an interpolation; masking must keep the sheet parseable
        // and must not invent a structural finding.
        let src = "const T = styled.div`\n\
                   color: ${theme.primary};\n\
                   padding: ${y}px;\n\
                   ${mixin};\n\
                   margin: ${a} ${b};\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has a styled template");
        let analytics =
            compute_css_analytics(&vcss).expect("interpolation-masked CSS must parse, not None");
        // No `!important`, no id-selector, no deep nesting was authored, so no
        // structural notable rule should be invented by the masking.
        assert!(
            analytics.important_declarations == 0,
            "masking must not invent !important: {analytics:?}"
        );
    }

    #[test]
    fn emotion_css_and_keyframes_tags_are_lifted() {
        let src = "import { css, keyframes } from '@emotion/react';\n\
                   const fade = keyframes`\n\
                   from { opacity: 0; }\n\
                   to { opacity: 1; }\n\
                   `;\n\
                   const box = css`\n\
                   display: flex;\n\
                   gap: 8px;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has css/keyframes templates");
        let analytics = compute_css_analytics(&vcss).expect("must parse");
        assert!(
            analytics.rule_count >= 1,
            "rules should be counted: {analytics:?}"
        );
    }

    #[test]
    fn styled_call_form_is_lifted() {
        let src = "const Primary = styled(Button)`\n\
                   font-weight: bold;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("styled(Component) is lifted");
        assert!(vcss.contains("font-weight"), "vcss={vcss:?}");
    }

    #[test]
    fn line_numbers_map_back_to_source() {
        // The `color` declaration is on source line 4; the lifted sheet must keep
        // a non-blank token on line 4 so metric line numbers map back.
        let src = "import styled from 'styled-components';\n\
                   \n\
                   const A = styled.div`\n\
                   color: red;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has a template");
        let color_pos = vcss.find("color").expect("color present");
        let vcss_line = 1 + vcss[..color_pos].bytes().filter(|&b| b == b'\n').count();
        let src_color = src.find("color: red").unwrap();
        let src_line = 1 + src[..src_color].bytes().filter(|&b| b == b'\n').count();
        assert_eq!(vcss_line, src_line, "vcss={vcss:?}");
    }

    #[test]
    fn nested_template_in_interpolation_does_not_break_extent() {
        // An interpolation containing a nested template literal must not end the
        // outer template early; the trailing `border` declaration must survive.
        let src = "const A = styled.div`\n\
                   color: ${(p) => css`color: ${p.c}`};\n\
                   border: 1px solid black;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has a template");
        assert!(
            vcss.contains("border"),
            "outer template extent must include the post-interpolation decl: {vcss:?}"
        );
    }

    #[test]
    fn deeply_nested_template_does_not_overflow_stack() {
        // Issue #1843 follow-up (FIX A): scan_template_body <-> scan_interpolation
        // are mutually recursive, so a pathologically nested `${`${`...`}`}` must
        // hit MAX_CSS_IN_JS_TEMPLATE_DEPTH and bail (unterminated-equivalent)
        // rather than recursing per input level and overflowing the stack
        // (SIGABRT under panic=abort). Far past the cap; without it this recursion
        // depth would blow the stack.
        let levels = 50_000;
        let mut src = String::from("const T = styled.div`");
        for _ in 0..levels {
            src.push_str("${`");
        }
        src.push_str("color: red;");
        for _ in 0..levels {
            src.push_str("`}");
        }
        src.push_str("`;\n");
        // Past-cap bail drops the whole lift gracefully (no panic, no overflow).
        assert!(css_in_js_virtual_stylesheet(&src).is_none());
    }

    #[test]
    fn multi_template_block_lines_map_back_to_source() {
        // Issue #1843 follow-up (FIX B): the incremental line cursor must yield
        // the SAME block line numbers as a from-scratch newline count for EVERY
        // template in a multi-template file, not just the first.
        let src = "import styled from 'styled-components';\n\
                   \n\
                   const A = styled.div`\n\
                   color: red;\n\
                   `;\n\
                   \n\
                   const B = styled.span`\n\
                   margin: 0;\n\
                   `;\n\
                   \n\
                   const C = styled.p`\n\
                   padding: 4px;\n\
                   `;\n";
        let vcss = css_in_js_virtual_stylesheet(src).expect("has templates");
        for needle in ["color: red", "margin: 0", "padding: 4px"] {
            let decl = needle.split(':').next().unwrap();
            let vcss_pos = vcss.find(decl).expect("decl present in vcss");
            let vcss_line = 1 + vcss[..vcss_pos].bytes().filter(|&b| b == b'\n').count();
            let src_pos = src.find(needle).unwrap();
            let src_line = 1 + src[..src_pos].bytes().filter(|&b| b == b'\n').count();
            assert_eq!(vcss_line, src_line, "decl {needle:?} vcss={vcss:?}");
        }
    }
}
