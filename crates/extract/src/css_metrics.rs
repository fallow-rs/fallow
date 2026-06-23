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
use lightningcss::properties::animation::AnimationName;
use lightningcss::properties::box_shadow::BoxShadow;
use lightningcss::properties::custom::{
    CustomProperty, CustomPropertyName, Token, TokenOrValue, Variable,
};
use lightningcss::properties::font::FontFamily;
use lightningcss::rules::CssRule;
use lightningcss::rules::font_face::FontFaceProperty;
use lightningcss::rules::keyframes::KeyframesName;
use lightningcss::rules::style::StyleRule;
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::ToCss;
use lightningcss::values::color::CssColor;
use lightningcss::visitor::{VisitTypes, Visitor};
use rustc_hash::FxHashSet;

use fallow_types::extract::{CssAnalytics, CssDeclarationBlock, CssRuleMetric};

/// Selector component count above which a rule is considered over-complex.
const MAX_PLAIN_COMPLEXITY: u16 = 4;

/// Style-rule nesting depth at or above which a rule is recorded.
const NOTABLE_NESTING_DEPTH: u8 = 3;

/// Upper bound on per-file recorded rules. Compiled utility frameworks can emit
/// thousands of `!important` rules; the scalar aggregates stay accurate while
/// the per-rule finding list is capped to keep output and storage bounded.
const MAX_NOTABLE_RULES: usize = 500;

/// Minimum declaration count for a rule to be fingerprinted as a duplicate-block
/// candidate. Small blocks (e.g. `display: flex; align-items: center`) repeat
/// legitimately, so the floor keeps the signal a strong copy-paste indicator.
const MIN_BLOCK_DECLARATIONS: usize = 4;

/// Upper bound on per-file declaration-block fingerprints. The `MIN_BLOCK`
/// floor already bounds compiled utility CSS (whose rules are tiny), so this
/// only guards a pathological hand-written stylesheet.
const MAX_DECLARATION_BLOCKS: usize = 2000;

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
    let mut collector = ValueCollector::default();
    let _ = collector.visit_stylesheet(&mut stylesheet);

    let mut analytics = acc.analytics;
    analytics.colors = sorted_vec(collector.colors);
    analytics.referenced_custom_properties = sorted_vec(collector.referenced_custom_properties);
    analytics.font_sizes = sorted_vec(acc.font_sizes);
    analytics.z_indexes = sorted_vec(acc.z_indexes);
    analytics.box_shadows = sorted_vec(acc.box_shadows);
    analytics.border_radii = sorted_vec(acc.border_radii);
    analytics.line_heights = sorted_vec(acc.line_heights);
    analytics.defined_custom_properties = sorted_vec(acc.defined_custom_properties);
    analytics.defined_keyframes = sorted_vec(acc.defined_keyframes);
    analytics.referenced_keyframes = sorted_vec(acc.referenced_keyframes);
    analytics.registered_custom_properties = sorted_vec(acc.registered_custom_properties);
    analytics.declared_layers = sorted_vec(acc.declared_layers);
    analytics.populated_layers = sorted_vec(acc.populated_layers);
    analytics.defined_font_faces = sorted_vec(acc.defined_font_faces);
    analytics.referenced_font_families = sorted_vec(acc.referenced_font_families);
    Some(analytics)
}

/// Working accumulator threaded through the rule walk: the structural analytics
/// plus the per-stylesheet sets of distinct `font-size` / `z-index` values.
#[derive(Default)]
struct Accumulator {
    analytics: CssAnalytics,
    font_sizes: FxHashSet<String>,
    z_indexes: FxHashSet<String>,
    box_shadows: FxHashSet<String>,
    border_radii: FxHashSet<String>,
    line_heights: FxHashSet<String>,
    defined_custom_properties: FxHashSet<String>,
    defined_keyframes: FxHashSet<String>,
    referenced_keyframes: FxHashSet<String>,
    registered_custom_properties: FxHashSet<String>,
    declared_layers: FxHashSet<String>,
    populated_layers: FxHashSet<String>,
    defined_font_faces: FxHashSet<String>,
    referenced_font_families: FxHashSet<String>,
}

/// The concrete family name of a `font-family` value, or `None` for a generic
/// keyword (`serif`, `sans-serif`, `monospace`, ...), which is never an authored
/// `@font-face`.
fn font_family_name(family: &FontFamily<'_>) -> Option<String> {
    match family {
        // Render the family via ToCss and strip surrounding quotes so a declared
        // `font-family: "Inter"` and a referenced `font-family: Inter` normalize
        // to the same key.
        FontFamily::FamilyName(_) => family
            .to_css_string(PrinterOptions::default())
            .ok()
            .map(|s| s.trim_matches(['"', '\'']).to_string()),
        FontFamily::Generic(_) => None,
    }
}

/// Collects value-level design tokens via the lightningcss visitor: every
/// distinct color (including colors nested in shorthands like `border` /
/// `background` and gradients, not just standalone `color:` values) and every
/// `var()` custom-property reference.
#[derive(Default)]
struct ValueCollector {
    colors: FxHashSet<String>,
    referenced_custom_properties: FxHashSet<String>,
}

impl Visitor<'_> for ValueCollector {
    type Error = std::convert::Infallible;

    fn visit_types(&self) -> VisitTypes {
        VisitTypes::COLORS | VisitTypes::VARIABLES
    }

    fn visit_color(&mut self, color: &mut CssColor) -> Result<(), Self::Error> {
        if let Ok(rendered) = color.to_css_string(PrinterOptions::default()) {
            self.colors.insert(rendered);
        }
        Ok(())
    }

    fn visit_variable(&mut self, var: &mut Variable<'_>) -> Result<(), Self::Error> {
        self.referenced_custom_properties
            .insert(var.name.ident.0.to_string());
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
            CssRule::LayerBlock(rule) => {
                // A named `@layer a { }` both declares and populates layer `a`.
                if let Some(name) = &rule.name {
                    let name = layer_name_string(name);
                    acc.declared_layers.insert(name.clone());
                    acc.populated_layers.insert(name);
                }
                walk_rules(&rule.rules.0, depth, acc);
            }
            CssRule::LayerStatement(stmt) => {
                // `@layer a, b, c;` declares ordering but populates nothing.
                for name in &stmt.names {
                    acc.declared_layers.insert(layer_name_string(name));
                }
            }
            CssRule::Property(prop) => {
                acc.registered_custom_properties
                    .insert(prop.name.0.to_string());
            }
            CssRule::FontFace(font_face) => {
                for property in &font_face.properties {
                    if let FontFaceProperty::FontFamily(family) = property
                        && let Some(name) = font_family_name(family)
                    {
                        acc.defined_font_faces.insert(name);
                    }
                }
            }
            CssRule::MozDocument(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::StartingStyle(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Scope(rule) => walk_rules(&rule.rules.0, depth, acc),
            CssRule::Nesting(rule) => {
                record_style_rule(&rule.style, depth, acc);
                walk_rules(&rule.style.rules.0, depth.saturating_add(1), acc);
            }
            CssRule::Keyframes(keyframes) => {
                acc.defined_keyframes
                    .insert(keyframes_name_string(&keyframes.name));
            }
            _ => {}
        }
    }
}

fn layer_name_string(name: &lightningcss::rules::layer::LayerName<'_>) -> String {
    name.0
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn keyframes_name_string(name: &KeyframesName<'_>) -> String {
    match name {
        KeyframesName::Ident(ident) => ident.0.to_string(),
        KeyframesName::Custom(value) => value.to_string(),
    }
}

fn collect_animation_name(name: &AnimationName<'_>, out: &mut FxHashSet<String>) {
    if let AnimationName::Ident(ident) = name {
        out.insert(ident.0.to_string());
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

    // Fingerprint the declaration block (sorted, !important-tagged) for cross-file
    // duplicate-block detection, gated on the minimum block size and a per-file cap.
    if declaration_count >= MIN_BLOCK_DECLARATIONS
        && analytics.declaration_blocks.len() < MAX_DECLARATION_BLOCKS
        && let Some(fingerprint) = declaration_block_fingerprint(style)
    {
        analytics.declaration_blocks.push(CssDeclarationBlock {
            fingerprint,
            line: style.loc.line.saturating_add(1),
            declaration_count: saturate_u16(declaration_count),
        });
    }

    collect_rule_property_tokens(style, acc);
}

/// Scan a rule's declarations (normal + `!important`) for design-token values,
/// custom-property definitions, and `@keyframes` / font-family references,
/// folding them into `acc`. Colors and `var()` references are collected
/// separately by the value visitor.
fn collect_rule_property_tokens(style: &StyleRule<'_>, acc: &mut Accumulator) {
    for property in style
        .declarations
        .declarations
        .iter()
        .chain(style.declarations.important_declarations.iter())
    {
        collect_property_tokens(property, acc);
    }
}

/// Fold a single declaration's design-token value, custom-property definition,
/// `@keyframes` reference, or font-family reference into `acc`.
fn collect_property_tokens(property: &Property<'_>, acc: &mut Accumulator) {
    match property {
        Property::FontSize(font_size) => {
            insert_rendered_css(font_size, &mut acc.font_sizes);
        }
        Property::ZIndex(z_index) => {
            insert_rendered_css(z_index, &mut acc.z_indexes);
        }
        // Shadow / radius / line-height tokens (design-token-sprawl axes).
        // The INNER value is serialized (not the property), so the vendor
        // prefix is dropped and `-webkit-box-shadow: X` collapses to the same
        // distinct value as `box-shadow: X` rather than inflating the count.
        Property::BoxShadow(shadows, _) => collect_box_shadow_tokens(shadows, acc),
        Property::BorderRadius(radius, _) => {
            insert_rendered_css(radius, &mut acc.border_radii);
        }
        Property::LineHeight(line_height) => {
            insert_rendered_css(line_height, &mut acc.line_heights);
        }
        Property::Custom(custom) => collect_custom_property_tokens(custom, acc),
        Property::AnimationName(names, _) => {
            collect_animation_references(names, &mut acc.referenced_keyframes);
        }
        Property::Animation(animations, _) => {
            for animation in animations {
                collect_animation_name(&animation.name, &mut acc.referenced_keyframes);
            }
        }
        Property::FontFamily(families) => {
            collect_font_family_references(families, &mut acc.referenced_font_families);
        }
        Property::Font(font) => {
            collect_font_family_references(&font.family, &mut acc.referenced_font_families);
        }
        _ => {}
    }
}

fn insert_rendered_css<T: ToCss>(value: &T, out: &mut FxHashSet<String>) {
    if let Ok(rendered) = value.to_css_string(PrinterOptions::default()) {
        out.insert(rendered);
    }
}

fn collect_box_shadow_tokens(shadows: &[BoxShadow], acc: &mut Accumulator) {
    let rendered: Vec<String> = shadows
        .iter()
        .filter_map(|shadow| shadow.to_css_string(PrinterOptions::default()).ok())
        .collect();
    if !rendered.is_empty() && rendered.len() == shadows.len() {
        acc.box_shadows.insert(rendered.join(", "));
    }
}

fn collect_animation_references(names: &[AnimationName<'_>], out: &mut FxHashSet<String>) {
    for name in names {
        collect_animation_name(name, out);
    }
}

fn collect_font_family_references(families: &[FontFamily<'_>], out: &mut FxHashSet<String>) {
    for family in families {
        if let Some(name) = font_family_name(family) {
            out.insert(name);
        }
    }
}

/// Record a custom-property definition and credit any font-family string / ident
/// values referenced inside its raw token stream.
fn collect_custom_property_tokens(custom: &CustomProperty<'_>, acc: &mut Accumulator) {
    if let CustomPropertyName::Custom(name) = &custom.name {
        acc.defined_custom_properties.insert(name.0.to_string());
    }
    // A custom-property value can REFERENCE a font family without a
    // `font-family:` declaration: a Tailwind v4 `--font-*` theme token
    // (`--font-display: "Departure Mono", monospace`) is the canonical
    // case. lightningcss's `Property::FontFamily` / `Property::Font`
    // arms above never see this (a `--*:` declaration is an opaque
    // token stream), so scan the raw tokens for string / ident values
    // and credit them as referenced families. Generic keywords
    // (`serif`, `monospace`) never appear in `defined_font_faces`, so
    // crediting them here is inert; the `unused_font_faces`
    // set-difference only ever drops a genuinely-declared family.
    for token in &custom.value.0 {
        if let TokenOrValue::Token(Token::String(value) | Token::Ident(value)) = token {
            acc.referenced_font_families.insert(value.to_string());
        }
    }
}

/// Fingerprint a rule's declaration block: serialize each declaration (tagging
/// `!important` ones, which lightningcss stores without the flag, so they do not
/// collide with their non-important twin), sort for order-insensitivity, join,
/// and xxh3-hash. Returns `None` if any declaration fails to serialize, so a
/// partial block is never fingerprinted (a false duplicate match would be worse
/// than missing one).
fn declaration_block_fingerprint(style: &StyleRule<'_>) -> Option<u64> {
    let block = &style.declarations;
    let mut parts: Vec<String> =
        Vec::with_capacity(block.declarations.len() + block.important_declarations.len());
    for decl in &block.declarations {
        parts.push(decl.to_css_string(false, PrinterOptions::default()).ok()?);
    }
    for decl in &block.important_declarations {
        // `important = true` renders the `!important` suffix, so a block with an
        // important declaration never collides with its non-important twin.
        parts.push(decl.to_css_string(true, PrinterOptions::default()).ok()?);
    }
    parts.sort_unstable();
    Some(xxhash_rust::xxh3::xxh3_64(parts.join(";").as_bytes()))
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

#[cfg(all(test, not(miri)))]
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

    #[test]
    fn collects_defined_and_referenced_custom_properties() {
        let a = analytics(":root { --brand: red; --unused: blue; }\n.a { color: var(--brand); }");
        assert!(
            a.defined_custom_properties.contains(&"--brand".to_string()),
            "defined: {:?}",
            a.defined_custom_properties
        );
        assert!(
            a.defined_custom_properties
                .contains(&"--unused".to_string())
        );
        assert!(
            a.referenced_custom_properties
                .contains(&"--brand".to_string()),
            "referenced: {:?}",
            a.referenced_custom_properties
        );
        assert!(
            !a.referenced_custom_properties
                .contains(&"--unused".to_string()),
            "--unused has no var() reference"
        );
    }

    #[test]
    fn collects_defined_and_referenced_keyframes() {
        let a = analytics(
            "@keyframes spin { from {} to {} }\n@keyframes unused { from {} }\n.a { animation-name: spin; }",
        );
        assert!(a.defined_keyframes.contains(&"spin".to_string()));
        assert!(a.defined_keyframes.contains(&"unused".to_string()));
        assert!(a.referenced_keyframes.contains(&"spin".to_string()));
        assert!(
            !a.referenced_keyframes.contains(&"unused".to_string()),
            "no animation references `unused`"
        );
    }

    #[test]
    fn animation_shorthand_references_keyframes() {
        let a = analytics("@keyframes pulse { from {} }\n.a { animation: pulse 1s infinite; }");
        assert!(
            a.referenced_keyframes.contains(&"pulse".to_string()),
            "referenced: {:?}",
            a.referenced_keyframes
        );
    }

    #[test]
    fn fingerprints_blocks_at_floor_order_insensitive() {
        // Two 4-declaration rules with the same declarations in different order
        // share a fingerprint; a 3-declaration rule is below the floor and is
        // not fingerprinted.
        let a = analytics(
            ".x { color: red; margin: 1px; padding: 2px; top: 3px; }\n\
             .y { top: 3px; padding: 2px; margin: 1px; color: red; }\n\
             .z { color: red; margin: 1px; padding: 2px; }\n",
        );
        assert_eq!(
            a.declaration_blocks.len(),
            2,
            "two 4-decl rules fingerprinted, the 3-decl one skipped: {:?}",
            a.declaration_blocks
        );
        assert_eq!(
            a.declaration_blocks[0].fingerprint, a.declaration_blocks[1].fingerprint,
            "same declarations in different order share a fingerprint"
        );
        assert_eq!(a.declaration_blocks[0].declaration_count, 4);
    }

    #[test]
    fn important_distinguishes_block_fingerprint() {
        let a = analytics(
            ".x { color: red; margin: 1px; padding: 2px; top: 3px; }\n\
             .y { color: red !important; margin: 1px; padding: 2px; top: 3px; }\n",
        );
        assert_eq!(a.declaration_blocks.len(), 2);
        assert_ne!(
            a.declaration_blocks[0].fingerprint, a.declaration_blocks[1].fingerprint,
            "!important changes the block fingerprint"
        );
    }
}
