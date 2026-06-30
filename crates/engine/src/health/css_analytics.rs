//! CSS analytics execution for `fallow health`.

use fallow_config::ResolvedConfig;

use super::package_json::{
    class_matches_dependency_prefix, dependency_class_prefixes, project_uses_tailwind,
    project_uses_tailwind_plugin, published_css_paths,
};
use super::tailwind_theme;

/// The per-run scan filters shared by every CSS and markup health scanner:
/// resolved config, the ignore globset, the optional changed-file set, and
/// the optional workspace roots.
#[derive(Clone, Copy)]
pub(super) struct HealthScanCtx<'a> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
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
#[derive(Default)]
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
    /// `(path, line)`, for the unused-theme-token candidate.
    theme_token_definers: rustc_hash::FxHashMap<String, (String, u32)>,
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
                .or_insert_with(|| (rel.to_owned(), token.line));
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
/// `.svelte`) for Tailwind arbitrary-value utility tokens, honoring the same
/// ignore / changed / workspace filters as the CSS scan. Aggregates by token
/// (total count + first location), sets the summary counts, and returns the
/// located list sorted by use count descending.
/// One eligible markup file (`jsx`/`tsx`/`html`/`astro`/`vue`/`svelte`) for a
/// class-token scan: the forward-slash relative path plus source, or `None` when
/// the file is filtered out (extension, ignore set, changed-files, workspace
/// scope) or unreadable.
fn read_markup_scan_source(
    file: &fallow_types::discover::DiscoveredFile,
    ctx: HealthScanCtx<'_>,
) -> Option<(String, String)> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        ws_roots,
    } = ctx;

    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    if !matches!(
        extension,
        Some("jsx" | "tsx" | "html" | "astro" | "vue" | "svelte")
    ) {
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
    if preprocessor_files > css_files {
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

/// Per-stylesheet located class definitions from STANDALONE `.css`/`.scss` files
/// (not SFC `<style>` blocks, which are component-scoped and covered by the
/// scoped-unused check). Returns `(rel_path, [(class, 1-based line)])`, each
/// class deduped to its first definition. The defined surface for the
/// unreferenced-global-class candidate. Classes wrapped in `:global(...)` are
/// dropped: they target externally-applied DOM and are never authored in markup.
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
        let is_scss = extension == Some("scss");
        if extension != Some("css") && !is_scss {
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
        for export in crate::css::extract_css_module_exports(&source, is_scss) {
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
) -> Vec<fallow_output::UnreferencedCssClass> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        ws_roots,
    } = ctx;

    use fallow_output::UnreferencedCssClass;

    // Partial scope cannot prove a global class dead.
    if changed_files.is_some() || ws_roots.is_some() {
        return Vec::new();
    }
    // Preprocessor-dominant projects have an unreliable defined/used join.
    let (css_files, preprocessor_files) = count_stylesheet_kinds(files, config, ignore_set);
    if preprocessor_files > css_files {
        return Vec::new();
    }

    let reference_surface = css_reference_surface(files, config, ignore_set);

    let published = published_css_paths(config);
    let dependency_prefixes = dependency_class_prefixes(config);
    let located = collect_defined_css_classes_located(files, config, ignore_set);

    let mut out: Vec<UnreferencedCssClass> = Vec::new();
    for (rel, classes) in located {
        push_unreferenced_css_class_candidates(
            &mut out,
            &rel,
            classes,
            &published,
            &dependency_prefixes,
            &reference_surface,
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

struct CssReferenceSurface {
    static_tokens: rustc_hash::FxHashSet<String>,
    dynamic_corpus: String,
}

impl CssReferenceSurface {
    fn references(&self, class: &str) -> bool {
        self.static_tokens.contains(class)
            || self.dynamic_corpus.contains(class)
            || self.dynamic_prefix_referenced(class)
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
}

fn css_reference_surface(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssReferenceSurface {
    let mut surface = CssReferenceSurface {
        static_tokens: rustc_hash::FxHashSet::default(),
        dynamic_corpus: String::new(),
    };
    for file in files {
        collect_css_reference_surface_file(&mut surface, file, config, ignore_set);
    }
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
    if !matches!(
        extension,
        Some("jsx" | "tsx" | "html" | "astro" | "vue" | "svelte")
    ) {
        return;
    }
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    if ignore_set.is_match(relative) {
        return;
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let scan = crate::css::scan_markup_class_tokens(&source);
    for token in scan.static_tokens {
        surface.static_tokens.insert(token.value);
    }
    collect_quoted_class_tokens(&source, &mut surface.static_tokens, true);
    if scan.has_dynamic {
        surface.dynamic_corpus.push_str(&source);
        surface.dynamic_corpus.push('\n');
    }
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
    ws_roots: Option<&'a [std::path::PathBuf]>,
    summary: &'a mut fallow_output::CssAnalyticsSummary,
}

/// A classified `@theme` token candidate (namespace + name + definition site)
/// surviving the variant / published-library / unknown-namespace filters.
struct ThemeTokenCandidate {
    token: String,
    namespace: String,
    name: String,
    path: String,
    line: u32,
}

/// Classify the project's `@theme` token definers, dropping variant namespaces,
/// published-library stylesheets, and anything outside a known namespace.
fn classify_theme_token_candidates(
    input: &UnusedThemeTokenScanInput<'_>,
) -> Vec<ThemeTokenCandidate> {
    let published = published_css_paths(input.config);
    let mut candidates: Vec<ThemeTokenCandidate> = Vec::new();
    for (raw, (path, line)) in &input.tokens.theme_token_definers {
        if published.contains(path) {
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
            path: path.clone(),
            line: *line,
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

fn consumer_kind_rank(kind: fallow_output::ConsumerKind) -> u8 {
    use fallow_output::ConsumerKind;
    match kind {
        ConsumerKind::ThemeVar => 0,
        ConsumerKind::CssVar => 1,
        ConsumerKind::Utility => 2,
        ConsumerKind::Apply => 3,
    }
}

/// The markup / source-derived CSS candidate lists, gathered in one pass-set so
/// the orchestrator stays a thin assembler.
struct MarkupCssCandidates {
    tailwind_arbitrary_values: Vec<fallow_output::TailwindArbitraryValue>,
    unresolved_class_references: Vec<fallow_output::UnresolvedClassReference>,
    unreferenced_css_classes: Vec<fallow_output::UnreferencedCssClass>,
    unused_theme_tokens: Vec<fallow_output::UnusedThemeToken>,
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
    ws_roots: Option<&'a [std::path::PathBuf]>,
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
                ws_roots: input.ws_roots,
            },
            input.summary,
        ),
        // Static markup class tokens one edit from a defined class (likely typos).
        unresolved_class_references: scan_unresolved_class_references(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
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
                ws_roots: input.ws_roots,
            },
            input.summary,
        ),
        // Tailwind v4 @theme design tokens used by no utility / var() / @apply
        // anywhere (heavily gated: v4 + non-plugin + non-published + whole-scope).
        unused_theme_tokens: scan_unused_theme_tokens(&mut UnusedThemeTokenScanInput {
            tokens: input.tokens,
            files: input.files,
            config: input.config,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            summary: input.summary,
        }),
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
        ws_roots,
    } = ctx;

    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    let kind = match extension {
        Some("css") => CssScanKind::Css,
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
        CssScanKind::Sfc => crate::css::sfc_virtual_stylesheet(source)
            .map(|virtual_css| {
                vec![CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                }]
            })
            .unwrap_or_default(),
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
struct CssWalkAccum {
    file_reports: Vec<fallow_output::CssFileAnalytics>,
    summary: fallow_output::CssAnalyticsSummary,
    scoped_unused: Vec<fallow_output::ScopedUnusedClasses>,
    tokens: CssTokenSets,
    scoring: CssGradeScoring,
}

#[derive(Default)]
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

pub(super) fn compute_css_analytics_report(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
) -> Option<CssAnalyticsComputation> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        ws_roots,
    } = ctx;

    let mut walk = walk_css_files(files, ctx);
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
        ws_roots,
        summary: &mut walk.summary,
    });
    let token_consumers = build_token_consumers(&TokenConsumersInput {
        tokens: &walk.tokens,
        files,
        config,
        ignore_set,
        changed_files,
        ws_roots,
    });
    let scoring_inputs = super::styling_score::StylingScoringInputs {
        theme_tokens_defined: saturate_len(walk.tokens.theme_token_definers.len()),
        non_atomic_declarations: walk.scoring.non_atomic_declarations,
        non_atomic_important_declarations: walk.scoring.non_atomic_important_declarations,
        non_atomic_max_nesting_depth: walk.scoring.non_atomic_max_nesting_depth,
        atomic_declarations: walk.scoring.atomic_declarations,
    };
    let report = assemble_css_report(walk, metrics, candidates, token_consumers)?;
    Some(CssAnalyticsComputation {
        report,
        scoring_inputs,
    })
}

/// Assemble the final CSS analytics report from the walk accumulator, finalized
/// token metrics, and markup candidates; returns `None` when nothing notable was
/// found (no analyzed files and every candidate list empty).
fn assemble_css_report(
    walk: CssWalkAccum,
    metrics: CssTokenMetrics,
    candidates: MarkupCssCandidates,
    token_consumers: Vec<fallow_output::TokenConsumers>,
) -> Option<fallow_output::CssAnalyticsReport> {
    use fallow_output::CssAnalyticsReport;

    let candidates_empty = candidates.tailwind_arbitrary_values.is_empty()
        && candidates.unresolved_class_references.is_empty()
        && candidates.unreferenced_css_classes.is_empty()
        && metrics.unused_font_faces.is_empty()
        && candidates.unused_theme_tokens.is_empty()
        && token_consumers.is_empty();
    if walk.summary.files_analyzed == 0 && walk.scoped_unused.is_empty() && candidates_empty {
        return None;
    }
    let mut scoped_unused = walk.scoped_unused;
    scoped_unused.sort_by(|a, b| a.path.cmp(&b.path));
    Some(CssAnalyticsReport {
        files: walk.file_reports,
        summary: walk.summary,
        scoped_unused,
        unreferenced_keyframes: metrics.unreferenced_keyframes,
        undefined_keyframes: metrics.undefined_keyframes,
        duplicate_declaration_blocks: metrics.duplicate_declaration_blocks,
        tailwind_arbitrary_values: candidates.tailwind_arbitrary_values,
        unused_at_rules: metrics.unused_at_rules,
        unresolved_class_references: candidates.unresolved_class_references,
        unreferenced_css_classes: candidates.unreferenced_css_classes,
        unused_font_faces: metrics.unused_font_faces,
        unused_theme_tokens: candidates.unused_theme_tokens,
        token_consumers,
        font_size_unit_mix: metrics.font_size_unit_mix,
    })
}
