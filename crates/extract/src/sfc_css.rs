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
    let transition_classes = vue_transition_classes(&search);
    let mut candidates: Vec<String> = scoped_classes
        .into_iter()
        .filter(|class| !transition_classes.contains(class) && !class_token_appears(&search, class))
        .collect();
    candidates.sort_unstable();
    candidates
}

/// Matches an opening Vue `<Transition>` / `<TransitionGroup>` tag in
/// PascalCase or kebab-case. The caller rejects a longer tag name such as
/// `<TransitionWrapper>`, because the regex crate has no look-ahead.
static TRANSITION_TAG_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"<(?P<tag>TransitionGroup|transition-group|Transition|transition)(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>"#,
    )
});

/// Matches one attribute of an opening tag. Quoted values are consumed whole,
/// so an attribute-like text inside a value is never read as an attribute.
static TAG_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?P<key>[^\s=/>"']+)(?:\s*=\s*(?:"(?P<dq>[^"]*)"|'(?P<sq>[^']*)'|(?P<uq>[^\s>"']+)))?"#,
    )
});

/// Class suffixes that Vue applies for every `<Transition>` name.
const TRANSITION_CLASS_SUFFIXES: [&str; 6] = [
    "enter-from",
    "enter-active",
    "enter-to",
    "leave-from",
    "leave-active",
    "leave-to",
];

/// The class suffix that Vue adds for a `<TransitionGroup>` move.
const TRANSITION_GROUP_MOVE_SUFFIX: &str = "move";

/// The name prefix that Vue uses when a transition has no `name` attribute.
const DEFAULT_TRANSITION_NAME: &str = "v";

/// Props that give literal custom transition classes, in kebab-case and
/// camelCase.
const TRANSITION_CLASS_PROPS: [&str; 20] = [
    "enter-from-class",
    "enter-active-class",
    "enter-to-class",
    "leave-from-class",
    "leave-active-class",
    "leave-to-class",
    "appear-from-class",
    "appear-active-class",
    "appear-to-class",
    "move-class",
    "enterFromClass",
    "enterActiveClass",
    "enterToClass",
    "leaveFromClass",
    "leaveActiveClass",
    "leaveToClass",
    "appearFromClass",
    "appearActiveClass",
    "appearToClass",
    "moveClass",
];

/// How a transition tag sets its `name`.
enum TransitionName<'a> {
    Absent,
    Literal(&'a str),
    Bound,
}

/// Returns the classes that Vue applies through the `<Transition>` and
/// `<TransitionGroup>` tags in `source` (issue #3350).
///
/// A literal `name="fade"` gives `fade-enter-from` through `fade-leave-to`, and
/// a TransitionGroup also gives `fade-move`. A tag without `name` gives the
/// same classes with the `v` prefix. A bound `:name` or a `v-bind` object gives
/// no generated classes, because the name is not known statically. Literal
/// custom class props such as `enter-active-class="a b"` give their classes.
#[must_use]
pub fn vue_transition_classes(source: &str) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    for caps in TRANSITION_TAG_RE.captures_iter(source) {
        let attrs = caps.name("attrs").map_or("", |m| m.as_str());
        if attrs
            .chars()
            .next()
            .is_some_and(|c| !c.is_whitespace() && c != '/')
        {
            continue;
        }
        let is_group = caps
            .name("tag")
            .is_some_and(|m| matches!(m.as_str(), "TransitionGroup" | "transition-group"));
        collect_transition_tag_classes(attrs, is_group, &mut out);
    }
    out
}

/// Returns `true` for an attribute that binds `name` to an expression:
/// `:name`, `v-bind:name`, either with modifiers such as `.camel`, or a
/// `v-bind` object that can carry `name`.
fn is_bound_name_attr(key: &str) -> bool {
    if key == "v-bind" {
        return true;
    }
    let Some(arg) = key
        .strip_prefix(':')
        .or_else(|| key.strip_prefix("v-bind:"))
    else {
        return false;
    };
    arg.split('.').next() == Some("name")
}

fn collect_transition_tag_classes(attrs: &str, is_group: bool, out: &mut FxHashSet<String>) {
    let mut name = TransitionName::Absent;
    for caps in TAG_ATTR_RE.captures_iter(attrs) {
        let key = caps.name("key").map_or("", |m| m.as_str());
        let value = caps
            .name("dq")
            .or_else(|| caps.name("sq"))
            .or_else(|| caps.name("uq"))
            .map(|m| m.as_str());
        match (key, value) {
            ("name", Some(value)) => name = TransitionName::Literal(value.trim()),
            (key, _) if is_bound_name_attr(key) => name = TransitionName::Bound,
            (key, Some(value)) if TRANSITION_CLASS_PROPS.contains(&key) => {
                out.extend(value.split_whitespace().map(str::to_string));
            }
            _ => {}
        }
    }
    let prefix = match name {
        TransitionName::Bound => return,
        TransitionName::Absent | TransitionName::Literal("") => DEFAULT_TRANSITION_NAME,
        TransitionName::Literal(value) if value.contains(char::is_whitespace) => return,
        TransitionName::Literal(value) => value,
    };
    out.extend(
        TRANSITION_CLASS_SUFFIXES
            .iter()
            .map(|suffix| format!("{prefix}-{suffix}")),
    );
    if is_group {
        out.insert(format!("{prefix}-{TRANSITION_GROUP_MOVE_SUFFIX}"));
    }
}

/// Build a "virtual stylesheet" from an SFC's plain-CSS `<style>` blocks (any
/// scoping). Each block body is placed at its real line in the SFC via blank-line
/// padding, so CSS metric line numbers from `compute_css_analytics` map straight
/// back onto the SFC. Returns `None` when the SFC has no plain-CSS `<style>`
/// block (e.g. only `lang="scss"` blocks, which the CSS parser cannot read), so
/// callers run the standard `.css` metric path on Vue/Svelte component styles.
#[must_use]
pub fn sfc_virtual_stylesheet(source: &str) -> Option<String> {
    virtual_stylesheet(source, |attrs| !has_non_css_lang(attrs))
}

/// Build a virtual stylesheet from SFC preprocessor `<style>` blocks that the
/// health layer can conservatively lower before CSS analytics.
#[must_use]
pub fn sfc_preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    virtual_stylesheet(source, has_preprocessor_lang)
}

/// Build a virtual stylesheet from the `<style>` blocks whose opening-tag
/// attribute string satisfies `keep`. Each kept body is placed at its real line
/// in the SFC via blank-line padding, so CSS metric line numbers map straight
/// back onto the SFC. Returns `None` when `keep` selected no block.
///
/// The two callers' predicates are deliberately not complements: a
/// `lang="stylus"` or `lang="postcss"` block is rejected by both, so neither
/// predicate may be rewritten as the negation of the other.
fn virtual_stylesheet(source: &str, keep: impl Fn(&str) -> bool) -> Option<String> {
    let mut out = String::new();
    let mut current_line: usize = 1;
    let mut found = false;
    for caps in STYLE_BLOCK_RE.captures_iter(source) {
        let attrs = caps.name("attrs").map_or("", |m| m.as_str());
        if !keep(attrs) {
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
    fn stylus_and_postcss_blocks_reach_neither_virtual_stylesheet() {
        // `has_non_css_lang` keeps stylus and postcss out of the plain-CSS
        // sheet, and `has_preprocessor_lang` does not select them for the
        // preprocessor sheet, so the two predicates are not complements and
        // neither may be rewritten as the negation of the other.
        for source in [
            "<template>\n  <div/>\n</template>\n<style lang=\"stylus\">\n.a\n  color red\n</style>",
            "<template>\n  <div/>\n</template>\n<style lang=\"postcss\">\n.a { color: red; }\n</style>",
        ] {
            assert!(
                super::sfc_virtual_stylesheet(source).is_none(),
                "plain-CSS sheet must skip it: {source:?}"
            );
            assert!(
                super::sfc_preprocessor_virtual_stylesheet(source).is_none(),
                "preprocessor sheet must skip it: {source:?}"
            );
        }
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

    #[test]
    fn literal_transition_name_credits_generated_classes() {
        let dead = scoped_unused_classes(
            "<template><Transition name=\"fade\"><div v-if=\"v\" class=\"panel\"/></Transition></template>\n\
             <style scoped>.panel {} .fade-enter-active { transition: opacity 1s; } \
             .fade-enter-from, .fade-leave-to { opacity: 0; } .fade-move {} .stale {}</style>",
        );
        // `fade-move` belongs to TransitionGroup only.
        assert_eq!(dead, vec!["fade-move".to_string(), "stale".to_string()]);
    }

    #[test]
    fn kebab_transition_group_credits_move_class() {
        let dead = scoped_unused_classes(
            "<template><transition-group name='list' tag=\"ul\"><li/></transition-group></template>\n\
             <style scoped>.list-move {} .list-leave-active {} .other-move {}</style>",
        );
        assert_eq!(dead, vec!["other-move".to_string()]);
    }

    #[test]
    fn unnamed_transition_credits_v_prefix() {
        let dead = scoped_unused_classes(
            "<template><TransitionGroup><li/></TransitionGroup></template>\n\
             <style scoped>.v-enter-active {} .v-move {} .fade-enter-active {}</style>",
        );
        assert_eq!(dead, vec!["fade-enter-active".to_string()]);
    }

    #[test]
    fn bound_transition_name_with_modifier_credits_nothing() {
        let dead = scoped_unused_classes(
            "<template><Transition :name.camel=\"kind\"><div/></Transition>\
             <Transition v-bind:name.prop=\"kind\"><div/></Transition></template>\n\
             <style scoped>.v-enter-active {}</style>",
        );
        assert_eq!(dead, vec!["v-enter-active".to_string()]);
    }

    #[test]
    fn bound_transition_name_credits_nothing() {
        let dead = scoped_unused_classes(
            "<template><Transition :name=\"kind\"><div/></Transition></template>\n\
             <style scoped>.fade-enter-active {} .v-enter-active {}</style>",
        );
        assert_eq!(
            dead,
            vec![
                "fade-enter-active".to_string(),
                "v-enter-active".to_string()
            ]
        );
    }

    #[test]
    fn transition_prefixed_component_is_not_a_transition() {
        let dead = scoped_unused_classes(
            "<template><TransitionWrapper name=\"fade\"/></template>\n\
             <style scoped>.fade-enter-active {}</style>",
        );
        assert_eq!(dead, vec!["fade-enter-active".to_string()]);
    }

    #[test]
    fn vue_transition_classes_reads_custom_class_props() {
        let classes = vue_transition_classes(
            "<Transition name=\"x\" enter-active-class=\"animate__animated animate__bounce\" \
             leaveToClass=\"gone\"><p/></Transition>\
             <TransitionGroup :name=\"n\" move-class=\"shuffle\"/>",
        );
        let mut sorted: Vec<&str> = classes.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![
                "animate__animated",
                "animate__bounce",
                "gone",
                "shuffle",
                "x-enter-active",
                "x-enter-from",
                "x-enter-to",
                "x-leave-active",
                "x-leave-from",
                "x-leave-to",
            ]
        );
    }
}
