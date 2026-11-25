//! CSS rule metrics, declaration blocks, raw style values, custom property
//! definitions, and CSS analytics.

/// Structural CSS metrics for one style rule of the parsed CSS. Only a rule
/// over a structural floor (an id selector, a complex selector, `!important`,
/// or deep nesting) is recorded, so the vector stays bounded. Not cached:
/// `fallow health` computes these from the CSS source (no `bitcode` derive).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssRuleMetric {
    /// 1-based line of the rule's first selector.
    pub line: u32,
    /// 1-based column of the rule's first selector.
    pub col: u32,
    /// Specificity component `a` (id selectors), max across the rule's selectors.
    pub specificity_a: u16,
    /// Specificity component `b` (class / attribute / pseudo-class selectors).
    pub specificity_b: u16,
    /// Specificity component `c` (type / pseudo-element selectors).
    pub specificity_c: u16,
    /// Largest selector component count across the rule's selector list.
    pub complexity: u16,
    /// Declaration count in the rule (normal plus `!important`).
    pub declaration_count: u16,
    /// `!important` declaration count in the rule.
    pub important_count: u16,
    /// Style-rule nesting depth (0 = top level).
    pub nesting_depth: u8,
    /// Selector list of a notable rule, for audit keys. Never serialized.
    #[serde(skip)]
    pub selector: String,
}

/// A style rule's declaration-block fingerprint and location, for cross-file
/// duplicate-block detection. Only rules with a meaningful number of
/// declarations are recorded (small blocks repeat legitimately). Internal
/// staging only: this is consumed in-process by the health layer to build the
/// grouped `duplicate_declaration_blocks` output and is never serialized.
#[derive(Debug, Clone)]
pub struct CssDeclarationBlock {
    /// xxh3 fingerprint over the rule's normalized (sorted, `!important`-tagged)
    /// declaration set.
    pub fingerprint: u64,
    /// 1-based line of the rule's first selector.
    pub line: u32,
    /// Declaration count in the rule (normal plus `!important`).
    pub declaration_count: u16,
}

/// Located raw styling value authored directly in CSS rather than via a
/// custom property or design-token helper. Internal staging for the health
/// layer; public output adds actions and confidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssRawStyleValue {
    /// Value axis, e.g. `color`, `font-size`, `line-height`, `radius`, or `shadow`.
    pub axis: String,
    /// CSS property where the value appears.
    pub property: String,
    /// Rendered declaration value.
    pub value: String,
    /// 1-based line of the containing style rule.
    pub line: u32,
}

/// Located CSS custom-property definition with its rendered value. Internal
/// staging for design-token reuse suggestions in the health layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssCustomPropertyDefinition {
    /// Custom property name, including the leading `--`.
    pub name: String,
    /// Rendered custom property value.
    pub value: String,
    /// 1-based line of the containing style rule.
    pub line: u32,
}

/// Stylesheet-level structural CSS analytics, computed from the parsed CSS
/// syntax tree. Feeds `fallow health` penalty weights and located findings,
/// never a standalone CSS score.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssAnalytics {
    /// Total declarations across every style rule (normal plus `!important`).
    pub total_declarations: u32,
    /// Total `!important` declarations across every style rule.
    pub important_declarations: u32,
    /// Number of style rules.
    pub rule_count: u32,
    /// Number of style rules with no declarations.
    pub empty_rule_count: u32,
    /// Deepest style-rule nesting depth observed (0 = no nesting).
    pub max_nesting_depth: u8,
    /// Rules that crossed the structural floor, in source order. Bounded; see
    /// [`Self::notable_truncated`]. The scalar aggregates above always reflect
    /// the full stylesheet regardless of truncation.
    pub notable_rules: Vec<CssRuleMetric>,
    /// `true` when more rules crossed the structural floor than `notable_rules`
    /// retains (compiled utility CSS can emit thousands of `!important` rules),
    /// so consumers can note that per-rule findings were capped.
    pub notable_truncated: bool,
    /// Distinct color values in the stylesheet, sorted (a palette-size /
    /// design-token-sprawl signal). The parser canonicalizes notation, so the
    /// authored format is NOT preserved: `red`, `#f00`, `#ff0000`, and
    /// `rgb(255,0,0)` all collapse to one entry, and every legacy sRGB notation
    /// renders as hex. Notation-MIXING (hex vs rgb vs hsl) is therefore not
    /// detectable from this set; it would need a separate raw-token pass.
    pub colors: Vec<String>,
    /// Distinct `font-size` declaration values in the stylesheet, sorted.
    pub font_sizes: Vec<String>,
    /// Distinct `z-index` declaration values in the stylesheet, sorted.
    pub z_indexes: Vec<String>,
    /// Distinct `box-shadow` declaration values in the stylesheet, sorted. A
    /// high count signals an uncontrolled shadow scale (design-token sprawl).
    pub box_shadows: Vec<String>,
    /// Distinct `border-radius` declaration values in the stylesheet, sorted.
    pub border_radii: Vec<String>,
    /// Distinct `line-height` declaration values in the stylesheet, sorted.
    pub line_heights: Vec<String>,
    /// Bounded located raw styling values that bypass custom properties or
    /// token helpers. These are conservative declaration-level candidates for
    /// audit introduced-vs-base gating.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub raw_style_values: Vec<CssRawStyleValue>,
    /// Located custom-property definitions with values. Internal staging
    /// consumed by the health layer for nearest-token suggestions.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub custom_property_definitions: Vec<CssCustomPropertyDefinition>,
    /// Distinct custom properties (`--x`) defined in the stylesheet, sorted.
    pub defined_custom_properties: Vec<String>,
    /// Distinct custom properties referenced via `var()` in the stylesheet.
    pub referenced_custom_properties: Vec<String>,
    /// Distinct `@keyframes` names defined in the stylesheet, sorted.
    pub defined_keyframes: Vec<String>,
    /// Distinct `@keyframes` names referenced via `animation` / `animation-name`.
    pub referenced_keyframes: Vec<String>,
    /// First 1-based rule line per `referenced_keyframes` name, sorted (internal staging).
    #[serde(skip)]
    pub keyframe_reference_lines: Vec<(String, u32)>,
    /// Distinct custom properties registered via an `@property` rule, sorted.
    pub registered_custom_properties: Vec<String>,
    /// Distinct cascade layers declared (via `@layer a, b;` statements or named
    /// `@layer a { }` blocks), sorted.
    pub declared_layers: Vec<String>,
    /// Distinct cascade layers populated by a named `@layer a { }` block, sorted.
    /// A layer declared but never populated (and not imported into) is a
    /// cleanup candidate.
    pub populated_layers: Vec<String>,
    /// Distinct font families declared by an `@font-face` rule in the stylesheet,
    /// sorted. A declared family referenced by no `font-family` anywhere is a
    /// dead web-font payload (cleanup candidate).
    pub defined_font_faces: Vec<String>,
    /// Distinct font families referenced via `font-family` / `font` in the
    /// stylesheet, sorted (generic keywords like `serif` excluded).
    pub referenced_font_families: Vec<String>,
    /// Declaration-block fingerprints of rules at or above the minimum block size.
    /// Internal staging for duplicate-block detection, never serialized.
    #[serde(skip)]
    pub declaration_blocks: Vec<CssDeclarationBlock>,
}
