//! Dead scoped-CSS class detection for Vue/Svelte single-file components.
//!
//! A class defined in a `<style scoped>` block applies only to its own
//! component's markup (that is what `scoped` means), so a scoped class whose
//! name appears nowhere else in the same SFC is a cleanup candidate. The
//! "appears nowhere else" test is deliberately broad: any occurrence of the
//! class name as a whole token anywhere outside the `<style>` blocks (a static
//! `class="..."`, a dynamic `:class="{ name: x }"` key, a `class:name`
//! directive, or even a string in `<script>`) counts as a use. That keeps the
//! signal conservative (it errs toward "used"), so it is reported as a candidate
//! rather than a hard dead-code finding.

use std::sync::LazyLock;

use rustc_hash::FxHashSet;

use crate::ExportName;
use crate::css::extract_css_module_exports;

/// Matches `<style ...>BODY</style>` blocks, capturing the opening-tag
/// attributes and the body. Mirrors the SFC style scanner: handles `>` inside
/// quoted attribute values.
static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>"#,
    )
});

/// Returns `true` when an opening-`<style>` attribute string carries a bare
/// `scoped` attribute.
fn has_scoped_attr(attrs: &str) -> bool {
    attrs
        .split(|c: char| c.is_whitespace() || c == '=' || c == '"' || c == '\'')
        .any(|token| token.eq_ignore_ascii_case("scoped"))
}

/// Returns `true` when the `<style>` block declares a non-CSS preprocessor
/// language (`scss` / `sass` / `less` / `stylus` / `postcss`), which lightningcss
/// does not parse, so we skip scoped-deadness analysis for it.
fn has_non_css_lang(attrs: &str) -> bool {
    let lower = attrs.to_ascii_lowercase();
    has_preprocessor_lang_value(&lower)
        || [
            "lang=\"stylus\"",
            "lang='stylus'",
            "lang=\"postcss\"",
            "lang='postcss'",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn has_preprocessor_lang(attrs: &str) -> bool {
    has_preprocessor_lang_value(&attrs.to_ascii_lowercase())
}

fn has_preprocessor_lang_value(lower_attrs: &str) -> bool {
    [
        "lang=\"scss\"",
        "lang='scss'",
        "lang=\"sass\"",
        "lang='sass'",
        "lang=\"less\"",
        "lang='less'",
    ]
    .iter()
    .any(|needle| lower_attrs.contains(needle))
}

/// A `<style scoped>` block whose classes escape the component (`:global`,
/// `:deep`, `::v-deep`) or whose used-set we cannot fully see (`@apply` pulls in
/// classes by name) is skipped wholesale, conservatively.
fn block_escapes_scope(body: &str) -> bool {
    body.contains(":global")
        || body.contains(":deep")
        || body.contains("::v-deep")
        || body.contains("/deep/")
        || body.contains("@apply")
}

/// Returns class names defined in `<style scoped>` blocks of an SFC that appear
/// nowhere else in the component (cleanup candidates), sorted. Returns an empty
/// vec when the source has no analyzable scoped block.
#[must_use]
pub fn scoped_unused_classes(source: &str) -> Vec<String> {
    let mut scoped_classes: FxHashSet<String> = FxHashSet::default();
    // Byte ranges of every `<style>` block, blanked out of the search text so a
    // class's own definition does not count as a use of itself.
    let mut style_ranges: Vec<(usize, usize)> = Vec::new();

    for caps in STYLE_BLOCK_RE.captures_iter(source) {
        if let Some(whole) = caps.get(0) {
            style_ranges.push((whole.start(), whole.end()));
        }
        let attrs = caps.name("attrs").map_or("", |m| m.as_str());
        let body = caps.name("body").map_or("", |m| m.as_str());
        if !has_scoped_attr(attrs) || has_non_css_lang(attrs) || block_escapes_scope(body) {
            continue;
        }
        for export in extract_css_module_exports(body, false) {
            if let ExportName::Named(name) = export.name {
                scoped_classes.insert(name);
            }
        }
    }

    if scoped_classes.is_empty() {
        return Vec::new();
    }

    let search = blank_ranges(source, &style_ranges);
    let mut candidates: Vec<String> = scoped_classes
        .into_iter()
        .filter(|class| !class_token_appears(&search, class))
        .collect();
    candidates.sort_unstable();
    candidates
}

/// Build a "virtual stylesheet" from an SFC's plain-CSS `<style>` blocks (any
/// scoping). Each block body is placed at its real line in the SFC via blank-line
/// padding, so CSS metric line numbers from `compute_css_analytics` map straight
/// back onto the SFC. Returns `None` when the SFC has no plain-CSS `<style>`
/// block (e.g. only `lang="scss"` blocks, which the CSS parser cannot read), so
/// callers run the standard `.css` metric path on Vue/Svelte component styles.
#[must_use]
pub fn sfc_virtual_stylesheet(source: &str) -> Option<String> {
    let mut out = String::new();
    let mut current_line: usize = 1;
    let mut found = false;
    for caps in STYLE_BLOCK_RE.captures_iter(source) {
        let attrs = caps.name("attrs").map_or("", |m| m.as_str());
        if has_non_css_lang(attrs) {
            continue;
        }
        let Some(body) = caps.name("body") else {
            continue;
        };
        found = true;
        let block_line = 1 + source[..body.start()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        while current_line < block_line {
            out.push('\n');
            current_line += 1;
        }
        out.push_str(body.as_str());
        current_line += body.as_str().bytes().filter(|&b| b == b'\n').count();
    }
    found.then_some(out)
}

/// Build a virtual stylesheet from SFC preprocessor `<style>` blocks that the
/// health layer can conservatively lower before CSS analytics.
#[must_use]
pub fn sfc_preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    let mut out = String::new();
    let mut current_line: usize = 1;
    let mut found = false;
    for caps in STYLE_BLOCK_RE.captures_iter(source) {
        let attrs = caps.name("attrs").map_or("", |m| m.as_str());
        if !has_preprocessor_lang(attrs) {
            continue;
        }
        let Some(body) = caps.name("body") else {
            continue;
        };
        found = true;
        let block_line = 1 + source[..body.start()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        while current_line < block_line {
            out.push('\n');
            current_line += 1;
        }
        out.push_str(body.as_str());
        current_line += body.as_str().bytes().filter(|&b| b == b'\n').count();
    }
    found.then_some(out)
}

/// Replace the given byte ranges in `source` with spaces (preserving length),
/// so the returned string can be searched for class uses without the `<style>`
/// blocks themselves matching.
fn blank_ranges(source: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = source.as_bytes().to_vec();
    for &(start, end) in ranges {
        if start <= end && end <= out.len() {
            for byte in &mut out[start..end] {
                *byte = b' ';
            }
        }
    }
    // The blanked ranges align to `<style>`/`</style>` tag boundaries, which are
    // ASCII, so the result stays valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

/// Returns `true` when `name` appears as a whole class token in `text` (not as a
/// substring of a longer identifier). `-` and `_` are treated as identifier
/// characters so `foo` does not match inside `foo-bar`.
fn class_token_appears(text: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let bytes = text.as_bytes();
    let len = name.len();
    let mut from = 0;
    while let Some(offset) = text[from..].find(name) {
        let start = from + offset;
        let end = start + len;
        let before_ok = start == 0 || !is_identifier_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_identifier_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
        if from >= text.len() {
            break;
        }
    }
    false
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn flags_unused_scoped_class() {
        let dead = scoped_unused_classes(
            "<template><div class=\"used\"></div></template>\n\
             <style scoped>.used { color: red; } .dead { color: blue; }</style>",
        );
        assert_eq!(dead, vec!["dead".to_string()]);
    }

    #[test]
    fn class_used_in_dynamic_binding_is_not_flagged() {
        // The `active` token appears in the `:class` binding object, so it is a use.
        let dead = scoped_unused_classes(
            "<template><div :class=\"{ active: isActive }\"></div></template>\n\
             <style scoped>.active { color: red; }</style>",
        );
        assert!(dead.is_empty(), "got {dead:?}");
    }

    #[test]
    fn class_used_in_svelte_directive_is_not_flagged() {
        let dead = scoped_unused_classes(
            "<button class:selected={on}>x</button>\n\
             <style>.selected { color: red; }</style>",
        );
        // No `scoped` attr on Svelte (styles are scoped by default), so this
        // block is not analyzed and nothing is flagged.
        assert!(dead.is_empty(), "got {dead:?}");
    }

    #[test]
    fn class_referenced_in_script_is_not_flagged() {
        let dead = scoped_unused_classes(
            "<script>const c = \"highlight\";</script>\n\
             <template><div :class=\"c\"></div></template>\n\
             <style scoped>.highlight { color: red; }</style>",
        );
        assert!(dead.is_empty(), "got {dead:?}");
    }

    #[test]
    fn global_selector_block_is_skipped() {
        let dead = scoped_unused_classes(
            "<template><div></div></template>\n\
             <style scoped>:global(.x) { color: red; } .y { color: blue; }</style>",
        );
        assert!(dead.is_empty(), "blocks with :global are skipped wholesale");
    }

    #[test]
    fn scss_scoped_block_is_skipped() {
        let dead = scoped_unused_classes(
            "<template><div></div></template>\n\
             <style scoped lang=\"scss\">.dead { color: red; }</style>",
        );
        assert!(dead.is_empty(), "scss is not parsed");
    }

    #[test]
    fn non_scoped_block_is_not_analyzed() {
        let dead = scoped_unused_classes(
            "<template><div></div></template>\n\
             <style>.dead { color: red; }</style>",
        );
        assert!(dead.is_empty(), "only scoped blocks are analyzed");
    }

    #[test]
    fn virtual_stylesheet_places_rules_at_sfc_lines() {
        // The `.a` rule is on line 3 of the SFC; the virtual stylesheet must keep
        // it on line 3 so metric line numbers map back onto the source.
        let source = "<template>\n  <div/>\n</template>\n<style>\n.a { color: red; }\n</style>";
        let vcss = super::sfc_virtual_stylesheet(source).expect("has a plain-CSS style block");
        let line_of_a = 1 + vcss[..vcss.find(".a").unwrap()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        let sfc_line_of_a = 1 + source[..source.find(".a").unwrap()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        assert_eq!(line_of_a, sfc_line_of_a, "vcss={vcss:?}");
    }

    #[test]
    fn virtual_stylesheet_none_without_plain_css_block() {
        assert!(super::sfc_virtual_stylesheet("<template><div/></template>").is_none());
        assert!(
            super::sfc_virtual_stylesheet("<style lang=\"scss\">.a { .b {} }</style>").is_none(),
            "scss-only SFC yields no virtual stylesheet"
        );
    }

    #[test]
    fn preprocessor_virtual_stylesheet_keeps_sfc_lines() {
        let source =
            "<template>\n  <div/>\n</template>\n<style lang=\"scss\">\n.a { .b {} }\n</style>";
        let vcss = super::sfc_preprocessor_virtual_stylesheet(source)
            .expect("has a preprocessor style block");
        let line_of_a = 1 + vcss[..vcss.find(".a").unwrap()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        let sfc_line_of_a = 1 + source[..source.find(".a").unwrap()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        assert_eq!(line_of_a, sfc_line_of_a, "vcss={vcss:?}");
    }

    #[test]
    fn hyphenated_class_token_boundary() {
        // `.foo` is unused even though `foo-bar` appears in the template.
        let dead = scoped_unused_classes(
            "<template><div class=\"foo-bar\"></div></template>\n\
             <style scoped>.foo { color: red; } .foo-bar { color: blue; }</style>",
        );
        assert_eq!(dead, vec!["foo".to_string()]);
    }
}
