use super::*;
use fallow_config::{FallowConfig, OutputFormat};
use fallow_output::ConsumerKind;
use fallow_types::discover::{DiscoveredFile, FileId};
use std::path::Path;

/// Resolve a default config rooted at `root`.
fn config_at(root: &Path) -> ResolvedConfig {
    FallowConfig::default().resolve(root.to_path_buf(), OutputFormat::Human, 1, true, true, None)
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

/// Compare the uncached and session-artifact CSS paths for one project. The
/// complete serialized reports must stay identical because the artifact cache
/// is an execution detail, not an analysis-mode switch.
fn assert_cached_css_report_parity(
    root: &Path,
    files: &[DiscoveredFile],
    health_ignore: &[&str],
) -> CssAnalyticsComputation {
    let mut config = config_at(root);
    config.health.ignore = health_ignore
        .iter()
        .map(|pattern| (*pattern).to_owned())
        .collect();
    let ignore_set = super::super::ignore::build_ignore_set(&config.health.ignore);
    let ctx = HealthScanCtx {
        config: &config,
        ignore_set: &ignore_set,
        changed_files: None,
        output_changed_files: None,
        ws_roots: None,
    };
    let fresh = compute_css_analytics_report_with_artifacts(files, &[], ctx, None)
        .expect("fresh CSS report is present");
    let artifacts = build_styling_analysis_artifacts(files, &config);
    let cached = compute_css_analytics_report_with_artifacts(files, &[], ctx, Some(&artifacts))
        .expect("cached CSS report is present");

    assert_eq!(
        serde_json::to_value(&cached.report).unwrap(),
        serde_json::to_value(&fresh.report).unwrap(),
        "cached class inventory must preserve the complete CSS report"
    );
    cached
}

#[test]
fn cached_class_inventory_preserves_vue_style_typo_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"vue-classes"}"#).unwrap();
    let component = write_file(
        root,
        0,
        "src/Card.vue",
        "<template><section class=\"panel-tile exact-card\" /></template>\n\
         <style scoped>\n.panel-title { color: red; }\n.exact-card { display: block; }\n</style>\n",
    );

    let computation = assert_cached_css_report_parity(root, &[component], &[]);
    let unresolved = &computation.report.unresolved_class_references;
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].class, "panel-tile");
    assert_eq!(unresolved[0].suggestion, "panel-title");
}

#[test]
fn cached_class_inventory_preserves_health_ignore_for_typo_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"ignored-classes"}"#).unwrap();
    let ignored = write_file(
        root,
        0,
        "ignored/legacy.css",
        ".ignored-card { color: red; }\n",
    );
    let visible = write_file(root, 1, "src/app.css", ".visible-card { color: blue; }\n");
    let markup = write_file(
        root,
        2,
        "src/App.tsx",
        "export const App = () => <div className=\"ignored-cord visible-card\" />;\n",
    );

    let computation =
        assert_cached_css_report_parity(root, &[ignored, visible, markup], &["ignored/**"]);
    assert!(
        computation.report.unresolved_class_references.is_empty(),
        "an ignored stylesheet must not contribute typo targets"
    );
}

#[test]
fn cached_class_inventory_preserves_preprocessor_abstention_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"preprocessor-classes"}"#,
    )
    .unwrap();
    let css = write_file(root, 0, "src/app.css", ".sidebar-nav { color: red; }\n");
    let scss = write_file(root, 1, "src/theme.scss", ".theme-card { color: blue; }\n");
    let less = write_file(
        root,
        2,
        "src/legacy.less",
        ".legacy-card { color: gray; }\n",
    );
    let markup = write_file(
        root,
        3,
        "src/App.tsx",
        "export const App = () => <nav className=\"sidebar-nev\" />;\n",
    );

    let computation = assert_cached_css_report_parity(root, &[css, scss, less, markup], &[]);
    assert!(computation.report.unresolved_class_references.is_empty());
    assert_eq!(computation.report.summary.preprocessor_stylesheets, 2);
    assert!(
        computation
            .report
            .summary
            .preprocessor_reachability_abstained
    );
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

#[test]
fn lazy_styling_candidates_preserve_raw_style_annotations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
    )
    .unwrap();
    let styles = write_file(
        root,
        0,
        "src/styles.css",
        "@theme { --color-brand: #f05a28; }\n.card { color: #f05a29; }\n",
    );

    let computation = css_computation(root, &[styles]).expect("raw CSS keeps report");
    let raw = computation
        .report
        .raw_style_values
        .iter()
        .find(|raw| raw.value == "#f05a29")
        .expect("raw style value is reported");
    assert_eq!(
        raw.nearest_token.as_ref().map(|token| token.name.as_str()),
        Some("--color-brand")
    );
}

#[test]
fn lazy_styling_candidates_preserve_cva_variant_drift() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"class-variance-authority":"0.7.0","tailwindcss":"4.0.0"}}"#,
    )
    .unwrap();
    let theme = write_file(
        root,
        0,
        "src/theme.css",
        "@theme {\n  --color-brand: #f05a28;\n}\n",
    );
    let button = write_file(
        root,
        1,
        "src/button.ts",
        "import { cva } from 'class-variance-authority';\n\
         export const button = cva('inline-flex', {\n\
           variants: {\n\
             tone: {\n\
               primary: 'px-3 py-2 text-sm font-medium bg-[#f05a28]',\n\
               secondary: 'px-3 py-2 text-sm font-medium bg-[#f05a28]',\n\
             },\n\
           },\n\
         });\n",
    );

    let computation = css_computation(root, &[theme, button]).expect("CVA drift keeps report");
    let drift = computation
        .report
        .cva_variant_token_drifts
        .first()
        .expect("CVA arbitrary value points at a theme token");
    assert_eq!(drift.class_token, "bg-[#f05a28]");
    assert_eq!(drift.nearest_token.name, "--color-brand");
}

#[test]
fn lazy_styling_candidates_preserve_css_deep_near_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"tailwindcss":"4.0.0"}}"#,
    )
    .unwrap();
    let theme = write_file(
        root,
        0,
        "src/theme.css",
        "@theme {\n  --color-zbrand: #f05a28;\n  --color-abrand: rgb(240 90 41);\n}\n",
    );
    let mut changed = rustc_hash::FxHashSet::default();
    changed.insert(theme.path.clone());

    let computation = css_computation_3d_with_output_changed_files(root, &[theme], Some(&changed));
    let near_duplicates = &computation.report.near_duplicate_theme_tokens;
    assert!(near_duplicates.iter().any(|candidate| {
        candidate.token == "--color-abrand" && candidate.nearest_token.name == "--color-zbrand"
    }));
}

// --- CSS program Phase 3d: CSS-in-JS design-token blast-radius ---

/// Like [`css_computation`] but parses each file into a `ModuleInfo` so the
/// Phase 3d CSS-in-JS token-consumer driver (which reads imports +
/// member-access) actually runs.
fn css_computation_3d(root: &Path, files: &[DiscoveredFile]) -> CssAnalyticsComputation {
    css_computation_3d_with_output_changed_files(root, files, None)
}

fn css_computation_3d_with_output_changed_files(
    root: &Path,
    files: &[DiscoveredFile],
    output_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> CssAnalyticsComputation {
    css_computation_3d_with_filters(root, files, None, output_changed_files, None)
}

fn css_computation_3d_with_filters(
    root: &Path,
    files: &[DiscoveredFile],
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    output_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
) -> CssAnalyticsComputation {
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
            changed_files,
            output_changed_files,
            ws_roots,
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
    let secondary = find_token(&computation, "vars.color.secondary").expect("secondary present");
    assert_eq!(secondary.consumer_count, 0);
}

#[test]
fn stylex_define_vars_reports_near_duplicate_css_in_js_tokens_in_deep_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
    )
    .unwrap();
    let base = write_file(
        root,
        0,
        "src/base-tokens.stylex.ts",
        "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { brand: '#33679a' } });\n",
    );
    let feature = write_file(
        root,
        1,
        "src/feature-tokens.stylex.ts",
        "import * as stylex from '@stylexjs/stylex';\n\
             export const featureVars = stylex.defineVars({ color: { accent: '#33679b' } });\n",
    );
    let mut changed = rustc_hash::FxHashSet::default();
    changed.insert(feature.path.clone());

    let computation =
        css_computation_3d_with_output_changed_files(root, &[base, feature], Some(&changed));
    let candidates = &computation.report.near_duplicate_css_in_js_tokens;
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].token, "featureVars.color.accent");
    assert_eq!(candidates[0].value, "#33679b");
    assert_eq!(candidates[0].nearest_token.name, "vars.color.brand");
    assert_eq!(candidates[0].path, "src/feature-tokens.stylex.ts");
    assert_eq!(
        computation.report.summary.near_duplicate_css_in_js_tokens,
        1
    );
}

#[test]
fn stylex_near_duplicate_css_in_js_tokens_abstain_in_partial_scope() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"@stylexjs/stylex":"0.1.0"}}"#,
    )
    .unwrap();
    let base = write_file(
        root,
        0,
        "src/base-tokens.stylex.ts",
        "import * as stylex from '@stylexjs/stylex';\n\
             export const vars = stylex.defineVars({ color: { brand: '#33679a' } });\n",
    );
    let feature = write_file(
        root,
        1,
        "src/feature-tokens.stylex.ts",
        "import * as stylex from '@stylexjs/stylex';\n\
             export const featureVars = stylex.defineVars({ color: { accent: '#33679b' } });\n",
    );
    let mut changed = rustc_hash::FxHashSet::default();
    changed.insert(feature.path.clone());

    let computation = css_computation_3d_with_filters(
        root,
        &[base, feature],
        Some(&changed),
        Some(&changed),
        None,
    );
    assert!(
        computation
            .report
            .near_duplicate_css_in_js_tokens
            .is_empty()
    );
    assert_eq!(
        computation.report.summary.near_duplicate_css_in_js_tokens,
        0
    );
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
    let brand = find_token(&computation, "vars.color.brand").expect("brand blast radius present");
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
    let brand =
        find_token(&computation, "tokens.colors.brand").expect("Panda token blast radius present");
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
    let accent = find_token(&computation, "tokens.colors.accent").expect("accent token present");
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

    let brand = find_token(&computation, "pandaConfig.colors.brand").expect("config token present");
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
    let brand =
        find_token(&computation, "tokens.colors.brand").expect("Panda token blast radius present");
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
    let brand = find_token(&computation, "vars.color.brand").expect("brand blast radius present");
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
