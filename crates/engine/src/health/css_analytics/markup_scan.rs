use super::*;

/// The markup / source-derived CSS candidate lists, gathered in one pass-set so
/// the orchestrator stays a thin assembler.
pub(super) struct MarkupCssCandidates {
    pub(super) tailwind_arbitrary_values: Vec<fallow_output::TailwindArbitraryValue>,
    pub(super) cva_duplicate_variant_blocks: Vec<fallow_output::CvaDuplicateVariantBlock>,
    pub(super) cva_variant_token_drifts: Vec<fallow_output::CvaVariantTokenDrift>,
    pub(super) unresolved_class_references: Vec<fallow_output::UnresolvedClassReference>,
    pub(super) unreferenced_css_classes: Vec<fallow_output::UnreferencedCssClass>,
    pub(super) unused_theme_tokens: Vec<fallow_output::UnusedThemeToken>,
    pub(super) near_duplicate_theme_tokens: Vec<fallow_output::NearDuplicateThemeToken>,
    pub(super) near_duplicate_css_in_js_tokens: Vec<fallow_output::NearDuplicateThemeToken>,
}

struct MarkupTokenCandidates {
    tailwind_arbitrary_values: Vec<fallow_output::TailwindArbitraryValue>,
    cva_duplicate_variant_blocks: Vec<fallow_output::CvaDuplicateVariantBlock>,
    cva_variant_token_drifts: Vec<fallow_output::CvaVariantTokenDrift>,
}

struct MarkupReferenceCandidates {
    unresolved_class_references: Vec<fallow_output::UnresolvedClassReference>,
    unreferenced_css_classes: Vec<fallow_output::UnreferencedCssClass>,
}

struct ThemeTokenCandidates {
    unused: Vec<fallow_output::UnusedThemeToken>,
    near_duplicates: Vec<fallow_output::NearDuplicateThemeToken>,
    css_in_js_near_duplicates: Vec<fallow_output::NearDuplicateThemeToken>,
}

/// Run the markup / source-scanning CSS candidates (Tailwind arbitrary values,
/// likely class typos, unreferenced global classes, unused `@theme` tokens),
/// each honoring the same ignore / changed / workspace filters and setting its
/// own summary counts.
pub(super) struct MarkupCssCandidateInput<'a> {
    pub(super) tokens: &'a CssTokenSets,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) css_in_js_definers: Option<&'a CssInJsDefiners>,
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) css_deep: bool,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) styling_artifacts: Option<&'a StylingAnalysisArtifacts>,
    pub(super) token_candidates: &'a StylingTokenCandidateCache<'a>,
    pub(super) summary: &'a mut fallow_output::CssAnalyticsSummary,
}

pub(super) fn scan_markup_css_candidates(
    input: &mut MarkupCssCandidateInput<'_>,
) -> MarkupCssCandidates {
    let markup = scan_markup_token_candidates(input);
    let references = scan_markup_reference_candidates(input);
    let theme = scan_theme_token_candidates(input);

    MarkupCssCandidates {
        tailwind_arbitrary_values: markup.tailwind_arbitrary_values,
        cva_duplicate_variant_blocks: markup.cva_duplicate_variant_blocks,
        cva_variant_token_drifts: markup.cva_variant_token_drifts,
        unresolved_class_references: references.unresolved_class_references,
        unreferenced_css_classes: references.unreferenced_css_classes,
        unused_theme_tokens: theme.unused,
        near_duplicate_theme_tokens: theme.near_duplicates,
        near_duplicate_css_in_js_tokens: theme.css_in_js_near_duplicates,
    }
}

fn scan_markup_token_candidates(input: &mut MarkupCssCandidateInput<'_>) -> MarkupTokenCandidates {
    let ctx = markup_scan_ctx(input);
    MarkupTokenCandidates {
        tailwind_arbitrary_values: scan_markup_tailwind_arbitrary_values(
            input.files,
            ctx,
            input.summary,
        ),
        cva_duplicate_variant_blocks: scan_cva_duplicate_variant_blocks(input.files, ctx),
        cva_variant_token_drifts: scan_cva_variant_token_drifts(
            input.files,
            ctx,
            input.token_candidates,
        ),
    }
}

fn scan_markup_reference_candidates(
    input: &mut MarkupCssCandidateInput<'_>,
) -> MarkupReferenceCandidates {
    let ctx = markup_scan_ctx(input);
    let fallback_class_inventory;
    let class_inventory = if let Some(artifacts) = input.styling_artifacts {
        &artifacts.class_inventory
    } else {
        fallback_class_inventory = css_class_inventory(input.files, input.config, input.ignore_set);
        &fallback_class_inventory
    };
    MarkupReferenceCandidates {
        unresolved_class_references: scan_unresolved_class_references(
            input.files,
            ctx,
            input.summary,
            Some(class_inventory),
        ),
        unreferenced_css_classes: scan_unreferenced_css_classes(
            input.files,
            ctx,
            input.summary,
            input
                .styling_artifacts
                .map(|artifacts| &artifacts.reference_surface),
            Some(class_inventory),
        ),
    }
}

fn scan_theme_token_candidates(input: &mut MarkupCssCandidateInput<'_>) -> ThemeTokenCandidates {
    let unused_theme_tokens = scan_unused_theme_tokens(&mut UnusedThemeTokenScanInput {
        tokens: input.tokens,
        files: input.files,
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        output_changed_files: input.output_changed_files,
        ws_roots: input.ws_roots,
        summary: input.summary,
    });
    let near_duplicate_theme_tokens = if input.css_deep {
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
    };
    let near_duplicate_css_in_js_tokens = if input.css_deep {
        scan_near_duplicate_css_in_js_tokens(&mut NearDuplicateCssInJsTokenScanInput {
            config: input.config,
            changed_files: input.changed_files,
            output_changed_files: input.output_changed_files,
            ws_roots: input.ws_roots,
            summary: input.summary,
            css_in_js_definers: input.css_in_js_definers,
        })
    } else {
        Vec::new()
    };

    ThemeTokenCandidates {
        unused: unused_theme_tokens,
        near_duplicates: near_duplicate_theme_tokens,
        css_in_js_near_duplicates: near_duplicate_css_in_js_tokens,
    }
}

fn markup_scan_ctx<'a>(input: &MarkupCssCandidateInput<'a>) -> HealthScanCtx<'a> {
    HealthScanCtx {
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        output_changed_files: None,
        ws_roots: input.ws_roots,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CssScanKind {
    Css,
    Preprocessor,
    Sfc,
    CssInJs,
}

pub(super) fn css_report_scan_target<'a>(
    file: &'a fallow_types::discover::DiscoveredFile,
    ctx: HealthScanCtx<'_>,
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
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts") => CssScanKind::CssInJs,
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

pub(super) fn record_scoped_unused_classes(
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
pub(super) enum GradePolicy {
    Structural,
    StructuralNoDedup,
    Atomic,
}

pub(super) struct CssScanItem<'a> {
    pub(super) source: std::borrow::Cow<'a, str>,
    /// Further stylesheets of the same source whose analytics merge into
    /// `source`'s (hoisted Sass parent-suffix rules).
    pub(super) layers: Vec<String>,
    pub(super) policy: GradePolicy,
    pub(super) report_notable: bool,
}

impl<'a> CssScanItem<'a> {
    fn new(source: std::borrow::Cow<'a, str>, policy: GradePolicy, report_notable: bool) -> Self {
        Self {
            source,
            layers: Vec::new(),
            policy,
            report_notable,
        }
    }
}

pub(super) fn css_report_scan_items<'a>(
    source: &'a str,
    path: &std::path::Path,
    kind: CssScanKind,
) -> Vec<CssScanItem<'a>> {
    use std::borrow::Cow;
    match kind {
        CssScanKind::Css => vec![CssScanItem::new(
            Cow::Borrowed(source),
            GradePolicy::Structural,
            true,
        )],
        CssScanKind::Preprocessor => preprocessor_scan_item(source).into_iter().collect(),
        CssScanKind::Sfc => sfc_css_scan_items(source),
        CssScanKind::CssInJs => css_in_js_scan_items(source, path),
    }
}

fn preprocessor_scan_item(source: &str) -> Option<CssScanItem<'static>> {
    let mut layers = preprocessor_virtual_stylesheets(source).into_iter();
    let main = layers.next()?;
    let mut item = CssScanItem::new(std::borrow::Cow::Owned(main), GradePolicy::Structural, true);
    item.layers = layers.collect();
    Some(item)
}

fn sfc_css_scan_items(source: &str) -> Vec<CssScanItem<'_>> {
    use std::borrow::Cow;

    let mut items = Vec::new();
    if let Some(virtual_css) = crate::css::sfc_virtual_stylesheet(source) {
        items.push(CssScanItem::new(
            Cow::Owned(virtual_css),
            GradePolicy::Structural,
            true,
        ));
    }
    if let Some(preprocessor_source) = crate::css::sfc_preprocessor_virtual_stylesheet(source)
        && let Some(item) = preprocessor_scan_item(&preprocessor_source)
    {
        items.push(item);
    }
    items
}

fn css_in_js_scan_items<'a>(source: &'a str, path: &std::path::Path) -> Vec<CssScanItem<'a>> {
    use std::borrow::Cow;

    if !source_may_import_css_in_js(source) {
        return Vec::new();
    }
    let mut items = Vec::new();
    if let Some(virtual_css) = crate::css::css_in_js_virtual_stylesheet(source) {
        items.push(CssScanItem::new(
            Cow::Owned(virtual_css),
            GradePolicy::Structural,
            true,
        ));
    }
    let sheets = crate::css::css_in_js_object_sheets(source, path);
    if let Some(structural) = sheets.structural {
        items.push(CssScanItem::new(
            Cow::Owned(structural),
            GradePolicy::Structural,
            false,
        ));
    }
    if let Some(partial) = sheets.structural_partial {
        items.push(CssScanItem::new(
            Cow::Owned(partial),
            GradePolicy::StructuralNoDedup,
            false,
        ));
    }
    if let Some(atomic) = sheets.atomic {
        items.push(CssScanItem::new(
            Cow::Owned(atomic),
            GradePolicy::Atomic,
            false,
        ));
    }
    items
}

fn source_may_import_css_in_js(source: &str) -> bool {
    const MARKERS: &[&str] = &[
        "styled-components",
        "@emotion/styled",
        "@emotion/react",
        "@emotion/css",
        "@linaria/core",
        "@linaria/react",
        "@vanilla-extract/css",
        "@vanilla-extract/recipes",
        "@pandacss/dev",
        "@stylexjs/stylex",
        "styled-system",
    ];
    MARKERS.iter().any(|marker| source.contains(marker))
        || source.contains("'stylex'")
        || source.contains("\"stylex\"")
}

pub(super) fn is_css_in_js_style_source(specifier: &str) -> bool {
    matches!(
        specifier,
        "styled-components"
            | "@emotion/styled"
            | "@emotion/react"
            | "@emotion/css"
            | "@linaria/core"
            | "@linaria/react"
            | "@vanilla-extract/css"
            | "@vanilla-extract/recipes"
            | "@pandacss/dev"
            | "@stylexjs/stylex"
            | "stylex"
    ) || specifier
        .split(['/', '\\'])
        .any(|segment| segment == "styled-system")
}
