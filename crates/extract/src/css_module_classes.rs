//! Class-selector names of a CSS Modules stylesheet, read from the lightningcss
//! AST. `crate::css` locates each name in the source.

use lightningcss::rules::CssRule;
use lightningcss::selector::{Component, PseudoClass, Selector, SelectorList};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use rustc_hash::FxHashSet;

use crate::css::blank_bare_css_module_scope_keywords;

/// Collect the authoritative set of class-selector names from a CSS source by
/// parsing it into a real AST (lightningcss). Returns `None` on a
/// catastrophic parse failure (Sass syntax that is not standard CSS), in which
/// case the caller falls back to the regex scanner. With `error_recovery` on,
/// individual malformed rules are recovered silently and contribute a partial
/// set rather than triggering the fallback, so a broken rule drops only its own
/// classes (a conservative miss) instead of returning `None`.
///
/// This is the source of truth for which `.token` occurrences are genuine class
/// selectors. It natively excludes `@layer foo.bar` layer names, `@import ...
/// layer(theme.button)` layer references, `@keyframes` step selectors, id and
/// element selectors, and the contents of comments / strings / `url()`, which
/// the older regex-only scanner had to approximate with a stack of masking
/// passes. Classes nested inside `:is()` / `:where()` / `:not()` / `:has()` /
/// `:any()` / `::slotted()` / `:host()` / `:nth-child(... of ...)` are
/// collected too, matching the regex scanner's "every `.class` token" behavior.
/// A source that nests brackets too deep for the parser stack returns `None`.
pub fn lightningcss_class_set(source: &str) -> Option<FxHashSet<String>> {
    crate::parse_style_bounded(source, || class_set_unbounded(source)).flatten()
}

fn class_set_unbounded(source: &str) -> Option<FxHashSet<String>> {
    // lightningcss rejects the bare `:global` / `:local` forms, and error
    // recovery then drops the full rule with its local classes. Blank the bare
    // keywords so the rule parses. The scope scan in
    // `mask_css_module_global_scopes` removes the global classes later.
    let source = blank_bare_css_module_scope_keywords(source);
    let options = ParserOptions {
        // Recover from individual malformed rules so a single bad rule does not
        // discard class names from the rest of the file.
        error_recovery: true,
        // These files are CSS Modules, so parse standard CSS syntax in CSS Modules
        // mode. That makes the `:local()` / `:global()` pseudo-classes parse as
        // real selectors rather than erroring. Classes in `:local()` are
        // collected, classes in `:global()` are not. Renaming is a print-time
        // concern, so the AST class names stay the original author-written names.
        css_modules: Some(lightningcss::css_modules::Config::default()),
        ..ParserOptions::default()
    };
    let stylesheet = StyleSheet::parse(&source, options).ok()?;
    let mut classes = FxHashSet::default();
    collect_classes_from_rules(&stylesheet.rules.0, &mut classes);
    Some(classes)
}

/// Recursively collect class-selector names from a list of CSS rules, descending
/// into every grouping rule (`@media`, `@supports`, `@container`, `@layer {}`,
/// `@document`, `@starting-style`, `@scope`, nested style rules) so a class
/// declared anywhere contributes to the set.
fn collect_classes_from_rules(rules: &[CssRule<'_>], classes: &mut FxHashSet<String>) {
    for rule in rules {
        match rule {
            CssRule::Style(style) => {
                collect_classes_from_selector_list(&style.selectors, classes);
                collect_classes_from_rules(&style.rules.0, classes);
            }
            CssRule::Media(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::Supports(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::Container(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::LayerBlock(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::MozDocument(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::StartingStyle(rule) => collect_classes_from_rules(&rule.rules.0, classes),
            CssRule::Nesting(rule) => {
                collect_classes_from_selector_list(&rule.style.selectors, classes);
                collect_classes_from_rules(&rule.style.rules.0, classes);
            }
            CssRule::Scope(rule) => {
                if let Some(scope_start) = &rule.scope_start {
                    collect_classes_from_selector_list(scope_start, classes);
                }
                if let Some(scope_end) = &rule.scope_end {
                    collect_classes_from_selector_list(scope_end, classes);
                }
                collect_classes_from_rules(&rule.rules.0, classes);
            }
            _ => {}
        }
    }
}

fn collect_classes_from_selector_list(list: &SelectorList<'_>, classes: &mut FxHashSet<String>) {
    for selector in &list.0 {
        collect_classes_from_selector(selector, classes);
    }
}

fn collect_classes_from_selector(selector: &Selector<'_>, classes: &mut FxHashSet<String>) {
    for component in selector.iter_raw_match_order() {
        match component {
            Component::Class(name) => {
                classes.insert(name.0.to_string());
            }
            Component::Is(list)
            | Component::Where(list)
            | Component::Has(list)
            | Component::Negation(list)
            | Component::Any(_, list) => {
                for nested in list.as_ref() {
                    collect_classes_from_selector(nested, classes);
                }
            }
            Component::Slotted(nested) | Component::Host(Some(nested)) => {
                collect_classes_from_selector(nested, classes);
            }
            Component::NthOf(data) => {
                for nested in data.selectors() {
                    collect_classes_from_selector(nested, classes);
                }
            }
            // CSS Modules `:local(.foo)` wraps a real local selector. A
            // `:global(.foo)` class is not in the class map, so it is skipped.
            Component::NonTSPseudoClass(PseudoClass::Local { selector }) => {
                collect_classes_from_selector(selector, classes);
            }
            _ => {}
        }
    }
}
