//! CSS analytics execution for `fallow health`.

use fallow_config::ResolvedConfig;

use super::package_json::{
    class_matches_dependency_prefix, dependency_class_prefixes, project_uses_tailwind,
    project_uses_tailwind_plugin, published_css_paths,
};
use super::runtime_filter::relative_to_root;
use super::tailwind_theme;

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
        self.colors.extend(analytics.colors.iter().cloned());
        self.font_sizes.extend(analytics.font_sizes.iter().cloned());
        self.z_indexes.extend(analytics.z_indexes.iter().cloned());
        self.box_shadows
            .extend(analytics.box_shadows.iter().cloned());
        self.border_radii
            .extend(analytics.border_radii.iter().cloned());
        self.line_heights
            .extend(analytics.line_heights.iter().cloned());
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
        for block in &analytics.declaration_blocks {
            self.declaration_blocks
                .entry(block.fingerprint)
                .or_insert_with(|| (block.declaration_count, Vec::new()))
                .1
                .push((rel.to_owned(), block.line));
        }
        for name in &analytics.registered_custom_properties {
            self.registered_custom_props.insert(name.clone());
            self.property_registrars
                .entry(name.clone())
                .or_insert_with(|| rel.to_owned());
        }
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
                .difference(&self.referenced_keyframes)
                .count(),
        );
        summary.keyframes_undefined = saturate_len(
            self.referenced_keyframes
                .difference(&self.defined_keyframes)
                .count(),
        );

        // @keyframes are low-cardinality, so BOTH directions are located (not
        // just counted): defined-but-unused, and used-but-defined-nowhere.
        let unreferenced_keyframes = locate_keyframe_diff(
            &self.defined_keyframes,
            &self.referenced_keyframes,
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
            &self.referenced_keyframes,
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

fn scan_cva_duplicate_variant_blocks(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
) -> Vec<fallow_output::CvaDuplicateVariantBlock> {
    let mut blocks: rustc_hash::FxHashMap<String, Vec<fallow_output::CssBlockOccurrence>> =
        rustc_hash::FxHashMap::default();
    for file in files {
        let Some((rel, source)) = read_js_style_scan_source(file, ctx) else {
            continue;
        };
        if !source_contains_cva_variants(&source) {
            continue;
        }
        for (value, line) in collect_cva_class_blocks(&source) {
            blocks
                .entry(value)
                .or_default()
                .push(fallow_output::CssBlockOccurrence {
                    path: rel.clone(),
                    line,
                });
        }
    }
    let mut out: Vec<_> = blocks
        .into_iter()
        .filter_map(|(value, mut occurrences)| {
            if occurrences.len() < 2 {
                return None;
            }
            occurrences.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
            let occurrence_count = saturate_len(occurrences.len());
            Some(fallow_output::CvaDuplicateVariantBlock {
                value,
                occurrence_count,
                occurrences,
                actions: vec![fallow_output::CssCandidateAction::consolidate_block(
                    occurrence_count,
                )],
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.occurrence_count
            .cmp(&a.occurrence_count)
            .then_with(|| {
                let a_key = a
                    .occurrences
                    .first()
                    .map_or(("", 0), |occ| (occ.path.as_str(), occ.line));
                let b_key = b
                    .occurrences
                    .first()
                    .map_or(("", 0), |occ| (occ.path.as_str(), occ.line));
                a_key.cmp(&b_key)
            })
            .then_with(|| a.value.cmp(&b.value))
    });
    out
}

fn scan_cva_variant_token_drifts(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    token_candidates: &[ComparableThemeTokenCandidate],
) -> Vec<fallow_output::CvaVariantTokenDrift> {
    if token_candidates.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, u32, String, String)> =
        rustc_hash::FxHashSet::default();
    for file in files {
        let Some((rel, source)) = read_js_style_scan_source(file, ctx) else {
            continue;
        };
        if !source_contains_cva_variants(&source) {
            continue;
        }
        for (variant_classes, line) in collect_cva_class_blocks(&source) {
            for arbitrary in crate::css::scan_tailwind_arbitrary_values(&variant_classes) {
                let Some((namespace, value, metric)) = cva_arbitrary_value_metric(&arbitrary.value)
                else {
                    continue;
                };
                let Some((nearest, distance)) =
                    nearest_styling_token(namespace, &metric, token_candidates)
                else {
                    continue;
                };
                let key = (
                    rel.clone(),
                    line,
                    arbitrary.value.clone(),
                    nearest.token.clone(),
                );
                if !seen.insert(key) {
                    continue;
                }
                out.push(fallow_output::CvaVariantTokenDrift {
                    class_token: arbitrary.value.clone(),
                    value: value.clone(),
                    variant_classes: variant_classes.clone(),
                    path: rel.clone(),
                    line,
                    nearest_token: fallow_output::NearestStylingToken {
                        name: nearest.token.clone(),
                        value: nearest.value.clone(),
                        path: nearest.path.clone(),
                        line: nearest.line,
                        distance: round_distance(distance),
                    },
                    actions: vec![
                        fallow_output::CssCandidateAction::replace_cva_variant_arbitrary_value(
                            &arbitrary.value,
                            &nearest.token,
                        ),
                    ],
                });
            }
        }
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class_token.cmp(&b.class_token))
            .then_with(|| a.nearest_token.name.cmp(&b.nearest_token.name))
    });
    out
}

fn cva_arbitrary_value_metric(
    class_token: &str,
) -> Option<(&'static str, String, ThemeTokenMetric)> {
    let marker = "-[";
    let start = class_token.find(marker)?;
    let value_start = start + marker.len();
    let raw = class_token.get(value_start..class_token.len().checked_sub(1)?)?;
    let value = raw.replace('_', " ");
    let prefix = class_token.get(..start)?;
    let namespace = match prefix {
        "bg" | "border" | "fill" | "stroke" | "ring" | "outline" | "decoration" | "accent"
        | "caret" | "from" | "via" | "to" => "color",
        "text" if parse_theme_token_metric("color", &value).is_some() => "color",
        "text" => "text",
        "rounded" => "radius",
        "shadow" => "shadow",
        _ if prefix.starts_with("rounded-") => "radius",
        _ if prefix.starts_with("shadow-") => "shadow",
        _ => return None,
    };
    let metric = parse_theme_token_metric(namespace, &value)?;
    Some((namespace, value, metric))
}

fn nearest_styling_token<'a>(
    namespace: &str,
    metric: &ThemeTokenMetric,
    candidates: &'a [ComparableThemeTokenCandidate],
) -> Option<(&'a ComparableThemeTokenCandidate, f64)> {
    candidates
        .iter()
        .filter(|candidate| candidate.namespace == namespace)
        .filter_map(|candidate| {
            let distance = metric.distance(&candidate.metric)?;
            (distance <= metric.threshold()).then_some((candidate, distance))
        })
        .min_by(|(left, left_distance), (right, right_distance)| {
            left_distance
                .total_cmp(right_distance)
                .then_with(|| theme_token_sort_key(left).cmp(&theme_token_sort_key(right)))
        })
}

fn read_js_style_scan_source(
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
    if !matches!(extension, Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs")) {
        return None;
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".d.ts"))
    {
        return None;
    }
    let path_text = path.to_string_lossy();
    if path_text.contains("__tests__")
        || path_text.contains("/test/")
        || path_text.contains("/tests/")
        || path_text.contains(".test.")
        || path_text.contains(".spec.")
    {
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

fn source_contains_cva_variants(source: &str) -> bool {
    source.contains("cva(")
        && source.contains("variants")
        && (source.contains("class-variance-authority") || source.contains("styled-system"))
}

fn collect_cva_class_blocks(source: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(rel) = source[search..].find("cva(") {
        let start = search + rel;
        search = start + 4;
        if start > 0 && is_identifier_byte(source.as_bytes()[start - 1]) {
            continue;
        }
        let Some(end) = scan_call_end(source, start + 3) else {
            continue;
        };
        let base_line = source[..start].bytes().filter(|b| *b == b'\n').count() as u32 + 1;
        collect_quoted_cva_class_blocks(&source[start..end], base_line, &mut out);
    }
    out
}

fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn scan_call_end(source: &str, open_paren: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = open_paren;
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if matches!(b, b'\'' | b'"' | b'`') {
            quote = Some(b);
            i += 1;
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn collect_quoted_cva_class_blocks(source: &str, base_line: u32, out: &mut Vec<(String, u32)>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut line = base_line;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line = line.saturating_add(1);
            i += 1;
            continue;
        }
        if !matches!(b, b'\'' | b'"' | b'`') {
            i += 1;
            continue;
        }
        let quote = b;
        let start_line = line;
        i += 1;
        let start = i;
        let mut escaped = false;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\n' {
                line = line.saturating_add(1);
            }
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if c == b'\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if c == quote {
                if let Some(block) = normalize_cva_class_block(&source[start..i]) {
                    out.push((block, start_line));
                }
                i += 1;
                break;
            }
            i += 1;
        }
    }
}

fn normalize_cva_class_block(value: &str) -> Option<String> {
    let tokens: Vec<_> = value.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let class_like = tokens
        .iter()
        .filter(|token| {
            token.contains('-')
                || token.contains(':')
                || token.contains('[')
                || token.contains('/')
                || matches!(
                    **token,
                    "flex" | "grid" | "block" | "inline-flex" | "hidden"
                )
        })
        .count();
    (class_like >= 2).then(|| tokens.join(" "))
}

/// True for a byte that can appear inside a Tailwind class token (used to anchor
/// the `animate-` prefix at a token boundary so `xanimate-` does not match).
fn is_tailwind_class_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Extract `@keyframes` names applied via Tailwind from one source string: the
/// custom-ident after `animate-[<name>_...]` (arbitrary value, up to the first
/// `_`/`]`) and after a bare `animate-<name>` utility. The `animate-` prefix must
/// sit at a token boundary. Names are collected raw; the caller filters them to
/// actually-defined keyframes.
fn collect_animate_keyframe_names(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    const PREFIX: &str = "animate-";
    let mut search = 0;
    while let Some(rel) = source[search..].find(PREFIX) {
        let start = search + rel;
        search = start + PREFIX.len();
        // The prefix must start at a token boundary (`hover:animate-x` is fine,
        // `myanimate-x` is not).
        if start > 0 && is_tailwind_class_byte(bytes[start - 1]) {
            continue;
        }
        let after = start + PREFIX.len();
        if after >= bytes.len() {
            continue;
        }
        if bytes[after] == b'[' {
            // Arbitrary value: `animate-[badge-pop_0.5s_...]` -> `badge-pop`.
            let name_start = after + 1;
            let mut j = name_start;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'-' || c.is_ascii_alphanumeric() {
                    j += 1;
                } else {
                    break;
                }
            }
            if j > name_start {
                out.insert(source[name_start..j].to_owned());
            }
        } else {
            // Named utility: `animate-bar-fill` -> `bar-fill`.
            let mut j = after;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'-' || c.is_ascii_lowercase() || c.is_ascii_digit() {
                    j += 1;
                } else {
                    break;
                }
            }
            let name = source[after..j].trim_end_matches('-');
            if !name.is_empty() {
                out.insert(name.to_owned());
            }
        }
    }
}

/// Collect `@keyframes` names applied via Tailwind markup utilities
/// (`animate-[name_...]` / `animate-name`) across the project's markup and JS,
/// so a keyframe used only that way (never via a CSS `animation:` declaration)
/// is not wrongly flagged `unreferenced`. Not gated on the Tailwind dependency:
/// the `animate-[...]` / `animate-<name>` shapes are distinctive, the caller
/// filters the result to actually-defined keyframes, and a project can apply
/// Tailwind utilities without declaring the npm dep at the scanned root
/// (CDN / PostCSS / monorepo subpackage).
fn collect_markup_keyframe_references(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    let mut out: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(
            extension,
            Some("jsx" | "tsx" | "html" | "astro" | "vue" | "svelte" | "js" | "ts" | "mjs" | "cjs")
        ) {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_animate_keyframe_names(&source, &mut out);
            // Also a keyframe named in a JS inline-style `animation:` /
            // `animationName:` string (`animation: 'progress-indeterminate 1.5s'`)
            // appears as a dashed token in a quoted string; the caller filters
            // these to actually-defined keyframes, so an unrelated dashed token
            // can never manufacture a reference. `require_dash: false` so a
            // single-word keyframe name (`spin`, `jsanim`) is credited too.
            collect_quoted_class_tokens(&source, &mut out, false);
        }
    }
    out
}

/// Shortest authored CSS class that can be a credible typo target. Below this a
/// one-edit near miss is too likely to be a coincidental collision between two
/// short real words (`catch` vs `match`, `list` vs `last`) rather than a typo.
/// Real component-class typos are compound / hyphenated and comfortably longer.
/// (Real-world smoke on Svelte: `catch` vs `match` in test fixtures.)
const MIN_DEFINED_CLASS_LEN: usize = 6;
/// Shortest markup token worth typo-checking, for the same reason. One below the
/// defined floor, since a one-edit pair differs in length by at most one.
const MIN_TOKEN_LEN: usize = 5;

/// Count plain-CSS vs preprocessor (`.scss`/`.sass`/`.less`) stylesheet files in
/// the project (ignore-filtered). Used to abstain from class-typo detection when
/// preprocessors dominate, because the parser cannot expand their loops/mixins,
/// so the defined-class set is unreliable.
fn count_stylesheet_kinds(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> (usize, usize) {
    let mut css = 0usize;
    let mut preprocessor = 0usize;
    for file in files {
        let path = &file.path;
        let kind = match path.extension().and_then(|ext| ext.to_str()) {
            Some("css") => &mut css,
            Some("scss" | "sass" | "less") => &mut preprocessor,
            _ => continue,
        };
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        *kind += 1;
    }
    (css, preprocessor)
}

/// Collect every authored CSS class name defined anywhere in the project (plain
/// and module `.css`/`.scss`, plus Astro/SFC `<style>` blocks of any scoping). The set
/// is the typo-suggestion target for [`scan_unresolved_class_references`], so it
/// is NOT narrowed by `changed_files` / `ws_roots`: a class defined in an
/// unchanged file must still count as defined, or a markup token referencing it
/// would false-positive as unresolved. Only the ignore filter applies.
fn collect_defined_css_classes(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    use fallow_types::extract::ExportName;
    let mut defined: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        let is_preprocessor = matches!(extension, Some("scss" | "sass" | "less"));
        let is_css = extension == Some("css") || is_preprocessor;
        let has_style_blocks = matches!(extension, Some("astro" | "vue" | "svelte"));
        if !is_css && !has_style_blocks {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if has_style_blocks {
            for style in crate::css::extract_sfc_styles(&source) {
                let is_style_scss = style
                    .lang
                    .as_deref()
                    .is_some_and(|lang| matches!(lang, "scss" | "sass"));
                for export in crate::css::extract_css_module_exports(&style.body, is_style_scss) {
                    if let ExportName::Named(name) = export.name {
                        defined.insert(name);
                    }
                }
            }
            continue;
        }
        for export in crate::css::extract_css_module_exports(&source, is_preprocessor) {
            if let ExportName::Named(name) = export.name {
                defined.insert(name);
            }
        }
    }
    defined
}

/// Find the best one-edit typo suggestion for a markup token among the defined
/// classes, using a length-bucketed index so only classes of length `len-1`,
/// `len`, `len+1` are compared. Returns the lexicographically smallest defined
/// class at edit distance one (deterministic), or `None`.
fn best_class_suggestion<'a>(
    token: &str,
    by_len: &'a rustc_hash::FxHashMap<usize, Vec<&'a str>>,
) -> Option<&'a str> {
    let len = token.len();
    let mut best: Option<&str> = None;
    for candidate_len in [len.wrapping_sub(1), len, len + 1] {
        let Some(bucket) = by_len.get(&candidate_len) else {
            continue;
        };
        for &defined in bucket {
            if defined.len() < MIN_DEFINED_CLASS_LEN {
                continue;
            }
            if crate::css::is_typo_edit(token, defined)
                && best.is_none_or(|current| defined < current)
            {
                best = Some(defined);
            }
        }
    }
    best
}

/// True when a markup class token is Tailwind-flavored (a variant prefix `:`,
/// an opacity `/`, or an arbitrary-value bracket), so it is not an authored CSS
/// class and never a typo candidate.
fn is_tailwind_shaped(token: &str) -> bool {
    token.contains([':', '/', '[', ']'])
}

/// Length-bucketed index over the typo-target classes for O(1)-ish near-miss.
/// Drops names ending in `-` / `_`: those are SCSS interpolation artifacts
/// (`.display-#{$i}` parsed by lightningcss as a partial `display-`), never a
/// real typo target.
fn build_typo_target_index(
    defined: &rustc_hash::FxHashSet<String>,
) -> rustc_hash::FxHashMap<usize, Vec<&str>> {
    let mut by_len: rustc_hash::FxHashMap<usize, Vec<&str>> = rustc_hash::FxHashMap::default();
    for class in defined {
        if class.len() >= MIN_DEFINED_CLASS_LEN && !class.ends_with('-') && !class.ends_with('_') {
            by_len.entry(class.len()).or_default().push(class.as_str());
        }
    }
    by_len
}

/// Collect the likely-typo class references in one markup source into `out`,
/// deduping by `(rel, line, value)` via `seen`.
fn collect_unresolved_class_refs_in_file<'a>(
    source: &str,
    rel: &str,
    defined: &rustc_hash::FxHashSet<String>,
    by_len: &'a rustc_hash::FxHashMap<usize, Vec<&'a str>>,
    seen: &mut rustc_hash::FxHashSet<(String, u32, String)>,
    out: &mut Vec<fallow_output::UnresolvedClassReference>,
) {
    use fallow_output::{CssCandidateAction, UnresolvedClassReference};
    for token in crate::css::scan_markup_class_tokens(source).static_tokens {
        if token.value.len() < MIN_TOKEN_LEN
            || is_tailwind_shaped(&token.value)
            || defined.contains(&token.value)
        {
            continue;
        }
        let Some(suggestion) = best_class_suggestion(&token.value, by_len) else {
            continue;
        };
        let key = (rel.to_owned(), token.line, token.value.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(UnresolvedClassReference {
            actions: vec![CssCandidateAction::verify_unresolved_class(
                &token.value,
                suggestion,
            )],
            class: token.value,
            suggestion: suggestion.to_owned(),
            path: rel.to_owned(),
            line: token.line,
        });
    }
}

/// Scan markup for static `class` / `className` tokens that match no defined CSS
/// class but are one edit from a defined class (a likely typo / stale rename).
/// The defined set is the full project; markup honors the ignore / changed /
/// workspace filters (a typo is local). Near-zero false-positive by the near-miss
/// restriction: Tailwind utilities and third-party classes are not one edit from
/// an authored class. Candidates, never gated.
fn scan_unresolved_class_references(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    summary: &mut fallow_output::CssAnalyticsSummary,
) -> Vec<fallow_output::UnresolvedClassReference> {
    let HealthScanCtx {
        config, ignore_set, ..
    } = ctx;

    use fallow_output::UnresolvedClassReference;

    // Abstain on preprocessor-dominant projects. lightningcss parses `.scss` /
    // `.sass` / `.less` source textually but cannot expand loops / mixins, so a
    // generated class (`.bg-#{$color}`, `.col-#{$i}`) is invisible to the defined
    // set. On a SCSS framework like Bootstrap that makes a real, used class
    // (`bg-white`) look unresolved and false-positive as a typo of a parsed
    // sibling. When preprocessor stylesheets outnumber plain CSS, the defined set
    // is too incomplete to trust, so emit nothing (real-world smoke: Bootstrap).
    let (css_files, preprocessor_files) = count_stylesheet_kinds(files, config, ignore_set);
    summary.preprocessor_stylesheets = saturate_len(preprocessor_files);
    if preprocessor_files > css_files {
        summary.preprocessor_reachability_abstained = true;
        return Vec::new();
    }

    let defined = collect_defined_css_classes(files, config, ignore_set);
    if defined.is_empty() {
        return Vec::new();
    }
    let by_len = build_typo_target_index(&defined);

    let mut out: Vec<UnresolvedClassReference> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, u32, String)> = rustc_hash::FxHashSet::default();
    for file in files {
        let Some((rel, source)) = read_markup_scan_source(file, ctx) else {
            continue;
        };
        collect_unresolved_class_refs_in_file(
            &source, &rel, &defined, &by_len, &mut seen, &mut out,
        );
    }

    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class.cmp(&b.class))
    });
    summary.unresolved_class_references = saturate_len(out.len());
    out
}

/// Blank every `@font-face { ... }` block in a (lowercased) source so a declared
/// family's own `font-family:` inside its definition does not self-credit when
/// the source is scanned for OTHER references to that family. The `@font-face`,
/// `{`, and `}` boundaries are ASCII, so replacing the whole block range with
/// spaces preserves UTF-8 validity (any multi-byte family name inside the block
/// is fully within the replaced range).
fn mask_font_face_blocks(lower_source: &str) -> String {
    if !lower_source.contains("@font-face") {
        return lower_source.to_owned();
    }
    let mut bytes = lower_source.as_bytes().to_vec();
    let sb = lower_source.as_bytes();
    let mut search = 0;
    while let Some(rel) = lower_source[search..].find("@font-face") {
        let start = search + rel;
        let Some(brace_rel) = lower_source[start..].find('{') else {
            break;
        };
        let mut depth = 0usize;
        let mut j = start + brace_rel;
        while j < sb.len() {
            match sb[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let end = (j + 1).min(bytes.len());
        for b in &mut bytes[start..end] {
            *b = b' ';
        }
        search = end;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| lower_source.to_owned())
}

/// Of the candidate unused `@font-face` families, the subset whose name appears
/// as a substring in some other source file (`.css`/`.scss`/`.sass`/`.less`,
/// JS/TS, or markup), OUTSIDE its own `@font-face` block. Such a family is
/// applied somewhere the structural `font-family` reference set cannot see (a
/// Tailwind v4 `--font-*` theme token in a `@theme` block lightningcss skips, a
/// `.scss` theme, a canvas/JS `fontFamily` assignment, an inline style), so it
/// is NOT dead.
fn font_families_referenced_in_source(
    candidates: &[fallow_output::UnusedFontFace],
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    // `(original-case family, lowercase family)`; the lowercase form drives the
    // substring test because CSS font-family names are case-insensitive, while the
    // original case is what gets returned for the caller's retain.
    let mut pending: Vec<(String, String)> = candidates
        .iter()
        .map(|c| (c.family.clone(), c.family.to_ascii_lowercase()))
        .collect();
    let mut found: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for file in files {
        if pending.is_empty() {
            break;
        }
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(
            extension,
            Some(
                "css"
                    | "scss"
                    | "sass"
                    | "less"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "mjs"
                    | "cjs"
                    | "vue"
                    | "svelte"
                    | "astro"
                    | "html"
                    | "mdx"
            )
        ) {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        // `.css` is scanned too: a family can be referenced via a custom-property
        // value (a Tailwind v4 `--font-*` theme token, which lives inside a
        // `@theme` block that lightningcss skips, so the structural reference set
        // never sees it). The family's OWN `@font-face` definition is masked so it
        // does not self-credit (every declared family appears in its own block).
        let source_lower = mask_font_face_blocks(&source.to_ascii_lowercase());
        pending.retain(|(family, family_lower)| {
            if source_lower.contains(family_lower.as_str()) {
                found.insert(family.clone());
                false
            } else {
                true
            }
        });
    }
    found
}

/// Shortest global class worth reporting as unreferenced. Shorter names are
/// substring-prone (their literal appears inside many longer strings, so the
/// substring reference check already keeps them safe) and low-signal.
const MIN_UNREF_CLASS_LEN: usize = 5;

/// Extract class-shaped tokens from quoted string literals (`'...'` / `"..."` /
/// `` `...` ``) in a source string and add them to `out`, crediting a name
/// applied outside a `class=` / `className=` attribute (a config-object
/// `className: 'leveret-toast'`, a helper `return "x-y"`, a JS inline-style
/// `animation: 'progress-indeterminate 1s'`).
///
/// `require_dash` controls strictness. For CLASS crediting it is `true`: only
/// compound (dash-bearing) tokens are taken, so a generic single word never
/// coincidentally credits a class and breaks the whole-sheet abstain that
/// protects classes used in a surface fallow cannot read (Phoenix `.heex`). For
/// KEYFRAME crediting it is `false` (the caller filters to actually-defined
/// keyframes, so over-extraction is inert), letting a single-word keyframe name
/// (`spin`, `jsanim`) be credited from a JS `animation:` string too.
fn collect_quoted_class_tokens(
    source: &str,
    out: &mut rustc_hash::FxHashSet<String>,
    require_dash: bool,
) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote == b'"' || quote == b'\'' || quote == b'`' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if let Some(content) = source.get(start..j) {
                for token in content
                    .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                {
                    let shaped = token.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                        && !token.ends_with('-')
                        && (if require_dash {
                            token.contains('-')
                        } else {
                            token.len() >= 3
                        });
                    if shaped {
                        out.insert(token.to_owned());
                    }
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Class names wrapped in a CSS Modules `:global(...)` selector. Such a class is
/// applied by code OUTSIDE this stylesheet, most often a third-party library's
/// runtime DOM that the module styles via an escape hatch (an antd
/// `.validatiemeldingenModal :global(.ant-modal-header)` override). The project's
/// own markup never writes that class, so it can never be credited and would
/// always surface as a (false) unreferenced-class candidate. `:global` is the
/// author's explicit "not locally scoped, applied elsewhere" marker, so excluding
/// these from the candidate set is semantically correct, not a heuristic guess.
fn collect_global_scoped_classes(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(rel) = source[i..].find(":global(") {
        let open = i + rel + ":global(".len();
        // Balance parentheses so a `:global(:is(.a, .b))` still closes correctly.
        let mut depth = 1usize;
        let mut j = open;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let inner_end = j.saturating_sub(1).max(open);
        if let Some(inner) = source.get(open..inner_end) {
            extract_dotted_class_names(inner, out);
        }
        i = j.max(open + 1);
    }
}

/// Push every `.class` token in a CSS selector fragment (the bare name, no dot)
/// into `out`. A class name is a dot followed by `[A-Za-z_-]` then any run of
/// `[A-Za-z0-9_-]`.
fn extract_dotted_class_names(selector: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = selector.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let start = i + 1;
            if start < bytes.len()
                && (bytes[start].is_ascii_alphabetic() || matches!(bytes[start], b'_' | b'-'))
            {
                let mut j = start;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'_' | b'-'))
                {
                    j += 1;
                }
                if let Some(name) = selector.get(start..j) {
                    out.insert(name.to_owned());
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
}

/// Per-stylesheet located class definitions from STANDALONE `.css`/`.scss`/
/// `.sass`/`.less` files (not SFC `<style>` blocks, which are component-scoped
/// and covered by the scoped-unused check). Returns `(rel_path, [(class, 1-based
/// line)])`, each class deduped to its first definition. The defined surface for
/// the unreferenced-global-class candidate. Classes wrapped in `:global(...)`
/// are dropped: they target externally-applied DOM and are never authored in
/// markup.
fn collect_defined_css_classes_located(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> Vec<(String, Vec<(String, u32)>)> {
    use fallow_types::extract::ExportName;
    let mut out: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        let is_preprocessor = matches!(extension, Some("scss" | "sass" | "less"));
        if extension != Some("css") && !is_preprocessor {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut global_scoped: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        collect_global_scoped_classes(&source, &mut global_scoped);
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let mut classes: Vec<(String, u32)> = Vec::new();
        for export in crate::css::extract_css_module_exports(&source, is_preprocessor) {
            let ExportName::Named(name) = export.name else {
                continue;
            };
            // A `:global(.foo)` override targets DOM applied outside this module
            // (a third-party library's runtime markup), so it is never authored in
            // project markup and must not be an unreferenced-class candidate.
            if global_scoped.contains(&name) {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            let start = export.span.start as usize;
            let line = 1 + source
                .get(..start)
                .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
            classes.push((name, u32::try_from(line).unwrap_or(u32::MAX)));
        }
        if !classes.is_empty() {
            out.push((relative.to_string_lossy().replace('\\', "/"), classes));
        }
    }
    out
}

#[derive(Clone, Debug)]
struct CssClassInventory {
    css_files: usize,
    preprocessor_files: usize,
    defined_classes: Vec<(String, Vec<(String, u32)>)>,
}

fn css_class_inventory(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssClassInventory {
    let (css_files, preprocessor_files) = count_stylesheet_kinds(files, config, ignore_set);
    CssClassInventory {
        css_files,
        preprocessor_files,
        defined_classes: collect_defined_css_classes_located(files, config, ignore_set),
    }
}

/// Scan for global CSS classes referenced by NO in-project markup (the CSS
/// analogue of an unused export). Heavily gated to stay near-zero-false-positive:
///
/// - **Partial scope** (`changed_files` / `ws_roots`): abstain. A partial markup
///   view cannot prove a global class dead.
/// - **Preprocessor-dominant** (`.scss`/`.sass`/`.less` outnumber plain `.css`):
///   abstain. The parser cannot expand loops/mixins, so the markup-vs-CSS join
///   is unreliable.
/// - **Published surface**: a stylesheet reachable from `package.json` entries,
///   or whose classes are referenced by zero in-project markup (a design system
///   consumed elsewhere), abstains entirely.
/// - **Reference test** (panel gate 1): a class is referenced if it is a whole
///   static markup token OR a substring of any dynamic-class source, so a class
///   assembled from a `${...}` / `clsx(...)` fragment is never flagged.
fn scan_unreferenced_css_classes(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    summary: &mut fallow_output::CssAnalyticsSummary,
    reference_surface: Option<&CssReferenceSurface>,
    class_inventory: Option<&CssClassInventory>,
) -> Vec<fallow_output::UnreferencedCssClass> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files: _,
        ws_roots,
    } = ctx;

    use fallow_output::UnreferencedCssClass;

    // Partial scope cannot prove a global class dead.
    if changed_files.is_some() || ws_roots.is_some() {
        return Vec::new();
    }
    // Preprocessor-dominant projects have an unreliable defined/used join.
    let fallback_class_inventory;
    let class_inventory = if let Some(inventory) = class_inventory {
        inventory
    } else {
        fallback_class_inventory = css_class_inventory(files, config, ignore_set);
        &fallback_class_inventory
    };
    let css_files = class_inventory.css_files;
    let preprocessor_files = class_inventory.preprocessor_files;
    if preprocessor_files > css_files {
        return Vec::new();
    }

    let fallback_reference_surface;
    let reference_surface = if let Some(surface) = reference_surface {
        surface
    } else {
        fallback_reference_surface = css_reference_surface(files, config, ignore_set);
        &fallback_reference_surface
    };

    let published = published_css_paths(config);
    let dependency_prefixes = dependency_class_prefixes(config);

    let mut out: Vec<UnreferencedCssClass> = Vec::new();
    for (rel, classes) in &class_inventory.defined_classes {
        push_unreferenced_css_class_candidates(
            &mut out,
            rel,
            classes.clone(),
            &published,
            &dependency_prefixes,
            reference_surface,
        );
    }

    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class.cmp(&b.class))
    });
    summary.unreferenced_css_classes = saturate_len(out.len());
    out
}

#[derive(Clone, Debug)]
struct CssReferenceSurface {
    static_tokens: rustc_hash::FxHashSet<String>,
    dynamic_corpus: String,
    source_corpus: String,
    dynamic_interpolants: rustc_hash::FxHashSet<String>,
}

impl CssReferenceSurface {
    fn references(&self, class: &str) -> bool {
        self.static_tokens.contains(class)
            || class_name_occurrences(&self.dynamic_corpus, class)
                .next()
                .is_some()
            || self.css_module_property_referenced(class)
            || self.dynamic_prefix_referenced(class)
            || self.dynamic_literal_referenced(class)
    }

    fn css_module_property_referenced(&self, class: &str) -> bool {
        let Some(alias) = css_module_property_alias(class) else {
            return false;
        };
        self.source_corpus.contains(&format!(".{alias}"))
            || self.source_corpus.contains(&format!("['{alias}']"))
            || self.source_corpus.contains(&format!("[\"{alias}\"]"))
    }

    fn dynamic_prefix_referenced(&self, class: &str) -> bool {
        let Some(dash) = class.rfind('-') else {
            return false;
        };
        let head = &class[..=dash];
        const INTERP_MARKERS: [&str; 6] = ["${", "' +", "'+", "\" +", "\"+", "` +"];
        INTERP_MARKERS
            .iter()
            .any(|marker| self.dynamic_corpus.contains(&format!("{head}{marker}")))
    }

    fn dynamic_literal_referenced(&self, class: &str) -> bool {
        if !is_plain_dynamic_class_value(class) || self.dynamic_interpolants.is_empty() {
            return false;
        }
        class_literal_occurrences(&self.source_corpus, class).any(|offset| {
            let start = offset.saturating_sub(120);
            let end = self.source_corpus.len().min(offset + class.len() + 120);
            let Some(window) = self.source_corpus.get(start..end) else {
                return false;
            };
            let window = window.to_ascii_lowercase();
            self.dynamic_interpolants
                .iter()
                .any(|name| window.contains(&name.to_ascii_lowercase()))
        })
    }
}

fn css_module_property_alias(class: &str) -> Option<String> {
    if !class.contains('-') {
        return None;
    }
    let mut alias = String::with_capacity(class.len());
    let mut uppercase_next = false;
    for c in class.chars() {
        if c == '-' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            alias.extend(c.to_uppercase());
            uppercase_next = false;
        } else {
            alias.push(c);
        }
    }
    (alias != class && is_valid_js_property_ident(&alias)).then_some(alias)
}

fn is_valid_js_property_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
}

fn is_plain_dynamic_class_value(class: &str) -> bool {
    class.len() >= MIN_UNREF_CLASS_LEN
        && class
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn class_literal_occurrences<'a>(
    source: &'a str,
    class: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    source.match_indices(class).filter_map(move |(offset, _)| {
        let before = source.as_bytes().get(offset.wrapping_sub(1)).copied();
        let after = source.as_bytes().get(offset + class.len()).copied();
        match (before, after) {
            (Some(b'\''), Some(b'\'' | b',' | b';' | b')' | b']' | b'}'))
            | (Some(b'"'), Some(b'"' | b',' | b';' | b')' | b']' | b'}'))
            | (Some(b'`'), Some(b'`' | b',' | b';' | b')' | b']' | b'}')) => Some(offset),
            _ => None,
        }
    })
}

fn class_name_occurrences<'a>(source: &'a str, class: &'a str) -> impl Iterator<Item = usize> + 'a {
    source.match_indices(class).filter_map(move |(offset, _)| {
        let before = source.as_bytes().get(offset.wrapping_sub(1)).copied();
        let after = source.as_bytes().get(offset + class.len()).copied();
        if before.is_some_and(is_class_name_byte) || after.is_some_and(is_class_name_byte) {
            None
        } else {
            Some(offset)
        }
    })
}

fn is_class_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn collect_dynamic_class_interpolants(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = source.get(i..).and_then(|tail| tail.find("${")) {
        let start = i + rel + 2;
        let mut name_start = start;
        while bytes
            .get(name_start)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            name_start += 1;
        }
        let Some(first) = bytes.get(name_start).copied() else {
            break;
        };
        if !is_js_identifier_start(first) {
            i = start;
            continue;
        }
        let mut name_end = name_start + 1;
        while bytes
            .get(name_end)
            .is_some_and(|b| is_js_identifier_continue(*b))
        {
            name_end += 1;
        }
        let mut cursor = name_end;
        while bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'}') {
            out.insert(source[name_start..name_end].to_owned());
        }
        i = cursor.saturating_add(1);
    }
}

fn is_js_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_js_identifier_continue(byte: u8) -> bool {
    is_js_identifier_start(byte) || byte.is_ascii_digit()
}

fn css_reference_surface(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssReferenceSurface {
    let mut surface = CssReferenceSurface {
        static_tokens: rustc_hash::FxHashSet::default(),
        dynamic_corpus: String::new(),
        source_corpus: String::new(),
        dynamic_interpolants: rustc_hash::FxHashSet::default(),
    };
    for file in files {
        collect_css_reference_surface_file(&mut surface, file, config, ignore_set);
    }
    collect_markdown_reference_surface_files(&mut surface, config, ignore_set);
    surface
}

fn collect_css_reference_surface_file(
    surface: &mut CssReferenceSurface,
    file: &fallow_types::discover::DiscoveredFile,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    if !matches!(extension, Some("js" | "ts" | "mjs" | "cjs"))
        && !extension.is_some_and(is_markup_source_extension)
    {
        return;
    }
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    if ignore_set.is_match(relative) {
        return;
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    surface.source_corpus.push_str(&source);
    surface.source_corpus.push('\n');
    let is_markup_surface = extension.is_some_and(is_markup_source_extension);
    if !is_markup_surface {
        return;
    }
    let scan = crate::css::scan_markup_class_tokens(&source);
    for token in scan.static_tokens {
        surface.static_tokens.insert(token.value);
    }
    collect_quoted_class_tokens(&source, &mut surface.static_tokens, true);
    if scan.has_dynamic {
        collect_dynamic_class_interpolants(&source, &mut surface.dynamic_interpolants);
        surface.dynamic_corpus.push_str(&source);
        surface.dynamic_corpus.push('\n');
    }
}

fn collect_markdown_reference_surface_files(
    surface: &mut CssReferenceSurface,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    collect_markdown_reference_surface_dir(surface, &config.root, config, ignore_set);
}

fn collect_markdown_reference_surface_dir(
    surface: &mut CssReferenceSurface,
    dir: &std::path::Path,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(&config.root).unwrap_or(&path);
        if ignore_set.is_match(relative) || is_skipped_markdown_reference_path(relative) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_markdown_reference_surface_dir(surface, &path, config, ignore_set);
            continue;
        }
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(extension, Some("md" | "mdx")) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        surface.source_corpus.push_str(&source);
        surface.source_corpus.push('\n');
        let scan = crate::css::scan_markup_class_tokens(&source);
        for token in scan.static_tokens {
            surface.static_tokens.insert(token.value);
        }
        collect_quoted_class_tokens(&source, &mut surface.static_tokens, true);
        if scan.has_dynamic {
            collect_dynamic_class_interpolants(&source, &mut surface.dynamic_interpolants);
            surface.dynamic_corpus.push_str(&source);
            surface.dynamic_corpus.push('\n');
        }
    }
}

fn is_skipped_markdown_reference_path(relative: &std::path::Path) -> bool {
    relative.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        matches!(
            name.to_str(),
            Some(
                "node_modules"
                    | ".git"
                    | ".next"
                    | ".nuxt"
                    | ".svelte-kit"
                    | "dist"
                    | "build"
                    | "target"
                    | "coverage"
                    | ".turbo"
                    | ".cache"
            )
        )
    })
}

fn is_markup_source_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jsx" | "tsx" | "html" | "astro" | "vue" | "svelte" | "md" | "mdx"
    )
}

fn push_unreferenced_css_class_candidates(
    out: &mut Vec<fallow_output::UnreferencedCssClass>,
    rel: &str,
    classes: Vec<(String, u32)>,
    published: &rustc_hash::FxHashSet<String>,
    dependency_prefixes: &rustc_hash::FxHashSet<String>,
    reference_surface: &CssReferenceSurface,
) {
    use fallow_output::{CssCandidateAction, UnreferencedCssClass};

    if published.contains(rel)
        || !classes
            .iter()
            .any(|(class, _)| reference_surface.references(class))
    {
        return;
    }
    for (class, line) in classes {
        if class.len() >= MIN_UNREF_CLASS_LEN
            && !reference_surface.references(&class)
            && !class_matches_dependency_prefix(&class, dependency_prefixes)
        {
            out.push(UnreferencedCssClass {
                actions: vec![CssCandidateAction::verify_unreferenced_class(&class)],
                class,
                path: rel.to_string(),
                line,
            });
        }
    }
}

/// Source-file extensions scanned for Tailwind utility-class-shaped tokens when
/// crediting `@theme` token usage. Mirrors the font-family source scan (markup,
/// JS/TS className strings / `clsx` args / CSS-in-JS, preprocessor stylesheets)
/// but deliberately EXCLUDES plain `.css`, which would re-read the `@theme`
/// DEFINITION and self-credit every token.
const THEME_USAGE_SOURCE_EXTS: &[&str] = &[
    "scss", "sass", "less", "js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte", "astro",
    "html", "mdx",
];

/// Collect every Tailwind-utility-shaped token from `source` into `out`: a
/// maximal run of `[a-z0-9-]` that, with leading/trailing `-` trimmed, still
/// contains a `-` and starts with a lowercase letter. Captures `bg-brand`,
/// `rounded-card`, `text-2xl`, and the `color-brand` core of a
/// `var(--color-brand)` / `[--color-brand]` reference. Deliberately captures the
/// dashed SHAPE, never a bare word, so a dictionary-word theme name
/// (`brand`/`card`/`muted`) is credited only by a real `-<name>` utility suffix,
/// not by the word appearing anywhere in source.
fn collect_class_shaped_tokens(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            let start = i;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' {
                    i += 1;
                } else {
                    break;
                }
            }
            let tok = source[start..i].trim_matches('-');
            if tok.contains('-') && tok.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
                out.insert(tok.to_owned());
            }
        } else {
            i += 1;
        }
    }
}

/// Location-aware sibling of [`collect_class_shaped_tokens`]: appends every
/// Tailwind-utility-shaped token in `source` to `out` as `(token, rel, line)`.
fn collect_class_shaped_tokens_located(
    source: &str,
    rel: &str,
    out: &mut Vec<(String, String, u32)>,
) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            let start = i;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' {
                    i += 1;
                } else {
                    break;
                }
            }
            let tok = source[start..i].trim_matches('-');
            if tok.contains('-') && tok.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
                out.push((
                    tok.to_owned(),
                    rel.to_owned(),
                    line_at_offset(source, start),
                ));
            }
        } else {
            i += 1;
        }
    }
}

fn line_at_offset(source: &str, offset: usize) -> u32 {
    let count = source
        .get(..offset)
        .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
    u32::try_from(1 + count).unwrap_or(u32::MAX)
}

/// Tailwind v4 `@theme` design tokens (`--color-brand`, `--radius-card`) defined
/// in a stylesheet but used by no generated utility, `var()` read, `@apply`, or
/// arbitrary value anywhere in the project: dead design tokens (the
/// `unused-export` of the token era). Heavily gated to stay near-zero-false-
/// positive (panel BLOCKs):
///
/// - **Partial scope** (`changed_files` / `ws_roots`): abstain. A partial view
///   cannot prove a token dead.
/// - **v4 gate**: emit only when the project declares a `tailwindcss` dependency
///   AND at least one `@theme` token was found.
/// - **Tailwind plugin** (`@plugin` / config `plugins[]`): abstain. A plugin can
///   consume tokens invisibly to the scan (the DI blind spot).
/// - **Published library**: a token defined in a stylesheet that is a published
///   package surface is a public design-token API consumed downstream; skip it.
/// - **Variant namespaces** (`--breakpoint-*` / `--container-*`): excluded from
///   candidacy in this version. Crediting their `<name>:` / `@<name>:` variant
///   usage robustly needs a dedicated variant parser; a follow-up can add it.
///   (Acceptance criterion 7: excluded when the variant scan is not built.)
///
/// The usage test is false-negative-leaning by design: every check CREDITS usage,
/// so a genuinely-dead token is missed before a live one is flagged.
struct UnusedThemeTokenScanInput<'a> {
    tokens: &'a CssTokenSets,
    files: &'a [fallow_types::discover::DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    summary: &'a mut fallow_output::CssAnalyticsSummary,
}

/// A classified `@theme` token candidate (namespace + name + definition site)
/// surviving the variant / published-library / unknown-namespace filters.
struct ThemeTokenCandidate {
    token: String,
    namespace: String,
    name: String,
    value: String,
    path: String,
    line: u32,
}

/// Classify the project's `@theme` token definers, dropping variant namespaces,
/// published-library stylesheets, and anything outside a known namespace.
fn classify_theme_token_candidates(
    input: &UnusedThemeTokenScanInput<'_>,
) -> Vec<ThemeTokenCandidate> {
    classify_theme_token_candidates_from_tokens(input.tokens, input.config)
}

fn classify_theme_token_candidates_from_tokens(
    tokens: &CssTokenSets,
    config: &ResolvedConfig,
) -> Vec<ThemeTokenCandidate> {
    let published = published_css_paths(config);
    let mut candidates: Vec<ThemeTokenCandidate> = Vec::new();
    for (raw, definition) in &tokens.theme_token_definers {
        if published.contains(&definition.path) {
            continue;
        }
        let Some(classified) = tailwind_theme::classify(raw) else {
            continue;
        };
        if classified.is_variant {
            continue;
        }
        candidates.push(ThemeTokenCandidate {
            token: format!("--{raw}"),
            namespace: classified.namespace,
            name: classified.name,
            value: definition.value.clone(),
            path: definition.path.clone(),
            line: definition.line,
        });
    }
    candidates
}

/// Build the utility-shaped usage surface: every class-shaped token from `@apply`
/// bodies plus non-CSS source (markup class attributes, `clsx` args, CSS-in-JS).
fn collect_theme_usage_tokens(
    input: &UnusedThemeTokenScanInput<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut utility_tokens: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for apply in &input.tokens.apply_tokens {
        collect_class_shaped_tokens(apply, &mut utility_tokens);
    }
    for file in input.files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !extension.is_some_and(|ext| THEME_USAGE_SOURCE_EXTS.contains(&ext)) {
            continue;
        }
        let relative = path.strip_prefix(&input.config.root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_class_shaped_tokens(&source, &mut utility_tokens);
        }
    }
    utility_tokens
}

/// The `var()` read surface: CSS-side `@theme` reads plus referenced custom
/// properties (leading dashes trimmed to the property key form).
fn collect_theme_var_reads(tokens: &CssTokenSets) -> rustc_hash::FxHashSet<String> {
    let mut var_reads: rustc_hash::FxHashSet<String> = tokens.theme_var_reads.clone();
    for referenced in &tokens.referenced_custom_props {
        var_reads.insert(referenced.trim_start_matches('-').to_owned());
    }
    var_reads
}

fn scan_unused_theme_tokens(
    input: &mut UnusedThemeTokenScanInput<'_>,
) -> Vec<fallow_output::UnusedThemeToken> {
    use fallow_output::{CssCandidateAction, UnusedThemeToken};

    // Partial scope cannot prove a token dead.
    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    // v4 gate: a Tailwind dependency AND at least one @theme token present.
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    // Tailwind-plugin abstain (DI blind spot).
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    let candidates = classify_theme_token_candidates(input);
    if candidates.is_empty() {
        input.summary.unused_theme_tokens = 0;
        return Vec::new();
    }

    let utility_tokens = collect_theme_usage_tokens(input);
    let var_reads = collect_theme_var_reads(input.tokens);

    let mut out: Vec<UnusedThemeToken> = Vec::new();
    for candidate in candidates {
        let dash_name = format!("-{}", candidate.name);
        // The token's own custom-property key, used by the var() read test.
        let raw = candidate.token.trim_start_matches('-');
        let used = var_reads.contains(raw)
            || utility_tokens
                .iter()
                .any(|t| t.len() > dash_name.len() && t.ends_with(&dash_name));
        if used {
            continue;
        }
        out.push(UnusedThemeToken {
            actions: vec![CssCandidateAction::verify_unused_theme_token(
                &candidate.token,
                &candidate.namespace,
                &candidate.name,
            )],
            token: candidate.token,
            namespace: candidate.namespace,
            path: candidate.path,
            line: candidate.line,
        });
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.token.cmp(&b.token))
    });
    input.summary.unused_theme_tokens = saturate_len(out.len());
    out
}

const NEAR_DUPLICATE_COLOR_DISTANCE: f64 = 2.0;
const NEAR_DUPLICATE_LENGTH_DISTANCE_PX: f64 = 0.5;
const NEAR_DUPLICATE_DURATION_DISTANCE_MS: f64 = 10.0;
const NEAR_DUPLICATE_SHADOW_DISTANCE_PX: f64 = 1.0;

#[derive(Clone, Debug)]
struct ComparableThemeTokenCandidate {
    token: String,
    namespace: String,
    name: String,
    value: String,
    path: String,
    line: u32,
    metric: ThemeTokenMetric,
    origin: ComparableTokenOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComparableTokenOrigin {
    Explicit,
    ProjectVocabulary,
}

impl ComparableTokenOrigin {
    fn priority(self) -> u8 {
        match self {
            Self::Explicit => 0,
            Self::ProjectVocabulary => 1,
        }
    }
}

#[derive(Clone, Debug)]
enum ThemeTokenMetric {
    Color(OklabColor),
    LengthPx(f64),
    DurationMs(f64),
    ShadowPx(Vec<f64>),
}

impl ThemeTokenMetric {
    fn distance(&self, other: &Self) -> Option<f64> {
        match (self, other) {
            (Self::Color(left), Self::Color(right)) => Some(oklab_distance(*left, *right)),
            (Self::LengthPx(left), Self::LengthPx(right))
            | (Self::DurationMs(left), Self::DurationMs(right)) => Some((left - right).abs()),
            (Self::ShadowPx(left), Self::ShadowPx(right)) if left.len() == right.len() => Some(
                left.iter()
                    .zip(right)
                    .map(|(l, r)| {
                        let delta = l - r;
                        delta * delta
                    })
                    .sum::<f64>()
                    .sqrt(),
            ),
            _ => None,
        }
    }

    fn threshold(&self) -> f64 {
        match self {
            Self::Color(_) => NEAR_DUPLICATE_COLOR_DISTANCE,
            Self::LengthPx(_) => NEAR_DUPLICATE_LENGTH_DISTANCE_PX,
            Self::DurationMs(_) => NEAR_DUPLICATE_DURATION_DISTANCE_MS,
            Self::ShadowPx(_) => NEAR_DUPLICATE_SHADOW_DISTANCE_PX,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OklabColor {
    l: f64,
    a: f64,
    b: f64,
}

fn scan_near_duplicate_theme_tokens(
    input: &mut UnusedThemeTokenScanInput<'_>,
) -> Vec<fallow_output::NearDuplicateThemeToken> {
    use fallow_output::{CssCandidateAction, NearDuplicateThemeToken, NearestStylingToken};

    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    let mut candidates = comparable_theme_token_candidates(input.tokens, input.config);
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    if candidates.len() < 2 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let changed = input.output_changed_files;
    for candidate in &candidates {
        if let Some(changed) = changed
            && !css_output_path_in_changed_scope(&candidate.path, input.config, changed)
        {
            continue;
        }
        let nearest = find_nearest_duplicate_theme_token(candidate, &candidates, changed.is_some());

        let Some((nearest, distance)) = nearest else {
            continue;
        };
        let distance = round_distance(distance);
        let nearest_token = NearestStylingToken {
            name: nearest.token.clone(),
            value: nearest.value.clone(),
            path: nearest.path.clone(),
            line: nearest.line,
            distance,
        };
        out.push(NearDuplicateThemeToken {
            token: candidate.token.clone(),
            value: candidate.value.clone(),
            path: candidate.path.clone(),
            line: candidate.line,
            actions: vec![CssCandidateAction::replace_near_duplicate_token(
                &candidate.token,
                &nearest.token,
            )],
            nearest_token,
        });
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.token.cmp(&b.token))
    });
    input.summary.near_duplicate_theme_tokens = saturate_len(out.len());
    out
}

fn annotate_raw_style_value_nearest_tokens(
    tokens: &mut CssTokenSets,
    candidates: &[ComparableThemeTokenCandidate],
) {
    if tokens.raw_style_values.is_empty() || candidates.is_empty() {
        return;
    }
    let raw_value_counts = raw_style_value_counts(&tokens.raw_style_values);
    for raw in &mut tokens.raw_style_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        let Some(metric) = parse_theme_token_metric(namespace, &raw.value) else {
            continue;
        };
        let raw_value = normalize_theme_token_value(&raw.value);
        if namespace == "color" && color_value_has_alpha(&raw_value) {
            continue;
        }
        let raw_key = (namespace.to_string(), raw_value.clone());
        let raw_value_is_repeated = raw_value_counts.get(&raw_key).copied().unwrap_or(0) > 1;
        let nearest = candidates
            .iter()
            .filter(|candidate| candidate.namespace == namespace)
            .filter_map(|candidate| {
                if candidate.origin == ComparableTokenOrigin::ProjectVocabulary
                    && (raw_value == candidate.value || raw_value_is_repeated)
                {
                    return None;
                }
                let distance = metric.distance(&candidate.metric)?;
                (distance <= metric.threshold()).then_some((candidate, round_distance(distance)))
            })
            .min_by(|(left, left_distance), (right, right_distance)| {
                left_distance
                    .total_cmp(right_distance)
                    .then_with(|| left.origin.priority().cmp(&right.origin.priority()))
                    .then_with(|| theme_token_sort_key(left).cmp(&theme_token_sort_key(right)))
            });
        if let Some((nearest, distance)) = nearest {
            raw.nearest_token = Some(fallow_output::NearestStylingToken {
                name: nearest.token.clone(),
                value: nearest.value.clone(),
                path: nearest.path.clone(),
                line: nearest.line,
                distance,
            });
        }
    }
}

fn raw_style_value_counts(
    raw_values: &[fallow_output::RawStyleValue],
) -> rustc_hash::FxHashMap<(String, String), u32> {
    let mut counts = rustc_hash::FxHashMap::default();
    for raw in raw_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        *counts
            .entry((
                namespace.to_string(),
                normalize_theme_token_value(&raw.value),
            ))
            .or_insert(0) += 1;
    }
    counts
}

fn comparable_css_in_js_token_candidates(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
) -> Vec<ComparableThemeTokenCandidate> {
    if !project_uses_css_in_js(&config.root) {
        return Vec::new();
    }
    let path_by_id: rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path> =
        files.iter().map(|f| (f.id, f.path.as_path())).collect();
    let definers = collect_css_in_js_definers(modules, &path_by_id, config);
    let mut candidates = Vec::new();
    for definer in definers.entries {
        for leaf in definer.leaves {
            let Some(value) = leaf.value else {
                continue;
            };
            let Some(namespace) = css_in_js_token_namespace(definer.origin, &leaf.path) else {
                continue;
            };
            let Some(metric) = parse_theme_token_metric(namespace, &value) else {
                continue;
            };
            candidates.push(ComparableThemeTokenCandidate {
                token: format!("{}.{}", definer.binding, leaf.path),
                namespace: namespace.to_string(),
                name: leaf.path,
                value: normalize_theme_token_value(&value),
                path: definer.rel_path.clone(),
                line: leaf.def_line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            });
        }
    }
    candidates
}

fn css_in_js_token_namespace(
    origin: fallow_extract::CssInJsTokenOrigin,
    path: &str,
) -> Option<&'static str> {
    let first = path.split('.').next().unwrap_or(path);
    let normalized = first.to_ascii_lowercase();
    match origin {
        fallow_extract::CssInJsTokenOrigin::Panda => match normalized.as_str() {
            "colors" | "color" => Some("color"),
            "fontsizes" | "font-sizes" | "text" => Some("text"),
            "radii" | "radius" | "radiitokens" | "border-radii" => Some("radius"),
            "shadows" | "shadow" => Some("shadow"),
            _ => None,
        },
        _ => match normalized.as_str() {
            "color" | "colors" | "palette" => Some("color"),
            "fontsize" | "fontsizes" | "font-size" | "text" => Some("text"),
            "radius" | "radii" | "borderradius" | "border-radius" => Some("radius"),
            "shadow" | "shadows" | "boxshadow" | "box-shadow" => Some("shadow"),
            _ => None,
        },
    }
}

fn raw_style_token_namespace(axis: &str) -> Option<&'static str> {
    match axis {
        "color" => Some("color"),
        "font-size" => Some("text"),
        "radius" => Some("radius"),
        "shadow" => Some("shadow"),
        _ => None,
    }
}

fn comparable_custom_property_token_candidates(
    tokens: &CssTokenSets,
) -> Vec<ComparableThemeTokenCandidate> {
    tokens
        .custom_property_definers
        .iter()
        .filter_map(|(token, definition)| {
            let namespace = custom_property_token_namespace(token)?;
            let metric = parse_theme_token_metric(namespace, &definition.value)?;
            Some(ComparableThemeTokenCandidate {
                token: token.clone(),
                namespace: namespace.to_string(),
                name: token.trim_start_matches('-').to_owned(),
                value: normalize_theme_token_value(&definition.value),
                path: definition.path.clone(),
                line: definition.line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            })
        })
        .collect()
}

fn comparable_project_vocabulary_candidates(
    tokens: &CssTokenSets,
) -> Vec<ComparableThemeTokenCandidate> {
    let mut groups: rustc_hash::FxHashMap<(String, String), ProjectVocabularyValue> =
        rustc_hash::FxHashMap::default();
    for raw in &tokens.raw_style_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        let value = normalize_theme_token_value(&raw.value);
        if namespace == "color" && color_value_has_alpha(&value) {
            continue;
        }
        let Some(metric) = parse_theme_token_metric(namespace, &value) else {
            continue;
        };
        let key = (namespace.to_string(), value.clone());
        let entry = groups.entry(key).or_insert_with(|| ProjectVocabularyValue {
            namespace: namespace.to_string(),
            value,
            path: raw.path.clone(),
            line: raw.line,
            count: 0,
            metric,
        });
        entry.count += 1;
        if (raw.path.as_str(), raw.line) < (entry.path.as_str(), entry.line) {
            entry.path.clone_from(&raw.path);
            entry.line = raw.line;
        }
    }

    let mut candidates: Vec<ComparableThemeTokenCandidate> = groups
        .into_values()
        .filter(|value| value.count >= 2)
        .map(|value| ComparableThemeTokenCandidate {
            token: project_vocabulary_token_name(&value.namespace, &value.value),
            namespace: value.namespace.clone(),
            name: value.value.clone(),
            value: value.value,
            path: value.path,
            line: value.line,
            metric: value.metric,
            origin: ComparableTokenOrigin::ProjectVocabulary,
        })
        .collect();
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    candidates
}

#[derive(Clone, Debug)]
struct ProjectVocabularyValue {
    namespace: String,
    value: String,
    path: String,
    line: u32,
    count: u32,
    metric: ThemeTokenMetric,
}

fn project_vocabulary_token_name(namespace: &str, value: &str) -> String {
    let stable_value = value.split_whitespace().collect::<Vec<_>>().join("_");
    format!("project-vocabulary.{namespace}.{stable_value}")
}

fn color_value_has_alpha(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(hex) = trimmed.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 4 | 8)
}

fn custom_property_token_namespace(token: &str) -> Option<&'static str> {
    let key = token.trim_start_matches('-');
    if key.starts_with("color-") {
        Some("color")
    } else if key.starts_with("text-") || key.starts_with("font-size-") {
        Some("text")
    } else if key.starts_with("radius-") || key.starts_with("border-radius-") {
        Some("radius")
    } else if key.starts_with("shadow-") || key.starts_with("box-shadow-") {
        Some("shadow")
    } else {
        None
    }
}

fn comparable_theme_token_candidates(
    tokens: &CssTokenSets,
    config: &ResolvedConfig,
) -> Vec<ComparableThemeTokenCandidate> {
    classify_theme_token_candidates_from_tokens(tokens, config)
        .into_iter()
        .filter_map(|candidate| {
            let metric = parse_theme_token_metric(&candidate.namespace, &candidate.value)?;
            Some(ComparableThemeTokenCandidate {
                token: candidate.token,
                namespace: candidate.namespace,
                name: candidate.name,
                value: normalize_theme_token_value(&candidate.value),
                path: candidate.path,
                line: candidate.line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            })
        })
        .collect()
}

fn find_nearest_duplicate_theme_token<'a>(
    candidate: &'a ComparableThemeTokenCandidate,
    candidates: &'a [ComparableThemeTokenCandidate],
    include_later_tokens: bool,
) -> Option<(&'a ComparableThemeTokenCandidate, f64)> {
    candidates
        .iter()
        .filter(|other| other.token != candidate.token)
        .filter(|other| other.namespace == candidate.namespace)
        .filter(|other| {
            include_later_tokens || theme_token_sort_key(other) < theme_token_sort_key(candidate)
        })
        .filter(|other| {
            !theme_token_names_are_deliberate_pair(
                &candidate.namespace,
                &candidate.name,
                &other.name,
            )
        })
        .filter_map(|other| {
            let distance = candidate.metric.distance(&other.metric)?;
            if distance > 0.0 && distance <= candidate.metric.threshold() {
                Some((other, distance))
            } else {
                None
            }
        })
        .min_by(
            |(left_candidate, left_distance), (right_candidate, right_distance)| {
                left_distance
                    .partial_cmp(right_distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        theme_token_sort_key(left_candidate)
                            .cmp(&theme_token_sort_key(right_candidate))
                    })
            },
        )
}

fn theme_token_sort_key(candidate: &ComparableThemeTokenCandidate) -> (&str, u32, &str) {
    (&candidate.path, candidate.line, &candidate.token)
}

fn normalize_theme_token_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_theme_token_metric(namespace: &str, value: &str) -> Option<ThemeTokenMetric> {
    match namespace {
        "color" => fallow_extract::parse_css_color_rgb(value)
            .map(rgb_to_oklab)
            .map(ThemeTokenMetric::Color),
        "spacing" | "radius" | "text" => parse_length_px(value).map(ThemeTokenMetric::LengthPx),
        "duration" => parse_duration_ms(value).map(ThemeTokenMetric::DurationMs),
        "shadow" => parse_shadow_lengths_px(value).map(ThemeTokenMetric::ShadowPx),
        _ => None,
    }
}

fn parse_length_px(value: &str) -> Option<f64> {
    let (number, unit) = parse_number_with_unit(value.trim())?;
    match unit {
        "" if number == 0.0 => Some(0.0),
        "px" => Some(number),
        "rem" | "em" => Some(number * 16.0),
        _ => None,
    }
}

fn parse_duration_ms(value: &str) -> Option<f64> {
    let (number, unit) = parse_number_with_unit(value.trim())?;
    match unit {
        "ms" => Some(number),
        "s" => Some(number * 1000.0),
        _ => None,
    }
}

fn parse_shadow_lengths_px(value: &str) -> Option<Vec<f64>> {
    if value.contains(',') {
        return None;
    }
    let mut lengths = Vec::new();
    for part in value.split_whitespace() {
        let Some(length) = parse_length_px(part) else {
            break;
        };
        lengths.push(length);
    }
    if (2..=4).contains(&lengths.len()) {
        Some(lengths)
    } else {
        None
    }
}

fn parse_number_with_unit(value: &str) -> Option<(f64, &str)> {
    let split = value
        .char_indices()
        .find(|(idx, c)| *idx > 0 && !matches!(c, '0'..='9' | '.' | '+' | '-'))
        .map_or(value.len(), |(idx, _)| idx);
    let number = value[..split].parse::<f64>().ok()?;
    let unit = &value[split..];
    if number.is_finite() {
        Some((number, unit))
    } else {
        None
    }
}

#[expect(
    clippy::suboptimal_flops,
    reason = "OKLab conversion mirrors the reference matrix; mul_add obscures the coefficients."
)]
fn rgb_to_oklab((red, green, blue): (f64, f64, f64)) -> OklabColor {
    let linear_red = srgb_to_linear(red / 255.0);
    let linear_green = srgb_to_linear(green / 255.0);
    let linear_blue = srgb_to_linear(blue / 255.0);
    let long_cone = 0.412_221_470_8 * linear_red
        + 0.536_332_536_3 * linear_green
        + 0.051_445_992_9 * linear_blue;
    let medium_cone = 0.211_903_498_2 * linear_red
        + 0.680_699_545_1 * linear_green
        + 0.107_396_956_6 * linear_blue;
    let short_cone = 0.088_302_461_9 * linear_red
        + 0.281_718_837_6 * linear_green
        + 0.629_978_700_5 * linear_blue;
    let long_cone = long_cone.cbrt();
    let medium_cone = medium_cone.cbrt();
    let short_cone = short_cone.cbrt();
    OklabColor {
        l: 0.210_454_255_3 * long_cone + 0.793_617_785_0 * medium_cone
            - 0.004_072_046_8 * short_cone,
        a: 1.977_998_495_1 * long_cone - 2.428_592_205_0 * medium_cone
            + 0.450_593_709_9 * short_cone,
        b: 0.025_904_037_1 * long_cone + 0.782_771_766_2 * medium_cone
            - 0.808_675_766_0 * short_cone,
    }
}

fn srgb_to_linear(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[expect(
    clippy::suboptimal_flops,
    reason = "Distance formula is clearer in expanded Euclidean form."
)]
fn oklab_distance(left: OklabColor, right: OklabColor) -> f64 {
    let l = left.l - right.l;
    let a = left.a - right.a;
    let b = left.b - right.b;
    ((l * l + a * a + b * b).sqrt()) * 100.0
}

fn round_distance(distance: f64) -> f64 {
    (distance * 100.0).round() / 100.0
}

fn theme_token_names_are_deliberate_pair(namespace: &str, left: &str, right: &str) -> bool {
    if namespace == "color" && color_token_name_is_semantic_ui_role(left, right) {
        return true;
    }
    if let (Some((left_base, _)), Some((right_base, _))) =
        (split_numeric_suffix(left), split_numeric_suffix(right))
        && left_base == right_base
    {
        return true;
    }
    let state_suffixes = [
        "-hover",
        "-active",
        "-focus",
        "-disabled",
        "-pressed",
        "-selected",
    ];
    state_suffixes.iter().any(|suffix| {
        left.strip_suffix(suffix) == Some(right) || right.strip_suffix(suffix) == Some(left)
    })
}

fn color_token_name_is_semantic_ui_role(left: &str, right: &str) -> bool {
    const ROLES: &[&str] = &[
        "accent",
        "accent-foreground",
        "background",
        "border",
        "card",
        "card-foreground",
        "destructive",
        "destructive-foreground",
        "foreground",
        "input",
        "muted",
        "muted-foreground",
        "popover",
        "popover-foreground",
        "primary",
        "primary-foreground",
        "ring",
        "secondary",
        "secondary-foreground",
    ];
    ROLES.contains(&left) || ROLES.contains(&right)
}

fn split_numeric_suffix(name: &str) -> Option<(&str, &str)> {
    let split = name
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(idx, c)| idx + c.len_utf8())?;
    if split == name.len() {
        return None;
    }
    Some((&name[..split], &name[split..]))
}

/// Input for the location-aware reverse index of Tailwind v4 `@theme` token
/// consumers. The index is descriptive only and sets no summary count.
struct TokenConsumersInput<'a> {
    tokens: &'a CssTokenSets,
    files: &'a [fallow_types::discover::DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn collect_located_utility_consumers(
    input: &TokenConsumersInput<'_>,
) -> Vec<(String, String, u32)> {
    let mut located: Vec<(String, String, u32)> = Vec::new();
    for file in input.files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !extension.is_some_and(|ext| THEME_USAGE_SOURCE_EXTS.contains(&ext)) {
            continue;
        }
        let relative = path.strip_prefix(&input.config.root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        let rel = relative.to_string_lossy().replace('\\', "/");
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_class_shaped_tokens_located(&source, &rel, &mut located);
        }
    }
    located
}

fn build_token_consumers(input: &TokenConsumersInput<'_>) -> Vec<fallow_output::TokenConsumers> {
    use fallow_output::{
        ConsumerKind, TOKEN_CONSUMER_SAMPLE_CAP, TokenConsumerLocation, TokenConsumers,
    };

    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    let mut summary = fallow_output::CssAnalyticsSummary::default();
    let candidates = classify_theme_token_candidates(&UnusedThemeTokenScanInput {
        tokens: input.tokens,
        files: input.files,
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        output_changed_files: None,
        ws_roots: input.ws_roots,
        summary: &mut summary,
    });
    if candidates.is_empty() {
        return Vec::new();
    }

    let utility_located = collect_located_utility_consumers(input);

    let mut out: Vec<TokenConsumers> = candidates
        .into_iter()
        .map(|candidate| {
            let dash_name = format!("-{}", candidate.name);
            let raw = candidate.token.trim_start_matches('-').to_owned();
            let mut consumers: Vec<TokenConsumerLocation> = Vec::new();

            for (name, path, line) in &input.tokens.theme_var_reads_located {
                if *name == raw {
                    consumers.push(TokenConsumerLocation {
                        path: path.clone(),
                        line: *line,
                        kind: ConsumerKind::ThemeVar,
                    });
                }
            }
            for (name, path, line) in &input.tokens.css_var_reads_located {
                if *name == raw {
                    consumers.push(TokenConsumerLocation {
                        path: path.clone(),
                        line: *line,
                        kind: ConsumerKind::CssVar,
                    });
                }
            }
            for (token, path, line) in &input.tokens.apply_uses_located {
                if token.len() > dash_name.len() && token.ends_with(&dash_name) {
                    consumers.push(TokenConsumerLocation {
                        path: path.clone(),
                        line: *line,
                        kind: ConsumerKind::Apply,
                    });
                }
            }
            for (token, path, line) in &utility_located {
                if token.len() > dash_name.len() && token.ends_with(&dash_name) {
                    consumers.push(TokenConsumerLocation {
                        path: path.clone(),
                        line: *line,
                        kind: ConsumerKind::Utility,
                    });
                }
            }

            consumers.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then_with(|| a.line.cmp(&b.line))
                    .then_with(|| consumer_kind_rank(a.kind).cmp(&consumer_kind_rank(b.kind)))
            });
            let consumer_count = saturate_len(consumers.len());
            consumers.truncate(TOKEN_CONSUMER_SAMPLE_CAP);

            TokenConsumers {
                token: candidate.token,
                namespace: candidate.namespace,
                definition_path: candidate.path,
                definition_line: candidate.line,
                consumer_count,
                consumers,
            }
        })
        .collect();

    out.sort_by(|a, b| a.token.cmp(&b.token));
    out
}

/// A CSS-in-JS token-definition site discovered during the definer pass: the
/// root-relative definition file, the access binding consumers read through, and
/// its flattened leaf tokens.
struct CssInJsDefiner {
    rel_path: String,
    binding: String,
    origin: fallow_extract::CssInJsTokenOrigin,
    leaves: Vec<fallow_extract::CssInJsToken>,
}

/// The definer-pass result: every `(file, binding)` token-definition site plus the
/// lookups the consumer pass keys on (normalized definer path + binding -> entry
/// index, and the set of normalized definer paths for relative-import resolution).
struct CssInJsDefiners {
    entries: Vec<CssInJsDefiner>,
    index: rustc_hash::FxHashMap<(std::path::PathBuf, String), usize>,
    paths: rustc_hash::FxHashSet<std::path::PathBuf>,
}

type CssInJsConsumerKey = (usize, String);
type CssInJsConsumerHit = (String, u32, fallow_output::ConsumerKind);
type CssInJsConsumerHits =
    rustc_hash::FxHashMap<CssInJsConsumerKey, rustc_hash::FxHashSet<CssInJsConsumerHit>>;
type CssInJsImportKey = (fallow_types::discover::FileId, String, String, String);
type ResolvedCssInJsImportTargets =
    rustc_hash::FxHashMap<CssInJsImportKey, fallow_types::discover::FileId>;

/// Whether a specifier names a CSS-in-JS token-DEFINITION library. `@vanilla-extract/recipes`
/// is excluded: it exports no token-definition function (`createTheme` family lives
/// in `@vanilla-extract/css`), so it is not a definer-pass pre-filter source.
fn is_css_in_js_token_lib(specifier: &str) -> bool {
    matches!(
        specifier,
        "@stylexjs/stylex" | "@vanilla-extract/css" | "@pandacss/dev"
    )
}

/// A cheap source pre-filter: only re-parse a token-lib-importing file as a
/// potential definer if its source mentions a token-definition function, so a
/// StyleX file that only calls `stylex.create` (no `defineVars`) is not parsed.
fn source_mentions_token_definer(source: &str) -> bool {
    source.contains("defineVars")
        || source.contains("createThemeContract")
        || source.contains("createGlobalTheme")
        || source.contains("createTheme")
        || source.contains("defineTokens")
        || source.contains("defineConfig")
}

fn source_mentions_theme_definer(source: &str) -> bool {
    source.contains("theme") || source.contains("Theme")
}

fn is_theme_provider_source(specifier: &str) -> bool {
    matches!(specifier, "styled-components" | "@emotion/react")
}

fn project_imports_theme_provider(modules: &[fallow_types::extract::ModuleInfo]) -> bool {
    use fallow_types::extract::ImportedName;

    modules.iter().any(|module| {
        module.imports.iter().any(|import| {
            !import.is_type_only
                && is_theme_provider_source(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if name == "ThemeProvider")
        })
    })
}

/// Whether an import specifier is a relative path. The shared graph resolver
/// handles tsconfig aliases and workspace packages first; this light resolver is
/// the zero-FP local fallback for cases where a graph edge was not available.
fn is_relative_specifier(specifier: &str) -> bool {
    specifier.starts_with('.')
}

fn is_panda_generated_specifier(specifier: &str) -> bool {
    specifier
        .split(['/', '\\'])
        .any(|segment| segment == "styled-system")
}

fn is_panda_style_function(name: &str) -> bool {
    matches!(name, "css" | "cva" | "sva" | "recipe" | "styled")
}

/// Lexically normalize a path (resolve `.` / `..` without touching the
/// filesystem), so a consumer-relative join compares equal to a definer's
/// discovered absolute path regardless of `./` / `../` segments.
fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve a relative import specifier from a consuming file to a known definer
/// path (extension + `/index` candidates, lexically normalized). Returns the
/// matched, normalized definer path or `None`. Zero-FP for relative imports: a
/// specifier that resolves to a non-definer path yields `None`, so an unrelated
/// `import { vars } from './other'` is never matched against a design-token `vars`.
fn resolve_relative_specifier(
    consumer_abs: &std::path::Path,
    specifier: &str,
    definer_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    const EXTS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];
    let base = lexical_normalize(&consumer_abs.parent()?.join(specifier));
    // 1. Exact (specifier already carried a resolvable filename).
    if definer_paths.contains(&base) {
        return Some(base);
    }
    // 2. `<base>.<ext>` (`./tokens` -> `./tokens.ts`; `./theme.css` -> `./theme.css.ts`).
    for ext in EXTS {
        let mut candidate = base.clone().into_os_string();
        candidate.push(".");
        candidate.push(ext);
        let candidate = std::path::PathBuf::from(candidate);
        if definer_paths.contains(&candidate) {
            return Some(candidate);
        }
    }
    // 3. `<base>/index.<ext>`.
    for ext in EXTS {
        let candidate = base.join(format!("index.{ext}"));
        if definer_paths.contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn css_in_js_import_key(
    file_id: fallow_types::discover::FileId,
    import: &fallow_types::extract::ImportInfo,
) -> Option<CssInJsImportKey> {
    let fallow_types::extract::ImportedName::Named(imported_name) = &import.imported_name else {
        return None;
    };
    Some((
        file_id,
        import.source.clone(),
        imported_name.clone(),
        import.local_name.clone(),
    ))
}

fn resolve_css_in_js_import_targets(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
) -> ResolvedCssInJsImportTargets {
    let workspaces = fallow_config::discover_workspaces(&config.root);
    let active_plugins: Vec<String> = Vec::new();
    let path_aliases: Vec<(String, String)> = Vec::new();
    let auto_imports: Vec<fallow_config::AutoImportRule> = Vec::new();
    let scss_include_paths: Vec<std::path::PathBuf> = Vec::new();
    let static_dir_mappings: Vec<(std::path::PathBuf, String)> = Vec::new();
    let input = fallow_graph::resolve::ResolveAllImportsInput {
        modules,
        files,
        workspaces: &workspaces,
        active_plugins: &active_plugins,
        path_aliases: &path_aliases,
        auto_imports: &auto_imports,
        scss_include_paths: &scss_include_paths,
        static_dir_mappings: &static_dir_mappings,
        root: &config.root,
        extra_conditions: &config.resolve.conditions,
    };
    let mut targets = ResolvedCssInJsImportTargets::default();
    for resolved in fallow_graph::resolve::resolve_all_imports(&input) {
        for import in resolved.resolved_imports {
            let Some(file_id) = import.target.internal_file_id() else {
                continue;
            };
            let Some(key) = css_in_js_import_key(resolved.file_id, &import.info) else {
                continue;
            };
            targets.insert(key, file_id);
        }
    }
    targets
}

fn resolve_css_in_js_definer_import(
    consumer_file_id: fallow_types::discover::FileId,
    consumer_abs: &std::path::Path,
    import: &fallow_types::extract::ImportInfo,
    definers: &CssInJsDefiners,
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    resolved_targets: &ResolvedCssInJsImportTargets,
) -> Option<usize> {
    let fallow_types::extract::ImportedName::Named(imported_name) = &import.imported_name else {
        return None;
    };
    if let Some(key) = css_in_js_import_key(consumer_file_id, import)
        && let Some(target_id) = resolved_targets.get(&key)
        && let Some(target_abs) = path_by_id.get(target_id)
    {
        let resolved = lexical_normalize(target_abs);
        if let Some(&idx) = definers.index.get(&(resolved, imported_name.clone())) {
            return Some(idx);
        }
    }
    if !is_relative_specifier(&import.source) {
        return None;
    }
    let resolved = resolve_relative_specifier(consumer_abs, &import.source, &definers.paths)?;
    definers
        .index
        .get(&(resolved, imported_name.clone()))
        .copied()
}

/// Definer pass: re-parse every token-lib-importing file that mentions a
/// token-definition function, collecting each `(file, binding)` token-definition
/// site plus the lookup structures the consumer pass needs.
fn collect_css_in_js_definers(
    modules: &[fallow_types::extract::ModuleInfo],
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    config: &ResolvedConfig,
) -> CssInJsDefiners {
    let mut definers: Vec<CssInJsDefiner> = Vec::new();
    let mut definer_index: rustc_hash::FxHashMap<(std::path::PathBuf, String), usize> =
        rustc_hash::FxHashMap::default();
    let mut definer_paths: rustc_hash::FxHashSet<std::path::PathBuf> =
        rustc_hash::FxHashSet::default();
    let has_theme_provider = project_imports_theme_provider(modules);

    for module in modules {
        let imports_token_lib = module
            .imports
            .iter()
            .any(|i| !i.is_type_only && is_css_in_js_token_lib(&i.source));
        let Some(abs) = path_by_id.get(&module.file_id).copied() else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(abs) else {
            continue;
        };
        let mut defs = Vec::new();
        if imports_token_lib && source_mentions_token_definer(&source) {
            defs.extend(fallow_extract::css_in_js_token_defs(&source, abs));
        }
        if has_theme_provider && source_mentions_theme_definer(&source) {
            defs.extend(fallow_extract::css_in_js_theme_token_defs(&source, abs));
        }
        if defs.is_empty() {
            continue;
        }
        let Some(rel) = relative_to_root(abs, &config.root) else {
            continue;
        };
        let norm = lexical_normalize(abs);
        for def in defs {
            let idx = definers.len();
            definer_index.insert((norm.clone(), def.binding.clone()), idx);
            definer_paths.insert(norm.clone());
            definers.push(CssInJsDefiner {
                rel_path: rel.clone(),
                binding: def.binding,
                origin: def.origin,
                leaves: def.tokens,
            });
        }
    }
    CssInJsDefiners {
        entries: definers,
        index: definer_index,
        paths: definer_paths,
    }
}

/// Consumer pass: for each file whose named imports resolve to a definer binding
/// through the shared graph resolver or local relative fallback, re-parse it and
/// collect located member-access reads, deduped by `(consumer file, line)` per
/// `(definer, leaf token path)`.
fn collect_css_in_js_consumers(
    modules: &[fallow_types::extract::ModuleInfo],
    path_by_id: &rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path>,
    config: &ResolvedConfig,
    definers: &CssInJsDefiners,
    resolved_targets: &ResolvedCssInJsImportTargets,
) -> CssInJsConsumerHits {
    use fallow_output::ConsumerKind;
    use fallow_types::extract::ImportedName;
    let mut hits: CssInJsConsumerHits = rustc_hash::FxHashMap::default();
    let has_theme_definers = definers
        .entries
        .iter()
        .any(|definer| definer.origin == fallow_extract::CssInJsTokenOrigin::Theme);

    for module in modules {
        let Some(consumer_abs) = path_by_id.get(&module.file_id).copied() else {
            continue;
        };
        // (definer index, local alias the file imported the binding under).
        let mut matches: Vec<(usize, &str)> = Vec::new();
        for import in &module.imports {
            if import.is_type_only {
                continue;
            }
            if !matches!(&import.imported_name, ImportedName::Named(_)) {
                continue;
            }
            if let Some(idx) = resolve_css_in_js_definer_import(
                module.file_id,
                consumer_abs,
                import,
                definers,
                path_by_id,
                resolved_targets,
            ) {
                matches.push((idx, import.local_name.as_str()));
            }
        }
        let has_panda_generated_alias = module.imports.iter().any(|import| {
            !import.is_type_only
                && is_panda_generated_specifier(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if name == "token" || is_panda_style_function(name))
        });
        if matches.is_empty() && !has_panda_generated_alias && !has_theme_definers {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(consumer_abs) else {
            continue;
        };
        let Some(consumer_rel) = relative_to_root(consumer_abs, &config.root) else {
            continue;
        };
        for (idx, alias) in matches {
            let leaf_set: rustc_hash::FxHashSet<String> = definers.entries[idx]
                .leaves
                .iter()
                .map(|t| t.path.clone())
                .collect();
            for hit in
                fallow_extract::css_in_js_token_consumers(&source, consumer_abs, alias, &leaf_set)
            {
                hits.entry((idx, hit.token_path)).or_default().insert((
                    consumer_rel.clone(),
                    hit.line,
                    ConsumerKind::JsMember,
                ));
            }
        }
        collect_panda_token_call_consumers(
            module,
            consumer_abs,
            &source,
            &consumer_rel,
            definers,
            &mut hits,
        );
        collect_theme_member_consumers(&source, consumer_abs, &consumer_rel, definers, &mut hits);
    }
    hits
}

fn collect_theme_member_consumers(
    source: &str,
    consumer_abs: &std::path::Path,
    consumer_rel: &str,
    definers: &CssInJsDefiners,
    hits: &mut CssInJsConsumerHits,
) {
    use fallow_output::ConsumerKind;

    for (idx, definer) in definers.entries.iter().enumerate() {
        if definer.origin != fallow_extract::CssInJsTokenOrigin::Theme {
            continue;
        }
        let leaf_set: rustc_hash::FxHashSet<String> =
            definer.leaves.iter().map(|t| t.path.clone()).collect();
        for hit in fallow_extract::css_in_js_theme_consumers(source, consumer_abs, &leaf_set) {
            hits.entry((idx, hit.token_path)).or_default().insert((
                consumer_rel.to_owned(),
                hit.line,
                ConsumerKind::JsMember,
            ));
        }
    }
}

fn collect_panda_token_call_consumers(
    module: &fallow_types::extract::ModuleInfo,
    consumer_abs: &std::path::Path,
    source: &str,
    consumer_rel: &str,
    definers: &CssInJsDefiners,
    hits: &mut CssInJsConsumerHits,
) {
    use fallow_output::ConsumerKind;
    use fallow_types::extract::ImportedName;

    let token_aliases: Vec<&str> = module
        .imports
        .iter()
        .filter(|import| {
            !import.is_type_only
                && is_panda_generated_specifier(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if name == "token")
        })
        .map(|import| import.local_name.as_str())
        .collect();
    let style_aliases: rustc_hash::FxHashSet<String> = module
        .imports
        .iter()
        .filter(|import| {
            !import.is_type_only
                && is_panda_generated_specifier(&import.source)
                && matches!(&import.imported_name, ImportedName::Named(name) if is_panda_style_function(name))
        })
        .map(|import| import.local_name.clone())
        .collect();
    if token_aliases.is_empty() && style_aliases.is_empty() {
        return;
    }
    for (idx, definer) in definers.entries.iter().enumerate() {
        if definer.origin != fallow_extract::CssInJsTokenOrigin::Panda {
            continue;
        }
        let leaf_set: rustc_hash::FxHashSet<String> =
            definer.leaves.iter().map(|t| t.path.clone()).collect();
        for alias in &token_aliases {
            for hit in
                fallow_extract::panda_token_call_consumers(source, consumer_abs, alias, &leaf_set)
            {
                hits.entry((idx, hit.token_path)).or_default().insert((
                    consumer_rel.to_owned(),
                    hit.line,
                    ConsumerKind::JsCall,
                ));
            }
        }
        for hit in fallow_extract::panda_style_value_consumers(
            source,
            consumer_abs,
            &style_aliases,
            &leaf_set,
        ) {
            hits.entry((idx, hit.token_path)).or_default().insert((
                consumer_rel.to_owned(),
                hit.line,
                ConsumerKind::JsCall,
            ));
        }
    }
}

/// Build the CSS-in-JS design-token blast-radius: StyleX `defineVars`,
/// vanilla-extract `createTheme`-family, PandaCSS `defineTokens`, and
/// styled-components / Emotion theme objects. Uses resolved import edges for
/// relative imports, tsconfig aliases, and workspace packages, then falls back to
/// the light relative resolver for zero-FP local cases.
fn build_css_in_js_token_consumers(
    files: &[fallow_types::discover::DiscoveredFile],
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
) -> Vec<fallow_output::TokenConsumers> {
    use fallow_output::{TOKEN_CONSUMER_SAMPLE_CAP, TokenConsumerLocation, TokenConsumers};

    if !project_uses_css_in_js(&config.root) {
        return Vec::new();
    }
    let path_by_id: rustc_hash::FxHashMap<fallow_types::discover::FileId, &std::path::Path> =
        files.iter().map(|f| (f.id, f.path.as_path())).collect();

    let definers = collect_css_in_js_definers(modules, &path_by_id, config);
    if definers.entries.is_empty() {
        return Vec::new();
    }
    let resolved_targets = resolve_css_in_js_import_targets(files, modules, config);
    let hits =
        collect_css_in_js_consumers(modules, &path_by_id, config, &definers, &resolved_targets);

    let mut out: Vec<TokenConsumers> = Vec::new();
    for (idx, definer) in definers.entries.iter().enumerate() {
        for leaf in &definer.leaves {
            let mut consumers: Vec<TokenConsumerLocation> = hits
                .get(&(idx, leaf.path.clone()))
                .map(|set| {
                    set.iter()
                        .map(|(path, line, kind)| TokenConsumerLocation {
                            path: path.clone(),
                            line: *line,
                            kind: *kind,
                        })
                        .collect()
                })
                .unwrap_or_default();
            consumers.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
            let consumer_count = saturate_len(consumers.len());
            consumers.truncate(TOKEN_CONSUMER_SAMPLE_CAP);
            out.push(TokenConsumers {
                token: format!("{}.{}", definer.binding, leaf.path),
                namespace: definer.binding.clone(),
                definition_path: definer.rel_path.clone(),
                definition_line: leaf.def_line,
                consumer_count,
                consumers,
            });
        }
    }
    // Deterministic order among the CSS-in-JS entries. The caller
    // (`compute_css_analytics_report`) applies a final sort over the COMBINED
    // Tailwind + CSS-in-JS list, so the emitted `token_consumers` is globally
    // ordered by `(token, definition_path)`.
    out.sort_by(|a, b| {
        a.token
            .cmp(&b.token)
            .then_with(|| a.definition_path.cmp(&b.definition_path))
    });
    out
}

fn consumer_kind_rank(kind: fallow_output::ConsumerKind) -> u8 {
    use fallow_output::ConsumerKind;
    match kind {
        ConsumerKind::ThemeVar => 0,
        ConsumerKind::CssVar => 1,
        ConsumerKind::Utility => 2,
        ConsumerKind::Apply => 3,
        ConsumerKind::JsMember => 4,
        ConsumerKind::JsCall => 5,
    }
}

/// The markup / source-derived CSS candidate lists, gathered in one pass-set so
/// the orchestrator stays a thin assembler.
struct MarkupCssCandidates {
    tailwind_arbitrary_values: Vec<fallow_output::TailwindArbitraryValue>,
    cva_duplicate_variant_blocks: Vec<fallow_output::CvaDuplicateVariantBlock>,
    cva_variant_token_drifts: Vec<fallow_output::CvaVariantTokenDrift>,
    unresolved_class_references: Vec<fallow_output::UnresolvedClassReference>,
    unreferenced_css_classes: Vec<fallow_output::UnreferencedCssClass>,
    unused_theme_tokens: Vec<fallow_output::UnusedThemeToken>,
    near_duplicate_theme_tokens: Vec<fallow_output::NearDuplicateThemeToken>,
}

/// Run the markup / source-scanning CSS candidates (Tailwind arbitrary values,
/// likely class typos, unreferenced global classes, unused `@theme` tokens),
/// each honoring the same ignore / changed / workspace filters and setting its
/// own summary counts.
struct MarkupCssCandidateInput<'a> {
    tokens: &'a CssTokenSets,
    files: &'a [fallow_types::discover::DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    css_deep: bool,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    styling_artifacts: Option<&'a StylingAnalysisArtifacts>,
    token_candidates: &'a [ComparableThemeTokenCandidate],
    summary: &'a mut fallow_output::CssAnalyticsSummary,
}

fn scan_markup_css_candidates(input: &mut MarkupCssCandidateInput<'_>) -> MarkupCssCandidates {
    MarkupCssCandidates {
        // Markup arbitrary-value scan (gated on the project using Tailwind).
        tailwind_arbitrary_values: scan_markup_tailwind_arbitrary_values(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: None,
                ws_roots: input.ws_roots,
            },
            input.summary,
        ),
        cva_duplicate_variant_blocks: scan_cva_duplicate_variant_blocks(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: None,
                ws_roots: input.ws_roots,
            },
        ),
        cva_variant_token_drifts: scan_cva_variant_token_drifts(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: None,
                ws_roots: input.ws_roots,
            },
            input.token_candidates,
        ),
        // Static markup class tokens one edit from a defined class (likely typos).
        unresolved_class_references: scan_unresolved_class_references(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: None,
                ws_roots: input.ws_roots,
            },
            input.summary,
        ),
        // Global classes referenced by no in-project markup (heavily gated).
        unreferenced_css_classes: scan_unreferenced_css_classes(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: None,
                ws_roots: input.ws_roots,
            },
            input.summary,
            input
                .styling_artifacts
                .map(|artifacts| &artifacts.reference_surface),
            input
                .styling_artifacts
                .map(|artifacts| &artifacts.class_inventory),
        ),
        // Tailwind v4 @theme design tokens used by no utility / var() / @apply
        // anywhere (heavily gated: v4 + non-plugin + non-published + whole-scope).
        unused_theme_tokens: scan_unused_theme_tokens(&mut UnusedThemeTokenScanInput {
            tokens: input.tokens,
            files: input.files,
            config: input.config,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            output_changed_files: input.output_changed_files,
            ws_roots: input.ws_roots,
            summary: input.summary,
        }),
        // Perceptually-close Tailwind v4 color tokens, whole-scope only.
        near_duplicate_theme_tokens: if input.css_deep {
            scan_near_duplicate_theme_tokens(&mut UnusedThemeTokenScanInput {
                tokens: input.tokens,
                files: input.files,
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                output_changed_files: input.output_changed_files,
                ws_roots: input.ws_roots,
                summary: input.summary,
            })
        } else {
            Vec::new()
        },
    }
}

fn project_uses_css_in_js(root: &std::path::Path) -> bool {
    const CSS_IN_JS_DEPS: &[&str] = &[
        "styled-components",
        "@emotion/styled",
        "@emotion/react",
        "@emotion/css",
        "@linaria/core",
        "@linaria/react",
        "@vanilla-extract/css",
        "@pandacss/dev",
        "@stylexjs/stylex",
    ];
    let Ok(text) = std::fs::read_to_string(root.join("package.json")) else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    ["dependencies", "devDependencies", "peerDependencies"]
        .iter()
        .any(|key| {
            json.get(key)
                .and_then(serde_json::Value::as_object)
                .is_some_and(|deps| deps.keys().any(|k| CSS_IN_JS_DEPS.contains(&k.as_str())))
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CssScanKind {
    Css,
    Preprocessor,
    Sfc,
    CssInJs,
}

fn css_report_scan_target<'a>(
    file: &'a fallow_types::discover::DiscoveredFile,
    ctx: HealthScanCtx<'_>,
    css_in_js: bool,
) -> Option<(&'a std::path::Path, CssScanKind)> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files: _,
        ws_roots,
    } = ctx;

    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    let kind = match extension {
        Some("css") => CssScanKind::Css,
        Some("scss" | "sass" | "less") => CssScanKind::Preprocessor,
        Some("vue") | Some("svelte") => CssScanKind::Sfc,
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts") if css_in_js => {
            CssScanKind::CssInJs
        }
        _ => return None,
    };

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
    Some((relative, kind))
}

fn record_scoped_unused_classes(
    source: &str,
    relative: &std::path::Path,
    summary: &mut fallow_output::CssAnalyticsSummary,
    scoped_unused: &mut Vec<fallow_output::ScopedUnusedClasses>,
) {
    let classes = crate::css::scoped_unused_classes(source);
    if classes.is_empty() {
        return;
    }

    summary.scoped_unused_classes = summary
        .scoped_unused_classes
        .saturating_add(u32::try_from(classes.len()).unwrap_or(u32::MAX));
    scoped_unused.push(fallow_output::ScopedUnusedClasses {
        path: relative.to_string_lossy().replace('\\', "/"),
        classes,
        actions: vec![fallow_output::CssCandidateAction::verify_scoped_classes()],
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GradePolicy {
    Structural,
    StructuralNoDedup,
    Atomic,
}

struct CssScanItem<'a> {
    source: std::borrow::Cow<'a, str>,
    policy: GradePolicy,
    report_notable: bool,
}

fn css_report_scan_items<'a>(
    source: &'a str,
    path: &std::path::Path,
    kind: CssScanKind,
) -> Vec<CssScanItem<'a>> {
    use std::borrow::Cow;
    match kind {
        CssScanKind::Css => vec![CssScanItem {
            source: Cow::Borrowed(source),
            policy: GradePolicy::Structural,
            report_notable: true,
        }],
        CssScanKind::Preprocessor => preprocessor_virtual_stylesheet(source)
            .map(|virtual_css| {
                vec![CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                }]
            })
            .unwrap_or_default(),
        CssScanKind::Sfc => {
            let mut items = Vec::new();
            if let Some(virtual_css) = crate::css::sfc_virtual_stylesheet(source) {
                items.push(CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                });
            }
            if let Some(preprocessor_source) =
                crate::css::sfc_preprocessor_virtual_stylesheet(source)
                && let Some(virtual_css) = preprocessor_virtual_stylesheet(&preprocessor_source)
            {
                items.push(CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                });
            }
            items
        }
        CssScanKind::CssInJs => {
            let mut items = Vec::new();
            if let Some(virtual_css) = crate::css::css_in_js_virtual_stylesheet(source) {
                items.push(CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                });
            }
            let sheets = crate::css::css_in_js_object_sheets(source, path);
            if let Some(structural) = sheets.structural {
                items.push(CssScanItem {
                    source: Cow::Owned(structural),
                    policy: GradePolicy::Structural,
                    report_notable: false,
                });
            }
            if let Some(partial) = sheets.structural_partial {
                items.push(CssScanItem {
                    source: Cow::Owned(partial),
                    policy: GradePolicy::StructuralNoDedup,
                    report_notable: false,
                });
            }
            if let Some(atomic) = sheets.atomic {
                items.push(CssScanItem {
                    source: Cow::Owned(atomic),
                    policy: GradePolicy::Atomic,
                    report_notable: false,
                });
            }
            items
        }
    }
}

fn preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    let clean = strip_preprocessor_comments(source);
    let output = render_preprocessor_children(&clean, 0, clean.len(), 0);
    (!output.trim().is_empty()).then_some(output)
}

fn strip_preprocessor_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push_str(&source[cursor..i]);
            out.push_str("  ");
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            cursor = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&source[cursor..]);
    out
}

fn render_preprocessor_children(source: &str, start: usize, end: usize, indent: usize) -> String {
    let bytes = source.as_bytes();
    let mut output = String::new();
    let mut statement_start = start;
    let mut i = start;
    while i < end {
        if bytes[i] == b'{' {
            let prelude = source[statement_start..i].trim();
            let Some(close) = find_matching_brace(source, i, end) else {
                return output;
            };
            if let Some(block) = render_preprocessor_block(source, prelude, i + 1, close, indent) {
                output.push_str(&block);
            }
            i = close + 1;
            statement_start = i;
        } else if bytes[i] == b';' {
            i += 1;
            statement_start = i;
        } else {
            i += 1;
        }
    }
    output
}

fn render_preprocessor_block(
    source: &str,
    prelude: &str,
    body_start: usize,
    body_end: usize,
    indent: usize,
) -> Option<String> {
    let prelude = prelude.trim();
    if prelude.is_empty()
        || prelude.contains("#{")
        || prelude.starts_with("@mixin")
        || prelude.starts_with("@function")
        || prelude.starts_with("@for")
        || prelude.starts_with("@each")
        || prelude.starts_with("@if")
        || prelude.starts_with("@else")
        || prelude.starts_with("@while")
    {
        return None;
    }
    if prelude.starts_with("@media")
        || prelude.starts_with("@supports")
        || prelude.starts_with("@container")
        || prelude.starts_with("@layer")
    {
        let body = render_preprocessor_children(source, body_start, body_end, indent + 1);
        if body.trim().is_empty() {
            return None;
        }
        let mut output = String::new();
        push_indent(&mut output, indent);
        output.push_str(prelude);
        output.push_str(" {\n");
        output.push_str(&body);
        push_indent(&mut output, indent);
        output.push_str("}\n");
        return Some(output);
    }
    if prelude.starts_with('@') || prelude.ends_with(':') {
        return None;
    }

    let selectors = clean_preprocessor_selector_list(prelude)?;
    let (declarations, children) =
        render_preprocessor_body(source, body_start, body_end, indent + 1);
    if declarations.is_empty() && children.trim().is_empty() {
        return None;
    }
    let mut output = String::new();
    push_indent(&mut output, indent);
    output.push_str(&selectors);
    output.push_str(" {\n");
    for declaration in declarations {
        push_indent(&mut output, indent + 1);
        output.push_str(&declaration);
        output.push('\n');
    }
    output.push_str(&children);
    push_indent(&mut output, indent);
    output.push_str("}\n");
    Some(output)
}

fn render_preprocessor_body(
    source: &str,
    body_start: usize,
    body_end: usize,
    indent: usize,
) -> (Vec<String>, String) {
    let bytes = source.as_bytes();
    let mut declarations = Vec::new();
    let mut children = String::new();
    let mut statement_start = body_start;
    let mut i = body_start;
    while i < body_end {
        match bytes[i] {
            b'{' => {
                let prelude = source[statement_start..i].trim();
                let Some(close) = find_matching_brace(source, i, body_end) else {
                    break;
                };
                if let Some(block) =
                    render_preprocessor_block(source, prelude, i + 1, close, indent)
                {
                    children.push_str(&block);
                }
                i = close + 1;
                statement_start = i;
            }
            b';' => {
                let statement = source[statement_start..=i].trim();
                if let Some(declaration) = normalize_preprocessor_declaration(statement) {
                    declarations.push(declaration);
                }
                i += 1;
                statement_start = i;
            }
            _ => i += 1,
        }
    }
    (declarations, children)
}

fn clean_preprocessor_selector_list(prelude: &str) -> Option<String> {
    let children: Vec<&str> = prelude
        .split(',')
        .map(str::trim)
        .filter(|selector| {
            !selector.is_empty()
                && !selector.contains("#{")
                && !selector.starts_with('@')
                && !selector.ends_with(':')
        })
        .collect();
    if children.is_empty() {
        None
    } else {
        Some(children.join(", "))
    }
}

fn normalize_preprocessor_declaration(statement: &str) -> Option<String> {
    let statement = statement.trim().trim_end_matches(';').trim();
    if statement.is_empty()
        || statement.starts_with('$')
        || statement.starts_with("@include")
        || statement.starts_with("@extend")
        || statement.starts_with("@debug")
        || statement.starts_with("@warn")
        || statement.starts_with("@error")
        || statement.contains("#{")
    {
        return None;
    }
    let (property, value) = statement.split_once(':')?;
    let property = property.trim();
    let value = value.trim();
    if property.is_empty() || value.is_empty() || property.starts_with('@') {
        return None;
    }
    Some(format!(
        "{property}: {};",
        normalize_preprocessor_value(value)
    ))
}

fn normalize_preprocessor_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if (bytes[i] == b'$' || bytes[i] == b'@') && is_preprocessor_ident_start(bytes.get(i + 1)) {
            out.push_str(&value[cursor..i]);
            out.push_str("var(--fallow-preprocessor-var)");
            i += 2;
            while i < bytes.len() && is_preprocessor_ident_continue(bytes[i]) {
                i += 1;
            }
            cursor = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[cursor..]);
    out
}

fn is_preprocessor_ident_start(byte: Option<&u8>) -> bool {
    byte.is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b == b'-')
}

fn is_preprocessor_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn push_indent(output: &mut String, indent: usize) {
    for _ in 0..indent {
        output.push_str("  ");
    }
}

fn find_matching_brace(source: &str, open: usize, limit: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < limit {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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
    use fallow_output::{CssAnalyticsSummary, CssFileAnalytics, ScopedUnusedClasses};

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
            let Some(mut analytics) = crate::css::compute_css_analytics(&item.source) else {
                continue;
            };
            file_had_sheet = true;
            record_css_analytics_summary(&mut summary, &analytics);
            tokens.record_theme(item.source.as_ref(), &rel);

            match item.policy {
                GradePolicy::Atomic => {
                    analytics.declaration_blocks.clear();
                    analytics.raw_style_values.clear();
                    tokens.record(&analytics, &rel);
                    scoring.atomic_declarations = scoring
                        .atomic_declarations
                        .saturating_add(analytics.total_declarations);
                }
                GradePolicy::Structural | GradePolicy::StructuralNoDedup => {
                    if item.policy == GradePolicy::StructuralNoDedup {
                        analytics.declaration_blocks.clear();
                    }
                    tokens.record(&analytics, &rel);
                    scoring.add_non_atomic(&analytics);
                    if item.report_notable && !analytics.notable_rules.is_empty() {
                        file_reports.push(CssFileAnalytics {
                            path: rel.clone(),
                            analytics,
                        });
                    }
                }
            }
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

/// Credit Tailwind-markup-applied keyframes, then finalize the whole-project
/// token metrics and prune unused `@font-face` families referenced elsewhere.
fn finalize_css_token_metrics(
    tokens: &mut CssTokenSets,
    summary: &mut fallow_output::CssAnalyticsSummary,
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssTokenMetrics {
    // Credit @keyframes applied via Tailwind markup (`animate-[name_...]` /
    // `animate-name`), not just CSS `animation:` declarations, before the
    // unreferenced diff. Filtered to actually-defined keyframes so a stray
    // `animate-*` suffix never manufactures a false `undefined_keyframes`.
    for name in collect_markup_keyframe_references(files, config, ignore_set) {
        if tokens.defined_keyframes.contains(&name) {
            tokens.referenced_keyframes.insert(name);
        }
    }

    let (unreferenced_keyframes, undefined_keyframes) = tokens.finalize(summary);
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

    let mut walk = styling_artifacts
        .filter(|_| changed_files.is_none() && output_changed_files.is_none() && ws_roots.is_none())
        .map_or_else(
            || walk_css_files(files, ctx),
            |artifacts| artifacts.whole_scope_walk.clone(),
        );
    let mut styling_token_candidates = comparable_theme_token_candidates(&walk.tokens, config);
    styling_token_candidates.extend(comparable_custom_property_token_candidates(&walk.tokens));
    styling_token_candidates.extend(comparable_css_in_js_token_candidates(
        files, modules, config,
    ));
    styling_token_candidates.extend(comparable_project_vocabulary_candidates(&walk.tokens));
    styling_token_candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    annotate_raw_style_value_nearest_tokens(&mut walk.tokens, &styling_token_candidates);
    let metrics = finalize_css_token_metrics(
        &mut walk.tokens,
        &mut walk.summary,
        files,
        config,
        ignore_set,
    );
    let candidates = scan_markup_css_candidates(&mut MarkupCssCandidateInput {
        tokens: &walk.tokens,
        files,
        config,
        ignore_set,
        changed_files,
        output_changed_files,
        css_deep,
        ws_roots,
        styling_artifacts,
        token_candidates: &styling_token_candidates,
        summary: &mut walk.summary,
    });
    let mut token_consumers = build_token_consumers(&TokenConsumersInput {
        tokens: &walk.tokens,
        files,
        config,
        ignore_set,
        changed_files,
        ws_roots,
    });
    // Phase 3d: additively append the CSS-in-JS design-token blast-radius (StyleX
    // `defineVars` / vanilla-extract `createTheme` family), derived from the
    // graph-independent `ModuleInfo` imports + a bounded re-parse, gated on the same
    // `project_uses_css_in_js` dep gate the CSS-in-JS walk uses (a non-CSS-in-JS
    // project appends nothing, so Tailwind output is byte-identical). The combined
    // list is then sorted globally by `(token, definition_path)` so the contract is
    // a single ordered list, not a Tailwind block then a CSS-in-JS block.
    token_consumers.extend(build_css_in_js_token_consumers(files, modules, config));
    token_consumers.sort_by(|a, b| {
        a.token
            .cmp(&b.token)
            .then_with(|| a.definition_path.cmp(&b.definition_path))
    });
    let scoring_inputs = super::styling_score::StylingScoringInputs {
        theme_tokens_defined: saturate_len(walk.tokens.theme_token_definers.len()),
        non_atomic_declarations: walk.scoring.non_atomic_declarations,
        non_atomic_important_declarations: walk.scoring.non_atomic_important_declarations,
        non_atomic_max_nesting_depth: walk.scoring.non_atomic_max_nesting_depth,
        atomic_declarations: walk.scoring.atomic_declarations,
    };
    let report = assemble_css_report(CssReportAssemblyInput {
        walk,
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

/// Assemble the final CSS analytics report from the walk accumulator, finalized
/// token metrics, and markup candidates; returns `None` when nothing notable was
/// found (no analyzed files and every candidate list empty).
struct CssReportAssemblyInput<'a> {
    walk: CssWalkAccum,
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
        mut walk,
        mut metrics,
        mut candidates,
        mut token_consumers,
        config,
        output_changed_files,
    } = input;

    if let Some(changed) = output_changed_files {
        retain_css_report_changed_scope(CssReportChangedScopeInput {
            walk: &mut walk,
            metrics: &mut metrics,
            candidates: &mut candidates,
            token_consumers: &mut token_consumers,
            config,
            changed,
        });
    }

    let candidates_empty = candidates.tailwind_arbitrary_values.is_empty()
        && candidates.cva_duplicate_variant_blocks.is_empty()
        && candidates.cva_variant_token_drifts.is_empty()
        && candidates.unresolved_class_references.is_empty()
        && candidates.unreferenced_css_classes.is_empty()
        && metrics.unused_font_faces.is_empty()
        && candidates.unused_theme_tokens.is_empty()
        && candidates.near_duplicate_theme_tokens.is_empty()
        && token_consumers.is_empty();
    if walk.summary.files_analyzed == 0 && walk.scoped_unused.is_empty() && candidates_empty {
        return None;
    }
    let mut scoped_unused = walk.scoped_unused;
    scoped_unused.sort_by(|a, b| a.path.cmp(&b.path));
    let mut raw_style_values = walk.tokens.raw_style_values;
    raw_style_values.sort_by(|a, b| {
        (&a.path, a.line, &a.axis, &a.property, &a.value).cmp(&(
            &b.path,
            b.line,
            &b.axis,
            &b.property,
            &b.value,
        ))
    });
    walk.summary.raw_style_values = saturate_len(raw_style_values.len());
    Some(CssAnalyticsReport {
        files: walk.file_reports,
        summary: walk.summary,
        scoped_unused,
        unreferenced_keyframes: metrics.unreferenced_keyframes,
        undefined_keyframes: metrics.undefined_keyframes,
        duplicate_declaration_blocks: metrics.duplicate_declaration_blocks,
        cva_duplicate_variant_blocks: candidates.cva_duplicate_variant_blocks,
        cva_variant_token_drifts: candidates.cva_variant_token_drifts,
        tailwind_arbitrary_values: candidates.tailwind_arbitrary_values,
        raw_style_values,
        unused_at_rules: metrics.unused_at_rules,
        unresolved_class_references: candidates.unresolved_class_references,
        unreferenced_css_classes: candidates.unreferenced_css_classes,
        unused_font_faces: metrics.unused_font_faces,
        unused_theme_tokens: candidates.unused_theme_tokens,
        near_duplicate_theme_tokens: candidates.near_duplicate_theme_tokens,
        token_consumers,
        font_size_unit_mix: metrics.font_size_unit_mix,
    })
}

struct CssReportChangedScopeInput<'a> {
    walk: &'a mut CssWalkAccum,
    metrics: &'a mut CssTokenMetrics,
    candidates: &'a mut MarkupCssCandidates,
    token_consumers: &'a mut Vec<fallow_output::TokenConsumers>,
    config: &'a ResolvedConfig,
    changed: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
}

fn retain_css_report_changed_scope(input: CssReportChangedScopeInput<'_>) {
    let CssReportChangedScopeInput {
        walk,
        metrics,
        candidates,
        token_consumers,
        config,
        changed,
    } = input;
    let in_scope = |path: &str| css_output_path_in_changed_scope(path, config, changed);
    walk.file_reports.retain(|file| in_scope(&file.path));
    walk.scoped_unused.retain(|item| in_scope(&item.path));
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
    walk.tokens
        .raw_style_values
        .retain(|item| in_scope(&item.path));
    token_consumers.retain(|item| in_scope(&item.definition_path));
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
mod token_consumer_tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_output::ConsumerKind;
    use fallow_types::discover::{DiscoveredFile, FileId};
    use std::path::Path;

    /// Resolve a default config rooted at `root`.
    fn config_at(root: &Path) -> ResolvedConfig {
        FallowConfig::default().resolve(
            root.to_path_buf(),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    /// Write `relative` under `root` with `body`, returning a `DiscoveredFile`.
    fn write_file(root: &Path, id: u32, relative: &str, body: &str) -> DiscoveredFile {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, body).unwrap();
        DiscoveredFile {
            id: FileId(id),
            size_bytes: u64::try_from(body.len()).unwrap(),
            path,
        }
    }

    /// A `CssTokenSets` populated from a single stylesheet's `@theme` / `@apply`
    /// / `var()` content (exercises the real located scans in `record_theme`).
    fn tokens_from(theme_css: &str, rel: &str) -> CssTokenSets {
        let mut tokens = CssTokenSets::default();
        tokens.record_theme(theme_css, rel);
        tokens
    }

    #[test]
    fn token_read_by_two_markup_files_counts_two_utility() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        let f1 = write_file(
            root,
            0,
            "src/Button.tsx",
            "export const Button = () => <button className=\"bg-brand\" />;",
        );
        let f2 = write_file(
            root,
            1,
            "src/Card.tsx",
            "export const Card = () => <div className=\"text-brand p-4\" />;",
        );
        let files = vec![f1, f2];
        let config = config_at(root);
        let tokens = tokens_from("@theme {\n  --color-brand: #f00;\n}", "src/theme.css");

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });

        assert_eq!(out.len(), 1);
        let entry = &out[0];
        assert_eq!(entry.token, "--color-brand");
        assert_eq!(entry.consumer_count, 2);
        assert!(
            entry
                .consumers
                .iter()
                .all(|c| c.kind == ConsumerKind::Utility)
        );
        let paths: Vec<&str> = entry.consumers.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, vec!["src/Button.tsx", "src/Card.tsx"]);
    }

    #[test]
    fn token_with_no_consumer_counts_zero() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        // Markup uses an unrelated utility, so `--color-unused` has no consumer.
        let files = vec![write_file(
            root,
            0,
            "src/App.tsx",
            "export const App = () => <div className=\"flex gap-2\" />;",
        )];
        let config = config_at(root);
        let tokens = tokens_from("@theme {\n  --color-unused: #abc;\n}", "src/theme.css");

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].token, "--color-unused");
        assert_eq!(out[0].consumer_count, 0);
        assert!(out[0].consumers.is_empty());
    }

    #[test]
    fn theme_var_and_css_var_reads_locate_distinct_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        // `--color-brand` is read once inside @theme (theme-var) and once in a
        // regular rule (css-var); both must surface as distinct kinds.
        let theme_css = "@theme {\n  --color-brand: #f00;\n  --color-accent: var(--color-brand);\n}\n.note {\n  color: var(--color-brand);\n}";
        let files: Vec<DiscoveredFile> = Vec::new();
        let config = config_at(root);
        let tokens = tokens_from(theme_css, "src/theme.css");

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });

        let brand = out
            .iter()
            .find(|t| t.token == "--color-brand")
            .expect("--color-brand present");
        assert_eq!(brand.consumer_count, 2);
        let kinds: Vec<ConsumerKind> = brand.consumers.iter().map(|c| c.kind).collect();
        assert!(kinds.contains(&ConsumerKind::ThemeVar));
        assert!(kinds.contains(&ConsumerKind::CssVar));
    }

    #[test]
    fn apply_body_locates_apply_kind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        let theme_css = "@theme {\n  --color-brand: #f00;\n}\n.btn {\n  @apply bg-brand;\n}";
        let files: Vec<DiscoveredFile> = Vec::new();
        let config = config_at(root);
        let tokens = tokens_from(theme_css, "src/theme.css");

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });

        let brand = out.iter().find(|t| t.token == "--color-brand").unwrap();
        assert_eq!(brand.consumer_count, 1);
        assert_eq!(brand.consumers[0].kind, ConsumerKind::Apply);
    }

    #[test]
    fn non_tailwind_project_emits_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let files = vec![write_file(
            root,
            0,
            "src/App.tsx",
            "export const App = () => <div className=\"bg-brand\" />;",
        )];
        let config = config_at(root);
        let tokens = tokens_from("@theme {\n  --color-brand: #f00;\n}", "src/theme.css");

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });
        assert!(out.is_empty(), "non-Tailwind project must abstain");
    }

    #[test]
    fn plugin_project_emits_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        let files: Vec<DiscoveredFile> = Vec::new();
        let config = config_at(root);
        // A `@plugin` directive trips the DI-blind-spot abstain.
        let tokens = tokens_from(
            "@plugin \"@tailwindcss/typography\";\n@theme {\n  --color-brand: #f00;\n}",
            "src/theme.css",
        );

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
        });
        assert!(out.is_empty(), "plugin project must abstain");
    }

    #[test]
    fn partial_scope_emits_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        let files: Vec<DiscoveredFile> = Vec::new();
        let config = config_at(root);
        let tokens = tokens_from("@theme {\n  --color-brand: #f00;\n}", "src/theme.css");
        let changed: rustc_hash::FxHashSet<std::path::PathBuf> = rustc_hash::FxHashSet::default();

        let out = build_token_consumers(&TokenConsumersInput {
            tokens: &tokens,
            files: &files,
            config: &config,
            ignore_set: &globset::GlobSet::empty(),
            changed_files: Some(&changed),
            ws_roots: None,
        });
        assert!(out.is_empty(), "partial scope must abstain");
    }

    // --- CSS program Phase 3c: object-notation CSS-in-JS engine wiring ---

    /// Run the CSS analytics walk over a temp project and return the computation
    /// (report + scoring inputs), or `None` when nothing analyzable was found.
    fn css_computation(root: &Path, files: &[DiscoveredFile]) -> Option<CssAnalyticsComputation> {
        let config = config_at(root);
        // The 3c CSS-analytics tests do not exercise the Phase 3d CSS-in-JS token
        // blast-radius (which needs `ModuleInfo`), so pass an empty module slice;
        // the token-consumer driver then no-ops (no definers).
        compute_css_analytics_report(
            files,
            &[],
            HealthScanCtx {
                config: &config,
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                output_changed_files: None,
                ws_roots: None,
            },
        )
    }

    #[test]
    fn cva_duplicate_variant_blocks_surface_as_css_copy_paste() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"class-variance-authority":"0.7.0","tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        let button = write_file(
            root,
            0,
            "src/button.ts",
            "import { cva } from 'class-variance-authority';\n\
             export const button = cva('inline-flex', {\n\
               variants: {\n\
                 tone: {\n\
                   primary: 'px-3 py-2 text-sm font-medium',\n\
                   secondary: 'px-3 py-2 text-sm font-medium',\n\
                 },\n\
               },\n\
             });\n",
        );

        let computation = css_computation(root, &[button]).expect("cva candidates keep report");
        let blocks = &computation.report.cva_duplicate_variant_blocks;
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].value, "px-3 py-2 text-sm font-medium");
        assert_eq!(blocks[0].occurrence_count, 2);
        assert_eq!(blocks[0].occurrences[0].path, "src/button.ts");
    }

    // --- CSS program Phase 3d: CSS-in-JS design-token blast-radius ---

    /// Like [`css_computation`] but parses each file into a `ModuleInfo` so the
    /// Phase 3d CSS-in-JS token-consumer driver (which reads imports +
    /// member-access) actually runs.
    fn css_computation_3d(root: &Path, files: &[DiscoveredFile]) -> CssAnalyticsComputation {
        let config = config_at(root);
        let modules: Vec<fallow_types::extract::ModuleInfo> = files
            .iter()
            .map(|f| {
                let src = std::fs::read_to_string(&f.path).unwrap_or_default();
                fallow_extract::parse_source_to_module(f.id, &f.path, &src, 0, false)
            })
            .collect();
        compute_css_analytics_report(
            files,
            &modules,
            HealthScanCtx {
                config: &config,
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                output_changed_files: None,
                ws_roots: None,
            },
        )
        .expect("css_analytics is non-null")
    }

    /// The CSS-in-JS (`js-member`) token-consumer entries from a computation.
    fn js_token_consumers(
        computation: &CssAnalyticsComputation,
    ) -> Vec<&fallow_output::TokenConsumers> {
        computation
            .report
            .token_consumers
            .iter()
            .filter(|t| {
                t.consumers
                    .iter()
                    .all(|c| c.kind == fallow_output::ConsumerKind::JsMember)
                    && t.token.contains('.')
                    && !t.token.starts_with("--")
            })
            .collect()
    }

    fn find_token<'a>(
        computation: &'a CssAnalyticsComputation,
        token: &str,
    ) -> Option<&'a fallow_output::TokenConsumers> {
        computation
            .report
            .token_consumers
            .iter()
            .find(|t| t.token == token)
    }

    #[test]
    fn stylex_define_vars_blast_radius_located_js_member_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/tokens.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000', secondary: '#fff' } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             import { vars } from './tokens.stylex';\n\
             export const s = stylex.create({ root: { color: vars.color.primary } });\n",
        );
        let computation = css_computation_3d(root, &[def, consumer]);
        let primary = find_token(&computation, "vars.color.primary")
            .expect("vars.color.primary blast radius present");
        assert_eq!(primary.namespace, "vars");
        assert_eq!(primary.definition_path, "src/tokens.stylex.ts");
        assert_eq!(primary.consumer_count, 1);
        assert_eq!(primary.consumers.len(), 1);
        assert_eq!(
            primary.consumers[0].kind,
            fallow_output::ConsumerKind::JsMember
        );
        assert_eq!(primary.consumers[0].path, "src/card.ts");
        // Defined-but-unconsumed leaf -> count 0 (criterion 6).
        let secondary =
            find_token(&computation, "vars.color.secondary").expect("secondary present");
        assert_eq!(secondary.consumer_count, 0);
    }

    #[test]
    fn stylex_define_vars_blast_radius_resolves_tsconfig_alias_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@tokens/*":["src/tokens/*"]}}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/tokens/theme.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000' } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { vars } from '@tokens/theme.stylex';\n\
             export const color = vars.color.primary;\n",
        );

        let computation = css_computation_3d(root, &[def, consumer]);
        let primary = find_token(&computation, "vars.color.primary")
            .expect("vars.color.primary blast radius present");
        assert_eq!(
            primary.consumer_count, 1,
            "tsconfig alias import should count as a CSS-in-JS token consumer"
        );
        assert_eq!(primary.consumers[0].path, "src/card.ts");
    }

    #[test]
    fn stylex_define_vars_blast_radius_resolves_workspace_package_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"private":true,"workspaces":["packages/*"],"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("packages/tokens")).unwrap();
        std::fs::write(
            root.join("packages/tokens/package.json"),
            r#"{"name":"@acme/tokens","exports":"./src/index.ts"}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "packages/tokens/src/index.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000' } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { vars } from '@acme/tokens';\n\
             export const color = vars.color.primary;\n",
        );

        let computation = css_computation_3d(root, &[def, consumer]);
        let primary = find_token(&computation, "vars.color.primary")
            .expect("vars.color.primary blast radius present");
        assert_eq!(
            primary.consumer_count, 1,
            "workspace package import should count as a CSS-in-JS token consumer"
        );
        assert_eq!(primary.consumers[0].path, "src/card.ts");
    }

    #[test]
    fn vanilla_extract_create_theme_blast_radius_resolves_tsconfig_alias_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@vanilla-extract/css":"1.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@theme/*":["src/theme/*"]}}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/theme/tokens.css.ts",
            "import { createTheme } from '@vanilla-extract/css';\n\
             export const [themeClass, vars] = createTheme({ color: { brand: 'red' } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/box.css.ts",
            "import { style } from '@vanilla-extract/css';\n\
             import { vars } from '@theme/tokens.css';\n\
             export const box = style({ color: vars.color.brand });\n",
        );

        let computation = css_computation_3d(root, &[def, consumer]);
        let brand =
            find_token(&computation, "vars.color.brand").expect("brand blast radius present");
        assert_eq!(
            brand.consumer_count, 1,
            "tsconfig alias import should count for vanilla-extract token consumers"
        );
        assert_eq!(brand.consumers[0].path, "src/box.css.ts");
        assert_eq!(
            brand.consumers[0].kind,
            fallow_output::ConsumerKind::JsMember
        );
    }

    #[test]
    fn pandacss_define_tokens_blast_radius_located_js_call_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@pandacss/dev":"0.54.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "panda.config.ts",
            "import { defineTokens } from '@pandacss/dev';\n\
             export const tokens = defineTokens({ colors: { brand: { value: '#f05a28' }, accent: { value: '#111' } } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { css } from '../styled-system/css';\n\
             import { token } from '../styled-system/tokens';\n\
             export const card = css({ color: token('colors.brand') });\n",
        );
        let computation = css_computation_3d(root, &[def, consumer]);
        let brand = find_token(&computation, "tokens.colors.brand")
            .expect("Panda token blast radius present");
        assert_eq!(brand.namespace, "tokens");
        assert_eq!(brand.definition_path, "panda.config.ts");
        assert_eq!(brand.consumer_count, 1);
        assert_eq!(brand.consumers.len(), 1);
        assert_eq!(brand.consumers[0].kind, fallow_output::ConsumerKind::JsCall);
        assert_eq!(brand.consumers[0].path, "src/card.ts");
        let accent = find_token(&computation, "tokens.colors.accent")
            .expect("unconsumed Panda token still present");
        assert_eq!(accent.consumer_count, 0);
    }

    #[test]
    fn pandacss_define_tokens_blast_radius_counts_style_object_token_strings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@pandacss/dev":"0.54.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "panda.config.ts",
            "import { defineTokens } from '@pandacss/dev';\n\
             export const tokens = defineTokens({ colors: { brand: { value: '#f05a28' }, accent: { value: '#111' } } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { css } from '../styled-system/css';\n\
             export const card = css({ color: 'colors.brand', _hover: { bg: 'colors.accent' } });\n",
        );
        let computation = css_computation_3d(root, &[def, consumer]);
        let brand = find_token(&computation, "tokens.colors.brand").expect("brand token present");
        assert_eq!(brand.consumer_count, 1);
        assert_eq!(brand.consumers[0].kind, fallow_output::ConsumerKind::JsCall);
        assert_eq!(brand.consumers[0].path, "src/card.ts");
        let accent =
            find_token(&computation, "tokens.colors.accent").expect("accent token present");
        assert_eq!(accent.consumer_count, 1);
        assert_eq!(
            accent.consumers[0].kind,
            fallow_output::ConsumerKind::JsCall
        );
    }

    #[test]
    fn pandacss_define_config_tokens_feed_blast_radius_and_raw_value_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@pandacss/dev":"0.54.0"}}"#,
        )
        .unwrap();
        let config = write_file(
            root,
            0,
            "panda.config.ts",
            "import { defineConfig } from '@pandacss/dev';\n\
             export default defineConfig({\n\
               theme: {\n\
                 tokens: { colors: { brand: { value: '#f05a28' } } },\n\
                 semanticTokens: { colors: { surface: { value: { base: '{colors.brand}', _dark: '#111111' } } } },\n\
                 recipes: { card: { base: { color: 'colors.brand' } } },\n\
               },\n\
             });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { css } from '../styled-system/css';\n\
             export const card = css({ color: 'colors.brand', bg: 'colors.surface' });\n",
        );
        let css = write_file(
            root,
            2,
            "src/styles.css",
            ".panda-match { color: #f05a28; }\n",
        );
        let computation = css_computation_3d(root, &[config, consumer, css]);

        let brand =
            find_token(&computation, "pandaConfig.colors.brand").expect("config token present");
        assert_eq!(brand.definition_path, "panda.config.ts");
        assert_eq!(brand.consumer_count, 1);
        assert_eq!(brand.consumers[0].kind, fallow_output::ConsumerKind::JsCall);

        let surface =
            find_token(&computation, "pandaConfig.colors.surface").expect("semantic token present");
        assert_eq!(surface.consumer_count, 1);

        assert!(
            computation.report.raw_style_values.iter().any(|raw| {
                raw.nearest_token
                    .as_ref()
                    .is_some_and(|token| token.name == "pandaConfig.colors.brand")
            }),
            "raw CSS should point at the static Panda config token"
        );
    }

    #[test]
    fn style_vocabulary_repeated_project_values_explain_nearby_raw_drift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let base = write_file(
            root,
            0,
            "src/base.css",
            ".card { color: #33679a; }\n.panel { border-color: #33679a; }\n",
        );
        let feature = write_file(root, 1, "src/feature.css", ".feature { color: #33679b; }\n");

        let computation = css_computation(root, &[base, feature]).expect("raw CSS keeps report");
        let feature_value = computation
            .report
            .raw_style_values
            .iter()
            .find(|raw| raw.path == "src/feature.css" && raw.value == "#33679b")
            .expect("feature raw value is reported");
        let nearest = feature_value
            .nearest_token
            .as_ref()
            .expect("nearby project vocabulary value is suggested");
        assert_eq!(nearest.name, "project-vocabulary.color.#33679a");
        assert_eq!(nearest.value, "#33679a");
        assert_eq!(nearest.path, "src/base.css");
    }

    #[test]
    fn style_vocabulary_abstains_on_alpha_color_nearest_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let base = write_file(
            root,
            0,
            "src/base.css",
            ".overlay { color: #00000040; }\n.scrim { border-color: #00000040; }\n",
        );
        let feature = write_file(root, 1, "src/feature.css", ".feature { color: #0000; }\n");

        let computation = css_computation(root, &[base, feature]).expect("raw CSS keeps report");
        let feature_value = computation
            .report
            .raw_style_values
            .iter()
            .find(|raw| raw.path == "src/feature.css" && raw.value == "#0000")
            .expect("feature alpha raw value is reported");
        assert!(
            feature_value.nearest_token.is_none(),
            "project-vocabulary should not compare alpha-bearing color values through RGB-only distance"
        );
    }

    #[test]
    fn style_vocabulary_abstains_when_raw_alpha_color_is_near_opaque_value() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let base = write_file(
            root,
            0,
            "src/base.css",
            ".card { color: #ffffff; }\n.panel { border-color: #ffffff; }\n",
        );
        let feature = write_file(
            root,
            1,
            "src/feature.css",
            ".feature { color: #ffffff80; }\n",
        );

        let computation = css_computation(root, &[base, feature]).expect("raw CSS keeps report");
        let feature_value = computation
            .report
            .raw_style_values
            .iter()
            .find(|raw| raw.path == "src/feature.css" && raw.value == "#ffffff80")
            .expect("feature alpha raw value is reported");
        assert!(
            feature_value.nearest_token.is_none(),
            "project-vocabulary should not compare alpha raw values through RGB-only distance"
        );
    }

    #[test]
    fn raw_style_value_abstains_when_alpha_color_is_near_explicit_token() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let file = write_file(
            root,
            0,
            "src/styles.css",
            ":root { --color-black: #000; }\n.feature { background-color: #0000; }\n",
        );

        let computation = css_computation(root, &[file]).expect("raw CSS keeps report");
        let feature_value = computation
            .report
            .raw_style_values
            .iter()
            .find(|raw| raw.path == "src/styles.css" && raw.value == "#0000")
            .expect("feature alpha raw value is reported");
        assert!(
            feature_value.nearest_token.is_none(),
            "raw alpha colors should not compare to opaque explicit tokens through RGB-only distance"
        );
    }

    #[test]
    fn style_vocabulary_abstains_between_two_repeated_project_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let base = write_file(
            root,
            0,
            "src/base.css",
            ".card { color: #ffffff; }\n.panel { border-color: #ffffff; }\n",
        );
        let alternate = write_file(
            root,
            1,
            "src/alternate.css",
            ".soft { color: #fafafa; }\n.muted { border-color: #fafafa; }\n",
        );

        let computation = css_computation(root, &[base, alternate]).expect("raw CSS keeps report");
        let repeated_with_suggestions = computation
            .report
            .raw_style_values
            .iter()
            .filter(|raw| raw.nearest_token.is_some())
            .count();
        assert_eq!(
            repeated_with_suggestions, 0,
            "project-vocabulary should not suggest one repeated local convention over another repeated convention"
        );
    }

    #[test]
    fn pandacss_define_tokens_blast_radius_accepts_aliased_generated_token_imports() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@pandacss/dev":"0.54.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "panda.config.ts",
            "import { defineTokens } from '@pandacss/dev';\n\
             export const tokens = defineTokens({ colors: { brand: { value: '#f05a28' } } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/card.ts",
            "import { token as pandaToken } from '@/styled-system/tokens';\n\
             export const cardColor = pandaToken('colors.brand');\n",
        );

        let computation = css_computation_3d(root, &[def, consumer]);
        let brand = find_token(&computation, "tokens.colors.brand")
            .expect("Panda token blast radius present");
        assert_eq!(
            brand.consumer_count, 1,
            "path-aliased styled-system token import should count for Panda consumers"
        );
        assert_eq!(brand.consumers[0].path, "src/card.ts");
        assert_eq!(brand.consumers[0].kind, fallow_output::ConsumerKind::JsCall);
    }

    #[test]
    fn both_tailwind_and_css_in_js_tokens_merge_in_deterministic_global_order() {
        // A project using BOTH Tailwind v4 @theme tokens AND StyleX defineVars: the
        // combined token_consumers carries both origins and is globally sorted by
        // (token, definition_path), not Tailwind-block-then-CSS-in-JS-block.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"tailwindcss":"4.0.0","@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let theme = write_file(
            root,
            0,
            "src/theme.css",
            "@theme {\n  --color-brand: #3b82f6;\n}\n",
        );
        // A markup consumer of the Tailwind token (utility class `text-brand`).
        let markup = write_file(
            root,
            1,
            "src/App.tsx",
            "export const A = () => <p className=\"text-brand\">x</p>;\n",
        );
        let tokens_file = write_file(
            root,
            2,
            "src/tokens.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ accent: '#000' });\n",
        );
        let card = write_file(
            root,
            3,
            "src/Card.ts",
            "import { vars } from './tokens.stylex';\nexport const x = vars.accent;\n",
        );
        let computation = css_computation_3d(root, &[theme, markup, tokens_file, card]);
        let tokens: Vec<&str> = computation
            .report
            .token_consumers
            .iter()
            .map(|t| t.token.as_str())
            .collect();
        // Both origins present.
        assert!(
            tokens.iter().any(|t| t.starts_with("--")),
            "Tailwind @theme token present: {tokens:?}"
        );
        assert!(
            tokens.iter().any(|t| t == &"vars.accent"),
            "CSS-in-JS token present: {tokens:?}"
        );
        // Globally sorted by token (the combined-list contract).
        let mut sorted = tokens.clone();
        sorted.sort_unstable();
        assert_eq!(
            tokens, sorted,
            "combined token_consumers is globally token-sorted"
        );
    }

    #[test]
    fn vanilla_extract_create_theme_tuple_blast_radius() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@vanilla-extract/css":"1.0.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/theme.css.ts",
            "import { createTheme } from '@vanilla-extract/css';\n\
             export const [themeClass, vars] = createTheme({ color: { brand: 'red' } });\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/box.css.ts",
            "import { style } from '@vanilla-extract/css';\n\
             import { vars } from './theme.css';\n\
             export const box = style({ color: vars.color.brand });\n",
        );
        let computation = css_computation_3d(root, &[def, consumer]);
        let brand =
            find_token(&computation, "vars.color.brand").expect("brand blast radius present");
        assert_eq!(brand.consumer_count, 1);
        assert_eq!(brand.consumers[0].path, "src/box.css.ts");
        assert_eq!(
            brand.consumers[0].kind,
            fallow_output::ConsumerKind::JsMember
        );
    }

    #[test]
    fn styled_components_and_emotion_theme_reads_feed_token_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"styled-components":"6.1.0","@emotion/react":"11.0.0","@emotion/styled":"11.0.0"}}"#,
        )
        .unwrap();
        let theme = write_file(
            root,
            0,
            "src/theme.ts",
            "export const appTheme = { colors: { brand: '#f05a28' }, space: { card: '1rem' } };\n",
        );
        let provider = write_file(
            root,
            1,
            "src/App.tsx",
            "import { ThemeProvider } from 'styled-components';\n\
             import { appTheme } from './theme';\n\
             export const App = ({ children }) => <ThemeProvider theme={appTheme}>{children}</ThemeProvider>;\n",
        );
        let styled_template = write_file(
            root,
            2,
            "src/Card.tsx",
            "import styled from 'styled-components';\n\
             export const Card = styled.div`\n\
               color: ${({ theme }) => theme.colors.brand};\n\
               margin: ${props => props.theme.space.card};\n\
             `;\n",
        );
        let emotion = write_file(
            root,
            3,
            "src/Emotion.tsx",
            "import styled from '@emotion/styled';\n\
             export const Link = styled.a(({ theme }) => ({ color: theme.colors.brand }));\n\
             export const Box = () => <div css={(theme) => ({ margin: theme.space.card })} />;\n",
        );

        let computation = css_computation_3d(root, &[theme, provider, styled_template, emotion]);
        let brand = find_token(&computation, "appTheme.colors.brand")
            .expect("theme brand blast radius present");
        assert_eq!(brand.definition_path, "src/theme.ts");
        assert_eq!(brand.consumer_count, 2);
        assert!(
            brand
                .consumers
                .iter()
                .all(|consumer| consumer.kind == fallow_output::ConsumerKind::JsMember)
        );
        let space = find_token(&computation, "appTheme.space.card")
            .expect("theme spacing blast radius present");
        assert_eq!(space.consumer_count, 2);
        let paths: Vec<&str> = space
            .consumers
            .iter()
            .map(|consumer| consumer.path.as_str())
            .collect();
        assert!(paths.contains(&"src/Card.tsx") && paths.contains(&"src/Emotion.tsx"));
    }

    #[test]
    fn theme_object_without_theme_provider_is_not_a_token_surface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"styled-components":"6.1.0"}}"#,
        )
        .unwrap();
        let theme = write_file(
            root,
            0,
            "src/theme.ts",
            "export const appTheme = { colors: { brand: '#f05a28' } };\n",
        );
        let consumer = write_file(
            root,
            1,
            "src/Card.tsx",
            "import styled from 'styled-components';\n\
             export const Card = styled.div`${({ theme }) => theme.colors.brand}`;\n",
        );
        let computation = css_computation_3d(root, &[theme, consumer]);
        assert!(
            find_token(&computation, "appTheme.colors.brand").is_none(),
            "theme-like objects require ThemeProvider wiring"
        );
    }

    #[test]
    fn zero_false_consumer_same_name_from_unrelated_module() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/tokens.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000' } });\n",
        );
        // A DIFFERENT module also exporting `vars`, read as `vars.color.primary`,
        // must NOT be counted against the design-token `vars`.
        let other = write_file(
            root,
            1,
            "src/other.ts",
            "export const vars = { color: { primary: 1 } };\n",
        );
        let consumer = write_file(
            root,
            2,
            "src/use-other.ts",
            "import { vars } from './other';\n\
             export const x = vars.color.primary;\n",
        );
        let computation = css_computation_3d(root, &[def, other, consumer]);
        let primary = find_token(&computation, "vars.color.primary").expect("token present");
        assert_eq!(
            primary.consumer_count, 0,
            "import of same-named `vars` from an unrelated module must not be a consumer",
        );
    }

    #[test]
    fn zero_double_count_one_site_counts_once_and_intermediate_not_counted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/t.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000' } });\n",
        );
        // One access site reads `vars.color.primary` (which records TWO member-access
        // records: {vars.color, primary} + {vars, color}). It must count ONCE, and
        // the intermediate `vars.color` group must not be a separate consumer.
        let consumer = write_file(
            root,
            1,
            "src/c.ts",
            "import { vars } from './t.stylex';\nexport const x = vars.color.primary;\n",
        );
        let computation = css_computation_3d(root, &[def, consumer]);
        let primary = find_token(&computation, "vars.color.primary").expect("token present");
        assert_eq!(primary.consumer_count, 1, "one access site counts once");
        // `vars.color` (intermediate group) is not a defined leaf, so no entry.
        assert!(find_token(&computation, "vars.color").is_none());
    }

    #[test]
    fn aliased_import_and_multi_file_counting() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let def = write_file(
            root,
            0,
            "src/t.stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { primary: '#000' } });\n",
        );
        let c1 = write_file(
            root,
            1,
            "src/a.ts",
            "import { vars as v } from './t.stylex';\nexport const x = v.color.primary;\n",
        );
        let c2 = write_file(
            root,
            2,
            "src/b.ts",
            "import { vars } from './t.stylex';\nexport const y = vars.color.primary;\n",
        );
        let computation = css_computation_3d(root, &[def, c1, c2]);
        let primary = find_token(&computation, "vars.color.primary").expect("token present");
        assert_eq!(
            primary.consumer_count, 2,
            "aliased + plain imports both counted across files"
        );
        let paths: Vec<&str> = primary.consumers.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"src/a.ts") && paths.contains(&"src/b.ts"));
    }

    #[test]
    fn non_css_in_js_project_emits_no_js_member_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"react":"18.0.0"}}"#,
        )
        .unwrap();
        let f = write_file(
            root,
            0,
            "src/x.ts",
            "export const vars = { color: { primary: '#000' } };\nexport const y = vars.color.primary;\n",
        );
        let modules = vec![fallow_extract::parse_source_to_module(
            f.id,
            &f.path,
            &std::fs::read_to_string(&f.path).unwrap(),
            0,
            false,
        )];
        let config = config_at(root);
        let computation = compute_css_analytics_report(
            &[f],
            &modules,
            HealthScanCtx {
                config: &config,
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                output_changed_files: None,
                ws_roots: None,
            },
        );
        // No CSS-in-JS deps -> the gate is closed; whether or not css_analytics is
        // None, there are no js-member token consumers.
        if let Some(computation) = computation {
            assert!(js_token_consumers(&computation).is_empty());
        }
    }

    #[test]
    fn vanilla_extract_object_styles_feed_css_analytics_and_grade() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@vanilla-extract/css":"1.0.0"}}"#,
        )
        .unwrap();
        // Two identical 4-declaration style() buckets -> a duplicate block; two
        // distinct colors -> token sprawl. vanilla-extract is non-atomic.
        let file = write_file(
            root,
            0,
            "src/styles.css.ts",
            "import { style } from '@vanilla-extract/css';\n\
             export const a = style({ color: 'red', padding: 8, margin: 4, top: 1 });\n\
             export const b = style({ color: 'red', padding: 8, margin: 4, top: 1 });\n\
             export const c = style({ color: 'blue' });\n",
        );
        let computation = css_computation(root, &[file]).expect("css_analytics is non-null");
        let report = &computation.report;
        assert!(
            report.summary.files_analyzed >= 1,
            "object styles analyzed: {:?}",
            report.summary
        );
        assert!(
            report.summary.unique_colors >= 2,
            "distinct colors counted from object styles: {:?}",
            report.summary
        );
        assert!(
            !report.duplicate_declaration_blocks.is_empty(),
            "identical object buckets surface a duplicate block",
        );
        // Non-atomic: the declarations feed the grade inputs, no atomic.
        assert!(computation.scoring_inputs.non_atomic_declarations >= 8);
        assert_eq!(computation.scoring_inputs.atomic_declarations, 0);
        let styling = crate::health::styling_score::compute_styling_health_with_inputs(
            report,
            &computation.scoring_inputs,
        );
        // A real (non-inflated) grade with a real duplication penalty.
        assert!(styling.penalties.duplication > 0.0, "duplication penalized");
    }

    #[test]
    fn stylex_atomic_styles_do_not_inflate_grade() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
        )
        .unwrap();
        let file = write_file(
            root,
            0,
            "src/styles.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const s = stylex.create({\n\
             root: { color: 'red', padding: 16, margin: 8, fontSize: 14 },\n\
             card: { color: 'blue', display: 'flex' },\n\
             });\n",
        );
        let computation = css_computation(root, &[file]).expect("css_analytics is non-null");
        let report = &computation.report;
        // Token sprawl IS fed for atomic CSS (two distinct colors).
        assert!(
            report.summary.unique_colors >= 2,
            "atomic token sprawl counted: {:?}",
            report.summary
        );
        // Atomic declarations are tracked but excluded from the grade inputs.
        assert!(computation.scoring_inputs.atomic_declarations >= 4);
        assert_eq!(
            computation.scoring_inputs.non_atomic_declarations, 0,
            "no non-atomic gradeable surface in a pure-StyleX project",
        );
        let styling = crate::health::styling_score::compute_styling_health_with_inputs(
            report,
            &computation.scoring_inputs,
        );
        // The structural penalty is not driven up OR down by the flat atomic
        // rules (computed over the empty non-atomic surface), and the grade is
        // marked low-confidence with the atomic reason rather than a confident A.
        assert_eq!(
            styling.confidence,
            fallow_output::StylingHealthConfidence::Low,
            "predominantly-atomic project is low-confidence",
        );
        let reason = styling.confidence_reason.expect("atomic caveat");
        assert!(
            reason.contains("compile-time-atomic"),
            "atomic reason names non-assessability: {reason:?}",
        );
    }

    #[test]
    fn non_object_css_in_js_project_is_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // No CSS-in-JS dependency declared at all.
        std::fs::write(root.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        // A local `style({...})` helper that LOOKS like vanilla-extract but is not
        // gated in: the JS/TS arm is never scanned, so there is nothing to analyze.
        let file = write_file(
            root,
            0,
            "src/styles.ts",
            "const style = (o) => o;\n\
             export const a = style({ color: 'red', padding: 8, margin: 4, top: 1 });\n",
        );
        assert!(
            css_computation(root, &[file]).is_none(),
            "a project with no CSS-in-JS deps yields no CSS analytics (byte-identical to pre-3c)",
        );
    }
}
