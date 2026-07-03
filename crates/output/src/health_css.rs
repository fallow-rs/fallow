/// Structural CSS analytics surfaced by `fallow health --css`.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssAnalyticsReport {
    /// Stylesheets with at least one structurally notable rule, in scan order.
    pub files: Vec<CssFileAnalytics>,
    /// Project-wide CSS aggregates across every analyzed stylesheet.
    pub summary: CssAnalyticsSummary,
    /// Vue SFCs whose `<style scoped>` defines classes used nowhere else in the
    /// component (cleanup candidates).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scoped_unused: Vec<ScopedUnusedClasses>,
    /// `@keyframes` defined but referenced via no `animation` / `animation-name`
    /// in any stylesheet, with the stylesheet that defines them (cleanup
    /// candidates; an animation name can still be applied from JavaScript).
    /// The "defined-but-unused" direction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreferenced_keyframes: Vec<UnreferencedKeyframes>,
    /// Animation references (`animation` / `animation-name`) to a `@keyframes`
    /// name that is defined in NO stylesheet anywhere in the project, with the
    /// first stylesheet that references them. The "used-but-undefined" direction
    /// (the inverse of `unreferenced_keyframes`): usually a typo or a removed
    /// animation, occasionally a `@keyframes` defined in CSS-in-JS (which the
    /// CSS parser never sees). Conservative candidates, never gated findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub undefined_keyframes: Vec<UndefinedKeyframes>,
    /// Groups of style rules across the project that share an identical
    /// declaration block (4+ declarations, sorted and `!important`-aware),
    /// grouped by content: copy-paste consolidation candidates (fallow's
    /// duplication signal applied to CSS). Sorted by estimated savings
    /// descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub duplicate_declaration_blocks: Vec<CssDuplicateBlock>,
    /// CVA / shadcn variant class strings that repeat the same normalized class
    /// block in several variant values. Kept separate from CSS declaration-block
    /// duplication because the source is JS config, not parsed CSS rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cva_duplicate_variant_blocks: Vec<CvaDuplicateVariantBlock>,
    /// CVA / shadcn variant class strings that hardcode a Tailwind arbitrary
    /// value even though an existing token has the same or nearest comparable
    /// value. Advisory: variants often encode product semantics, so agents
    /// should verify intent before replacing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cva_variant_token_drifts: Vec<CvaVariantTokenDrift>,
    /// Tailwind arbitrary-value utilities (`w-[13px]`, `bg-[#abc]`) found in
    /// markup, which hardcode a one-off value instead of a configured scale
    /// token (design-token bypass). Present only when the project uses Tailwind.
    /// Sorted by use count descending. Candidates, not findings: an arbitrary
    /// value is sometimes the right call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tailwind_arbitrary_values: Vec<TailwindArbitraryValue>,
    /// Located raw CSS declaration values that bypass token surfaces (`var()`,
    /// `token()`, `theme()`) on scale-sensitive axes such as color, font-size,
    /// line-height, radius, and shadow. Conservative candidates: a raw value can
    /// be intentional, but introduced raw values are useful audit feedback.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_style_values: Vec<RawStyleValue>,
    /// Unused CSS at-rule entities: an `@property` registered but never read via
    /// `var()` in any stylesheet, or an `@layer` declared but never populated by
    /// a block. Cleanup candidates (an `@property` can be read from JS; a layer
    /// can be populated via `@import layer()`). Located by first definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_at_rules: Vec<UnusedAtRule>,
    /// Static `class` / `className` tokens in markup that match no CSS class
    /// defined anywhere in the project AND are one edit away from a class that
    /// IS defined (a likely typo or stale rename, with the suggested class). The
    /// CSS analogue of an unresolved import; the near-miss restriction keeps it
    /// near-zero false-positive (Tailwind utilities and third-party classes are
    /// not one edit from an authored class). Candidates, never gated: the token
    /// could be defined in CSS-in-JS or an external stylesheet the parser never
    /// sees. Sorted by `(path, line, class)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_class_references: Vec<UnresolvedClassReference>,
    /// Global CSS classes (defined in a plain `.css`/`.scss` rule) whose literal
    /// name is referenced by NO in-project markup, static or dynamic (the CSS
    /// analogue of an unused export). Heavily gated to stay near-zero-false-
    /// positive: emitted only when the project is plain-CSS-dominant, the
    /// stylesheet is locally consumed (not a published design-system surface),
    /// and the whole project is in scope. Candidates, never gated findings: the
    /// class may be used by an HTML email, server template, CMS, or Markdown the
    /// parser never scans. Sorted by `(path, line, class)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreferenced_css_classes: Vec<UnreferencedCssClass>,
    /// `@font-face` families declared in a stylesheet but referenced by no
    /// `font-family` anywhere in the project: a dead web-font payload (the font
    /// file is downloaded but never applied). Located at the declaring
    /// stylesheet. Cleanup candidates: the family could be applied from inline
    /// styles or set via JavaScript. Sorted by `(path, family)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_font_faces: Vec<UnusedFontFace>,
    /// Tailwind v4 `@theme` design tokens (`--color-brand`, `--radius-card`)
    /// defined in a stylesheet but used by no generated utility, `var()` read,
    /// `@apply`, or arbitrary value anywhere in the project: dead design tokens
    /// (the `unused-export` of the token era). Present only when the project is
    /// Tailwind v4 (a `tailwindcss` dependency plus at least one `@theme` block)
    /// and not a plugin / published-library / partial-scope run. Candidates,
    /// never gated findings: the token may be consumed by a Tailwind plugin or a
    /// downstream repo. Sorted by `(path, line, token)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_theme_tokens: Vec<UnusedThemeToken>,
    /// Tailwind v4 theme tokens whose comparable values are close to another
    /// token in the same theme dictionary. These are opt-in `--css-deep`
    /// candidates because they need whole-project token context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub near_duplicate_theme_tokens: Vec<NearDuplicateThemeToken>,
    /// A location-aware reverse index of Tailwind v4 `@theme` token consumers:
    /// per token, where it is consumed (`var()` reads, `@apply` bodies, generated
    /// utility classes) and through which surface, plus the full `consumer_count`
    /// (a static lower bound) and the defining site. Built from the same gated
    /// candidate set as `unused_theme_tokens` (v4 + non-plugin + non-published +
    /// whole-scope), so a token with `consumer_count: 0` is the same "nothing
    /// consumes this" signal. Sorted by token; empty when the project is not
    /// Tailwind v4 or a plugin / published-library / partial-scope run gated the
    /// scan out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_consumers: Vec<TokenConsumers>,
    /// The project authors `font-size` values in several units (`px`, `rem`,
    /// `em`, `%`), with a per-unit distinct-value count: a type-scale
    /// inconsistency smell (mixing `px` and `rem` for type works against
    /// user-zoom accessibility). Present only above a conservative floor.
    /// Advisory candidate, never gated: the spread can be intentional (fixed
    /// chrome in `px`, body type in `rem`).
    ///
    /// Color-notation mixing (hex vs rgb vs hsl) is deliberately NOT surfaced:
    /// the CSS parser canonicalizes every legacy sRGB notation to hex before
    /// fallow sees the value, so the authored distinction is already gone and
    /// cannot be recovered without a separate raw-token pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_unit_mix: Option<CssNotationConsistency>,
}

/// A design-token notation-consistency candidate: the distinct notations used
/// across the codebase for one value axis (today, length units on `font-size`),
/// with a per-notation distinct-value count. Emitted only above a floor, since
/// mixing notations for one axis is a "no single source of truth" smell.
/// Advisory: the action is "standardize on one notation", not a single search.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssNotationConsistency {
    /// The value axis these notations describe, e.g. `"Colors"` or
    /// `"Font sizes"`.
    pub axis: String,
    /// Per-notation distinct-value counts, sorted by count descending then
    /// notation name (so the dominant notation is first and ties are stable).
    pub notations: Vec<CssNotationCount>,
    /// Read-only guidance step(s), so consumers can iterate `actions` uniformly
    /// across every candidate type. Always at least one entry.
    pub actions: Vec<CssCandidateAction>,
}

/// One notation bucket and the count of distinct values authored in it.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssNotationCount {
    /// The notation family, e.g. `"hex"`, `"rgb"`, `"hsl"`, `"modern"`, `"px"`,
    /// `"rem"`, `"em"`, `"%"`.
    pub notation: String,
    /// Distinct values authored in this notation across the codebase.
    pub count: u32,
}

/// An unused CSS at-rule entity (an `@property` registration with no `var()`
/// reference, or an `@layer` declaration never populated), located by its first
/// definition. A cleanup candidate, never a gated finding.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedAtRule {
    /// Which kind of at-rule entity is unused.
    #[serde(rename = "type")]
    pub kind: UnusedAtRuleKind,
    /// The entity name (`--x` for `@property`, the layer name for `@layer`).
    pub name: String,
    /// Project-root-relative, forward-slash path to the first defining stylesheet.
    pub path: String,
    /// Read-only verification step(s) before removal (parity with other findings).
    pub actions: Vec<CssCandidateAction>,
}

/// Discriminant for [`UnusedAtRule::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum UnusedAtRuleKind {
    /// An `@property --x { }` registered but never referenced via `var()`.
    PropertyRegistration,
    /// An `@layer a` declared (in a statement or named block) but never
    /// populated by a `@layer a { }` block.
    Layer,
}

/// A distinct Tailwind arbitrary-value utility token used in markup, with its
/// total use count and first location (a design-token-bypass candidate).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TailwindArbitraryValue {
    /// The `prefix-[value]` token (e.g. `w-[13px]`). Variant prefixes are
    /// stripped, so `hover:w-[13px]` and `w-[13px]` aggregate under `w-[13px]`.
    pub value: String,
    /// Total occurrences across all scanned markup files.
    pub count: u32,
    /// Project-root-relative, forward-slash path to the first file using it.
    pub path: String,
    /// 1-based line of the first occurrence.
    pub line: u32,
    /// Read-only action(s): a find-all-occurrences search so the token can be
    /// replaced with a scale token. Always at least one entry, so consumers can
    /// iterate `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// A located raw CSS declaration value on a scale-sensitive styling axis.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RawStyleValue {
    /// Value axis, e.g. `color`, `font-size`, `line-height`, `radius`, or `shadow`.
    pub axis: String,
    /// CSS property where the raw value appears.
    pub property: String,
    /// Rendered declaration value.
    pub value: String,
    /// Project-root-relative, forward-slash path to the stylesheet.
    pub path: String,
    /// 1-based line of the containing style rule.
    pub line: u32,
    /// Concrete token with the same or nearest comparable value, when resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest_token: Option<NearestStylingToken>,
    /// Read-only guidance step(s). Never auto-fixable.
    pub actions: Vec<CssCandidateAction>,
}

/// A group of style rules across the project that share an identical declaration
/// block: a copy-paste consolidation candidate (fallow's duplication signal
/// applied to CSS). Only blocks of 4+ declarations appearing in 2+ rules are
/// reported, so the signal stays a strong copy-paste indicator rather than
/// flagging legitimately-repeated small blocks.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssDuplicateBlock {
    /// Declarations in the shared block.
    pub declaration_count: u16,
    /// Number of rules that share the block (always >= 2).
    pub occurrence_count: u32,
    /// Declarations removable by extracting the block into one shared rule:
    /// `(occurrence_count - 1) * declaration_count`.
    pub estimated_savings: u32,
    /// The rules sharing the block, sorted by `(path, line)`.
    pub occurrences: Vec<CssBlockOccurrence>,
    /// Read-only guidance step(s), so consumers can iterate `actions`
    /// uniformly across every finding type. Always at least one entry.
    pub actions: Vec<CssCandidateAction>,
}

/// One occurrence of a duplicate declaration block.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssBlockOccurrence {
    /// Project-root-relative, forward-slash path to the stylesheet.
    pub path: String,
    /// 1-based line of the rule's first selector.
    pub line: u32,
}

/// A duplicated CVA / shadcn variant class block.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CvaDuplicateVariantBlock {
    /// Normalized class block shared by several variant values.
    pub value: String,
    /// Number of variant values with this class block.
    pub occurrence_count: u32,
    /// First locations of the duplicate values, sorted by path and line.
    pub occurrences: Vec<CssBlockOccurrence>,
    /// Read-only guidance step(s), so consumers can iterate `actions`
    /// uniformly across every candidate type.
    pub actions: Vec<CssCandidateAction>,
}

/// A CVA / shadcn variant class value that can reuse an existing styling token.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CvaVariantTokenDrift {
    /// Tailwind arbitrary-value utility inside the variant class string.
    pub class_token: String,
    /// Normalized value inside the arbitrary utility.
    pub value: String,
    /// Full normalized variant class block containing the token.
    pub variant_classes: String,
    /// Project-root-relative, forward-slash path to the variant definition.
    pub path: String,
    /// 1-based line of the variant class string.
    pub line: u32,
    /// Existing token candidate to reuse.
    pub nearest_token: NearestStylingToken,
    /// Read-only guidance step(s), so consumers can iterate `actions`
    /// uniformly across every candidate type.
    pub actions: Vec<CssCandidateAction>,
}

/// A `@keyframes` defined in a stylesheet but referenced by no animation in any
/// stylesheet (cleanup candidate).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnreferencedKeyframes {
    /// The `@keyframes` name.
    pub name: String,
    /// Project-root-relative, forward-slash path to the stylesheet that defines it.
    pub path: String,
    /// Read-only verification step(s) an agent can run before removing the
    /// candidate. Always at least one entry, so consumers can iterate
    /// `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// An `@font-face` family declared in a stylesheet but referenced by no
/// `font-family` anywhere in the project: a dead web-font payload. A cleanup
/// candidate (the family could be applied from inline styles or JavaScript).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedFontFace {
    /// The declared font family name (quotes stripped).
    pub family: String,
    /// Project-root-relative, forward-slash path to the declaring stylesheet.
    pub path: String,
    /// Read-only verification step(s) before removing. Always at least one entry,
    /// so consumers can iterate `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// A Tailwind v4 `@theme` design token defined in a stylesheet whose generated
/// utility, `var()` reads, and arbitrary-value references appear nowhere in the
/// project: a dead design token (the `unused-export` of the token era). A
/// candidate, never a gated finding: the token could be consumed by a Tailwind
/// plugin, a published design-system surface, or a non-CSS-aware build step the
/// scan cannot see (those cases are gated out before this is emitted).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedThemeToken {
    /// The full custom property as authored, including the `--` prefix
    /// (`--color-brand`).
    pub token: String,
    /// The Tailwind v4 theme namespace the token belongs to (`color`, `radius`,
    /// `font-weight`, `breakpoint`, ...).
    pub namespace: String,
    /// Project-root-relative, forward-slash path to the declaring stylesheet.
    pub path: String,
    /// 1-based line of the token's definition inside the `@theme` block.
    pub line: u32,
    /// Read-only verification step(s) before removing. Always at least one entry,
    /// so consumers can iterate `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// A Tailwind v4 `@theme` token that appears to duplicate an existing token by
/// value. Emitted conservatively for comparable token namespaces, with the
/// nearest existing token named so an agent has a concrete reuse target.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NearDuplicateThemeToken {
    /// The full custom property as authored, including the `--` prefix.
    pub token: String,
    /// The normalized authored token value.
    pub value: String,
    /// Project-root-relative, forward-slash path to the token definition.
    pub path: String,
    /// 1-based line of the token definition inside the `@theme` block.
    pub line: u32,
    /// The nearest existing token candidate to reuse instead.
    pub nearest_token: NearestStylingToken,
    /// Read-only guidance step(s) before replacing the token reference.
    pub actions: Vec<CssCandidateAction>,
}

/// A styling token candidate that can replace or explain a finding.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NearestStylingToken {
    /// Token name, e.g. `--color-brand`.
    pub name: String,
    /// Normalized token value.
    pub value: String,
    /// Project-root-relative, forward-slash definition path.
    pub path: String,
    /// 1-based definition line.
    pub line: u32,
    /// Distance from the finding value. Lower is closer; units depend on the
    /// comparable token namespace.
    pub distance: f64,
}

/// Where one Tailwind v4 `@theme` token is consumed, and through which surface.
/// One entry in a [`TokenConsumers::consumers`] sample.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TokenConsumerLocation {
    /// Project-root-relative, forward-slash path to the consuming file.
    pub path: String,
    /// 1-based line of the consuming reference in that file.
    pub line: u32,
    /// Which surface consumes the token at this location.
    pub kind: ConsumerKind,
}

/// The surface through which a design token is consumed. The `theme-var` /
/// `css-var` / `utility` / `apply` kinds are Tailwind v4 `@theme` consumption; the
/// `js-member` / `js-call` kinds are CSS-in-JS consumption (member access on an
/// imported StyleX/vanilla-extract token binding, or a PandaCSS `token('...')`
/// call). The kind is the disjoint origin signal that distinguishes a Tailwind
/// token entry from a CSS-in-JS token entry in the shared `token_consumers` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ConsumerKind {
    /// A `var(--token)` read inside a `@theme` block interior (a token backing
    /// another token).
    ThemeVar,
    /// A `var(--token)` read in regular CSS, outside any `@theme` block.
    CssVar,
    /// A generated utility class ending in `-<name>` (`bg-brand` consuming
    /// `--color-brand`) found in markup / className strings / CSS-in-JS.
    Utility,
    /// A class-shaped token inside an `@apply` body in a stylesheet.
    Apply,
    /// A cross-module JS member access on an imported CSS-in-JS token binding
    /// (`import { vars } from './tokens'; vars.color.primary`), for StyleX
    /// `defineVars` / vanilla-extract `createTheme` family tokens.
    JsMember,
    /// A CSS-in-JS function call that consumes a token by path, such as
    /// PandaCSS `token('colors.brand')` or `css({ color: 'colors.brand' })`.
    JsCall,
}

/// A location-aware reverse index of where one design token is consumed, so an
/// agent editing the token can see its blast radius before changing or removing
/// it. Covers TWO token origins. The always-available discriminator is the `token`
/// SHAPE: a Tailwind token is the `--`-prefixed custom property (`--color-brand`),
/// a CSS-in-JS token is a dotted access path with no `--` prefix
/// (`vars.color.primary`). The per-consumer `kind` also discriminates origin, but
/// only when `consumer_count > 0` (a `consumer_count: 0` entry has an empty
/// `consumers` array and thus no `kind`), so branch on the `token` prefix for the
/// zero-consumer case. The two origins:
///
/// - Tailwind v4 `@theme` tokens (kinds `theme-var` / `css-var` / `utility` /
///   `apply`), built from the same gated candidate set as `unused_theme_tokens`
///   (v4 + non-plugin + non-published + whole-scope), so a `consumer_count: 0`
///   corroborates the `unused_theme_tokens` "nothing consumes this" finding.
/// - CSS-in-JS tokens (kind `js-member` / `js-call`) from StyleX `defineVars`,
///   vanilla-extract `createTheme` family definitions, and PandaCSS `defineTokens`,
///   consumed via cross-module member access or PandaCSS `token('...')` calls. NOTE:
///   CSS-in-JS has NO corroborating dead-token finding (there is no
///   `unused_theme_tokens` analogue), so a CSS-in-JS `consumer_count: 0` is a weaker
///   signal than the Tailwind case (and the cross-file scan is relative-import or
///   generated-token-helper only, so alias / bare-package imports are not counted).
///
/// This is DESCRIPTIVE context (a blast-radius lookup), not a finding, so it
/// deliberately carries no `actions` array (unlike the cleanup-candidate types in
/// this module). `consumer_count` is always a STATIC lower bound (a computed class
/// name like `bg-${color}`, or a CSS-in-JS access through an unresolved alias
/// import, is not counted).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TokenConsumers {
    /// The token identity. For a Tailwind `@theme` token this is the full custom
    /// property as authored, INCLUDING the `--` prefix (`--color-brand`). For a
    /// CSS-in-JS token (kind `js-member`) this is the binding-qualified dotted
    /// access path, NO `--` prefix (`vars.color.primary`), matching how consumers
    /// read it. The presence of the `--` prefix distinguishes the two origins.
    pub token: String,
    /// For a Tailwind token, the v4 theme namespace (`color`, `radius`,
    /// `font-weight`, ...). For a CSS-in-JS token (kind `js-member`), the defining
    /// export BINDING the token set is accessed through (`vars`), which identifies
    /// the token set, NOT a semantic group. (The field is thus overloaded by
    /// origin; branch on `consumers[].kind` or the `token` shape.)
    pub namespace: String,
    /// Project-root-relative, forward-slash path to the declaring stylesheet
    /// (Tailwind) or the JS/TS token-definition file (CSS-in-JS).
    pub definition_path: String,
    /// 1-based line of the token's definition (inside the `@theme` block for
    /// Tailwind; the token key inside the `defineVars`/`createTheme` object for
    /// CSS-in-JS).
    pub definition_line: u32,
    /// The FULL number of consumer locations found, a STATIC LOWER BOUND: a
    /// computed class name (`bg-${color}`) or a value read outside CSS/markup the
    /// scan never sees is not counted. This is the aggregate over every consumer,
    /// computed BEFORE [`consumers`](Self::consumers) is capped to a sample.
    pub consumer_count: u32,
    /// A capped, deterministically-sorted sample of consumer locations (at most
    /// [`TOKEN_CONSUMER_SAMPLE_CAP`]). The full count lives in
    /// [`consumer_count`](Self::consumer_count); use this list to jump to
    /// representative consumers, not to enumerate every one.
    pub consumers: Vec<TokenConsumerLocation>,
}

/// Maximum number of consumer locations sampled into [`TokenConsumers::consumers`].
/// The full count is preserved in [`TokenConsumers::consumer_count`]
/// (aggregate-before-truncate), so capping the sample never distorts the count.
pub const TOKEN_CONSUMER_SAMPLE_CAP: usize = 20;

/// A global CSS class defined in a plain `.css`/`.scss` rule whose literal name
/// is referenced by no in-project markup (the CSS analogue of an unused export).
/// A heavily-gated candidate, never a gated finding: the class may be applied
/// from an HTML email, server template, CMS, or Markdown the parser never sees.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnreferencedCssClass {
    /// The class name (no dot).
    pub class: String,
    /// Project-root-relative, forward-slash path to the defining stylesheet.
    pub path: String,
    /// 1-based line of the class's first definition.
    pub line: u32,
    /// Read-only verification step(s) before removing. Always at least one entry,
    /// so consumers can iterate `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// An animation reference (`animation` / `animation-name`) to a `@keyframes`
/// name that is defined in no stylesheet anywhere in the project (the
/// "used-but-undefined" direction). Usually a typo or a removed animation;
/// occasionally a `@keyframes` defined in CSS-in-JS the CSS parser never sees.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UndefinedKeyframes {
    /// The referenced `@keyframes` name that resolves to no definition.
    pub name: String,
    /// Project-root-relative, forward-slash path to the first stylesheet that
    /// references it.
    pub path: String,
    /// Read-only verification step(s) an agent can run before fixing the
    /// reference. Always at least one entry, so consumers can iterate `actions`
    /// uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// A static `class` / `className` token in markup that matches no CSS class
/// defined anywhere in the project but is one edit away from a class that IS
/// defined (a likely typo or stale rename). The CSS analogue of an unresolved
/// import. A candidate, never a gated finding: the token could be defined in
/// CSS-in-JS or an external stylesheet the parser never sees.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnresolvedClassReference {
    /// The static class token referenced in markup (no dot).
    pub class: String,
    /// The defined CSS class one edit away: the likely intended class.
    pub suggestion: String,
    /// Project-root-relative, forward-slash path to the markup file.
    pub path: String,
    /// 1-based line of the `class` / `className` attribute.
    pub line: u32,
    /// Read-only verification step(s) before fixing the reference. Always at
    /// least one entry, so consumers can iterate `actions` uniformly across
    /// every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// A Vue SFC's `<style scoped>` classes that appear nowhere else in the
/// component (cleanup candidates).
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ScopedUnusedClasses {
    /// Project-root-relative, forward-slash path to the SFC.
    pub path: String,
    /// The scoped class names with no use elsewhere in the component, sorted.
    pub classes: Vec<String>,
    /// Read-only verification step(s) an agent can run before removing the
    /// candidate. Always at least one entry, so consumers can iterate
    /// `actions` uniformly across every finding type.
    pub actions: Vec<CssCandidateAction>,
}

/// One advisory STYLING FINDING: the graduation of a descriptive css candidate
/// into a first-class, severity-aware, suppressible finding surfaced in
/// `fallow audit`. The styling domain's OWN finding type (not borrowed into the
/// dead-code `AnalysisResults`, and not glued in the CLI). `code` is the kebab
/// IssueKind code (e.g. `css-token-drift`), so severity / inline suppression /
/// SARIF / MCP all resolve via the shared `issue_meta` contract through
/// `IssueKind::parse(code)`. One `Vec<StylingFinding>` carries every styling
/// family; the `code` discriminates.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StylingFinding {
    /// The kebab IssueKind code, e.g. `css-token-drift`.
    pub code: String,
    /// The specific sub-kind within the family, e.g. `tailwind-arbitrary-value`.
    pub sub_kind: String,
    /// Workspace-relative path of the finding.
    pub path: String,
    /// 1-based line of the finding.
    pub line: u32,
    /// The offending literal value, e.g. `w-[13px]`.
    pub value: String,
    /// Effective severity after applying `rules.css-*` config. Styling defaults
    /// to `warn`, but projects can escalate a family to `error` for audit gates
    /// and CI formats.
    pub effective_severity: StylingFindingSeverity,
    /// Optional static lower-bound blast radius. For a dead design token this is
    /// `0`; for other styling findings it is omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<u32>,
    /// Confidence hint for agents and review UIs. Structural findings are high,
    /// reachability findings are low because dynamic consumers may exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<StylingFindingConfidence>,
    /// Suggested handling posture for agents. This is advisory data, fallow
    /// still never applies styling changes automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_disposition: Option<StylingAgentDisposition>,
    /// Concrete reuse target for token-drift findings, when one can be resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest_token: Option<NearestStylingToken>,
    /// One concise machine-readable edit hint for agent consumers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_hint: Option<String>,
    /// Suggested next steps (verify / suppress; never an auto-fix).
    pub actions: Vec<CssCandidateAction>,
}

/// Effective configured severity for a styling finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum StylingFindingSeverity {
    Warn,
    Error,
}

/// Confidence hint for a [`StylingFinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum StylingFindingConfidence {
    /// The finding is local and structural.
    High,
    /// The finding depends on reachability and should be verified.
    Low,
}

/// Agent handling hint for a [`StylingFinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum StylingAgentDisposition {
    /// The finding names a concrete structural edit target.
    FixConfidently,
    /// Verify dynamic or external consumers before changing code.
    VerifyFirst,
}

/// A read-only verification step attached to a CSS cleanup candidate.
///
/// CSS candidates (unreferenced `@keyframes`, unused scoped classes) are never
/// auto-removed: an animation name can still be applied from JavaScript, and a
/// class can be assembled from a dynamic string binding. The action gives an
/// agent a machine-readable next step, mirroring the `actions` array carried by
/// every other health finding, plus an optional runnable probe to confirm the
/// candidate is genuinely unused before deleting it.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssCandidateAction {
    /// Action type identifier (`verify-unused`).
    #[serde(rename = "type")]
    pub kind: CssCandidateActionType,
    /// Always `false`: CSS candidates are never auto-fixed (`fallow fix` does
    /// not touch them) because the residual consumer may live outside CSS.
    pub auto_fixable: bool,
    /// Human-readable description of what to confirm before removing.
    pub description: String,
    /// A runnable, read-only, placeholder-free token search that surfaces any
    /// out-of-CSS use of the candidate. Absent when no shell-safe command can
    /// be built (e.g. the residual risk is a dynamic string binding that a
    /// single search cannot probe), in which case `description` is the guide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Discriminant for [`CssCandidateAction::kind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CssCandidateActionType {
    /// Confirm the candidate has no JavaScript / HTML / dynamic consumer
    /// before removing it (the defined-but-unused candidates).
    VerifyUnused,
    /// Confirm the referenced name is genuinely undefined (not defined in
    /// CSS-in-JS the parser cannot see) before treating it as a typo (the
    /// used-but-undefined candidates).
    VerifyUndefined,
    /// Extract the shared declaration block into one rule and reference it from
    /// each occurrence (the duplicate-declaration-block candidates).
    Consolidate,
    /// Replace a Tailwind arbitrary value with a configured scale token, or
    /// confirm the one-off is intentional (the arbitrary-value candidates).
    ReplaceWithToken,
    /// Standardize an inconsistent value axis on a single notation (the
    /// color-format / length-unit mixing candidates).
    Standardize,
    /// Simplify a selector, reduce nesting, or remove unnecessary `!important`
    /// usage after verifying the cascade.
    SimplifySelector,
}

impl CssCandidateAction {
    /// Read-only guidance for a selector / nesting / important-density finding.
    #[must_use]
    pub fn simplify_selector(reason: &str) -> Self {
        Self {
            kind: CssCandidateActionType::SimplifySelector,
            auto_fixable: false,
            description: format!(
                "Review cascade impact, then simplify this selector or rule because {reason}."
            ),
            command: None,
        }
    }

    /// Verify action for an unused `@font-face` family: a read-only token search
    /// for any inline-style or JavaScript application of the family before
    /// removing the dead web-font.
    #[must_use]
    pub fn verify_unused_font_face(family: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description: format!(
                "Confirm the \"{family}\" font family is not applied from an inline style or JavaScript before removing the @font-face and its font files."
            ),
            command: safe_token_search(family),
        }
    }

    /// Verify action for an unused Tailwind v4 `@theme` token: a read-only search
    /// that embeds the LITERAL terms an agent should grep for, the generated
    /// utility suffix (`bg-<name>` / `text-<name>` / `<namespace>-<name>`), the
    /// `var(--<ns>-<name>)` read, and the arbitrary `[--<ns>-<name>]` value,
    /// before removing the token. Verify-then-remove; never auto-fixable.
    #[must_use]
    pub fn verify_unused_theme_token(token: &str, namespace: &str, name: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description: format!(
                "Confirm the {token} @theme token is used by nothing, no `*-{name}` utility (e.g. `bg-{name}` / `text-{name}` / `{namespace}-{name}`) in markup or @apply, no `var({token})` read in any stylesheet or JS, and no arbitrary `[{token}]` value, before removing it from the @theme block."
            ),
            command: theme_token_search(namespace, name),
        }
    }

    /// Guidance for a near-duplicate theme token: reuse the named existing
    /// token after checking semantic intent.
    #[must_use]
    pub fn replace_near_duplicate_token(token: &str, nearest: &str) -> Self {
        Self {
            kind: CssCandidateActionType::ReplaceWithToken,
            auto_fixable: false,
            description: format!(
                "Verify {token} is not an intentional semantic alias, then reuse {nearest} instead."
            ),
            command: safe_token_search(token),
        }
    }

    /// Verify action for an unreferenced global CSS class: name the surfaces the
    /// in-project scan does NOT cover (the class could be applied from there) and
    /// ship a read-only token search to double-check before removing.
    #[must_use]
    pub fn verify_unreferenced_class(name: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description: format!(
                "Confirm no HTML email, server-rendered template, or CMS content applies the \"{name}\" class before removing it (fallow scanned in-project JS/TS/HTML/Vue/Svelte/Astro/Markdown markup)."
            ),
            command: safe_token_search(name),
        }
    }

    /// Verify action for an unreferenced `@keyframes`: a read-only token search
    /// for any JavaScript or template reference that applies the animation
    /// (which the CSS-only scan cannot see).
    #[must_use]
    pub fn verify_keyframe(name: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description: format!(
                "Confirm no JavaScript or template applies the \"{name}\" animation before removing the @keyframes."
            ),
            command: safe_token_search(name),
        }
    }

    /// Verify action for an animation reference to a `@keyframes` that is
    /// defined in no stylesheet: a read-only token search for a CSS-in-JS
    /// `@keyframes`/animation definition of the name (styled-components,
    /// Emotion, vanilla-extract) before treating the reference as a typo.
    #[must_use]
    pub fn verify_undefined_keyframe(name: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUndefined,
            auto_fixable: false,
            description: format!(
                "Confirm \"{name}\" is not a @keyframes defined in CSS-in-JS (styled-components, Emotion, vanilla-extract) before treating the animation reference as a typo."
            ),
            command: safe_token_search(name),
        }
    }

    /// Guidance action for a mixed value axis (colors authored in several
    /// notations, or font sizes in several units): standardize on the single
    /// dominant notation. No command (this is a project-wide refactor, and the
    /// per-notation breakdown already quantifies the spread); the residual
    /// judgment is whether the spread is an intentional migration in progress.
    #[must_use]
    pub fn standardize_notation(axis: &str, dominant: &str) -> Self {
        Self {
            kind: CssCandidateActionType::Standardize,
            auto_fixable: false,
            description: format!(
                "{axis} are authored in several notations; standardize on one ({dominant} is the most common) so the scale is a single source of truth, unless this is an intentional migration in progress."
            ),
            command: None,
        }
    }

    /// Guidance action for a duplicate declaration block: consolidate the shared
    /// declarations into one rule. No command (consolidation is a refactor, and
    /// the occurrences list already names every site); the residual judgment is
    /// whether the rules are intentionally separate overrides.
    #[must_use]
    pub fn consolidate_block(occurrence_count: u32) -> Self {
        Self {
            kind: CssCandidateActionType::Consolidate,
            auto_fixable: false,
            description: format!(
                "Extract this declaration block into one rule and reference it from all {occurrence_count} occurrences, unless they are intentionally separate overrides."
            ),
            command: None,
        }
    }

    /// Action for a Tailwind arbitrary-value bypass: a read-only fixed-string
    /// search for every occurrence of the token so it can be replaced with a
    /// scale token (or confirmed an intentional one-off). The value is a Tailwind
    /// utility token (no quotes / whitespace by construction), so it is safe to
    /// single-quote; the `-F` keeps the `[` / `]` literal rather than a glob.
    #[must_use]
    pub fn replace_arbitrary_value(value: &str) -> Self {
        let command = (!value.contains('\'')).then(|| {
            format!(
                "grep -rnF '{value}' --include='*.jsx' --include='*.tsx' --include='*.html' --include='*.vue' --include='*.svelte' --include='*.astro' ."
            )
        });
        Self {
            kind: CssCandidateActionType::ReplaceWithToken,
            auto_fixable: false,
            description:
                "Replace this one-off arbitrary value with a scale token from your Tailwind theme, or confirm it is intentional."
                    .to_string(),
            command,
        }
    }

    /// Guidance for a CVA / shadcn variant arbitrary value: replace the
    /// utility with a token-backed variant class after checking variant intent.
    #[must_use]
    pub fn replace_cva_variant_arbitrary_value(class_token: &str, nearest: &str) -> Self {
        Self {
            kind: CssCandidateActionType::ReplaceWithToken,
            auto_fixable: false,
            description: format!(
                "Verify this CVA variant value is not an intentional one-off, then replace {class_token} with a class backed by {nearest}."
            ),
            command: safe_token_search(class_token),
        }
    }

    /// Guidance for a raw CSS value on a scale-sensitive axis: replace with an
    /// existing token or confirm the one-off is intentional.
    #[must_use]
    pub fn replace_raw_style_value(axis: &str, value: &str) -> Self {
        Self {
            kind: CssCandidateActionType::ReplaceWithToken,
            auto_fixable: false,
            description: format!(
                "Replace this raw {axis} value with an existing design token or CSS custom property, or confirm this one-off is intentional."
            ),
            command: safe_token_search(value),
        }
    }

    /// Verify action for an unused CSS at-rule entity: a read-only search for
    /// any out-of-CSS consumer (JS reading an `@property`; an `@import layer()`
    /// populating a layer) before removing it.
    #[must_use]
    pub fn verify_unused_at_rule(kind: UnusedAtRuleKind, name: &str) -> Self {
        let description = match kind {
            UnusedAtRuleKind::PropertyRegistration => format!(
                "Confirm \"{name}\" is not read or set from JavaScript before removing the @property registration."
            ),
            UnusedAtRuleKind::Layer => format!(
                "Confirm the @layer \"{name}\" is not populated via @import layer() before removing the declaration."
            ),
        };
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description,
            command: safe_token_search(name),
        }
    }

    /// Verify action for a markup class token that matches no defined CSS class
    /// but is one edit from a class that is defined: surface the suggestion and a
    /// read-only token search so the residual risk (a class defined in CSS-in-JS
    /// or an external stylesheet) can be ruled out before fixing the typo.
    #[must_use]
    pub fn verify_unresolved_class(class: &str, suggestion: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUndefined,
            auto_fixable: false,
            description: format!(
                "\"{class}\" matches no CSS class; did you mean \"{suggestion}\"? Confirm \"{class}\" is not defined in CSS-in-JS or an external stylesheet before fixing the reference."
            ),
            command: safe_token_search(class),
        }
    }

    /// Verify action for a Vue SFC's unused scoped classes. The component-scoped
    /// scan already covers every static use, so the only residual risk is a
    /// class assembled from a dynamic string; that is a manual check, so the
    /// action carries guidance but no command.
    #[must_use]
    pub fn verify_scoped_classes() -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description:
                "Confirm none of these scoped classes is assembled from a dynamic string (e.g. `:class=\"prefix + name\"`) before removing them."
                    .to_string(),
            command: None,
        }
    }
}

/// Build a read-only, placeholder-free, namespace-QUALIFIED search for a Tailwind
/// v4 `@theme` token, or `None` when the namespace / name is not a plain CSS
/// identifier (so the emitted command is always shell-safe). The pattern matches
/// any `*-<name>` utility (`bg-<name>`, `rounded-<name>`, `font-<name>`, ...) AND
/// the `--<ns>-<name>` custom property (covering `var()` reads and `[--ns-name]`
/// arbitrary values), deliberately NOT a bare `<name>` (which would substring-hit
/// every file for a dictionary-word token like `brand` / `card`).
fn theme_token_search(namespace: &str, name: &str) -> Option<String> {
    let is_plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };
    (is_plain(namespace) && is_plain(name)).then(|| {
        format!(
            "grep -rnE -- '-{name}\\b|--{namespace}-{name}' --include='*.css' --include='*.html' --include='*.js' --include='*.jsx' --include='*.ts' --include='*.tsx' --include='*.vue' --include='*.svelte' --include='*.astro' ."
        )
    })
}

/// Build a read-only, placeholder-free token search for `name`, or `None` when
/// the name is not a plain CSS identifier, so the emitted command is always
/// shell-safe without quoting tricks.
fn safe_token_search(name: &str) -> Option<String> {
    let is_plain = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    is_plain.then(|| {
        format!(
            "grep -rnw '{name}' --include='*.js' --include='*.jsx' --include='*.ts' --include='*.tsx' --include='*.vue' --include='*.svelte' --include='*.astro' --include='*.html' --include='*.md' --include='*.mdx' ."
        )
    })
}

/// Per-stylesheet CSS analytics.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssFileAnalytics {
    /// Project-root-relative, forward-slash path.
    pub path: String,
    /// The stylesheet's structural metrics.
    pub analytics: fallow_types::extract::CssAnalytics,
}

/// Project-wide CSS analytics aggregates across every analyzed stylesheet
/// (including stylesheets with no notable rule, which are not listed
/// individually in `files`).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssAnalyticsSummary {
    /// Stylesheets analyzed: standard `.css` files, Vue/Svelte SFC `<style>`
    /// blocks, and (dep-gated) CSS-in-JS, both the tagged-template form and the
    /// object form (`style({...})` / `stylex.create({...})` / `css({...})`). SCSS
    /// is skipped. Note: flat atomic object CSS-in-JS (StyleX/Panda) is counted
    /// here and contributes to these aggregates, but has no notable rules, so its
    /// files never appear in the per-file `files` list.
    pub files_analyzed: u32,
    /// Total style rules across analyzed stylesheets.
    pub total_rules: u32,
    /// Total declarations across analyzed stylesheets.
    pub total_declarations: u32,
    /// Total `!important` declarations across analyzed stylesheets.
    pub important_declarations: u32,
    /// Total empty style rules across analyzed stylesheets.
    pub empty_rules: u32,
    /// Deepest style-rule nesting depth observed across analyzed stylesheets.
    pub max_nesting_depth: u8,
    /// Distinct color values (authored form) across the whole codebase. A high
    /// count signals an uncontrolled palette (design-token sprawl).
    pub unique_colors: u32,
    /// Distinct `font-size` values across the whole codebase.
    pub unique_font_sizes: u32,
    /// Distinct `z-index` values across the whole codebase.
    pub unique_z_indexes: u32,
    /// Distinct `box-shadow` values across the whole codebase (shadow-scale sprawl).
    pub unique_box_shadows: u32,
    /// Distinct `border-radius` values across the whole codebase (radius-scale sprawl).
    pub unique_border_radii: u32,
    /// Distinct `line-height` values across the whole codebase (type-scale sprawl).
    pub unique_line_heights: u32,
    /// Distinct custom properties (`--x`) defined anywhere in the codebase.
    pub custom_properties_defined: u32,
    /// Custom properties defined but never referenced via `var()` in any
    /// stylesheet (the defined-but-unused direction). These are cleanup
    /// CANDIDATES, not confirmed dead: a property may still be read or set from
    /// JavaScript or inline HTML styles.
    pub custom_properties_unreferenced: u32,
    /// Distinct custom properties referenced via `var()` that are defined in no
    /// stylesheet anywhere (the used-but-undefined direction). A COUNT only, not
    /// a located list: a `var(--x)` with no CSS definition is extremely common
    /// in JavaScript-driven theming and design-token libraries, so locating
    /// these would be net-noise. The count is an architecture signal (how much
    /// of the `var()` surface is resolved outside CSS), not a finding.
    pub custom_properties_undefined: u32,
    /// Distinct `@keyframes` defined anywhere in the codebase.
    pub keyframes_defined: u32,
    /// `@keyframes` defined but never referenced via `animation` /
    /// `animation-name` in any stylesheet (the defined-but-unused direction;
    /// cleanup CANDIDATES; an animation name can still be applied from
    /// JavaScript).
    pub keyframes_unreferenced: u32,
    /// Distinct animation names referenced via `animation` / `animation-name`
    /// that resolve to no `@keyframes` definition anywhere (the used-but-
    /// undefined direction). Located in `undefined_keyframes`; usually a typo or
    /// a removed animation.
    pub keyframes_undefined: u32,
    /// Total Vue `<style scoped>` classes used nowhere else in their component
    /// (cleanup candidates), across all SFCs.
    pub scoped_unused_classes: u32,
    /// Number of distinct declaration blocks (4+ declarations) that appear in
    /// two or more rules across the project (copy-paste consolidation
    /// candidates). Located in `duplicate_declaration_blocks`.
    pub duplicate_declaration_blocks: u32,
    /// Total declarations removable by consolidating every duplicate block:
    /// the sum of `(occurrence_count - 1) * declaration_count` across groups.
    pub duplicate_declarations_total: u32,
    /// Distinct Tailwind arbitrary-value tokens used in markup (design-token
    /// bypass). Zero when the project does not use Tailwind. Located in
    /// `tailwind_arbitrary_values`.
    pub tailwind_arbitrary_values: u32,
    /// Total Tailwind arbitrary-value occurrences across markup.
    pub tailwind_arbitrary_value_uses: u32,
    /// Preprocessor stylesheets (`.scss`, `.sass`, `.less`) seen by the styling
    /// scan. These are parsed textually for local candidates, not compiled.
    pub preprocessor_stylesheets: u32,
    /// True when project-wide class reachability was skipped because
    /// preprocessor stylesheets outnumber plain CSS, making generated classes
    /// invisible without a Sass/Less compiler.
    pub preprocessor_reachability_abstained: bool,
    /// Located raw CSS declaration values that bypass token surfaces on
    /// scale-sensitive axes. Located in `raw_style_values`.
    pub raw_style_values: u32,
    /// `@property` registrations never referenced via `var()` in any stylesheet
    /// (located in `unused_at_rules`). Cleanup candidates.
    pub unused_property_registrations: u32,
    /// Cascade layers declared but never populated by a block (located in
    /// `unused_at_rules`). Cleanup candidates.
    pub unused_layers: u32,
    /// Static markup class tokens that match no defined CSS class but are one
    /// edit from a defined class (likely typos / stale renames). Located in
    /// `unresolved_class_references`. Candidates, never gated.
    pub unresolved_class_references: u32,
    /// Global CSS classes defined in a stylesheet but referenced by no in-project
    /// markup (located in `unreferenced_css_classes`). Heavily gated cleanup
    /// candidates; zero on preprocessor-dominant or partial-scope runs.
    pub unreferenced_css_classes: u32,
    /// `@font-face` families declared but referenced by no `font-family` anywhere
    /// (located in `unused_font_faces`). Dead web-font cleanup candidates.
    pub unused_font_faces: u32,
    /// Tailwind v4 `@theme` design tokens defined but used by no generated
    /// utility, `var()`, `@apply`, or arbitrary value anywhere (located in
    /// `unused_theme_tokens`). Dead-design-token cleanup candidates; zero when
    /// the project is not Tailwind v4 or a plugin / published-library /
    /// partial-scope run gated the scan out.
    pub unused_theme_tokens: u32,
    /// Tailwind v4 theme tokens whose comparable values are close to another
    /// token in the same theme dictionary. Located in
    /// `near_duplicate_theme_tokens`.
    pub near_duplicate_theme_tokens: u32,
    /// Number of distinct `font-size` units (`px` / `rem` / `em` / `%`) authored
    /// across the codebase. Mixing units is a type-scale consistency smell,
    /// broken out in `font_size_unit_mix`.
    pub font_size_units_used: u32,
    /// Number of analyzed stylesheets whose per-rule `notable_rules` list was
    /// truncated at the per-file cap, so a consumer knows the per-rule detail is
    /// incomplete without walking every file.
    pub notable_truncated_files: u32,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep serialization assertions concise"
)]
mod tests {
    use super::*;

    #[test]
    fn consumer_kind_serializes_kebab_case() {
        let kinds = [
            (ConsumerKind::ThemeVar, "\"theme-var\""),
            (ConsumerKind::CssVar, "\"css-var\""),
            (ConsumerKind::Utility, "\"utility\""),
            (ConsumerKind::Apply, "\"apply\""),
        ];
        for (kind, expected) in kinds {
            assert_eq!(serde_json::to_string(&kind).unwrap(), expected);
        }
    }

    #[test]
    fn token_consumers_serializes_full_shape() {
        let entry = TokenConsumers {
            token: "--color-brand".to_string(),
            namespace: "color".to_string(),
            definition_path: "src/theme.css".to_string(),
            definition_line: 4,
            consumer_count: 2,
            consumers: vec![
                TokenConsumerLocation {
                    path: "src/Button.tsx".to_string(),
                    line: 12,
                    kind: ConsumerKind::Utility,
                },
                TokenConsumerLocation {
                    path: "src/theme.css".to_string(),
                    line: 9,
                    kind: ConsumerKind::CssVar,
                },
            ],
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["consumer_count"], 2);
        assert_eq!(value["definition_line"], 4);
        assert_eq!(value["consumers"][0]["kind"], "utility");
        assert_eq!(value["consumers"][1]["kind"], "css-var");
    }

    #[test]
    fn token_consumers_omitted_when_empty() {
        let report = CssAnalyticsReport {
            files: Vec::new(),
            summary: CssAnalyticsSummary::default(),
            scoped_unused: Vec::new(),
            unreferenced_keyframes: Vec::new(),
            undefined_keyframes: Vec::new(),
            duplicate_declaration_blocks: Vec::new(),
            cva_duplicate_variant_blocks: Vec::new(),
            cva_variant_token_drifts: Vec::new(),
            tailwind_arbitrary_values: Vec::new(),
            raw_style_values: Vec::new(),
            unused_at_rules: Vec::new(),
            unresolved_class_references: Vec::new(),
            unreferenced_css_classes: Vec::new(),
            unused_font_faces: Vec::new(),
            unused_theme_tokens: Vec::new(),
            near_duplicate_theme_tokens: Vec::new(),
            token_consumers: Vec::new(),
            font_size_unit_mix: None,
        };
        let value = serde_json::to_value(&report).unwrap();
        assert!(
            value.get("token_consumers").is_none(),
            "empty token_consumers must be skipped"
        );
    }

    #[test]
    fn token_consumers_present_when_non_empty() {
        let report = CssAnalyticsReport {
            files: Vec::new(),
            summary: CssAnalyticsSummary::default(),
            scoped_unused: Vec::new(),
            unreferenced_keyframes: Vec::new(),
            undefined_keyframes: Vec::new(),
            duplicate_declaration_blocks: Vec::new(),
            cva_duplicate_variant_blocks: Vec::new(),
            cva_variant_token_drifts: Vec::new(),
            tailwind_arbitrary_values: Vec::new(),
            raw_style_values: Vec::new(),
            unused_at_rules: Vec::new(),
            unresolved_class_references: Vec::new(),
            unreferenced_css_classes: Vec::new(),
            unused_font_faces: Vec::new(),
            unused_theme_tokens: Vec::new(),
            near_duplicate_theme_tokens: Vec::new(),
            token_consumers: vec![TokenConsumers {
                token: "--color-brand".to_string(),
                namespace: "color".to_string(),
                definition_path: "src/theme.css".to_string(),
                definition_line: 4,
                consumer_count: 0,
                consumers: Vec::new(),
            }],
            font_size_unit_mix: None,
        };
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["token_consumers"][0]["consumer_count"], 0);
    }
}
