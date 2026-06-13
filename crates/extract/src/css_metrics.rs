//! Structural CSS analytics computed from the parsed CSS syntax tree.
//!
//! `fallow health` consumes these on demand to surface specificity hotspots,
//! `!important` density, over-complex selectors, and deep nesting: the kind of
//! codebase-scale structural CSS slop that per-rule linters do not aggregate.
//! The metrics come from the same lightningcss parse used for CSS Module class
//! extraction. Callers gate by file extension: lightningcss parses standard CSS,
//! not Sass, so `.scss` sources are NOT passed here (with error recovery on,
//! Sass syntax recovers into a partial, inaccurate result rather than failing).
//! A hard parse failure yields `None`.

use lightningcss::printer::PrinterOptions;
use lightningcss::properties::Property;
use lightningcss::rules::CssRule;
use lightningcss::rules::style::StyleRule;
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::ToCss;
use lightningcss::values::color::CssColor;
use lightningcss::visitor::{VisitTypes, Visitor};
use rustc_hash::FxHashSet;

use fallow_types::extract::{CssAnalytics, CssRuleMetric};

/// Selector component count above which a rule is considered over-complex.
const MAX_PLAIN_COMPLEXITY: u16 = 4;

/// Style-rule nesting depth at or above which a rule is recorded.
const NOTABLE_NESTING_DEPTH: u8 = 3;

/// Upper bound on per-file recorded rules. Compiled utility frameworks can emit
/// thousands of `!important` rules; the scalar aggregates stay accurate while
/// the per-rule finding list is capped to keep output and storage bounded.
const MAX_NOTABLE_RULES: usize = 500;

/// Mask for a single 10-bit CSS specificity component.
const SPECIFICITY_COMPONENT_MASK: u32 = 0x3FF;

/// Compute structural CSS analytics for a standard-CSS stylesheet source.
///
/// Returns `None` only on a hard parse failure; with error recovery on,
/// individual malformed rules are skipped and the rest of the sheet still
/// contributes. Callers must gate by extension and NOT pass `.scss` sources:
/// Sass syntax is not standard CSS and recovers into an inaccurate partial
/// rather than `None`. Parsing runs in CSS Modules mode so `:local()` /
/// `:global()` selectors are understood.
#[must_use]
pub fn compute_css_analytics(source: &str) -> Option<CssAnalytics> {
    let options = ParserOptions {
        error_recovery: true,
        css_modules: Some(lightningcss::css_modules::Config::default()),
        ..ParserOptions::default()
    };
    let mut stylesheet = StyleSheet::parse(source, options).ok()?;

    // Pass 1: walk the rule tree for structural metrics + font-size / z-index
    // design tokens (these are top-level declaration properties).
    let mut acc = Accumulator::default();
    walk_rules(&stylesheet.rules.0, 0, &mut acc);

    // Pass 2: visit every color value (including colors nested inside shorthands
    // and gradients) for the design-token-sprawl signal. The visitor needs `&mut`,
    // so it runs after the immutable rule walk above.
    let mut colors = ColorCollector::default();
    let _ = colors.visit_stylesheet(&mut stylesheet);

    let mut analytics = acc.analytics;
    analytics.colors = sorted_vec(colors.colors);
    analytics.font_sizes = sorted_vec(acc.font_sizes);
    analytics.z_indexes = sorted_vec(acc.z_indexes);
    Some(analytics)
}

/// Working accumulator threaded through the rule walk: the structural analytics
/// plus the per-stylesheet sets of distinct `font-size` / `z-index` values.
#[derive(Default)]
struct Accumulator {
    analytics: CssAnalytics,
    font_sizes: FxHashSet<String>,
    z_indexes: FxHashSet<String>,
}

/// Collects every distinct color value (authored form) in a stylesheet via the
/// lightningcss visitor, so colors nested inside shorthands (`border`,
/// `background`) and gradients are caught, not just standalone `color:` values.
#[derive(Default)]
struct ColorCollector {
    colors: FxHashSet<String>,
}

impl Visitor<'_> for ColorCollector {
    type Error = std::convert::Infallible;

    fn visit_types(&self) -> VisitTypes {
        VisitTypes::COLORS
    }

    fn visit_color(&mut self, color: &mut CssColor) -> Result<(), Self::Error> {
        if let Ok(rendered) = color.to_css_string(PrinterOptions::default()) {
            self.colors.insert(rendered);
        }
        Ok(())
    }
}

fn sorted_vec(set: FxHashSet<String>) -> Vec<String> {
    let mut values: Vec<String> = set.into_iter().collect();
    values.sort_unstable();
    values
}

/// Recursively walk rules, tracking style-rule nesting depth. Grouping rules
/// (`@media` / `@supports` / `@container` / `@layer {}` / `@document` /
/// `@starting-style` / `@scope`) pass their nesting depth through unchanged;
/// only nesting INSIDE a style rule increases the depth.
fn walk_rules(rules: &[CssRule<'_>], depth: u8, acc: &mut Accumulator) {
    for rule in rules {
        match rule {
            CssRule::Style(style) => {
                record_style_rule(style, depth, acc);
                walk_rules(&style.rules.0, depth.saturating_add(1), acc);
            }
            CssRule::Media(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Supports(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Container(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::LayerBlock(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::MozDocument(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::StartingStyle(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Scope(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Nesting(rule) => {
                record_style_rule(&rule.style, depth, acc);
                walk_rules(&rule.style.rules.0, depth.saturating_add(1), acc);
            }
            _ => {}
        }
    }
}

fn record_style_rule(style: &StyleRule<'_>, depth: u8, acc: &mut Accumulator) {
    let normal = style.declarations.declarations.len();
    let important = style.declarations.important_declarations.len();
    let declaration_count = normal + important;

    let analytics = &mut acc.analytics;
    analytics.rule_count = analytics.rule_count.saturating_add(1);
    analytics.total_declarations = analytics
        .total_declarations
        .saturating_add(saturate_u32(declaration_count));
    analytics.important_declarations = analytics
        .important_declarations
        .saturating_add(saturate_u32(important));
    if declaration_count == 0 {
        analytics.empty_rule_count = analytics.empty_rule_count.saturating_add(1);
    }
    analytics.max_nesting_depth = analytics.max_nesting_depth.max(depth);

    let (a, b, c, complexity) = rule_selector_metrics(style);
    let metric = CssRuleMetric {
        line: style.loc.line.saturating_add(1),
        col: style.loc.column,
        specificity_a: a,
        specificity_b: b,
        specificity_c: c,
        complexity,
        declaration_count: saturate_u16(declaration_count),
        important_count: saturate_u16(important),
        nesting_depth: depth,
    };

    if is_notable(&metric) {
        if analytics.notable_rules.len() < MAX_NOTABLE_RULES {
            analytics.notable_rules.push(metric);
        } else {
            analytics.notable_truncated = true;
        }
    }

    // Design-token values: collect distinct `font-size` / `z-index` declaration
    // values (their authored form). Colors are collected separately by the
    // visitor because they nest inside shorthands and gradients.
    for property in style
        .declarations
        .declarations
        .iter()
        .chain(style.declarations.important_declarations.iter())
    {
        match property {
            Property::FontSize(font_size) => {
                if let Ok(rendered) = font_size.to_css_string(PrinterOptions::default()) {
                    acc.font_sizes.insert(rendered);
                }
            }
            Property::ZIndex(z_index) => {
                if let Ok(rendered) = z_index.to_css_string(PrinterOptions::default()) {
                    acc.z_indexes.insert(rendered);
                }
            }
            _ => {}
        }
    }
}

/// Return the rule's `(specificity_a, specificity_b, specificity_c, complexity)`
/// taking the most specific selector and the most complex selector across the
/// rule's selector list.
fn rule_selector_metrics(style: &StyleRule<'_>) -> (u16, u16, u16, u16) {
    let mut max_spec = 0u32;
    let mut a = 0u16;
    let mut b = 0u16;
    let mut c = 0u16;
    let mut complexity = 0u16;
    for selector in &style.selectors.0 {
        let spec = selector.specificity();
        if spec >= max_spec {
            max_spec = spec;
            a = specificity_component(spec, 20);
            b = specificity_component(spec, 10);
            c = specificity_component(spec, 0);
        }
        complexity = complexity.max(selector_complexity(selector));
    }
    (a, b, c, complexity)
}

fn specificity_component(specificity: u32, shift: u32) -> u16 {
    saturate_u16_u32((specificity >> shift) & SPECIFICITY_COMPONENT_MASK)
}

fn is_notable(metric: &CssRuleMetric) -> bool {
    metric.specificity_a >= 1
        || metric.complexity > MAX_PLAIN_COMPLEXITY
        || metric.important_count >= 1
        || metric.nesting_depth >= NOTABLE_NESTING_DEPTH
}

fn selector_complexity(selector: &Selector<'_>) -> u16 {
    let mut count = 0u16;
    count_components(selector, &mut count);
    count
}

fn count_components(selector: &Selector<'_>, count: &mut u16) {
    for component in selector.iter_raw_match_order() {
        *count = count.saturating_add(1);
        match component {
            Component::Is(list)
            | Component::Where(list)
            | Component::Has(list)
            | Component::Negation(list)
            | Component::Any(_, list) => {
                for nested in list.as_ref() {
                    count_components(nested, count);
                }
            }
            Component::Slotted(nested) | Component::Host(Some(nested)) => {
                count_components(nested, count);
            }
            Component::NthOf(data) => {
                for nested in data.selectors() {
                    count_components(nested, count);
                }
            }
            _ => {}
        }
    }
}

fn saturate_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn saturate_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn saturate_u16_u32(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analytics(source: &str) -> CssAnalytics {
        compute_css_analytics(source).expect("standard CSS parses")
    }

    #[test]
    fn recovers_partial_metrics_around_a_malformed_rule() {
        // Error recovery skips the broken rule and still records the valid one,
        // so a file with one bad rule is not lost wholesale.
        let a = analytics("#main { color: red; } @@@ broken @@@ .ok { color: blue; }");
        assert!(a.rule_count >= 1);
        assert!(a.notable_rules.iter().any(|r| r.specificity_a == 1));
    }

    #[test]
    fn counts_declarations_and_important() {
        let a = analytics(".a { color: red; width: 1px !important; }");
        assert_eq!(a.rule_count, 1);
        assert_eq!(a.total_declarations, 2);
        assert_eq!(a.important_declarations, 1);
    }

    #[test]
    fn id_selector_is_notable_with_specificity() {
        let a = analytics("#main { color: red; }");
        assert_eq!(a.notable_rules.len(), 1);
        let rule = &a.notable_rules[0];
        assert_eq!(rule.specificity_a, 1);
        assert_eq!(rule.specificity_b, 0);
        assert_eq!(rule.specificity_c, 0);
    }

    #[test]
    fn plain_class_rule_is_not_notable() {
        let a = analytics(".btn { color: red; }");
        assert!(a.notable_rules.is_empty(), "got {:?}", a.notable_rules);
        assert_eq!(a.rule_count, 1);
    }

    #[test]
    fn important_declaration_makes_rule_notable() {
        let a = analytics(".btn { color: red !important; }");
        assert_eq!(a.notable_rules.len(), 1);
        assert_eq!(a.notable_rules[0].important_count, 1);
    }

    #[test]
    fn empty_rule_counted() {
        let a = analytics(".a { } .b { color: red; }");
        assert_eq!(a.rule_count, 2);
        assert_eq!(a.empty_rule_count, 1);
    }

    #[test]
    fn complex_selector_is_notable() {
        // Five compound selectors joined by combinators exceeds the floor.
        let a = analytics("div > ul > li > a > span { color: red; }");
        assert_eq!(a.notable_rules.len(), 1);
        assert!(a.notable_rules[0].complexity > MAX_PLAIN_COMPLEXITY);
    }

    #[test]
    fn nesting_depth_tracked() {
        let a = analytics(".a { .b { .c { .d { color: red; } } } }");
        assert!(a.max_nesting_depth >= 3, "got {}", a.max_nesting_depth);
        // The depth-3 rule (`.d`) crosses the nesting floor.
        assert!(
            a.notable_rules
                .iter()
                .any(|r| r.nesting_depth >= NOTABLE_NESTING_DEPTH)
        );
    }

    #[test]
    fn specificity_takes_most_specific_selector_in_list() {
        let a = analytics("#id, .cls { color: red; }");
        assert_eq!(a.notable_rules.len(), 1);
        // `#id` (1,0,0) is more specific than `.cls` (0,1,0).
        assert_eq!(a.notable_rules[0].specificity_a, 1);
    }

    #[test]
    fn line_is_one_based() {
        let a = analytics("\n\n#main { color: red; }");
        assert_eq!(a.notable_rules[0].line, 3);
    }

    #[test]
    fn media_query_rules_walked() {
        let a = analytics("@media (min-width: 600px) { #main { color: red; } }");
        assert_eq!(a.rule_count, 1);
        assert_eq!(a.notable_rules.len(), 1);
        assert_eq!(a.notable_rules[0].specificity_a, 1);
    }

    #[test]
    fn collects_distinct_colors() {
        let a = analytics(".a { color: red; } .b { color: blue; } .c { color: red; }");
        assert_eq!(a.colors.len(), 2, "distinct colors deduped: {:?}", a.colors);
    }

    #[test]
    fn collects_colors_nested_in_shorthands() {
        // The color inside the `border` shorthand must be caught, not just the
        // standalone `background` color: that is the point of the value visitor.
        let a = analytics(".a { border: 1px solid green; background: yellow; }");
        assert!(
            a.colors.len() >= 2,
            "shorthand + standalone colors collected: {:?}",
            a.colors
        );
    }

    #[test]
    fn collects_distinct_font_sizes() {
        let a =
            analytics(".a { font-size: 14px; } .b { font-size: 14px; } .c { font-size: 1rem; }");
        assert_eq!(a.font_sizes.len(), 2, "got {:?}", a.font_sizes);
    }

    #[test]
    fn collects_distinct_z_indexes() {
        let a = analytics(".a { z-index: 10; } .b { z-index: 10; } .c { z-index: 999; }");
        assert_eq!(a.z_indexes.len(), 2, "got {:?}", a.z_indexes);
    }
}
