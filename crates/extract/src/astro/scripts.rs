//! Health complexity of the inline `<script>` blocks in an Astro template.
//!
//! Astro bundles a plain `<script>` and ships an `is:inline` or `define:vars`
//! script unchanged. Both kinds are client code that the project owns, so the
//! functions in each block get the same scores as the functions of a Vue or
//! Svelte `<script>` block. A block that loads an external file (`src`) or that
//! holds data instead of code (`type="application/ld+json"`) has no score.

use std::sync::LazyLock;

use oxc_span::SourceType;

use fallow_types::extract::FunctionComplexity;

use super::{HTML_COMMENT_RE, SCRIPT_CLOSE_RE, SCRIPT_PREFIX_RE, SRC_ATTR_RE, blocks};
use crate::sfc::SfcScript;
use crate::template_expression_scan::pos_in_ranges;

/// Regex capturing the value of a `type` attribute on a script tag.
static TYPE_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?i)(?:^|\s)type\s*=\s*(?:"(?P<dq>[^"]*)"|'(?P<sq>[^']*)'|(?P<bare>[^\s"'>]+))"#,
    )
});

/// `type` attribute values that mark a script body as JavaScript. The browser
/// runs the classic and module types. Partytown runs `text/partytown` scripts
/// in a web worker, so they are JavaScript too.
const JS_SCRIPT_TYPES: &[&str] = &[
    "",
    "module",
    "text/javascript",
    "application/javascript",
    "text/ecmascript",
    "application/ecmascript",
    "text/partytown",
];

/// Score the functions of every inline `<script>` block in the Astro
/// `template`. `template_offset` is the byte offset of the template in the
/// full source, and `line_offsets` are the line offsets of the full source, so
/// each line and column is relative to the full `.astro` file.
pub(super) fn compute_script_block_complexity(
    template: &str,
    template_offset: usize,
    line_offsets: &[u32],
) -> Vec<FunctionComplexity> {
    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(template)
        .map(|m| (m.start(), m.end()))
        .collect();

    let mut complexity = Vec::new();
    for block in blocks::scan(template, &SCRIPT_PREFIX_RE, Some(&SCRIPT_CLOSE_RE)) {
        if pos_in_ranges(&comment_ranges, block.start)
            || !holds_inline_code(&template[block.attrs.clone()])
        {
            continue;
        }
        let body = &template[block.body.clone()];
        if body.trim().is_empty() {
            continue;
        }
        let script = SfcScript {
            body: body.to_string(),
            is_typescript: true,
            is_jsx: false,
            byte_offset: template_offset + block.body.start,
            src: None,
            src_span: None,
            is_setup: false,
            is_context_module: false,
            generic_attr: None,
        };
        complexity.extend(crate::nesting::parse_bounded_or_default(
            &script.body,
            SourceType::ts(),
            |parsed| {
                crate::sfc::translate_script_complexity(&script, &parsed.program, line_offsets)
            },
        ));
    }
    complexity
}

/// Return `true` when a script tag with these attributes runs its own body as
/// JavaScript: it has no `src` attribute and its `type` is a JavaScript type.
fn holds_inline_code(attrs: &str) -> bool {
    if SRC_ATTR_RE.is_match(attrs) {
        return false;
    }
    let Some(cap) = TYPE_ATTR_RE.captures(attrs) else {
        return true;
    };
    let value = cap
        .name("dq")
        .or_else(|| cap.name("sq"))
        .or_else(|| cap.name("bare"))
        .map_or("", |m| m.as_str())
        .trim();
    JS_SCRIPT_TYPES
        .iter()
        .any(|js_type| value.eq_ignore_ascii_case(js_type))
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::holds_inline_code;

    #[test]
    fn javascript_types_hold_inline_code() {
        for attrs in [
            "",
            " is:inline",
            " define:vars={{ a }}",
            r#" type="module""#,
            " type='text/javascript'",
            " type=module",
            r#" TYPE="Text/JavaScript""#,
            r#" type="text/partytown""#,
        ] {
            assert!(holds_inline_code(attrs), "{attrs:?}");
        }
    }

    #[test]
    fn data_types_and_external_sources_hold_no_inline_code() {
        for attrs in [
            r#" type="application/ld+json""#,
            r#" type="application/json""#,
            r#" type="text/x-template""#,
            r#" src="/app.js""#,
            r#" type="module" src="./app.ts""#,
        ] {
            assert!(!holds_inline_code(attrs), "{attrs:?}");
        }
    }
}
