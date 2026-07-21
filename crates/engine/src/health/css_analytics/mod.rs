//! CSS analytics execution for `fallow health`.

use fallow_config::ResolvedConfig;

use super::package_json::{
    class_matches_dependency_prefix, dependency_class_prefixes, project_uses_tailwind,
    project_uses_tailwind_plugin, published_css_paths,
};
use super::runtime_filter::relative_to_root;
use super::tailwind_theme;

mod classes;
mod cva;
mod markup_scan;
mod near_duplicates;
mod preprocessor;
mod theme_tokens;
mod token_consumers;

use classes::*;
use cva::*;
use markup_scan::*;
use near_duplicates::*;
use preprocessor::*;
use theme_tokens::*;
use token_consumers::*;

const MAX_REPORTED_RAW_STYLE_VALUES: usize = 200;

/// The per-run scan filters shared by every CSS and markup health scanner:
/// resolved config, the ignore globset, the optional changed-file set, and
/// the optional workspace roots.
#[derive(Clone, Copy)]
pub(super) struct HealthScanCtx<'a> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
}

/// Session-owned styling inputs that can be reused by health, audit, and future
/// editor surfaces without rebuilding every source reference corpus.
#[derive(Clone, Debug)]
pub struct StylingAnalysisArtifacts {
    reference_surface: CssReferenceSurface,
    class_inventory: CssClassInventory,
    whole_scope_walk: CssWalkAccum,
}

pub(super) fn build_styling_analysis_artifacts(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
) -> StylingAnalysisArtifacts {
    let ignore_set = super::ignore::build_ignore_set(&config.health.ignore);
    StylingAnalysisArtifacts {
        reference_surface: css_reference_surface(files, config, &ignore_set),
        class_inventory: css_class_inventory(files, config, &ignore_set),
        whole_scope_walk: walk_css_files(
            files,
            HealthScanCtx {
                config,
                ignore_set: &ignore_set,
                changed_files: None,
                output_changed_files: None,
                ws_roots: None,
            },
        ),
    }
}

/// Compute structural CSS analytics, honoring the same ignore / changed-since /
/// workspace filters as the rest of `fallow health`. Standard CSS is parsed for
/// structural metrics; preprocessor sources are only used by candidate checks
/// that can stay conservative without expanding Sass/Less semantics. Only
/// stylesheets with a structurally notable rule are listed individually; the
/// summary aggregates every analyzed stylesheet. Returns `None` when no
/// stylesheet was analyzed.
/// Project-wide CSS token accumulator: distinct design-token values plus the
/// custom-property / `@keyframes` definition and reference sets, with the first
/// stylesheet that defines/references each keyframe name so a candidate can be
/// located. Populated per stylesheet during the discovery walk, then finalized
/// into the summary counts and the two located keyframe candidate lists.
#[derive(Clone, Default, Debug)]
struct CssTokenSets {
    colors: rustc_hash::FxHashSet<String>,
    font_sizes: rustc_hash::FxHashSet<String>,
    z_indexes: rustc_hash::FxHashSet<String>,
    box_shadows: rustc_hash::FxHashSet<String>,
    border_radii: rustc_hash::FxHashSet<String>,
    line_heights: rustc_hash::FxHashSet<String>,
    defined_custom_props: rustc_hash::FxHashSet<String>,
    referenced_custom_props: rustc_hash::FxHashSet<String>,
    defined_keyframes: rustc_hash::FxHashSet<String>,
    referenced_keyframes: rustc_hash::FxHashSet<String>,
    keyframes_definers: rustc_hash::FxHashMap<String, String>,
    keyframe_referencers: rustc_hash::FxHashMap<String, String>,
    /// Declaration-block fingerprint -> (declaration count, occurrences as
    /// `(path, line)`), for cross-file duplicate-block detection.
    declaration_blocks: rustc_hash::FxHashMap<u64, (u16, Vec<(String, u32)>)>,
    /// `@property` registrations + cascade-layer declarations / populations for
    /// cross-file unused-at-rule detection, with the first defining file per name.
    registered_custom_props: rustc_hash::FxHashSet<String>,
    declared_layers: rustc_hash::FxHashSet<String>,
    populated_layers: rustc_hash::FxHashSet<String>,
    property_registrars: rustc_hash::FxHashMap<String, String>,
    layer_declarers: rustc_hash::FxHashMap<String, String>,
    /// `@font-face`-declared families + referenced font families for cross-file
    /// dead-web-font detection, with the first declaring file per family.
    defined_font_faces: rustc_hash::FxHashSet<String>,
    referenced_font_families: rustc_hash::FxHashSet<String>,
    font_face_definers: rustc_hash::FxHashMap<String, String>,
    /// Tailwind v4 `@theme` tokens (custom-property name without `--`) -> first
    /// definition, for token reachability and drift candidates.
    theme_token_definers: rustc_hash::FxHashMap<String, ThemeTokenDefinition>,
    /// CSS custom properties with literal values, including non-`@theme`
    /// variables, for raw-style nearest-token suggestions.
    custom_property_definers: rustc_hash::FxHashMap<String, ThemeTokenDefinition>,
    /// Utility tokens referenced in `@apply` bodies across all CSS, so a theme
    /// token whose utility is applied only in plain CSS is credited as used.
    apply_tokens: rustc_hash::FxHashSet<String>,
    /// Custom-property names (without `--`) read via `var()` inside `@theme`
    /// interiors (lightningcss skips the unknown at-rule, so these are tracked
    /// separately and never pollute the shared `referenced_custom_props` set
    /// the `@property` / unreferenced-custom-property candidates diff against).
    theme_var_reads: rustc_hash::FxHashSet<String>,
    /// Located `@theme`-interior `var()` reads: `(name, path, line)` per read.
    theme_var_reads_located: Vec<(String, String, u32)>,
    /// Located regular-CSS `var()` reads: `(name, path, line)` per read.
    css_var_reads_located: Vec<(String, String, u32)>,
    /// Located class-shaped tokens inside `@apply` bodies: `(token, path, line)`.
    apply_uses_located: Vec<(String, String, u32)>,
    /// `true` when any analyzed stylesheet declares a Tailwind `@plugin`
    /// directive: a plugin can consume theme tokens via `theme()` / `addUtilities`
    /// invisibly to the markup / CSS / `var()` scan, so the unused-theme-token
    /// candidate hard-abstains on plugin projects (the DI blind spot).
    any_plugin_directive: bool,
    /// Located raw CSS declaration values from authored structural stylesheets.
    raw_style_values: Vec<fallow_output::RawStyleValue>,
}

#[derive(Clone, Debug)]
struct ThemeTokenDefinition {
    path: String,
    line: u32,
    value: String,
}

impl CssTokenSets {
    /// Group declaration-block fingerprints seen in 2+ rules into located
    /// duplicate-block candidates, set the summary counts, and sort by estimated
    /// savings descending (then first occurrence path).
    fn group_duplicate_blocks(
        &self,
        summary: &mut fallow_output::CssAnalyticsSummary,
    ) -> Vec<fallow_output::CssDuplicateBlock> {
        use fallow_output::{CssBlockOccurrence, CssCandidateAction, CssDuplicateBlock};

        let mut groups: Vec<CssDuplicateBlock> = self
            .declaration_blocks
            .values()
            .filter(|(_, occurrences)| occurrences.len() >= 2)
            .map(|(declaration_count, occurrences)| {
                let occurrence_count = saturate_len(occurrences.len());
                let estimated_savings = occurrence_count
                    .saturating_sub(1)
                    .saturating_mul(u32::from(*declaration_count));
                let mut occ: Vec<CssBlockOccurrence> = occurrences
                    .iter()
                    .map(|(path, line)| CssBlockOccurrence {
                        path: path.clone(),
                        line: *line,
                    })
                    .collect();
                occ.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
                CssDuplicateBlock {
                    declaration_count: *declaration_count,
                    occurrence_count,
                    estimated_savings,
                    occurrences: occ,
                    actions: vec![CssCandidateAction::consolidate_block(occurrence_count)],
                }
            })
            .collect();
        // Highest-savings groups first; tie-break on the first occurrence path for
        // deterministic output.
        groups.sort_by(|a, b| {
            b.estimated_savings
                .cmp(&a.estimated_savings)
                .then_with(|| occurrence_sort_key(a).cmp(&occurrence_sort_key(b)))
        });
        summary.duplicate_declaration_blocks = saturate_len(groups.len());
        summary.duplicate_declarations_total = groups
            .iter()
            .fold(0u32, |acc, g| acc.saturating_add(g.estimated_savings));
        groups
    }

    /// Fold one stylesheet's analytics into the project-wide token sets,
    /// recording the first-defining file (`rel`) per located name.
    fn record(&mut self, analytics: &fallow_types::extract::CssAnalytics, rel: &str) {
        self.record_design_tokens(analytics);
        self.record_custom_properties(analytics, rel);
        self.record_keyframes(analytics, rel);
        self.record_declaration_blocks(analytics, rel);
        self.record_font_faces_and_layers(analytics, rel);
        self.record_raw_style_values(analytics, rel);
    }

    fn record_design_tokens(&mut self, analytics: &fallow_types::extract::CssAnalytics) {
        self.colors.extend(analytics.colors.iter().cloned());
        self.font_sizes.extend(analytics.font_sizes.iter().cloned());
        self.z_indexes.extend(analytics.z_indexes.iter().cloned());
        self.box_shadows
            .extend(analytics.box_shadows.iter().cloned());
        self.border_radii
            .extend(analytics.border_radii.iter().cloned());
        self.line_heights
            .extend(analytics.line_heights.iter().cloned());
    }

    fn record_custom_properties(
        &mut self,
        analytics: &fallow_types::extract::CssAnalytics,
        rel: &str,
    ) {
        self.defined_custom_props
            .extend(analytics.defined_custom_properties.iter().cloned());
        for token in &analytics.custom_property_definitions {
            self.custom_property_definers
                .entry(token.name.clone())
                .or_insert_with(|| ThemeTokenDefinition {
                    path: rel.to_owned(),
                    line: token.line,
                    value: token.value.clone(),
                });
        }
        self.referenced_custom_props
            .extend(analytics.referenced_custom_properties.iter().cloned());
        for name in &analytics.registered_custom_properties {
            self.registered_custom_props.insert(name.clone());
            self.property_registrars
                .entry(name.clone())
                .or_insert_with(|| rel.to_owned());
        }
    }

    fn record_keyframes(&mut self, analytics: &fallow_types::extract::CssAnalytics, rel: &str) {
        for keyframes in &analytics.referenced_keyframes {
            self.referenced_keyframes.insert(keyframes.clone());
            self.keyframe_referencers
                .entry(keyframes.clone())
                .or_insert_with(|| rel.to_owned());
        }
        for keyframes in &analytics.defined_keyframes {
            self.defined_keyframes.insert(keyframes.clone());
            self.keyframes_definers
                .entry(keyframes.clone())
                .or_insert_with(|| rel.to_owned());
        }
    }

    fn record_declaration_blocks(
        &mut self,
        analytics: &fallow_types::extract::CssAnalytics,
        rel: &str,
    ) {
        for block in &analytics.declaration_blocks {
            self.declaration_blocks
                .entry(block.fingerprint)
                .or_insert_with(|| (block.declaration_count, Vec::new()))
                .1
                .push((rel.to_owned(), block.line));
        }
    }

    fn record_font_faces_and_layers(
        &mut self,
        analytics: &fallow_types::extract::CssAnalytics,
        rel: &str,
    ) {
        for family in &analytics.referenced_font_families {
            self.referenced_font_families.insert(family.clone());
        }
        for family in &analytics.defined_font_faces {
            self.defined_font_faces.insert(family.clone());
            self.font_face_definers
                .entry(family.clone())
                .or_insert_with(|| rel.to_owned());
        }
        for name in &analytics.populated_layers {
            self.populated_layers.insert(name.clone());
        }
        for name in &analytics.declared_layers {
            self.declared_layers.insert(name.clone());
            self.layer_declarers
                .entry(name.clone())
                .or_insert_with(|| rel.to_owned());
        }
    }

    fn record_raw_style_values(
        &mut self,
        analytics: &fallow_types::extract::CssAnalytics,
        rel: &str,
    ) {
        for raw in &analytics.raw_style_values {
            if self.raw_style_values.len() >= MAX_REPORTED_RAW_STYLE_VALUES {
                break;
            }
            self.raw_style_values.push(fallow_output::RawStyleValue {
                axis: raw.axis.clone(),
                property: raw.property.clone(),
                value: raw.value.clone(),
                path: rel.to_owned(),
                line: raw.line,
                nearest_token: None,
                actions: vec![fallow_output::CssCandidateAction::replace_raw_style_value(
                    &raw.axis, &raw.value,
                )],
            });
        }
    }

    /// Fold one stylesheet's Tailwind v4 `@theme` tokens, `@apply` body tokens,
    /// and `@theme`-interior `var()` reads into the project-wide sets (the inputs
    /// to the unused-theme-token candidate). `scan_theme_blocks` /
    /// `extract_apply_tokens` fast-path out on sources with no `@theme` / `@apply`,
    /// so this is near-free for non-Tailwind stylesheets.
    fn record_theme(&mut self, source: &str, rel: &str) {
        let scan = crate::css::scan_theme_blocks(source);
        for token in scan.tokens {
            self.theme_token_definers
                .entry(token.name)
                .or_insert_with(|| ThemeTokenDefinition {
                    path: rel.to_owned(),
                    line: token.line,
                    value: token.value,
                });
        }
        for (name, line) in scan.theme_var_reads {
            self.theme_var_reads.insert(name.clone());
            self.theme_var_reads_located
                .push((name, rel.to_owned(), line));
        }
        self.apply_tokens
            .extend(crate::css::extract_apply_tokens(source));
        self.apply_uses_located.extend(
            crate::css::extract_apply_tokens_located(source)
                .into_iter()
                .map(|(token, line)| (token, rel.to_owned(), line)),
        );
        self.css_var_reads_located.extend(
            crate::css::extract_css_var_reads_located(source)
                .into_iter()
                .map(|(name, line)| (name, rel.to_owned(), line)),
        );
        if source.contains("@plugin") {
            self.any_plugin_directive = true;
        }
    }

    /// Group unused CSS at-rule entities: `@property` registrations never read
    /// via `var()`, and cascade layers declared but never populated. Sets the
    /// summary counts and returns the located list sorted by (kind, path, name).
    fn group_unused_at_rules(
        &self,
        summary: &mut fallow_output::CssAnalyticsSummary,
    ) -> Vec<fallow_output::UnusedAtRule> {
        use fallow_output::{CssCandidateAction, UnusedAtRule, UnusedAtRuleKind};

        let mut out: Vec<UnusedAtRule> = Vec::new();
        for name in self
            .registered_custom_props
            .difference(&self.referenced_custom_props)
        {
            out.push(UnusedAtRule {
                kind: UnusedAtRuleKind::PropertyRegistration,
                name: name.clone(),
                path: self
                    .property_registrars
                    .get(name)
                    .cloned()
                    .unwrap_or_default(),
                actions: vec![CssCandidateAction::verify_unused_at_rule(
                    UnusedAtRuleKind::PropertyRegistration,
                    name,
                )],
            });
        }
        summary.unused_property_registrations = saturate_len(out.len());
        let property_count = out.len();
        for name in self.declared_layers.difference(&self.populated_layers) {
            out.push(UnusedAtRule {
                kind: UnusedAtRuleKind::Layer,
                name: name.clone(),
                path: self.layer_declarers.get(name).cloned().unwrap_or_default(),
                actions: vec![CssCandidateAction::verify_unused_at_rule(
                    UnusedAtRuleKind::Layer,
                    name,
                )],
            });
        }
        summary.unused_layers = saturate_len(out.len() - property_count);
        out.sort_by(|a, b| (a.kind as u8, &a.path, &a.name).cmp(&(b.kind as u8, &b.path, &b.name)));
        out
    }

    /// Fill the summary token counts and return the two located keyframe
    /// candidate lists: defined-but-unused (`unreferenced`) and used-but-
    /// undefined (`undefined`).
    fn finalize(
        &self,
        referenced_keyframes: &rustc_hash::FxHashSet<String>,
        summary: &mut fallow_output::CssAnalyticsSummary,
    ) -> (
        Vec<fallow_output::UnreferencedKeyframes>,
        Vec<fallow_output::UndefinedKeyframes>,
    ) {
        use fallow_output::{CssCandidateAction, UndefinedKeyframes, UnreferencedKeyframes};

        summary.unique_colors = saturate_len(self.colors.len());
        summary.unique_font_sizes = saturate_len(self.font_sizes.len());
        summary.unique_z_indexes = saturate_len(self.z_indexes.len());
        summary.unique_box_shadows = saturate_len(self.box_shadows.len());
        summary.unique_border_radii = saturate_len(self.border_radii.len());
        summary.unique_line_heights = saturate_len(self.line_heights.len());
        summary.custom_properties_defined = saturate_len(self.defined_custom_props.len());
        summary.custom_properties_unreferenced = saturate_len(
            self.defined_custom_props
                .difference(&self.referenced_custom_props)
                .count(),
        );
        // Count-only (per panel review): a var() referenced but defined in no
        // stylesheet is dominated by JS-set design tokens, so locating these
        // would be net-noise. The count is an architecture signal.
        summary.custom_properties_undefined = saturate_len(
            self.referenced_custom_props
                .difference(&self.defined_custom_props)
                .count(),
        );
        summary.keyframes_defined = saturate_len(self.defined_keyframes.len());
        summary.keyframes_unreferenced = saturate_len(
            self.defined_keyframes
                .difference(referenced_keyframes)
                .count(),
        );
        summary.keyframes_undefined = saturate_len(
            referenced_keyframes
                .difference(&self.defined_keyframes)
                .count(),
        );

        // @keyframes are low-cardinality, so BOTH directions are located (not
        // just counted): defined-but-unused, and used-but-defined-nowhere.
        let unreferenced_keyframes = locate_keyframe_diff(
            &self.defined_keyframes,
            referenced_keyframes,
            &self.keyframes_definers,
        )
        .into_iter()
        .map(|(name, path)| UnreferencedKeyframes {
            actions: vec![CssCandidateAction::verify_keyframe(&name)],
            name,
            path,
        })
        .collect();
        let undefined_keyframes = locate_keyframe_diff(
            referenced_keyframes,
            &self.defined_keyframes,
            &self.keyframe_referencers,
        )
        .into_iter()
        .map(|(name, path)| UndefinedKeyframes {
            actions: vec![CssCandidateAction::verify_undefined_keyframe(&name)],
            name,
            path,
        })
        .collect();
        (unreferenced_keyframes, undefined_keyframes)
    }

    /// `@font-face`-declared families referenced by no `font-family` anywhere in
    /// the project: a dead web-font payload. Located at the declaring stylesheet,
    /// set the summary count.
    fn unused_font_faces(
        &self,
        summary: &mut fallow_output::CssAnalyticsSummary,
    ) -> Vec<fallow_output::UnusedFontFace> {
        use fallow_output::{CssCandidateAction, UnusedFontFace};
        // CSS font-family names are case-insensitive (CSS Fonts Level 4 4.2.1),
        // unlike `@keyframes` custom-ident names (case-sensitive, via
        // `locate_keyframe_diff`), so match case-insensitively while keeping the
        // declared casing for both display and the verify command.
        let referenced_lower: rustc_hash::FxHashSet<String> = self
            .referenced_font_families
            .iter()
            .map(|family| family.to_ascii_lowercase())
            .collect();
        let mut out: Vec<UnusedFontFace> = self
            .defined_font_faces
            .iter()
            .filter(|family| !referenced_lower.contains(&family.to_ascii_lowercase()))
            .map(|family| UnusedFontFace {
                actions: vec![CssCandidateAction::verify_unused_font_face(family)],
                path: self
                    .font_face_definers
                    .get(family)
                    .cloned()
                    .unwrap_or_default(),
                family: family.clone(),
            })
            .collect();
        out.sort_by(|a, b| (&a.path, &a.family).cmp(&(&b.path, &b.family)));
        summary.unused_font_faces = saturate_len(out.len());
        out
    }

    /// Group the distinct `font-size` values by length unit (`px`/`rem`/`em`/`%`/
    /// `pt`/other), set the `font_size_units_used` count, and, when the project
    /// mixes two or more units across enough distinct sizes, return a
    /// consistency candidate (mixing `px` and `rem` for type works against
    /// user-zoom accessibility). Advisory only, never gated.
    fn font_size_unit_mix(
        &self,
        summary: &mut fallow_output::CssAnalyticsSummary,
    ) -> Option<fallow_output::CssNotationConsistency> {
        use fallow_output::{CssCandidateAction, CssNotationConsistency, CssNotationCount};

        let mut counts: rustc_hash::FxHashMap<&'static str, u32> = rustc_hash::FxHashMap::default();
        for value in &self.font_sizes {
            if let Some(unit) = classify_font_size_unit(value) {
                *counts.entry(unit).or_insert(0) += 1;
            }
        }
        summary.font_size_units_used = saturate_len(counts.len());

        // Conservative floor: at least two distinct units AND enough classified
        // sizes that the project plainly has a type scale (so a tiny stylesheet
        // with one px and one rem does not trip it). Smoke-tunable.
        let total: u32 = counts.values().copied().sum();
        if counts.len() < 2 || total < MIN_FONT_SIZE_UNIT_MIX {
            return None;
        }
        let mut notations: Vec<CssNotationCount> = counts
            .into_iter()
            .map(|(notation, count)| CssNotationCount {
                notation: notation.to_owned(),
                count,
            })
            .collect();
        // Dominant unit first; tie-break on the unit name for deterministic output.
        notations.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.notation.cmp(&b.notation))
        });
        // Safe: the floor guard above guarantees at least two notations.
        let dominant = notations[0].notation.clone();
        Some(CssNotationConsistency {
            actions: vec![CssCandidateAction::standardize_notation(
                "Font sizes",
                &dominant,
            )],
            axis: "Font sizes".to_owned(),
            notations,
        })
    }
}

/// Fewest distinct unit-classified `font-size` values before a unit-mix candidate
/// is worth surfacing. Below this the project does not yet have a type scale, so
/// a px/rem split is noise rather than an inconsistency.
const MIN_FONT_SIZE_UNIT_MIX: u32 = 6;

/// Classify a `font-size` value's length unit for the unit-consistency
/// candidate. Returns `None` for function values (`clamp()` / `calc()` /
/// `min()` / `max()` / `var()`) and bare keywords (`medium`, `larger`,
/// `inherit`), which carry no single comparable unit. Unit names are lowercased;
/// recognized type units map to a stable label, anything else to `"other"`.
fn classify_font_size_unit(value: &str) -> Option<&'static str> {
    let v = value.trim();
    if v.is_empty() || v.contains('(') {
        return None;
    }
    if let Some(stripped) = v.strip_suffix('%') {
        // A bare `%` font-size is `<number>%`; reject anything else (defensive).
        return stripped
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
            .then_some("%");
    }
    let unit_start = v.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = v.split_at(unit_start);
    // A dimension is `<number><unit>`; a leading non-numeric prefix means a
    // keyword (e.g. `medium`), which has no unit.
    if number.is_empty()
        || !number
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+')
    {
        return None;
    }
    match unit.to_ascii_lowercase().as_str() {
        "px" => Some("px"),
        "rem" => Some("rem"),
        "em" => Some("em"),
        "pt" => Some("pt"),
        _ => Some("other"),
    }
}

/// Build the sorted `(name, path)` set difference `present - absent`, locating
/// each surviving name via `locator` (empty path when absent). Sorted by
/// `(path, name)` for deterministic output.
fn locate_keyframe_diff(
    present: &rustc_hash::FxHashSet<String>,
    absent: &rustc_hash::FxHashSet<String>,
    locator: &rustc_hash::FxHashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = present
        .difference(absent)
        .map(|name| (name.clone(), locator.get(name).cloned().unwrap_or_default()))
        .collect();
    out.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    out
}

/// Saturating `usize -> u32` for token counts.
fn saturate_len(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// `(first path, first line)` sort key for a duplicate block; occurrences are
/// pre-sorted, so the first is the lexicographic minimum.
fn occurrence_sort_key(block: &fallow_output::CssDuplicateBlock) -> (&str, u32) {
    block
        .occurrences
        .first()
        .map_or(("", 0), |occ| (occ.path.as_str(), occ.line))
}

/// Scan the project's markup (`.jsx` / `.tsx` / `.html` / `.astro` / `.vue` /
/// `.svelte` / `.md` / `.mdx`) for Tailwind arbitrary-value utility tokens,
/// honoring the same
/// ignore / changed / workspace filters as the CSS scan. Aggregates by token
/// (total count + first location), sets the summary counts, and returns the
/// located list sorted by use count descending.
/// One eligible markup file for a class-token scan: the forward-slash relative
/// path plus source, or `None` when the file is filtered out (extension, ignore
/// set, changed-files, workspace scope) or unreadable.
fn read_markup_scan_source(
    file: &fallow_types::discover::DiscoveredFile,
    ctx: HealthScanCtx<'_>,
) -> Option<(String, String)> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files: _,
        ws_roots,
    } = ctx;

    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    if !extension.is_some_and(is_markup_source_extension) {
        return None;
    }
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    if ignore_set.is_match(relative) {
        return None;
    }
    if let Some(changed) = changed_files
        && !changed.contains(path)
    {
        return None;
    }
    if let Some(roots) = ws_roots
        && !roots.iter().any(|root| path.starts_with(root))
    {
        return None;
    }
    let source = std::fs::read_to_string(path).ok()?;
    let rel = relative.to_string_lossy().replace('\\', "/");
    Some((rel, source))
}

fn scan_markup_tailwind_arbitrary_values(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    summary: &mut fallow_output::CssAnalyticsSummary,
) -> Vec<fallow_output::TailwindArbitraryValue> {
    let HealthScanCtx { config, .. } = ctx;

    use fallow_output::TailwindArbitraryValue;

    if !project_uses_tailwind(&config.root) {
        return Vec::new();
    }
    // token -> (total count, first path, first line). First-seen wins for the
    // location; files are path-sorted, so the first occurrence is deterministic.
    let mut agg: rustc_hash::FxHashMap<String, (u32, String, u32)> =
        rustc_hash::FxHashMap::default();
    let mut total_uses: u32 = 0;
    for file in files {
        let Some((rel, source)) = read_markup_scan_source(file, ctx) else {
            continue;
        };
        for arb in crate::css::scan_tailwind_arbitrary_values(&source) {
            total_uses = total_uses.saturating_add(1);
            let entry = agg
                .entry(arb.value)
                .or_insert_with(|| (0, rel.clone(), arb.line));
            entry.0 = entry.0.saturating_add(1);
        }
    }

    summary.tailwind_arbitrary_values = saturate_len(agg.len());
    summary.tailwind_arbitrary_value_uses = total_uses;
    let mut out: Vec<TailwindArbitraryValue> = agg
        .into_iter()
        .map(|(value, (count, path, line))| TailwindArbitraryValue {
            actions: vec![fallow_output::CssCandidateAction::replace_arbitrary_value(
                &value,
            )],
            value,
            count,
            path,
            line,
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
    out
}

fn record_css_analytics_summary(
    summary: &mut fallow_output::CssAnalyticsSummary,
    analytics: &fallow_types::extract::CssAnalytics,
) {
    summary.total_rules = summary.total_rules.saturating_add(analytics.rule_count);
    summary.total_declarations = summary
        .total_declarations
        .saturating_add(analytics.total_declarations);
    summary.important_declarations = summary
        .important_declarations
        .saturating_add(analytics.important_declarations);
    summary.empty_rules = summary
        .empty_rules
        .saturating_add(analytics.empty_rule_count);
    summary.max_nesting_depth = summary.max_nesting_depth.max(analytics.max_nesting_depth);
    if analytics.notable_truncated {
        summary.notable_truncated_files = summary.notable_truncated_files.saturating_add(1);
    }
}

/// The per-file CSS walk accumulator: structural file reports, the project-wide
/// token sets, scoped SFC unused-class findings, and the running summary.
#[derive(Clone, Debug)]
struct CssWalkAccum {
    file_reports: Vec<fallow_output::CssFileAnalytics>,
    summary: fallow_output::CssAnalyticsSummary,
    scoped_unused: Vec<fallow_output::ScopedUnusedClasses>,
    tokens: CssTokenSets,
    scoring: CssGradeScoring,
}

enum CssReportWalk<'a> {
    Cached(&'a CssWalkAccum),
    Fresh(Box<CssWalkAccum>),
}

impl CssReportWalk<'_> {
    fn as_ref(&self) -> &CssWalkAccum {
        match self {
            Self::Cached(walk) => walk,
            Self::Fresh(walk) => walk,
        }
    }

    fn into_output(
        self,
        summary: fallow_output::CssAnalyticsSummary,
        raw_style_values: Vec<fallow_output::RawStyleValue>,
    ) -> CssReportOutput {
        let (file_reports, scoped_unused) = match self {
            Self::Cached(walk) => (walk.file_reports.clone(), walk.scoped_unused.clone()),
            Self::Fresh(walk) => {
                let CssWalkAccum {
                    file_reports,
                    scoped_unused,
                    ..
                } = *walk;
                (file_reports, scoped_unused)
            }
        };
        CssReportOutput {
            file_reports,
            summary,
            scoped_unused,
            raw_style_values,
        }
    }
}

struct CssReportOutput {
    file_reports: Vec<fallow_output::CssFileAnalytics>,
    summary: fallow_output::CssAnalyticsSummary,
    scoped_unused: Vec<fallow_output::ScopedUnusedClasses>,
    raw_style_values: Vec<fallow_output::RawStyleValue>,
}

#[derive(Clone, Debug, Default)]
struct CssGradeScoring {
    non_atomic_declarations: u32,
    non_atomic_important_declarations: u32,
    non_atomic_max_nesting_depth: u8,
    atomic_declarations: u32,
}

impl CssGradeScoring {
    fn add_non_atomic(&mut self, analytics: &fallow_types::extract::CssAnalytics) {
        self.non_atomic_declarations = self
            .non_atomic_declarations
            .saturating_add(analytics.total_declarations);
        self.non_atomic_important_declarations = self
            .non_atomic_important_declarations
            .saturating_add(analytics.important_declarations);
        self.non_atomic_max_nesting_depth = self
            .non_atomic_max_nesting_depth
            .max(analytics.max_nesting_depth);
    }
}

/// The finalized whole-project token metrics (keyframes, duplicate blocks, unused
/// at-rules, font-size unit mix, unused font faces) derived after the file walk.
struct CssTokenMetrics {
    unreferenced_keyframes: Vec<fallow_output::UnreferencedKeyframes>,
    undefined_keyframes: Vec<fallow_output::UndefinedKeyframes>,
    duplicate_declaration_blocks: Vec<fallow_output::CssDuplicateBlock>,
    unused_at_rules: Vec<fallow_output::UnusedAtRule>,
    font_size_unit_mix: Option<fallow_output::CssNotationConsistency>,
    unused_font_faces: Vec<fallow_output::UnusedFontFace>,
}

/// CSS analytics plus internal-only inputs for the styling-health grade.
pub(super) struct CssAnalyticsComputation {
    pub(super) report: fallow_output::CssAnalyticsReport,
    pub(super) scoring_inputs: super::styling_score::StylingScoringInputs,
}

/// Walk every in-scope stylesheet / SFC, accumulating structural metrics, the
/// project token sets, and scoped SFC unused-class findings.
fn walk_css_files(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
) -> CssWalkAccum {
    use fallow_output::{CssAnalyticsSummary, ScopedUnusedClasses};

    let mut file_reports = Vec::new();
    let mut summary = CssAnalyticsSummary::default();
    let mut scoped_unused: Vec<ScopedUnusedClasses> = Vec::new();
    // Project-wide design-token + custom-property + @keyframes accumulator,
    // unioned across every analyzed stylesheet (including ones with no notable
    // rule, which are not listed individually), finalized after the walk.
    let mut tokens = CssTokenSets::default();
    let mut scoring = CssGradeScoring::default();
    let css_in_js = project_uses_css_in_js(&ctx.config.root);

    for file in files {
        let Some((relative, kind)) = css_report_scan_target(file, ctx, css_in_js) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&file.path) else {
            continue;
        };

        if kind == CssScanKind::Sfc {
            record_scoped_unused_classes(&source, relative, &mut summary, &mut scoped_unused);
        }

        let rel = relative.to_string_lossy().replace('\\', "/");
        let mut file_had_sheet = false;
        for item in css_report_scan_items(&source, &file.path, kind) {
            file_had_sheet |= record_css_scan_item(
                &item,
                &rel,
                &mut file_reports,
                &mut summary,
                &mut tokens,
                &mut scoring,
            );
        }
        if file_had_sheet {
            summary.files_analyzed = summary.files_analyzed.saturating_add(1);
        }
    }

    CssWalkAccum {
        file_reports,
        summary,
        scoped_unused,
        tokens,
        scoring,
    }
}

fn record_css_scan_item(
    item: &CssScanItem<'_>,
    rel: &str,
    file_reports: &mut Vec<fallow_output::CssFileAnalytics>,
    summary: &mut fallow_output::CssAnalyticsSummary,
    tokens: &mut CssTokenSets,
    scoring: &mut CssGradeScoring,
) -> bool {
    let Some(mut analytics) = crate::css::compute_css_analytics(&item.source) else {
        return false;
    };
    record_css_analytics_summary(summary, &analytics);
    tokens.record_theme(item.source.as_ref(), rel);

    match item.policy {
        GradePolicy::Atomic => {
            analytics.declaration_blocks.clear();
            analytics.raw_style_values.clear();
            tokens.record(&analytics, rel);
            scoring.atomic_declarations = scoring
                .atomic_declarations
                .saturating_add(analytics.total_declarations);
        }
        GradePolicy::Structural | GradePolicy::StructuralNoDedup => {
            if item.policy == GradePolicy::StructuralNoDedup {
                analytics.declaration_blocks.clear();
            }
            tokens.record(&analytics, rel);
            scoring.add_non_atomic(&analytics);
            if item.report_notable && !analytics.notable_rules.is_empty() {
                file_reports.push(fallow_output::CssFileAnalytics {
                    path: rel.to_owned(),
                    analytics,
                });
            }
        }
    }

    true
}

/// Credit Tailwind-markup-applied keyframes, then finalize the whole-project
/// token metrics and prune unused `@font-face` families referenced elsewhere.
fn finalize_css_token_metrics(
    tokens: &CssTokenSets,
    summary: &mut fallow_output::CssAnalyticsSummary,
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssTokenMetrics {
    // Credit @keyframes applied via Tailwind markup (`animate-[name_...]` /
    // `animate-name`), not just CSS `animation:` declarations, before the
    // unreferenced diff. Filtered to actually-defined keyframes so a stray
    // `animate-*` suffix never manufactures a false `undefined_keyframes`.
    let mut referenced_keyframes = tokens.referenced_keyframes.clone();
    for name in collect_markup_keyframe_references(files, config, ignore_set) {
        if tokens.defined_keyframes.contains(&name) {
            referenced_keyframes.insert(name);
        }
    }

    let (unreferenced_keyframes, undefined_keyframes) =
        tokens.finalize(&referenced_keyframes, summary);
    let duplicate_declaration_blocks = tokens.group_duplicate_blocks(summary);
    let unused_at_rules = tokens.group_unused_at_rules(summary);
    let font_size_unit_mix = tokens.font_size_unit_mix(summary);
    let mut unused_font_faces = tokens.unused_font_faces(summary);
    // The CSS-only set difference cannot see a font family applied from
    // JavaScript / canvas (Excalidraw) or referenced from a `.scss`/`.sass`
    // theme the parser never reads (reveal.js). Drop any candidate whose family
    // name appears as a substring in ANY non-CSS source file, so only a font
    // declared and used nowhere at all survives. (Real-world smoke.)
    if !unused_font_faces.is_empty() {
        let referenced =
            font_families_referenced_in_source(&unused_font_faces, files, config, ignore_set);
        unused_font_faces.retain(|ff| !referenced.contains(&ff.family));
        summary.unused_font_faces = saturate_len(unused_font_faces.len());
    }

    CssTokenMetrics {
        unreferenced_keyframes,
        undefined_keyframes,
        duplicate_declaration_blocks,
        unused_at_rules,
        font_size_unit_mix,
        unused_font_faces,
    }
}

#[cfg(test)]
fn compute_css_analytics_report(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    ctx: HealthScanCtx<'_>,
) -> Option<CssAnalyticsComputation> {
    compute_css_analytics_report_with_artifacts(files, modules, ctx, None)
}

pub(super) fn compute_css_analytics_report_with_artifacts(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    ctx: HealthScanCtx<'_>,
    styling_artifacts: Option<&StylingAnalysisArtifacts>,
) -> Option<CssAnalyticsComputation> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files,
        ws_roots,
    } = ctx;
    let css_deep = output_changed_files.is_some();

    // Collect CSS-in-JS token definers ONCE per run (parsing every candidate
    // definer file from disk). Both the comparable-token candidate pass and the
    // consumer blast-radius pass borrow this, instead of each recomputing it.
    // `None` mirrors the old `!project_uses_css_in_js` short-circuit exactly.
    let css_in_js_definers = project_uses_css_in_js(&config.root).then(|| {
        let path_by_id: rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path> =
            files.iter().map(|f| (f.id, f.path.as_path())).collect();
        collect_css_in_js_definers(modules, &path_by_id, config)
    });

    let walk = css_report_walk(files, ctx, styling_artifacts);
    let walk_ref = walk.as_ref();
    let mut summary = walk_ref.summary.clone();
    let mut raw_style_values = walk_ref.tokens.raw_style_values.clone();
    let styling_token_candidates =
        css_report_token_candidates(&walk_ref.tokens, config, css_in_js_definers.as_ref());
    annotate_raw_style_value_nearest_tokens(&mut raw_style_values, &styling_token_candidates);
    let metrics =
        finalize_css_token_metrics(&walk_ref.tokens, &mut summary, files, config, ignore_set);
    let candidates = scan_markup_css_candidates(&mut MarkupCssCandidateInput {
        tokens: &walk_ref.tokens,
        files,
        css_in_js_definers: css_in_js_definers.as_ref(),
        config,
        ignore_set,
        changed_files,
        output_changed_files,
        css_deep,
        ws_roots,
        styling_artifacts,
        token_candidates: &styling_token_candidates,
        summary: &mut summary,
    });
    let token_consumers = css_report_token_consumers(
        &TokenConsumersInput {
            tokens: &walk_ref.tokens,
            files,
            config,
            ignore_set,
            changed_files,
            ws_roots,
        },
        modules,
        css_in_js_definers.as_ref(),
    );
    let scoring_inputs = css_report_scoring_inputs(walk_ref);
    let output = walk.into_output(summary, raw_style_values);
    let report = assemble_css_report(CssReportAssemblyInput {
        output,
        metrics,
        candidates,
        token_consumers,
        config,
        output_changed_files,
    })?;
    Some(CssAnalyticsComputation {
        report,
        scoring_inputs,
    })
}

fn css_report_walk<'a>(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    styling_artifacts: Option<&'a StylingAnalysisArtifacts>,
) -> CssReportWalk<'a> {
    let HealthScanCtx {
        changed_files,
        output_changed_files,
        ws_roots,
        ..
    } = ctx;

    if let Some(artifacts) = styling_artifacts
        .filter(|_| changed_files.is_none() && output_changed_files.is_none() && ws_roots.is_none())
    {
        CssReportWalk::Cached(&artifacts.whole_scope_walk)
    } else {
        CssReportWalk::Fresh(Box::new(walk_css_files(files, ctx)))
    }
}

fn css_report_scoring_inputs(walk: &CssWalkAccum) -> super::styling_score::StylingScoringInputs {
    super::styling_score::StylingScoringInputs {
        theme_tokens_defined: saturate_len(walk.tokens.theme_token_definers.len()),
        non_atomic_declarations: walk.scoring.non_atomic_declarations,
        non_atomic_important_declarations: walk.scoring.non_atomic_important_declarations,
        non_atomic_max_nesting_depth: walk.scoring.non_atomic_max_nesting_depth,
        atomic_declarations: walk.scoring.atomic_declarations,
    }
}

fn css_report_token_candidates(
    tokens: &CssTokenSets,
    config: &ResolvedConfig,
    css_in_js_definers: Option<&CssInJsDefiners>,
) -> Vec<ComparableThemeTokenCandidate> {
    let mut candidates = comparable_theme_token_candidates(tokens, config);
    candidates.extend(comparable_custom_property_token_candidates(tokens));
    candidates.extend(comparable_css_in_js_token_candidates(css_in_js_definers));
    candidates.extend(comparable_project_vocabulary_candidates(tokens));
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    candidates
}

fn css_report_token_consumers(
    input: &TokenConsumersInput<'_>,
    modules: &[fallow_types::extract::ModuleInfo],
    css_in_js_definers: Option<&CssInJsDefiners>,
) -> Vec<fallow_output::TokenConsumers> {
    let mut consumers = build_token_consumers(input);
    consumers.extend(build_css_in_js_token_consumers(
        input.files,
        modules,
        input.config,
        css_in_js_definers,
    ));
    consumers.sort_by(|a, b| {
        a.token
            .cmp(&b.token)
            .then_with(|| a.definition_path.cmp(&b.definition_path))
    });
    consumers
}

/// Assemble the final CSS analytics report from the walk accumulator, finalized
/// token metrics, and markup candidates; returns `None` when nothing notable was
/// found (no analyzed files and every candidate list empty).
struct CssReportAssemblyInput<'a> {
    output: CssReportOutput,
    metrics: CssTokenMetrics,
    candidates: MarkupCssCandidates,
    token_consumers: Vec<fallow_output::TokenConsumers>,
    config: &'a ResolvedConfig,
    output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
}

fn assemble_css_report(
    input: CssReportAssemblyInput<'_>,
) -> Option<fallow_output::CssAnalyticsReport> {
    use fallow_output::CssAnalyticsReport;

    let CssReportAssemblyInput {
        mut output,
        mut metrics,
        mut candidates,
        mut token_consumers,
        config,
        output_changed_files,
    } = input;

    if let Some(changed) = output_changed_files {
        retain_css_report_changed_scope(CssReportChangedScopeInput {
            output: &mut output,
            metrics: &mut metrics,
            candidates: &mut candidates,
            token_consumers: &mut token_consumers,
            config,
            changed,
        });
    }

    if css_report_is_empty(&output, &metrics, &candidates, &token_consumers) {
        return None;
    }
    let mut scoped_unused = output.scoped_unused;
    scoped_unused.sort_by(|a, b| a.path.cmp(&b.path));
    sort_raw_style_values(&mut output.raw_style_values);
    output.summary.raw_style_values = saturate_len(output.raw_style_values.len());
    Some(CssAnalyticsReport {
        files: output.file_reports,
        summary: output.summary,
        scoped_unused,
        unreferenced_keyframes: metrics.unreferenced_keyframes,
        undefined_keyframes: metrics.undefined_keyframes,
        duplicate_declaration_blocks: metrics.duplicate_declaration_blocks,
        cva_duplicate_variant_blocks: candidates.cva_duplicate_variant_blocks,
        cva_variant_token_drifts: candidates.cva_variant_token_drifts,
        tailwind_arbitrary_values: candidates.tailwind_arbitrary_values,
        raw_style_values: output.raw_style_values,
        unused_at_rules: metrics.unused_at_rules,
        unresolved_class_references: candidates.unresolved_class_references,
        unreferenced_css_classes: candidates.unreferenced_css_classes,
        unused_font_faces: metrics.unused_font_faces,
        unused_theme_tokens: candidates.unused_theme_tokens,
        near_duplicate_theme_tokens: candidates.near_duplicate_theme_tokens,
        near_duplicate_css_in_js_tokens: candidates.near_duplicate_css_in_js_tokens,
        token_consumers,
        font_size_unit_mix: metrics.font_size_unit_mix,
    })
}

fn css_report_is_empty(
    output: &CssReportOutput,
    metrics: &CssTokenMetrics,
    candidates: &MarkupCssCandidates,
    token_consumers: &[fallow_output::TokenConsumers],
) -> bool {
    output.summary.files_analyzed == 0
        && output.scoped_unused.is_empty()
        && candidates.tailwind_arbitrary_values.is_empty()
        && candidates.cva_duplicate_variant_blocks.is_empty()
        && candidates.cva_variant_token_drifts.is_empty()
        && candidates.unresolved_class_references.is_empty()
        && candidates.unreferenced_css_classes.is_empty()
        && metrics.unused_font_faces.is_empty()
        && candidates.unused_theme_tokens.is_empty()
        && candidates.near_duplicate_theme_tokens.is_empty()
        && candidates.near_duplicate_css_in_js_tokens.is_empty()
        && token_consumers.is_empty()
}

fn sort_raw_style_values(values: &mut [fallow_output::RawStyleValue]) {
    values.sort_by(|a, b| {
        (&a.path, a.line, &a.axis, &a.property, &a.value).cmp(&(
            &b.path,
            b.line,
            &b.axis,
            &b.property,
            &b.value,
        ))
    });
}

struct CssReportChangedScopeInput<'a> {
    output: &'a mut CssReportOutput,
    metrics: &'a mut CssTokenMetrics,
    candidates: &'a mut MarkupCssCandidates,
    token_consumers: &'a mut Vec<fallow_output::TokenConsumers>,
    config: &'a ResolvedConfig,
    changed: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
}

fn retain_css_report_changed_scope(input: CssReportChangedScopeInput<'_>) {
    let CssReportChangedScopeInput {
        output,
        metrics,
        candidates,
        token_consumers,
        config,
        changed,
    } = input;
    let in_scope = |path: &str| css_output_path_in_changed_scope(path, config, changed);
    output.file_reports.retain(|file| in_scope(&file.path));
    output.scoped_unused.retain(|item| in_scope(&item.path));
    retain_css_metrics_changed_scope(metrics, &in_scope);
    retain_markup_candidates_changed_scope(candidates, &in_scope);
    output.raw_style_values.retain(|item| in_scope(&item.path));
    token_consumers.retain(|item| in_scope(&item.definition_path));
}

fn retain_css_metrics_changed_scope(
    metrics: &mut CssTokenMetrics,
    in_scope: &impl Fn(&str) -> bool,
) {
    metrics
        .unreferenced_keyframes
        .retain(|item| in_scope(&item.path));
    metrics
        .undefined_keyframes
        .retain(|item| in_scope(&item.path));
    metrics.duplicate_declaration_blocks.retain_mut(|block| {
        let has_scoped_occurrence = block.occurrences.iter().any(|item| in_scope(&item.path));
        if has_scoped_occurrence {
            block.occurrences.sort_by(|a, b| {
                let a_out_of_scope = !in_scope(&a.path);
                let b_out_of_scope = !in_scope(&b.path);
                a_out_of_scope
                    .cmp(&b_out_of_scope)
                    .then_with(|| a.path.cmp(&b.path))
                    .then_with(|| a.line.cmp(&b.line))
            });
        }
        has_scoped_occurrence
    });
    metrics.unused_at_rules.retain(|item| in_scope(&item.path));
    metrics
        .unused_font_faces
        .retain(|item| in_scope(&item.path));
}

fn retain_markup_candidates_changed_scope(
    candidates: &mut MarkupCssCandidates,
    in_scope: &impl Fn(&str) -> bool,
) {
    candidates
        .tailwind_arbitrary_values
        .retain(|item| in_scope(&item.path));
    candidates
        .cva_duplicate_variant_blocks
        .retain(|item| item.occurrences.iter().any(|occ| in_scope(&occ.path)));
    candidates
        .cva_variant_token_drifts
        .retain(|item| in_scope(&item.path));
    candidates
        .unresolved_class_references
        .retain(|item| in_scope(&item.path));
    candidates
        .unreferenced_css_classes
        .retain(|item| in_scope(&item.path));
    candidates
        .unused_theme_tokens
        .retain(|item| in_scope(&item.path));
    candidates
        .near_duplicate_theme_tokens
        .retain(|item| in_scope(&item.path));
    candidates
        .near_duplicate_css_in_js_tokens
        .retain(|item| in_scope(&item.path));
}

fn css_output_path_in_changed_scope(
    path: &str,
    config: &ResolvedConfig,
    changed: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> bool {
    let relative = std::path::Path::new(path);
    let absolute = config.root.join(relative);
    changed.contains(relative) || changed.contains(&absolute)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep token-consumer assertions concise"
)]
mod token_consumer_tests;
