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
    pub(super) token_candidates: &'a [ComparableThemeTokenCandidate],
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
    MarkupReferenceCandidates {
        unresolved_class_references: scan_unresolved_class_references(
            input.files,
            ctx,
            input.summary,
        ),
        unreferenced_css_classes: scan_unreferenced_css_classes(
            input.files,
            ctx,
            input.summary,
            input
                .styling_artifacts
                .map(|artifacts| &artifacts.reference_surface),
            input
                .styling_artifacts
                .map(|artifacts| &artifacts.class_inventory),
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

pub(super) fn project_uses_css_in_js(root: &std::path::Path) -> bool {
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
pub(super) enum CssScanKind {
    Css,
    Preprocessor,
    Sfc,
    CssInJs,
}

pub(super) fn css_report_scan_target<'a>(
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
    pub(super) policy: GradePolicy,
    pub(super) report_notable: bool,
}

pub(super) fn css_report_scan_items<'a>(
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
        CssScanKind::Sfc => sfc_css_scan_items(source),
        CssScanKind::CssInJs => css_in_js_scan_items(source, path),
    }
}

fn sfc_css_scan_items(source: &str) -> Vec<CssScanItem<'_>> {
    use std::borrow::Cow;

    let mut items = Vec::new();
    if let Some(virtual_css) = crate::css::sfc_virtual_stylesheet(source) {
        items.push(CssScanItem {
            source: Cow::Owned(virtual_css),
            policy: GradePolicy::Structural,
            report_notable: true,
        });
    }
    if let Some(preprocessor_source) = crate::css::sfc_preprocessor_virtual_stylesheet(source)
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

fn css_in_js_scan_items<'a>(source: &'a str, path: &std::path::Path) -> Vec<CssScanItem<'a>> {
    use std::borrow::Cow;

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
