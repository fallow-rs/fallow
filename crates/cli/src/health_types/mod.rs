//! Health / complexity analysis report types.
//!
//! Separated from the `health` command module so that report formatters
//! (which are compiled as part of both the lib and bin targets) can
//! reference these types without pulling in binary-only dependencies.

mod coverage;
mod coverage_intelligence;
mod finding;
mod grouped;
mod runtime_coverage;
mod scores;
mod targets;
mod trends;
mod vital_signs;

pub use coverage::*;
pub use coverage_intelligence::*;
pub use finding::*;
pub use grouped::*;
pub use runtime_coverage::*;
pub use scores::*;
pub use targets::*;
pub use trends::*;
pub use vital_signs::*;

/// Detailed timing breakdown for the health pipeline.
///
/// Only populated when `--performance` is passed.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthTimings {
    pub config_ms: f64,
    pub discover_ms: f64,
    pub parse_ms: f64,
    /// Summed wall-clock time of the actual AST parses across all rayon
    /// workers (the parse stage's CPU cost). `parse_ms` is the stage's
    /// wall-clock time. Observational and non-deterministic; do not assert
    /// against it. `0.0` when `shared_parse` is true (parse was reused).
    pub parse_cpu_ms: f64,
    pub complexity_ms: f64,
    pub file_scores_ms: f64,
    pub git_churn_ms: f64,
    pub git_churn_cache_hit: bool,
    pub hotspots_ms: f64,
    pub duplication_ms: f64,
    pub targets_ms: f64,
    pub total_ms: f64,
    /// True when discover + parse were reused from the upstream dead-code
    /// (check) pass in combined mode, so their timings are `0.0` here and
    /// the cost is attributed to the `Pipeline Performance` table instead.
    /// The renderer shows those two stages as `(measured above)`.
    pub shared_parse: bool,
}

/// Auditable breadcrumb recording when health-finding `suppress-line`
/// action hints were omitted from the report.
///
/// Set at construction time on [`HealthReport::actions_meta`] (and on
/// each [`HealthGroup::actions_meta`](crate::health_types::HealthGroup)
/// when grouped) by the report builder, derived from the active
/// [`HealthActionContext`]. Lets consumers see "where did the
/// suppress-line hints go?" without having to grep the config or CLI
/// history.
///
/// Stable `reason` codes:
/// - `baseline-active`: a baseline is active and inline ignores would
///   become dead annotations once the baseline regenerates.
/// - `config-disabled`: `health.suggestInlineSuppression` is `false`.
/// - `unspecified`: the caller did not record a reason.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthActionsMeta {
    /// Always `true` when the breadcrumb is emitted. Absent from the wire
    /// when no suppression occurred.
    pub suppression_hints_omitted: bool,
    /// Stable code describing why the suppression occurred.
    pub reason: String,
    /// Scope of the omission. Always `"health-findings"` today.
    pub scope: String,
}

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
    /// Tailwind arbitrary-value utilities (`w-[13px]`, `bg-[#abc]`) found in
    /// markup, which hardcode a one-off value instead of a configured scale
    /// token (design-token bypass). Present only when the project uses Tailwind.
    /// Sorted by use count descending. Candidates, not findings: an arbitrary
    /// value is sometimes the right call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tailwind_arbitrary_values: Vec<TailwindArbitraryValue>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
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
}

impl CssCandidateAction {
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

    /// Verify action for an unreferenced global CSS class: name the surfaces the
    /// in-project scan does NOT cover (the class could be applied from there) and
    /// ship a read-only token search to double-check before removing.
    #[must_use]
    pub fn verify_unreferenced_class(name: &str) -> Self {
        Self {
            kind: CssCandidateActionType::VerifyUnused,
            auto_fixable: false,
            description: format!(
                "Confirm no HTML email, server-rendered template, CMS content, or Markdown applies the \"{name}\" class before removing it (fallow scanned only in-project JS/TS/HTML/Vue/Svelte/Astro markup)."
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
            "grep -rnw '{name}' --include='*.js' --include='*.jsx' --include='*.ts' --include='*.tsx' --include='*.vue' --include='*.svelte' --include='*.html' ."
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
/// individually).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CssAnalyticsSummary {
    /// Stylesheets analyzed (standard CSS only; SCSS is skipped).
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
    /// Number of distinct `font-size` units (`px` / `rem` / `em` / `%`) authored
    /// across the codebase. Mixing units is a type-scale consistency smell,
    /// broken out in `font_size_unit_mix`.
    pub font_size_units_used: u32,
    /// Number of analyzed stylesheets whose per-rule `notable_rules` list was
    /// truncated at the per-file cap, so a consumer knows the per-rule detail is
    /// incomplete without walking every file.
    pub notable_truncated_files: u32,
}

/// Result of complexity analysis for reporting.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthReport {
    /// Functions and synthetic template entries exceeding complexity
    /// thresholds, sorted by the --sort criteria. Each entry wraps its
    /// inner [`ComplexityViolation`] payload (flattened on the wire) with
    /// the typed `actions` list and an optional audit-mode `introduced`
    /// flag.
    pub findings: Vec<HealthFinding>,
    /// Summary statistics.
    pub summary: HealthSummary,
    /// Configured threshold override states. Entries are emitted for active
    /// exceptions, stale exceptions, and full-run no-match cleanup hints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threshold_overrides: Vec<ThresholdOverrideState>,
    /// Project-wide vital signs (always computed from available data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vital_signs: Option<VitalSigns>,
    /// Project-wide health score (only populated with `--score`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<HealthScore>,
    /// Per-file health scores. Only present when --file-scores is used. Sorted
    /// by risk-aware triage concern, combining low maintainability and high
    /// CRAP risk. Zero-function files (barrels) are excluded by default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_scores: Vec<FileHealthScore>,
    /// Static coverage gaps.
    ///
    /// Populated when coverage gaps are explicitly requested, or when the
    /// top-level `health` command allows config severity to surface them in the
    /// default report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_gaps: Option<CoverageGaps>,
    /// Hotspot entries combining git churn with complexity. Only present when
    /// --hotspots is used. Sorted by score descending (highest risk first).
    /// Each entry wraps its inner [`HotspotEntry`] payload (flattened on the
    /// wire) with a typed `actions` list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<HotspotFinding>,
    /// Hotspot analysis summary (only set with `--hotspots`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_summary: Option<HotspotSummary>,
    /// Runtime coverage findings from the paid sidecar (only populated with
    /// `--runtime-coverage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_coverage: Option<RuntimeCoverageReport>,
    /// Combined coverage, runtime, complexity, and change-scope verdicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_intelligence: Option<CoverageIntelligenceReport>,
    /// Functions exceeding 60 LOC (very high risk). Only present when unit size
    /// very-high-risk bin >= 3%. Sorted by line count descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub large_functions: Vec<LargeFunctionEntry>,
    /// Ranked refactoring recommendations. Only present when --targets is used.
    /// Sorted by efficiency (priority/effort) descending. Each entry wraps
    /// its inner [`RefactoringTarget`] payload (flattened on the wire) with
    /// a typed `actions` list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<RefactoringTargetFinding>,
    /// Adaptive thresholds used for target scoring (only set with `--targets`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_thresholds: Option<TargetThresholds>,
    /// Health trend comparison against a previous snapshot (only set with `--trend`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_trend: Option<HealthTrend>,
    /// Audit breadcrumb explaining systemic action-array adjustments. Present
    /// only when at least one adjustment was made (e.g., health finding
    /// suppression hints omitted because a baseline is active). When --group-by
    /// is active, each entry of `groups` may carry its own `actions_meta`
    /// describing the same omission so per-group consumers do not need to walk
    /// back to the report root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actions_meta: Option<HealthActionsMeta>,
    /// Structural CSS analytics (specificity hotspots, `!important` density,
    /// over-complex selectors, deep nesting). Present only with `--css`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css_analytics: Option<CssAnalyticsReport>,
}

#[cfg(test)]
#[expect(
    clippy::derivable_impls,
    reason = "test-only Default with custom HealthSummary thresholds (20/15)"
)]
impl Default for HealthReport {
    fn default() -> Self {
        Self {
            findings: vec![],
            summary: HealthSummary::default(),
            threshold_overrides: vec![],
            vital_signs: None,
            health_score: None,
            file_scores: vec![],
            coverage_gaps: None,
            hotspots: vec![],
            hotspot_summary: None,
            runtime_coverage: None,
            coverage_intelligence: None,
            large_functions: vec![],
            targets: vec![],
            target_thresholds: None,
            health_trend: None,
            actions_meta: None,
            css_analytics: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_report_skips_empty_collections() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("file_scores"));
        assert!(!json.contains("hotspots"));
        assert!(!json.contains("hotspot_summary"));
        assert!(!json.contains("runtime_coverage"));
        assert!(!json.contains("coverage_intelligence"));
        assert!(!json.contains("large_functions"));
        assert!(!json.contains("targets"));
        assert!(!json.contains("threshold_overrides"));
        assert!(!json.contains("vital_signs"));
        assert!(!json.contains("health_score"));
    }

    #[test]
    fn health_score_none_skipped_in_report() {
        let report = HealthReport::default();
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("health_score"));
    }
}
