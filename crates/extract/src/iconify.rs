//! Static Iconify icon-string extraction (issue #608).
//!
//! Iconify-based icon components consume icon sets through a build-time string
//! name (`<Icon name="jam:github" />`, `<List icon="ic:round-home" />`) rather
//! than a JavaScript `import`, so the `@iconify-json/<prefix>` package that
//! supplies the `jam:` / `ic:` collection is invisible to import-graph analysis
//! and gets flagged as an unused dependency.
//!
//! This module scans the raw markup of template/JSX file kinds for icon-prop
//! string values shaped `<prefix>:<name>` and returns the deduped collection
//! prefixes. The analysis layer maps each prefix to `@iconify-json/<prefix>` and
//! credits it as a referenced dependency, gated on the project actually
//! declaring an Iconify-ecosystem dependency. Crediting can only ever exempt a
//! declared dependency from "unused"; it never produces a finding, so a stray
//! non-icon `name="foo:bar"` is harmless.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// Matches an icon prop (`icon` or `name`) whose value starts with an Iconify
/// collection prefix followed by a colon and an icon name.
///
/// The leading `[\s"'/]` requires whitespace, a quote, or a slash before the
/// attribute name so attribute names that merely end in `icon`/`name`
/// (`data-name`, `filename`) do not match; the `regex` crate has no lookbehind.
/// Capture group 1 is the collection prefix (`jam`, `ic`, `simple-icons`,
/// `fa6-solid`). The trailing `[a-z0-9]` guarantees a real `prefix:name`, not a
/// bare `prefix:`.
static ICON_PROP: LazyLock<Regex> = LazyLock::new(|| {
    crate::static_regex(r#"[\s"'/](?:icon|name)\s*=\s*["']([a-z0-9]+(?:-[a-z0-9]+)*):[a-z0-9]"#)
});

/// Matches HTML markup comments so a commented-out icon usage does not credit
/// its package. Mirrors the comment-strip-before-scan approach in `css.rs` /
/// `html.rs`. JS/JSX comment forms (`//`, `/* */`, `{/* */}`) are not stripped:
/// icon props rarely appear inside them and stripping risks mangling real
/// attribute lines (e.g. a `//` inside a URL).
static HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// File extensions whose source is markup that can carry icon-component props.
/// Plain `.js`/`.ts`/`.mjs`/`.cjs` are excluded: they have no template markup,
/// so scanning them would add a regex pass per file on large repos for no gain.
/// `.js`-with-JSX is a documented limitation.
const MARKUP_EXTENSIONS: &[&str] = &["astro", "jsx", "tsx", "svelte", "vue", "html", "htm", "mdx"];

/// Extract deduped Iconify collection prefixes from static icon props in
/// `source`. Returns an empty `Vec` for non-markup file kinds. See issue #608.
#[must_use]
pub fn extract_iconify_prefixes(path: &Path, source: &str) -> Vec<String> {
    let is_markup = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| MARKUP_EXTENSIONS.contains(&ext));
    if !is_markup {
        return Vec::new();
    }

    let scanned = HTML_COMMENT.replace_all(source, "");
    let mut prefixes: Vec<String> = ICON_PROP
        .captures_iter(&scanned)
        .map(|caps| caps[1].to_string())
        .collect();
    prefixes.sort_unstable();
    prefixes.dedup();
    prefixes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn prefixes(source: &str) -> Vec<String> {
        extract_iconify_prefixes(Path::new("src/pages/index.astro"), source)
    }

    #[test]
    fn extracts_name_prop_double_quoted() {
        assert_eq!(prefixes(r#"<Icon name="jam:github" />"#), vec!["jam"]);
    }

    #[test]
    fn extracts_icon_prop_single_quoted() {
        assert_eq!(prefixes(r"<List icon='ic:round-home' />"), vec!["ic"]);
    }

    #[test]
    fn dedupes_and_sorts_multiple_icons() {
        let source = r#"
            <Icon name="jam:github" />
            <Icon name="jam:linkedin" />
            <List icon="ic:round-home" />
        "#;
        assert_eq!(prefixes(source), vec!["ic", "jam"]);
    }

    #[test]
    fn handles_hyphenated_collection_prefixes() {
        let source = r#"<Icon name="simple-icons:github" /><Icon icon="fa6-solid:house" />"#;
        assert_eq!(prefixes(source), vec!["fa6-solid", "simple-icons"]);
    }

    #[test]
    fn ignores_attribute_names_that_merely_end_in_name() {
        assert!(prefixes(r#"<div data-name="jam:github" />"#).is_empty());
        assert!(prefixes(r#"<a filename="ic:home" />"#).is_empty());
    }

    #[test]
    fn ignores_values_without_a_colon_prefix() {
        assert!(prefixes(r#"<input name="email" />"#).is_empty());
        assert!(prefixes(r#"<Icon name="github" />"#).is_empty());
    }

    #[test]
    fn ignores_bare_prefix_with_no_icon_name() {
        assert!(prefixes(r#"<Icon name="jam:" />"#).is_empty());
    }

    #[test]
    fn ignores_dynamic_bindings() {
        assert!(prefixes(r#"<Icon :name="iconExpr" />"#).is_empty());
        assert!(prefixes(r"<Icon name={iconExpr} />").is_empty());
    }

    #[test]
    fn ignores_icons_inside_html_comments() {
        assert!(prefixes(r#"<!-- <Icon name="jam:github" /> -->"#).is_empty());
        let source = "<!--\n  <List icon=\"ic:round-home\" />\n-->\n<Icon name=\"mdi:home\" />";
        assert_eq!(prefixes(source), vec!["mdi"]);
    }

    #[test]
    fn returns_empty_for_non_markup_extensions() {
        let prefixes = extract_iconify_prefixes(
            Path::new("src/util.ts"),
            r#"const x = { name: "jam:github" };"#,
        );
        assert!(prefixes.is_empty());
    }
}
