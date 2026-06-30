//! Command-neutral health analysis execution.
//!
//! This module owns the health pipeline (scoring, hotspots, targets, grouping,
//! coverage gaps, vital signs, report assembly) so that the CLI and the
//! programmatic API can both run health analysis without the CLI orchestration
//! layer. CLI-only concerns (config loading, telemetry sinks, the runtime
//! coverage sidecar, ownership-resolver construction, and error rendering) are
//! threaded in through the [`HealthSeams`] carrier and the typed result.

#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "faithful move of the health pipeline from fallow-cli; structure preserved verbatim"
)]
#![allow(
    clippy::print_stderr,
    reason = "human stderr notes (coverage, churn, baseline) preserved verbatim from the CLI health path"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) marks items the sibling health submodules consume across the private execute module"
)]

use std::process::ExitCode;
use std::time::{Duration, Instant};

use colored::Colorize;
use fallow_config::{OutputFormat, PackageJson, ResolvedConfig, Severity};

use crate::baseline::{HealthBaselineData, filter_new_health_findings, filter_new_health_targets};
use crate::error::emit_error;
use crate::vital_signs;
use fallow_output::*;

use super::{HealthExecutionOptions, HealthSeams, HealthSort, RuntimeCoverageSeamInput};

use super::{grouping, hotspots, react_hooks, scoring, tailwind_theme, targets};

use super::assembly::assemble_health_report;
use super::hotspots::compute_hotspots;
use super::runtime_filter::{
    RuntimeCoverageFilterContext, apply_runtime_coverage_filters, relative_to_root,
};
use super::scoring::compute_file_scores;
use super::targets::{TargetAuxData, compute_refactoring_targets};

pub type HealthOptions<'a> = HealthExecutionOptions<'a>;

/// Typed health analysis result generic over the CLI-owned grouping resolver.
pub type HealthResultGeneric<R> = super::HealthAnalysisResult<R>;

/// Discovery / parse inputs the CLI resolves before calling the engine.
pub struct HealthPipelineInputs {
    pub config: ResolvedConfig,
    pub files: Vec<fallow_types::discover::DiscoveredFile>,
    pub modules: Vec<fallow_types::extract::ModuleInfo>,
    /// Pre-parse pipeline timings (config / discover / parse milliseconds).
    pub config_ms: f64,
    pub discover_ms: f64,
    pub parse_ms: f64,
    pub parse_cpu_ms: f64,
    /// True when discover + parse were reused from the upstream check pass.
    pub shared_parse: bool,
    pub pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
}

/// Scope inputs the CLI resolves before calling the engine.
///
/// The engine no longer fetches changed files, workspace roots, the shared diff
/// index, or the CODEOWNERS-backed grouping resolver itself: those touch CLI
/// state (the shared-diff `OnceLock`, CODEOWNERS parsing, workspace discovery
/// error rendering), so the CLI resolves them and threads them in here.
pub struct HealthScopeInputs<'a, R> {
    pub changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub diff_index: Option<&'a fallow_output::DiffIndex>,
    pub ws_roots: Option<Vec<std::path::PathBuf>>,
    pub group_resolver: Option<R>,
}

struct HealthPipelineTimings {
    config: f64,
    discover: f64,
    parse: f64,
    /// Summed parse CPU time across rayon workers; `0.0` when parse was reused.
    parse_cpu: f64,
    /// True when discover + parse were reused from the upstream check pass.
    shared_parse: bool,
}

impl HealthPipelineTimings {
    fn into_base_input(self, complexity_ms: f64) -> HealthTimingBaseInput {
        HealthTimingBaseInput {
            config_ms: self.config,
            discover_ms: self.discover,
            parse_ms: self.parse,
            parse_cpu_ms: self.parse_cpu,
            complexity_ms,
            shared_parse: self.shared_parse,
        }
    }
}

struct HealthScope<'a, R> {
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    enforce_crap: bool,
    ignore_set: globset::GlobSet,
    changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    ws_roots: Option<Vec<std::path::PathBuf>>,
    group_resolver: Option<R>,
    file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
}

/// Validate an explicit `--churn-file` up front so a malformed import is a loud
/// hard error (exit 2) rather than a silent hotspot skip. Runs before the
/// pipeline, and only when churn would actually be consumed (`--hotspots` /
/// `--targets`; `--ownership` is subsumed because the dispatch layer sets
/// `hotspots = hotspots || ownership` before building `HealthOptions`), so an
/// inert `--churn-file` on a non-churn run is not penalized. The gate condition
/// mirrors `hotspots::fetch_churn_data`'s `needs_churn` exactly, keeping the
/// validate-iff-consume invariant. Failing here (instead of inside the parallel
/// hotspot pass) keeps combined `--format json` to a single error document. The
/// file is re-read in `fetch_churn_data`; the duplicate read is negligible for
/// realistic churn files and bounded by `MAX_CHURN_EVENTS`.
fn validate_churn_file(opts: &HealthOptions<'_>) -> Result<(), ExitCode> {
    if let Some(churn_file) = opts.churn_file
        && (opts.hotspots || opts.targets)
    {
        let resolved = scoring::resolve_relative_to_root(churn_file, Some(opts.root));
        crate::churn::analyze_churn_from_file(&resolved, opts.root)
            .map_err(|e| emit_error(&e, 2, opts.output))?;
    }
    Ok(())
}

/// Validate the explicit `--churn-file` import up front (loud exit 2 on a
/// malformed file). Exposed so the CLI wrapper can run it before the pipeline.
pub fn validate_health_churn_file(opts: &HealthOptions<'_>) -> Result<(), ExitCode> {
    validate_churn_file(opts)
}

/// Run the command-neutral health analysis pipeline.
///
/// Config loading, discovery, and parsing are the CLI's responsibility (they
/// touch the parser cache and config telemetry); the caller passes the resolved
/// [`HealthPipelineInputs`] plus the pre-resolved [`HealthScopeInputs`] and the
/// [`HealthSeams`] callbacks. The returned result carries the typed health
/// report plus the caller's grouping resolver for downstream rendering.
///
/// # Errors
///
/// Returns the CLI exit code emitted by a failing analysis or invalid input.
pub fn execute_health_inner<'a, R: super::HealthGroupResolver>(
    opts: &HealthOptions<'a>,
    input: HealthPipelineInputs,
    scope_inputs: HealthScopeInputs<'a, R>,
    seams: &HealthSeams<'_>,
) -> Result<HealthResultGeneric<R>, ExitCode> {
    let start = Instant::now();
    let HealthPipelineInputs {
        config,
        files,
        modules,
        config_ms,
        discover_ms,
        parse_ms,
        parse_cpu_ms,
        shared_parse,
        pre_computed_analysis,
    } = input;
    let timings = HealthPipelineTimings {
        config: config_ms,
        discover: discover_ms,
        parse: parse_ms,
        parse_cpu: parse_cpu_ms,
        shared_parse,
    };

    let scope = prepare_health_scope(opts, &config, &files, scope_inputs);

    let HealthPreparedCore {
        findings_data,
        analysis_data,
        derived_sections,
        vital_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    } = prepare_health_core_sections(HealthCoreSectionsInput {
        opts,
        config: &config,
        files: &files,
        modules: &modules,
        scope: &scope,
        pre_computed_analysis,
        seams,
    })?;

    let HealthOutputContext { build, sections } =
        prepare_health_output_context(HealthOutputContextInput {
            config: &config,
            files: &files,
            modules: &modules,
            scope: &scope,
            needs_file_scores,
            report_coverage_gaps,
            has_istanbul_coverage,
            findings_data,
            analysis_data,
            derived_sections,
            vital_data,
            timings,
            start: &start,
        });

    let output = build_health_output_parts(opts, &build, sections);

    Ok(finalize_health_result(HealthFinalizeInput {
        opts,
        config,
        files: &files,
        scope,
        output,
        elapsed: start.elapsed(),
        should_fail_on_coverage_gaps: enforce_coverage_gaps,
    }))
}

struct HealthCoreSectionsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::extract::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    seams: &'a HealthSeams<'a>,
}

struct HealthAnalysisPreludeInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    seams: &'a HealthSeams<'a>,
}

struct HealthScopedFindingsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    score_output: Option<&'a scoring::FileScoreOutput>,
}

struct HealthAnalysisPrelude {
    analysis_data: HealthAnalysisData,
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    needs_file_scores: bool,
}

struct HealthPreparedCore {
    findings_data: HealthFindingsData,
    analysis_data: HealthAnalysisData,
    derived_sections: HealthDerivedSections,
    vital_data: HealthVitalData,
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    needs_file_scores: bool,
}

fn prepare_health_analysis_prelude<R>(
    input: HealthAnalysisPreludeInput<'_, R>,
) -> Result<HealthAnalysisPrelude, ExitCode> {
    let HealthCoverageSettings {
        report_coverage_gaps,
        enforce_coverage_gaps,
        istanbul_coverage,
    } = prepare_health_coverage_settings(input.opts, input.config)?;

    let needs_file_scores = needs_health_file_scores(
        input.opts,
        report_coverage_gaps,
        enforce_coverage_gaps,
        input.scope.enforce_crap,
    );
    let analysis_data = prepare_health_analysis_data(HealthAnalysisDataInput {
        opts: input.opts,
        config: input.config,
        modules: input.modules,
        file_paths: &input.scope.file_paths,
        ignore_set: &input.scope.ignore_set,
        changed_files: input.scope.changed_files.as_ref(),
        ws_roots: input.scope.ws_roots.as_deref(),
        istanbul_coverage: istanbul_coverage.as_ref(),
        pre_computed_analysis: input.pre_computed_analysis,
        needs_file_scores,
        seams: input.seams,
    })?;

    Ok(HealthAnalysisPrelude {
        analysis_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage: istanbul_coverage.is_some(),
        needs_file_scores,
    })
}

fn prepare_health_scoped_findings<R>(
    input: &HealthScopedFindingsInput<'_, R>,
) -> Result<HealthFindingsData, ExitCode> {
    prepare_health_findings(HealthFindingsInput {
        opts: input.opts,
        config: input.config,
        modules: input.modules,
        file_paths: &input.scope.file_paths,
        ignore_set: &input.scope.ignore_set,
        changed_files: input.scope.changed_files.as_ref(),
        ws_roots: input.scope.ws_roots.as_deref(),
        diff_index: input.scope.diff_index,
        max_cyclomatic: input.scope.max_cyclomatic,
        max_cognitive: input.scope.max_cognitive,
        max_crap: input.scope.max_crap,
        enforce_crap: input.scope.enforce_crap,
        score_output: input.score_output,
    })
}

fn prepare_health_core_sections<R>(
    input: HealthCoreSectionsInput<'_, R>,
) -> Result<HealthPreparedCore, ExitCode> {
    let HealthCoreSectionsInput {
        opts,
        config,
        files,
        modules,
        scope,
        pre_computed_analysis,
        seams,
    } = input;

    let HealthAnalysisPrelude {
        analysis_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    } = prepare_health_analysis_prelude(HealthAnalysisPreludeInput {
        opts,
        config,
        modules,
        scope,
        pre_computed_analysis,
        seams,
    })?;

    let findings_data = prepare_health_scoped_findings(&HealthScopedFindingsInput {
        opts,
        config,
        modules,
        scope,
        score_output: analysis_data.score_output.as_ref(),
    })?;

    let HealthRuntimeSections {
        analysis_data,
        derived_sections,
        vital_data,
    } = prepare_health_runtime_sections(
        opts,
        HealthRuntimeSectionsInput {
            config,
            files,
            modules,
            file_paths: &scope.file_paths,
            ignore_set: &scope.ignore_set,
            changed_files: scope.changed_files.as_ref(),
            ws_roots: scope.ws_roots.as_deref(),
            diff_index: scope.diff_index,
            loaded_baseline: findings_data.loaded_baseline.as_ref(),
            findings: &findings_data.findings,
            analysis_data,
            has_istanbul_coverage,
            needs_file_scores,
        },
    )?;

    Ok(HealthPreparedCore {
        findings_data,
        analysis_data,
        derived_sections,
        vital_data,
        report_coverage_gaps,
        enforce_coverage_gaps,
        has_istanbul_coverage,
        needs_file_scores,
    })
}

/// The per-run scan filters shared by every CSS and markup health scanner:
/// resolved config, the ignore globset, the optional changed-file set, and
/// the optional workspace roots. Bundled so the scanners take one context
/// instead of repeating the same four borrows.
#[derive(Clone, Copy)]
struct HealthScanCtx<'a> {
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

struct HealthReportSideEffectsInput<'a> {
    opts: &'a HealthOptions<'a>,
    report: &'a mut fallow_output::HealthReport,
    files: &'a [fallow_types::discover::DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

struct HealthFinalizeInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    scope: HealthScope<'a, R>,
    output: HealthOutputParts,
    elapsed: Duration,
    should_fail_on_coverage_gaps: bool,
}

fn finalize_health_report_side_effects(input: &mut HealthReportSideEffectsInput<'_>) {
    if input.opts.css {
        let computation = compute_css_analytics_report(
            input.files,
            HealthScanCtx {
                config: input.config,
                ignore_set: input.ignore_set,
                changed_files: input.changed_files,
                ws_roots: input.ws_roots,
            },
        );
        // Styling health is the SECOND health axis: a separate score + grade
        // derived from the CSS analytics, surfaced only alongside `css_analytics`
        // (the same `--css` condition) so a plain `fallow health` run stays
        // byte-identical. It never affects the code `health_score`. The scoring
        // inputs (`@theme`-token population + the atomic / non-atomic declaration
        // split) are internal only; they are NOT serialized, so the report wire
        // contract is unchanged.
        input.report.styling_health = computation.as_ref().map(|computation| {
            crate::health::styling_score::compute_styling_health_with_inputs(
                &computation.report,
                &computation.scoring_inputs,
            )
        });
        input.report.css_analytics = computation.map(|computation| computation.report);
    }
}

fn finalize_health_result<R>(input: HealthFinalizeInput<'_, R>) -> HealthResultGeneric<R> {
    let HealthFinalizeInput {
        opts,
        config,
        files,
        scope,
        output,
        elapsed,
        should_fail_on_coverage_gaps,
    } = input;
    let HealthOutputParts {
        mut report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    } = output;

    finalize_health_report_side_effects(&mut HealthReportSideEffectsInput {
        opts,
        report: &mut report,
        files,
        config: &config,
        ignore_set: &scope.ignore_set,
        changed_files: scope.changed_files.as_ref(),
        ws_roots: scope.ws_roots.as_deref(),
    });

    build_health_result(HealthResultInput {
        config,
        report,
        grouping,
        group_resolver: scope.group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    })
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
    /// The token-consumers reverse index keys on these (kind `theme-var`); the
    /// name-only `theme_var_reads` set above stays the untouched input to
    /// `scan_unused_theme_tokens`. Additive, never read by the unused-token scan.
    theme_var_reads_located: Vec<(String, String, u32)>,
    /// Located regular-CSS `var()` reads (outside any `@theme` interior):
    /// `(name, path, line)` per read. The token-consumers reverse index keys on
    /// these (kind `css-var`). Additive; the unused-token scan instead diffs the
    /// name-only `referenced_custom_props` set.
    css_var_reads_located: Vec<(String, String, u32)>,
    /// Located class-shaped tokens inside `@apply` bodies: `(token, path, line)`
    /// per match. The token-consumers reverse index keys on these (kind `apply`);
    /// the name-only `apply_tokens` set above stays the untouched unused-token
    /// scan input. Additive.
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
        let scan = crate::extract::scan_theme_blocks(source);
        for token in scan.tokens {
            self.theme_token_definers
                .entry(token.name)
                .or_insert_with(|| (rel.to_owned(), token.line));
        }
        // Name-only set: the untouched input to `scan_unused_theme_tokens`.
        // Located list: additive, keyed by the token-consumer reverse index.
        for (name, line) in scan.theme_var_reads {
            self.theme_var_reads.insert(name.clone());
            self.theme_var_reads_located
                .push((name, rel.to_owned(), line));
        }
        self.apply_tokens
            .extend(crate::extract::extract_apply_tokens(source));
        for (token, line) in crate::extract::extract_apply_tokens_located(source) {
            self.apply_uses_located.push((token, rel.to_owned(), line));
        }
        for (name, line) in crate::extract::extract_css_var_reads_located(source) {
            self.css_var_reads_located
                .push((name, rel.to_owned(), line));
        }
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

/// Returns `true` when the project's root `package.json` declares a Tailwind
/// dependency (`tailwindcss` or any `@tailwindcss/*`), used to gate the
/// arbitrary-value markup scan: the `prefix-[value]` token shape is Tailwind-
/// specific in practice but not formally exclusive.
fn project_uses_tailwind(root: &std::path::Path) -> bool {
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
                .is_some_and(|deps| {
                    deps.keys()
                        .any(|k| k == "tailwindcss" || k.starts_with("@tailwindcss/"))
                })
        })
}

/// Whether the project declares a CSS-in-JS library whose CSS the lifters
/// understand: the template-literal form
/// ([`fallow_extract::css_in_js_virtual_stylesheet`], Phase 3b) and/or the object
/// form ([`fallow_extract::css_in_js_object_sheets`], Phase 3c). Gates the JS/TS
/// arm of the CSS walk so a non-CSS-in-JS project never scans `.js` / `.ts` files
/// for styling analytics (no `files_analyzed` inflation). A project declaring
/// NONE of these is byte-identical to pre-CSS-in-JS output.
fn project_uses_css_in_js(root: &std::path::Path) -> bool {
    const CSS_IN_JS_DEPS: &[&str] = &[
        // Template-literal form (Phase 3b).
        "styled-components",
        "@emotion/styled",
        "@emotion/react",
        "@emotion/css",
        "@linaria/core",
        "@linaria/react",
        // Object form (Phase 3c): vanilla-extract, Panda, StyleX. The object
        // serializer's import-binding provenance does the per-call recognition;
        // these gate whether the JS/TS files are scanned at all.
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
        for arb in crate::extract::scan_tailwind_arbitrary_values(&source) {
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
            for style in crate::extract::extract_sfc_styles(&source) {
                let is_style_scss = style
                    .lang
                    .as_deref()
                    .is_some_and(|lang| matches!(lang, "scss" | "sass"));
                for export in crate::extract::extract_css_module_exports(&style.body, is_style_scss)
                {
                    if let ExportName::Named(name) = export.name {
                        defined.insert(name);
                    }
                }
            }
            continue;
        }
        for export in crate::extract::extract_css_module_exports(&source, is_preprocessor) {
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
            if crate::extract::is_typo_edit(token, defined)
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
    for token in crate::extract::scan_markup_class_tokens(source).static_tokens {
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

/// Shortest a dependency's normalized name may be to serve as a third-party
/// class-prefix abstain key. Below this a short package name (`vue`, `css`)
/// would swallow too many real authored classes.
const MIN_DEP_PREFIX_LEN: usize = 6;

/// Normalize an identifier to a run of lowercase ASCII alphanumerics (drop
/// scopes, hyphens, dots): `maplibre-gl` -> `maplibregl`, `@scope/pkg` keeps
/// only `pkg` because the caller de-scopes first.
fn normalize_dep_token(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Normalized names of the project's declared dependencies (length-floored),
/// used to abstain on third-party CSS classes a library applies to its own
/// runtime-created DOM (e.g. a `.maplibregl-*` rule that styles the
/// `maplibre-gl` library). Scoped packages are de-scoped to the bare name.
fn dependency_class_prefixes(config: &ResolvedConfig) -> rustc_hash::FxHashSet<String> {
    let mut prefixes: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    let Ok(text) = std::fs::read_to_string(config.root.join("package.json")) else {
        return prefixes;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return prefixes;
    };
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(deps) = json.get(key).and_then(serde_json::Value::as_object) {
            for name in deps.keys() {
                // De-scope `@scope/pkg` -> `pkg` so the prefix is the package's
                // own name, not the scope.
                let bare = name.rsplit('/').next().unwrap_or(name);
                let normalized = normalize_dep_token(bare);
                if normalized.len() >= MIN_DEP_PREFIX_LEN {
                    prefixes.insert(normalized);
                }
            }
        }
    }
    prefixes
}

/// True when a CSS class is a third-party library's class: its normalized form
/// starts with a declared dependency's normalized name. `maplibregl-popup-content`
/// -> `maplibreglpopupcontent` starts with `maplibregl`. Conservative
/// (abstain-leaning): a third-party class wrongly flagged dead is a far worse
/// candidate than a missed authored dead class.
fn class_matches_dependency_prefix(
    class: &str,
    dependency_prefixes: &rustc_hash::FxHashSet<String>,
) -> bool {
    if dependency_prefixes.is_empty() {
        return false;
    }
    let normalized = normalize_dep_token(class);
    dependency_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix.as_str()))
}

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
        for export in crate::extract::extract_css_module_exports(&source, is_scss) {
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

/// Project-root-relative CSS/SCSS paths published as a package entry
/// (`style` / `main` / `sass` / `module`, or any string ending in `.css`/`.scss`
/// anywhere in `exports`). A stylesheet on this list is a public surface
/// consumed by OTHER repos, so its classes are referenced externally and must
/// never be flagged unreferenced.
fn published_css_paths(config: &ResolvedConfig) -> rustc_hash::FxHashSet<String> {
    let mut published: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    let Ok(text) = std::fs::read_to_string(config.root.join("package.json")) else {
        return published;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return published;
    };
    let normalize = |s: &str| s.trim_start_matches("./").replace('\\', "/");
    let is_css = |s: &str| {
        matches!(
            std::path::Path::new(s)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some("css" | "scss")
        )
    };
    for key in ["style", "main", "sass", "module"] {
        if let Some(s) = json.get(key).and_then(serde_json::Value::as_str)
            && is_css(s)
        {
            published.insert(normalize(s));
        }
    }
    // Walk `exports` (arbitrarily nested) collecting every CSS string value.
    let mut stack = vec![
        json.get("exports")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    ];
    while let Some(node) = stack.pop() {
        match node {
            serde_json::Value::String(s) if is_css(&s) => {
                published.insert(normalize(&s));
            }
            serde_json::Value::Array(items) => stack.extend(items),
            serde_json::Value::Object(map) => stack.extend(map.into_values()),
            _ => {}
        }
    }
    published
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
    let scan = crate::extract::scan_markup_class_tokens(&source);
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
/// Tailwind-utility-shaped token in `source` to `out` as `(token, rel, line)`,
/// recording the 1-based line of each match start. Same shape rule (a maximal
/// `[a-z0-9-]` run that, trimmed of `-`, still contains a `-` and starts with a
/// lowercase letter), so the located scan credits exactly the tokens the
/// name-only scan does, with positions added.
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

/// 1-based line of byte `offset` in `source`, counting `\n` up to that offset.
/// Out-of-range offsets clamp to line 1.
fn line_at_offset(source: &str, offset: usize) -> u32 {
    let count = source
        .get(..offset)
        .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
    u32::try_from(1 + count).unwrap_or(u32::MAX)
}

/// True when a `tailwind.config.*` text declares a non-empty `plugins` array
/// (`plugins: [ <non-empty> ]`). Used by the unused-theme-token plugin abstain.
/// Whitespace-tolerant, conservative (abstain-leaning): any `plugins` key whose
/// next non-whitespace tokens are `:` `[` `<non-`]`>` counts.
fn text_has_nonempty_plugins_array(text: &str) -> bool {
    let bytes = text.as_bytes();
    let skip_ws = |mut k: usize| {
        while k < bytes.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        k
    };
    let mut from = 0;
    while let Some(rel) = text[from..].find("plugins") {
        let mut k = skip_ws(from + rel + "plugins".len());
        if k < bytes.len() && bytes[k] == b':' {
            k = skip_ws(k + 1);
            if k < bytes.len() && bytes[k] == b'[' {
                k = skip_ws(k + 1);
                if k < bytes.len() && bytes[k] != b']' {
                    return true;
                }
            }
        }
        from = from + rel + "plugins".len();
    }
    false
}

/// True when the project declares a Tailwind plugin: a `@plugin` directive in any
/// stylesheet (already accumulated) OR a `tailwind.config.*` with a non-empty
/// `plugins` array. A plugin can consume `@theme` tokens via `theme()` /
/// `addUtilities` invisibly to the markup / CSS / `var()` scan, so the
/// unused-theme-token candidate hard-abstains on plugin projects.
fn project_uses_tailwind_plugin(any_plugin_directive: bool, root: &std::path::Path) -> bool {
    if any_plugin_directive {
        return true;
    }
    for name in [
        "tailwind.config.js",
        "tailwind.config.ts",
        "tailwind.config.mjs",
        "tailwind.config.cjs",
        "tailwind.config.mts",
        "tailwind.config.cts",
    ] {
        if let Ok(text) = std::fs::read_to_string(root.join(name))
            && text_has_nonempty_plugins_array(&text)
        {
            return true;
        }
    }
    false
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

/// Inputs to the token-consumer reverse index. Mirrors the gating-relevant
/// fields of [`UnusedThemeTokenScanInput`] but carries no `&mut summary` (the
/// reverse index is descriptive and sets no summary count).
struct TokenConsumersInput<'a> {
    tokens: &'a CssTokenSets,
    files: &'a [fallow_types::discover::DiscoveredFile],
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

/// Build the located `utility`-kind consumer surface: every class-shaped token
/// in non-CSS source (markup class attributes, `clsx` args, CSS-in-JS), each as
/// `(token, path, line)`. Mirrors the file iteration and ignore filter of
/// `collect_theme_usage_tokens` but records positions instead of a name-only
/// set. `@apply` bodies are deliberately excluded here (they are the distinct
/// `apply` surface, located separately, so the two kinds never double-count one
/// consumer).
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

/// Build the location-aware reverse index of Tailwind v4 `@theme` token
/// consumers. Applies the IDENTICAL gate as `scan_unused_theme_tokens` (partial
/// scope / non-v4 / empty definers / plugin all abstain, returning empty), keys
/// on the SAME classified candidates (so it inherits the variant /
/// published-library filters), then for each candidate gathers consumer
/// locations across the four surfaces (`theme-var`, `css-var`, `apply`,
/// `utility`). `consumer_count` is the FULL count (a static lower bound), set
/// before the per-token sample is capped. Output sorted by token; a token with
/// zero consumers is retained (the actionable "nothing consumes this" signal).
fn build_token_consumers(input: &TokenConsumersInput<'_>) -> Vec<fallow_output::TokenConsumers> {
    use fallow_output::{
        ConsumerKind, TOKEN_CONSUMER_SAMPLE_CAP, TokenConsumerLocation, TokenConsumers,
    };

    // Gate parity with `scan_unused_theme_tokens` (must abstain identically).
    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    // Reuse the exact candidate set the unused-token scan keys on, so the reverse
    // index inherits identical variant / published-library / namespace gating.
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
            // The token's own custom-property key (raw = token without `--`).
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

            // Deterministic order; full count BEFORE capping the sample.
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

/// Stable sort rank for [`fallow_output::ConsumerKind`], so the consumer sample
/// orders deterministically when path + line tie (the `@apply` body and the
/// `@theme`-interior read can share a line in a single stylesheet).
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

/// What kind of source a CSS-walk target is, deciding how its stylesheet body is
/// obtained: a standalone `.css` file (used verbatim), a Vue/Svelte SFC (its
/// `<style>` blocks lifted to a virtual stylesheet), or a JS/TS CSS-in-JS file
/// (its `styled` / `css` tagged-template bodies lifted to a virtual stylesheet).
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
        // The JS/TS arm is admitted ONLY when the project declares a CSS-in-JS
        // library, so a non-CSS-in-JS project pays nothing and `files_analyzed`
        // is not inflated by ordinary `.ts` files.
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
    let classes = crate::extract::scoped_unused_classes(source);
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

/// How a lifted virtual stylesheet contributes to the styling-health grade and
/// duplicate-block detection (CSS program Phase 3c).
#[derive(Clone, Copy, PartialEq, Eq)]
enum GradePolicy {
    /// Authored `.css`, SFC `<style>`, template-literal CSS-in-JS, and
    /// object-form CSS-in-JS with real selectors (vanilla-extract / emotion):
    /// full structural grade inputs + duplicate fingerprints.
    Structural,
    /// Object CSS-in-JS buckets that DROPPED a dynamic declaration: structural
    /// grade inputs, but duplicate fingerprints suppressed (a dropped declaration
    /// could have distinguished two otherwise-identical blocks).
    StructuralNoDedup,
    /// StyleX / Panda atomic object CSS: descriptive token-sprawl only; excluded
    /// from the structural grade inputs AND duplicate fingerprints (flat by
    /// construction; structure is a build-output property, not authored).
    Atomic,
}

/// One lifted virtual stylesheet plus its grade policy and whether its notable
/// rules feed the per-file report list. Object CSS-in-JS sheets feed the summary,
/// tokens, and grade inputs but not the per-file notable list (deferred).
struct CssScanItem<'a> {
    source: std::borrow::Cow<'a, str>,
    policy: GradePolicy,
    report_notable: bool,
}

/// Gather every virtual stylesheet a CSS-walk target yields. A standalone `.css`
/// file and a Vue/Svelte SFC each yield one structural sheet; a CSS-in-JS file
/// yields the template-literal sheet (Phase 3b, structural) plus up to three
/// object-form sheets (Phase 3c: structural, structural-no-dedup, atomic).
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
        CssScanKind::Sfc => crate::extract::sfc_virtual_stylesheet(source)
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
            // Phase 3b: template-literal CSS-in-JS, real selectors -> structural.
            if let Some(virtual_css) = crate::extract::css_in_js_virtual_stylesheet(source) {
                items.push(CssScanItem {
                    source: Cow::Owned(virtual_css),
                    policy: GradePolicy::Structural,
                    report_notable: true,
                });
            }
            // Phase 3c: object-form CSS-in-JS, three streams. These feed the
            // summary, tokens, and grade inputs but not the per-file notable list.
            let sheets = crate::extract::css_in_js_object_sheets(source, path);
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

/// Fold one virtual stylesheet's per-sheet aggregates into the running summary.
/// `files_analyzed` is counted ONCE per source file (a single CSS-in-JS file can
/// yield several virtual sheets), so it is incremented by the caller, not here.
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
/// token sets, scoped SFC unused-class findings, the running summary, and the
/// atomic / non-atomic declaration split that feeds the styling-health grade.
struct CssWalkAccum {
    file_reports: Vec<fallow_output::CssFileAnalytics>,
    summary: fallow_output::CssAnalyticsSummary,
    scoped_unused: Vec<fallow_output::ScopedUnusedClasses>,
    tokens: CssTokenSets,
    scoring: CssGradeScoring,
}

/// The atomic / non-atomic declaration split accumulated across the CSS walk,
/// fed into the styling-health grade so flat atomic object CSS-in-JS does not
/// dilute the declaration-normalized penalties or inflate confidence
/// (CSS program Phase 3c). For a project with no atomic object CSS-in-JS the
/// non-atomic counts equal the summary aggregates and `atomic_declarations` is 0,
/// so the grade is byte-identical to the pre-3c behavior.
#[derive(Default)]
struct CssGradeScoring {
    non_atomic_declarations: u32,
    non_atomic_important_declarations: u32,
    non_atomic_max_nesting_depth: u8,
    atomic_declarations: u32,
}

impl CssGradeScoring {
    /// Fold a non-atomic stylesheet's structural metrics into the grade inputs.
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
    // The atomic / non-atomic declaration split for the styling-health grade.
    let mut scoring = CssGradeScoring::default();

    // Read the CSS-in-JS dep gate once; it admits the JS/TS arm of the walk.
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

        // A standalone `.css` is used verbatim; SFC `<style>` and CSS-in-JS
        // (template + object forms) are lifted into virtual stylesheets so their
        // structural metrics count the same as authored CSS. A CSS-in-JS file can
        // yield several sheets (template, object structural / partial / atomic),
        // each with its own grade policy. `files_analyzed` counts the SOURCE file
        // once regardless of how many sheets it yields (so the count stays 1:1
        // with files); the per-sheet aggregates accumulate across all sheets.
        let mut file_had_sheet = false;
        for item in css_report_scan_items(&source, &file.path, kind) {
            let Some(mut analytics) = crate::extract::compute_css_analytics(&item.source) else {
                continue;
            };
            file_had_sheet = true;
            // The descriptive summary always reflects every analyzed sheet,
            // including atomic object CSS-in-JS (honest total counts).
            record_css_analytics_summary(&mut summary, &analytics);
            tokens.record_theme(item.source.as_ref(), &rel);

            match item.policy {
                GradePolicy::Atomic => {
                    // Atomic flat CSS: token-sprawl only. Suppress its
                    // duplicate-block fingerprints and keep it out of the
                    // non-atomic grade inputs; record its declarations for the
                    // predominantly-atomic confidence caveat.
                    analytics.declaration_blocks.clear();
                    tokens.record(&analytics, &rel);
                    scoring.atomic_declarations = scoring
                        .atomic_declarations
                        .saturating_add(analytics.total_declarations);
                }
                GradePolicy::Structural | GradePolicy::StructuralNoDedup => {
                    // A bucket that dropped a dynamic declaration must not
                    // fingerprint (the drop could have distinguished it from
                    // another block); clear its blocks before recording.
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

/// The CSS analytics report plus the internal styling-health scoring inputs that
/// are NOT part of the serialized report (the `@theme` token population for the
/// per-population token-death ratio, plus the atomic / non-atomic declaration
/// split that keeps flat atomic object CSS-in-JS out of the grade). Captured from
/// the walk accumulator before report assembly consumes it; adds no wire field.
struct CssAnalyticsComputation {
    report: fallow_output::CssAnalyticsReport,
    scoring_inputs: crate::health::styling_score::StylingScoringInputs,
}

fn compute_css_analytics_report(
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
    // The full `@theme` token population (denominator for the per-population
    // token-death ratio in styling-health `dead_surface`). Captured before
    // `assemble_css_report` consumes `walk`. `summary.unused_theme_tokens` is the
    // unused subset of exactly this set (see `classify_theme_token_candidates`,
    // which only filters `theme_token_definers` down), so this is always
    // `>= summary.unused_theme_tokens`.
    let theme_tokens_defined = saturate_len(walk.tokens.theme_token_definers.len());
    // The atomic / non-atomic declaration split for the styling-health grade,
    // captured before `assemble_css_report` consumes `walk`. With no atomic object
    // CSS-in-JS these counts equal the summary aggregates, so the grade is
    // byte-identical to the pre-3c behavior.
    let scoring_inputs = crate::health::styling_score::StylingScoringInputs {
        theme_tokens_defined,
        non_atomic_declarations: walk.scoring.non_atomic_declarations,
        non_atomic_important_declarations: walk.scoring.non_atomic_important_declarations,
        non_atomic_max_nesting_depth: walk.scoring.non_atomic_max_nesting_depth,
        atomic_declarations: walk.scoring.atomic_declarations,
    };
    // The location-aware token-consumer reverse index, gated identically to
    // `unused_theme_tokens` (v4 + non-plugin + non-published + whole-scope), built
    // before `assemble_css_report` consumes `walk`.
    let token_consumers = build_token_consumers(&TokenConsumersInput {
        tokens: &walk.tokens,
        files,
        config,
        ignore_set,
        changed_files,
        ws_roots,
    });
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

struct HealthCoverageSettings {
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    istanbul_coverage: Option<scoring::IstanbulCoverage>,
}

struct HealthFindingsData {
    findings: Vec<ComplexityViolation>,
    threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    files_analyzed: usize,
    total_functions: usize,
    complexity_ms: f64,
    total_above_threshold: usize,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
    loaded_baseline: Option<HealthBaselineData>,
}

struct CollectedHealthFindings {
    findings: Vec<ComplexityViolation>,
    files_analyzed: usize,
    total_functions: usize,
    complexity_ms: f64,
}

struct HealthOutputContextInput<'a, R> {
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::extract::ModuleInfo],
    scope: &'a HealthScope<'a, R>,
    needs_file_scores: bool,
    report_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    findings_data: HealthFindingsData,
    analysis_data: HealthAnalysisData,
    derived_sections: HealthDerivedSections,
    vital_data: HealthVitalData,
    timings: HealthPipelineTimings,
    start: &'a Instant,
}

struct HealthOutputContext<'a, R> {
    build: HealthOutputBuildInput<'a, R>,
    sections: HealthOutputSectionInput,
}

struct HealthOutputBuildInput<'a, R> {
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    group_resolver: Option<&'a R>,
    needs_file_scores: bool,
    report_coverage_gaps: bool,
    has_istanbul_coverage: bool,
    threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
    timing_base: HealthTimingBaseInput,
    start: &'a Instant,
}

struct HealthOutputSectionInput {
    analysis_data: HealthAnalysisData,
    derived_sections: HealthDerivedSections,
    vital_data: HealthVitalData,
    findings: Vec<ComplexityViolation>,
}

struct HealthOutputParts {
    report: fallow_output::HealthReport,
    grouping: Option<fallow_output::HealthGrouping>,
    timings: Option<fallow_output::HealthTimings>,
    coverage_gaps_has_findings: bool,
}

struct HealthOutputSupportingParts {
    grouping: Option<fallow_output::HealthGrouping>,
    timings: Option<fallow_output::HealthTimings>,
}

fn prepare_health_output_context<R>(
    input: HealthOutputContextInput<'_, R>,
) -> HealthOutputContext<'_, R> {
    let HealthFindingsData {
        findings,
        threshold_overrides,
        files_analyzed,
        total_functions,
        complexity_ms,
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline: _,
    } = input.findings_data;

    HealthOutputContext {
        build: HealthOutputBuildInput {
            config: input.config,
            files: input.files,
            modules: input.modules,
            file_paths: &input.scope.file_paths,
            group_resolver: input.scope.group_resolver.as_ref(),
            needs_file_scores: input.needs_file_scores,
            report_coverage_gaps: input.report_coverage_gaps,
            has_istanbul_coverage: input.has_istanbul_coverage,
            threshold_overrides,
            max_cyclomatic: input.scope.max_cyclomatic,
            max_cognitive: input.scope.max_cognitive,
            max_crap: input.scope.max_crap,
            files_analyzed,
            total_functions,
            total_above_threshold,
            sev_critical,
            sev_high,
            sev_moderate,
            timing_base: input.timings.into_base_input(complexity_ms),
            start: input.start,
        },
        sections: HealthOutputSectionInput {
            analysis_data: input.analysis_data,
            derived_sections: input.derived_sections,
            vital_data: input.vital_data,
            findings,
        },
    }
}

fn build_health_output_parts<R: super::HealthGroupResolver>(
    opts: &HealthOptions<'_>,
    build: &HealthOutputBuildInput<'_, R>,
    sections: HealthOutputSectionInput,
) -> HealthOutputParts {
    let HealthOutputSectionInput {
        analysis_data,
        derived_sections,
        vital_data,
        findings,
    } = sections;
    let coverage_gaps_has_findings =
        health_coverage_gaps_has_findings(analysis_data.score_output.as_ref());
    let action_ctx = build_health_action_context(
        opts,
        build.config,
        build.max_cyclomatic,
        build.max_cognitive,
        build.max_crap,
    );

    let HealthOutputSupportingParts { grouping, timings } =
        build_health_supporting_parts(HealthSupportingPartsInput {
            opts,
            build,
            analysis_data: &analysis_data,
            derived_sections: &derived_sections,
            vital_data: &vital_data,
            findings: &findings,
            action_ctx: &action_ctx,
        });

    let framework_health =
        build_framework_health_diagnostics(build.config, analysis_data.framework_health_facts);

    let report = build_health_report_from_pipeline(
        opts,
        &action_ctx,
        build_health_report_pipeline_input(
            build,
            analysis_data,
            vital_data,
            derived_sections,
            findings,
            framework_health,
        ),
    );

    HealthOutputParts {
        report,
        grouping,
        timings,
        coverage_gaps_has_findings,
    }
}

fn build_health_report_pipeline_input<R>(
    build: &HealthOutputBuildInput<'_, R>,
    analysis_data: HealthAnalysisData,
    vital_data: HealthVitalData,
    derived_sections: HealthDerivedSections,
    findings: Vec<ComplexityViolation>,
    framework_health: Option<fallow_output::FrameworkHealthDiagnostics>,
) -> HealthReportPipelineInput {
    HealthReportPipelineInput {
        report_coverage_gaps: build.report_coverage_gaps,
        findings,
        threshold_overrides: build.threshold_overrides.clone(),
        files_analyzed: build.files_analyzed,
        total_functions: build.total_functions,
        total_above_threshold: build.total_above_threshold,
        max_cyclomatic: build.max_cyclomatic,
        max_cognitive: build.max_cognitive,
        max_crap: build.max_crap,
        analysis_data,
        vital_data,
        hotspots: derived_sections.hotspots,
        hotspot_summary: derived_sections.hotspot_summary,
        targets: derived_sections.targets,
        target_thresholds: derived_sections.target_thresholds,
        has_istanbul_coverage: build.has_istanbul_coverage,
        framework_health,
        sev_critical: build.sev_critical,
        sev_high: build.sev_high,
        sev_moderate: build.sev_moderate,
    }
}

#[derive(Clone, Copy)]
struct HealthSupportingPartsInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    build: &'a HealthOutputBuildInput<'a, R>,
    analysis_data: &'a HealthAnalysisData,
    derived_sections: &'a HealthDerivedSections,
    vital_data: &'a HealthVitalData,
    findings: &'a [ComplexityViolation],
    action_ctx: &'a fallow_output::HealthActionContext,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "input is a Copy struct; by-value matches the original CLI signature"
)]
fn build_health_supporting_parts<R: super::HealthGroupResolver>(
    input: HealthSupportingPartsInput<'_, R>,
) -> HealthOutputSupportingParts {
    let grouping = build_health_output_grouping(&input);
    let timings = build_health_timings_from_pipeline(
        input.opts,
        input.build.start,
        input.analysis_data,
        input.derived_sections,
        &input.build.timing_base,
    );

    HealthOutputSupportingParts { grouping, timings }
}

fn build_health_output_grouping<R: super::HealthGroupResolver>(
    input: &HealthSupportingPartsInput<'_, R>,
) -> Option<fallow_output::HealthGrouping> {
    let file_scores = health_file_scores_slice(input.analysis_data.score_output.as_ref());
    build_health_grouping_from_context(HealthGroupingContextInput {
        opts: input.opts,
        config: input.build.config,
        group_resolver: input.build.group_resolver,
        candidate_paths: &input.derived_sections.candidate_paths,
        files: input.build.files,
        modules: input.build.modules,
        file_paths: input.build.file_paths,
        score_output: input.analysis_data.score_output.as_ref(),
        file_scores,
        findings: input.findings,
        hotspots: &input.derived_sections.hotspots,
        vital_data: input.vital_data,
        targets: &input.derived_sections.targets,
        needs_file_scores: input.build.needs_file_scores,
        action_ctx: input.action_ctx,
    })
}

struct HealthDerivedSectionInput<'a> {
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    file_scores: &'a [FileHealthScore],
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    loaded_baseline: Option<&'a HealthBaselineData>,
}

struct HealthDerivedSections {
    candidate_paths: rustc_hash::FxHashSet<std::path::PathBuf>,
    dupes_report: Option<crate::duplicates::DuplicationReport>,
    duplication_ms: f64,
    hotspots: Vec<HotspotEntry>,
    hotspot_summary: Option<HotspotSummary>,
    hotspots_ms: f64,
    targets: Vec<RefactoringTarget>,
    target_thresholds: Option<fallow_output::TargetThresholds>,
    targets_ms: f64,
}

struct HealthReportPipelineInput {
    report_coverage_gaps: bool,
    findings: Vec<ComplexityViolation>,
    threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    files_analyzed: usize,
    total_functions: usize,
    total_above_threshold: usize,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    analysis_data: HealthAnalysisData,
    vital_data: HealthVitalData,
    hotspots: Vec<HotspotEntry>,
    hotspot_summary: Option<HotspotSummary>,
    targets: Vec<RefactoringTarget>,
    target_thresholds: Option<fallow_output::TargetThresholds>,
    has_istanbul_coverage: bool,
    framework_health: Option<fallow_output::FrameworkHealthDiagnostics>,
    sev_critical: usize,
    sev_high: usize,
    sev_moderate: usize,
}

fn build_health_report_from_pipeline(
    opts: &HealthOptions<'_>,
    action_ctx: &fallow_output::HealthActionContext,
    input: HealthReportPipelineInput,
) -> fallow_output::HealthReport {
    assemble_health_report(
        opts,
        action_ctx,
        HealthReportAssembly {
            report_coverage_gaps: input.report_coverage_gaps,
            findings: input.findings,
            threshold_overrides: input.threshold_overrides,
            files_analyzed: input.files_analyzed,
            total_functions: input.total_functions,
            total_above_threshold: input.total_above_threshold,
            max_cyclomatic: input.max_cyclomatic,
            max_cognitive: input.max_cognitive,
            max_crap: input.max_crap,
            files_scored: input.analysis_data.files_scored,
            average_maintainability: input.analysis_data.average_maintainability,
            vital_signs: input.vital_data.vital_signs,
            health_score: input.vital_data.health_score,
            score_output: input.analysis_data.score_output,
            hotspots: input.hotspots,
            hotspot_summary: input.hotspot_summary,
            targets: input.targets,
            target_thresholds: input.target_thresholds,
            health_trend: input.vital_data.health_trend,
            has_istanbul_coverage: input.has_istanbul_coverage,
            runtime_coverage: input.analysis_data.runtime_coverage,
            framework_health: input.framework_health,
            large_functions: input.vital_data.large_functions,
            sev_critical: input.sev_critical,
            sev_high: input.sev_high,
            sev_moderate: input.sev_moderate,
        },
    )
}

#[derive(Debug, Clone, Copy)]
struct GlobalHealthThresholds {
    cyclomatic: u16,
    cognitive: u16,
    crap: f64,
}

#[derive(Debug, Clone, Copy)]
struct AppliedHealthThresholds {
    effective: fallow_output::HealthEffectiveThresholds,
    override_index: Option<usize>,
}

struct CompiledThresholdOverride {
    index: usize,
    matchers: globset::GlobSet,
    functions: Vec<String>,
    configured: fallow_output::HealthConfiguredThresholds,
    reason: Option<String>,
}

struct ThresholdOverrideMatch<'a> {
    entry: &'a CompiledThresholdOverride,
    effective: fallow_output::HealthEffectiveThresholds,
}

struct ThresholdOverrideResolver {
    entries: Vec<CompiledThresholdOverride>,
    global: GlobalHealthThresholds,
}

impl ThresholdOverrideResolver {
    #[must_use]
    fn new(
        overrides: &[fallow_config::HealthThresholdOverride],
        global: GlobalHealthThresholds,
    ) -> Self {
        let entries = overrides
            .iter()
            .enumerate()
            .map(|(index, override_entry)| {
                let mut builder = globset::GlobSetBuilder::new();
                for pattern in &override_entry.files {
                    if let Ok(glob) = globset::Glob::new(pattern) {
                        builder.add(glob);
                    }
                }
                CompiledThresholdOverride {
                    index,
                    matchers: builder
                        .build()
                        .unwrap_or_else(|_| globset::GlobSet::empty()),
                    functions: override_entry.functions.clone(),
                    configured: fallow_output::HealthConfiguredThresholds {
                        max_cyclomatic: override_entry.max_cyclomatic,
                        max_cognitive: override_entry.max_cognitive,
                        max_crap: override_entry.max_crap,
                    },
                    reason: override_entry.reason.clone(),
                }
            })
            .collect();
        Self { entries, global }
    }

    #[must_use]
    fn resolve(
        &self,
        relative: &std::path::Path,
        function: &str,
    ) -> (AppliedHealthThresholds, Vec<ThresholdOverrideMatch<'_>>) {
        let mut effective = fallow_output::HealthEffectiveThresholds {
            max_cyclomatic: self.global.cyclomatic,
            max_cognitive: self.global.cognitive,
            max_crap: self.global.crap,
        };
        let mut override_index = None;
        let mut matches = Vec::new();

        for entry in &self.entries {
            if !entry.matchers.is_match(relative) {
                continue;
            }
            if !entry.functions.is_empty() && !entry.functions.iter().any(|f| f == function) {
                continue;
            }
            if let Some(max_cyclomatic) = entry.configured.max_cyclomatic {
                effective.max_cyclomatic = max_cyclomatic;
                override_index = Some(entry.index);
            }
            if let Some(max_cognitive) = entry.configured.max_cognitive {
                effective.max_cognitive = max_cognitive;
                override_index = Some(entry.index);
            }
            if let Some(max_crap) = entry.configured.max_crap {
                effective.max_crap = max_crap;
                override_index = Some(entry.index);
            }
            matches.push(ThresholdOverrideMatch { entry, effective });
        }

        (
            AppliedHealthThresholds {
                effective,
                override_index,
            },
            matches,
        )
    }

    fn entries(&self) -> &[CompiledThresholdOverride] {
        &self.entries
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ThresholdOverrideDimension {
    Complexity,
    Crap,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThresholdOverrideStateKey {
    status: &'static str,
    override_index: usize,
    path: Option<std::path::PathBuf>,
    function: Option<String>,
    dimension: ThresholdOverrideDimension,
}

#[derive(Debug, Clone, Copy)]
struct MeasuredThresholdMetrics {
    cyclomatic: u16,
    cognitive: u16,
    crap: f64,
}

#[derive(Default)]
struct ThresholdOverrideStateTracker {
    matched_indexes: rustc_hash::FxHashSet<usize>,
    seen: rustc_hash::FxHashSet<ThresholdOverrideStateKey>,
    states: Vec<fallow_output::ThresholdOverrideState>,
}

impl ThresholdOverrideStateTracker {
    fn record_complexity(
        &mut self,
        function: ComplexityFunctionContext<'_>,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
    ) {
        let ComplexityFunctionContext {
            path,
            function,
            cyclomatic,
            cognitive,
        } = function;
        for matched in matches {
            self.matched_indexes.insert(matched.entry.index);
            let configured = matched.entry.configured;
            let has_complexity_threshold =
                configured.max_cyclomatic.is_some() || configured.max_cognitive.is_some();
            if !has_complexity_threshold {
                continue;
            }
            let global_exceeded = configured
                .max_cyclomatic
                .is_some_and(|_| cyclomatic > global.cyclomatic)
                || configured
                    .max_cognitive
                    .is_some_and(|_| cognitive > global.cognitive);
            let local_exceeded = configured
                .max_cyclomatic
                .is_some_and(|threshold| cyclomatic > threshold)
                || configured
                    .max_cognitive
                    .is_some_and(|threshold| cognitive > threshold);
            let status = if global_exceeded && !local_exceeded {
                fallow_output::ThresholdOverrideStatus::Active
            } else if !global_exceeded {
                fallow_output::ThresholdOverrideStatus::Stale
            } else {
                continue;
            };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                configured_thresholds: configured,
                effective_thresholds: matched.effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic,
                    cognitive,
                    crap: None,
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Complexity,
            });
        }
    }

    fn record_crap(
        &mut self,
        path: &std::path::Path,
        function: &str,
        metrics: MeasuredThresholdMetrics,
        matches: &[ThresholdOverrideMatch<'_>],
        global: GlobalHealthThresholds,
    ) {
        for matched in matches {
            self.matched_indexes.insert(matched.entry.index);
            let Some(max_crap) = matched.entry.configured.max_crap else {
                continue;
            };
            let status = if metrics.crap >= global.crap && metrics.crap < max_crap {
                fallow_output::ThresholdOverrideStatus::Active
            } else if metrics.crap < global.crap {
                fallow_output::ThresholdOverrideStatus::Stale
            } else {
                continue;
            };
            self.push_state(ThresholdOverrideStateInput {
                status,
                override_index: matched.entry.index,
                path: Some(path.to_path_buf()),
                function: Some(function.to_string()),
                configured_thresholds: matched.entry.configured,
                effective_thresholds: matched.effective,
                metrics: Some(fallow_output::ThresholdOverrideMetrics {
                    cyclomatic: metrics.cyclomatic,
                    cognitive: metrics.cognitive,
                    crap: Some(metrics.crap),
                }),
                reason: matched.entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Crap,
            });
        }
    }

    fn record_no_match_entries(&mut self, resolver: &ThresholdOverrideResolver, should_emit: bool) {
        if !should_emit {
            return;
        }
        for entry in resolver.entries() {
            if self.matched_indexes.contains(&entry.index) {
                continue;
            }
            self.push_state(ThresholdOverrideStateInput {
                status: fallow_output::ThresholdOverrideStatus::NoMatch,
                override_index: entry.index,
                path: None,
                function: None,
                configured_thresholds: entry.configured,
                effective_thresholds: fallow_output::HealthEffectiveThresholds {
                    max_cyclomatic: entry
                        .configured
                        .max_cyclomatic
                        .unwrap_or(resolver.global.cyclomatic),
                    max_cognitive: entry
                        .configured
                        .max_cognitive
                        .unwrap_or(resolver.global.cognitive),
                    max_crap: entry.configured.max_crap.unwrap_or(resolver.global.crap),
                },
                metrics: None,
                reason: entry.reason.clone(),
                dimension: ThresholdOverrideDimension::Complexity,
            });
        }
    }

    fn into_states(mut self) -> Vec<fallow_output::ThresholdOverrideState> {
        self.states.sort_by(|a, b| {
            a.override_index
                .cmp(&b.override_index)
                .then(a.path.cmp(&b.path))
                .then(a.function.cmp(&b.function))
        });
        self.states
    }

    fn push_state(&mut self, input: ThresholdOverrideStateInput) {
        let status_key = match input.status {
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let key = ThresholdOverrideStateKey {
            status: status_key,
            override_index: input.override_index,
            path: input.path.clone(),
            function: input.function.clone(),
            dimension: input.dimension,
        };
        if !self.seen.insert(key) {
            return;
        }
        self.states.push(fallow_output::ThresholdOverrideState {
            status: input.status,
            override_index: input.override_index,
            path: input.path,
            function: input.function,
            configured_thresholds: input.configured_thresholds,
            effective_thresholds: input.effective_thresholds,
            metrics: input.metrics,
            reason: input.reason,
        });
    }
}

/// One function's identity (path + name) and measured complexity metrics,
/// bundled so `record_complexity` takes the function descriptor as a single
/// parameter instead of four.
#[derive(Clone, Copy)]
struct ComplexityFunctionContext<'a> {
    path: &'a std::path::Path,
    function: &'a str,
    cyclomatic: u16,
    cognitive: u16,
}

struct ThresholdOverrideStateInput {
    status: fallow_output::ThresholdOverrideStatus,
    override_index: usize,
    path: Option<std::path::PathBuf>,
    function: Option<String>,
    configured_thresholds: fallow_output::HealthConfiguredThresholds,
    effective_thresholds: fallow_output::HealthEffectiveThresholds,
    metrics: Option<fallow_output::ThresholdOverrideMetrics>,
    reason: Option<String>,
    dimension: ThresholdOverrideDimension,
}

#[derive(Clone, Copy)]
struct HealthGroupingContextInput<'a, R> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    group_resolver: Option<&'a R>,
    candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores: &'a [FileHealthScore],
    findings: &'a [ComplexityViolation],
    hotspots: &'a [HotspotEntry],
    vital_data: &'a HealthVitalData,
    targets: &'a [RefactoringTarget],
    needs_file_scores: bool,
    action_ctx: &'a fallow_output::HealthActionContext,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "input is a Copy struct; by-value matches the original CLI signature"
)]
fn build_health_grouping_from_context<R: super::HealthGroupResolver>(
    input: HealthGroupingContextInput<'_, R>,
) -> Option<fallow_output::HealthGrouping> {
    build_optional_health_grouping_opt(
        input.group_resolver,
        &input.config.root,
        input.candidate_paths,
        &grouping::HealthGroupingInput {
            files: input.files,
            modules: input.modules,
            file_paths: input.file_paths,
            score_output: input.score_output,
            file_scores: input.file_scores,
            findings: input.findings,
            hotspots: input.hotspots,
            large_functions: &input.vital_data.large_functions,
            targets: input.targets,
            score_requested: input.opts.score,
            duplicates_config: input.opts.score.then_some(&input.config.duplicates),
            needs_file_scores: input.needs_file_scores,
            needs_hotspots: input.opts.hotspots || input.opts.targets,
            show_vital_signs: !input.opts.score_only_output,
            action_ctx: input.action_ctx,
        },
    )
}

fn needs_health_file_scores(
    opts: &HealthOptions<'_>,
    report_coverage_gaps: bool,
    enforce_coverage_gaps: bool,
    enforce_crap: bool,
) -> bool {
    opts.file_scores
        || report_coverage_gaps
        || enforce_coverage_gaps
        || opts.hotspots
        || opts.targets
        || opts.force_full
        || enforce_crap
}

fn health_coverage_gaps_has_findings(score_output: Option<&scoring::FileScoreOutput>) -> bool {
    score_output.is_some_and(|output| !output.coverage.report.is_empty())
}

fn health_file_scores_slice(score_output: Option<&scoring::FileScoreOutput>) -> &[FileHealthScore] {
    score_output.map_or(&[] as &[_], |output| output.scores.as_slice())
}

fn prepare_health_derived_sections(
    opts: &HealthOptions<'_>,
    input: HealthDerivedSectionInput<'_>,
) -> HealthDerivedSections {
    let (candidate_paths, dupes_report, duplication_ms) =
        prepare_health_section_dupes(opts, &input);
    let (hotspots, hotspot_summary, hotspots_ms) = prepare_health_section_hotspots(
        opts,
        HealthHotspotSectionInput {
            config: input.config,
            file_scores: input.file_scores,
            ignore_set: input.ignore_set,
            ws_roots: input.ws_roots,
            churn_fetch: input.churn_fetch,
            diff_index: input.diff_index,
        },
    );
    let (targets, target_thresholds, targets_ms) = prepare_health_section_targets(
        opts,
        &HealthTargetSectionInput {
            score_output: input.score_output,
            file_scores: input.file_scores,
            hotspots: &hotspots,
            loaded_baseline: input.loaded_baseline,
            config: input.config,
            diff_index: input.diff_index,
            dupes_report: dupes_report.as_ref(),
        },
    );

    HealthDerivedSections {
        candidate_paths,
        dupes_report,
        duplication_ms,
        hotspots,
        hotspot_summary,
        hotspots_ms,
        targets,
        target_thresholds,
        targets_ms,
    }
}

fn prepare_health_section_dupes(
    opts: &HealthOptions<'_>,
    input: &HealthDerivedSectionInput<'_>,
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    prepare_health_duplication_data(
        opts,
        input.config,
        input.files,
        input.changed_files,
        input.ws_roots,
        input.ignore_set,
    )
}

struct HealthHotspotSectionInput<'a> {
    config: &'a ResolvedConfig,
    file_scores: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn prepare_health_section_hotspots(
    opts: &HealthOptions<'_>,
    input: HealthHotspotSectionInput<'_>,
) -> (Vec<HotspotEntry>, Option<HotspotSummary>, f64) {
    compute_filtered_hotspots(FilteredHotspotInput {
        opts,
        config: input.config,
        file_scores_slice: input.file_scores,
        ignore_set: input.ignore_set,
        ws_roots: input.ws_roots,
        churn_fetch: input.churn_fetch,
        diff_index: input.diff_index,
    })
}

struct HealthTargetSectionInput<'a> {
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores: &'a [FileHealthScore],
    hotspots: &'a [HotspotEntry],
    loaded_baseline: Option<&'a HealthBaselineData>,
    config: &'a ResolvedConfig,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
}

fn prepare_health_section_targets(
    opts: &HealthOptions<'_>,
    input: &HealthTargetSectionInput<'_>,
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>, f64) {
    compute_filtered_targets(FilteredTargetInput {
        opts,
        score_output: input.score_output,
        file_scores_slice: input.file_scores,
        hotspots: input.hotspots,
        loaded_baseline: input.loaded_baseline,
        config: input.config,
        diff_index: input.diff_index,
        dupes_report: input.dupes_report,
    })
}

struct HealthTimingInput {
    config_ms: f64,
    discover_ms: f64,
    parse_ms: f64,
    parse_cpu_ms: f64,
    complexity_ms: f64,
    file_scores_ms: f64,
    git_churn_ms: f64,
    git_churn_cache_hit: bool,
    hotspots_ms: f64,
    duplication_ms: f64,
    targets_ms: f64,
    shared_parse: bool,
}

struct HealthTimingBaseInput {
    config_ms: f64,
    discover_ms: f64,
    parse_ms: f64,
    parse_cpu_ms: f64,
    complexity_ms: f64,
    shared_parse: bool,
}

struct HealthResultInput<R> {
    config: ResolvedConfig,
    report: fallow_output::HealthReport,
    grouping: Option<fallow_output::HealthGrouping>,
    group_resolver: Option<R>,
    elapsed: Duration,
    timings: Option<fallow_output::HealthTimings>,
    coverage_gaps_has_findings: bool,
    should_fail_on_coverage_gaps: bool,
}

fn build_health_result<R>(input: HealthResultInput<R>) -> HealthResultGeneric<R> {
    let HealthResultInput {
        config,
        report,
        grouping,
        group_resolver,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    } = input;

    HealthResultGeneric {
        report,
        grouping,
        group_resolver,
        config,
        elapsed,
        timings,
        coverage_gaps_has_findings,
        should_fail_on_coverage_gaps,
    }
}

#[derive(Clone, Copy)]
struct HealthFindingsInput<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
    enforce_crap: bool,
    score_output: Option<&'a scoring::FileScoreOutput>,
}

fn prepare_health_findings(input: HealthFindingsInput<'_>) -> Result<HealthFindingsData, ExitCode> {
    let t = Instant::now();
    let global_thresholds = GlobalHealthThresholds {
        cyclomatic: input.max_cyclomatic,
        cognitive: input.max_cognitive,
        crap: input.max_crap,
    };
    let threshold_resolver =
        ThresholdOverrideResolver::new(&input.config.health.threshold_overrides, global_thresholds);
    let mut threshold_state_tracker = ThresholdOverrideStateTracker::default();
    let mut collected =
        collect_health_findings(input, &threshold_resolver, &mut threshold_state_tracker, t);

    let mut crap_ctx = HealthCrapMergeContext {
        modules: input.modules,
        file_paths: input.file_paths,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        max_cyclomatic: input.max_cyclomatic,
        max_cognitive: input.max_cognitive,
        enforce_crap: input.enforce_crap,
        score_output: input.score_output,
        config_root: &input.config.root,
        threshold_resolver: &threshold_resolver,
        threshold_state_tracker: &mut threshold_state_tracker,
    };
    apply_optional_crap_findings(input.opts, &mut collected.findings, &mut crap_ctx);
    let (total_above_threshold, sev_critical, sev_high, sev_moderate, loaded_baseline) =
        finalize_health_findings(
            input.opts,
            input.config,
            &mut collected.findings,
            input.diff_index,
        )?;
    threshold_state_tracker.record_no_match_entries(
        &threshold_resolver,
        should_emit_no_match_threshold_overrides(
            input.opts,
            input.changed_files,
            input.ws_roots,
            input.diff_index,
        ),
    );

    Ok(HealthFindingsData {
        findings: collected.findings,
        threshold_overrides: threshold_state_tracker.into_states(),
        files_analyzed: collected.files_analyzed,
        total_functions: collected.total_functions,
        complexity_ms: collected.complexity_ms,
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
    })
}

fn collect_health_findings(
    input: HealthFindingsInput<'_>,
    threshold_resolver: &ThresholdOverrideResolver,
    threshold_state_tracker: &mut ThresholdOverrideStateTracker,
    started_at: Instant,
) -> CollectedHealthFindings {
    let mut collect_input = CollectFindingsInput {
        modules: input.modules,
        file_paths: input.file_paths,
        config_root: &input.config.root,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        threshold_resolver,
        threshold_state_tracker,
        complexity_breakdown: input.opts.complexity_breakdown,
    };
    let (findings, files_analyzed, total_functions) =
        collect_findings_with_resolver(&mut collect_input);

    CollectedHealthFindings {
        findings,
        files_analyzed,
        total_functions,
        complexity_ms: started_at.elapsed().as_secs_f64() * 1000.0,
    }
}

struct HealthCrapMergeContext<'a> {
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    enforce_crap: bool,
    score_output: Option<&'a scoring::FileScoreOutput>,
    config_root: &'a std::path::Path,
    threshold_resolver: &'a ThresholdOverrideResolver,
    threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
}

fn apply_optional_crap_findings(
    opts: &HealthOptions<'_>,
    findings: &mut Vec<ComplexityViolation>,
    ctx: &mut HealthCrapMergeContext<'_>,
) {
    if ctx.enforce_crap
        && let Some(score_out) = ctx.score_output
    {
        let mut input = CrapFindingMergeInput {
            modules: ctx.modules,
            file_paths: ctx.file_paths,
            config_root: ctx.config_root,
            ignore_set: ctx.ignore_set,
            changed_files: ctx.changed_files,
            ws_roots: ctx.ws_roots,
            per_function_crap: &score_out.per_function_crap,
            template_inherit_provenance: &score_out.template_inherit_provenance,
            complexity_breakdown: opts.complexity_breakdown,
            threshold_resolver: ctx.threshold_resolver,
            threshold_state_tracker: ctx.threshold_state_tracker,
        };
        merge_crap_findings(findings, &mut input);
    }
    append_component_rollup_findings(
        findings,
        ctx.score_output
            .map(|output| &output.template_inherit_provenance),
        ctx.max_cyclomatic,
        ctx.max_cognitive,
    );
}

fn should_emit_no_match_threshold_overrides(
    opts: &HealthOptions<'_>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> bool {
    opts.changed_since.is_none()
        && opts.diff_index.is_none()
        && !opts.use_shared_diff_index
        && opts.workspace.is_none()
        && opts.changed_workspaces.is_none()
        && changed_files.is_none()
        && ws_roots.is_none()
        && diff_index.is_none()
}

type HealthFindingFinalizeResult = (usize, usize, usize, usize, Option<HealthBaselineData>);

fn finalize_health_findings(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
    diff_index: Option<&fallow_output::DiffIndex>,
) -> Result<HealthFindingFinalizeResult, ExitCode> {
    if let Some(diff_index) = diff_index {
        filter_complexity_findings_by_diff(findings, diff_index, &config.root);
    }
    sort_findings(findings, opts.sort);
    let total_above_threshold = findings.len();
    let (sev_critical, sev_high, sev_moderate) = count_finding_severities(findings);
    let loaded_baseline = apply_health_baseline_and_top(opts, config, findings)?;
    Ok((
        total_above_threshold,
        sev_critical,
        sev_high,
        sev_moderate,
        loaded_baseline,
    ))
}

fn build_health_timings_from_pipeline(
    opts: &HealthOptions<'_>,
    start: &Instant,
    analysis_data: &HealthAnalysisData,
    sections: &HealthDerivedSections,
    input: &HealthTimingBaseInput,
) -> Option<HealthTimings> {
    build_health_timings(
        opts,
        start,
        &HealthTimingInput {
            config_ms: input.config_ms,
            discover_ms: input.discover_ms,
            parse_ms: input.parse_ms,
            parse_cpu_ms: input.parse_cpu_ms,
            complexity_ms: input.complexity_ms,
            file_scores_ms: analysis_data.file_scores_ms,
            git_churn_ms: analysis_data.git_churn_ms,
            git_churn_cache_hit: analysis_data.git_churn_cache_hit,
            hotspots_ms: sections.hotspots_ms,
            duplication_ms: sections.duplication_ms,
            targets_ms: sections.targets_ms,
            shared_parse: input.shared_parse,
        },
    )
}

fn build_health_timings(
    opts: &HealthOptions<'_>,
    start: &Instant,
    input: &HealthTimingInput,
) -> Option<HealthTimings> {
    if !opts.performance {
        return None;
    }

    let inner_ms = start.elapsed().as_secs_f64() * 1000.0;
    let total_ms = input.config_ms + input.discover_ms + input.parse_ms + inner_ms;
    Some(HealthTimings {
        config_ms: input.config_ms,
        discover_ms: input.discover_ms,
        parse_ms: input.parse_ms,
        parse_cpu_ms: input.parse_cpu_ms,
        complexity_ms: input.complexity_ms,
        file_scores_ms: input.file_scores_ms,
        git_churn_ms: input.git_churn_ms,
        git_churn_cache_hit: input.git_churn_cache_hit,
        hotspots_ms: input.hotspots_ms,
        duplication_ms: input.duplication_ms,
        targets_ms: input.targets_ms,
        total_ms,
        shared_parse: input.shared_parse,
    })
}

fn prepare_health_coverage_settings(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
) -> Result<HealthCoverageSettings, ExitCode> {
    let config_coverage_enabled = config.rules.coverage_gaps != fallow_config::Severity::Off;
    let report_coverage_gaps =
        opts.coverage_gaps || (opts.config_activates_coverage_gaps && config_coverage_enabled);
    let enforce_coverage_gaps = opts.enforce_coverage_gap_gate
        && config.rules.coverage_gaps == fallow_config::Severity::Error;
    let istanbul_coverage = load_health_coverage(opts, config)?;

    Ok(HealthCoverageSettings {
        report_coverage_gaps,
        enforce_coverage_gaps,
        istanbul_coverage,
    })
}

fn build_optional_health_grouping_opt<R: super::HealthGroupResolver>(
    resolver: Option<&R>,
    project_root: &std::path::Path,
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
    input: &grouping::HealthGroupingInput<'_>,
) -> Option<HealthGrouping> {
    let resolver = resolver?;
    Some(grouping::build_health_grouping(
        resolver as &dyn super::HealthGroupResolver,
        project_root,
        candidate_paths,
        input,
    ))
}

fn active_health_coverage_model(has_istanbul_coverage: bool) -> fallow_output::CoverageModel {
    if has_istanbul_coverage {
        fallow_output::CoverageModel::Istanbul
    } else {
        fallow_output::CoverageModel::StaticEstimated
    }
}

fn build_health_action_context(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    max_cyclomatic: u16,
    max_cognitive: u16,
    max_crap: f64,
) -> fallow_output::HealthActionContext {
    let baseline_active = opts.baseline.is_some() || opts.save_baseline.is_some();
    let action_opts = if baseline_active {
        fallow_output::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("baseline-active"),
        }
    } else if !config.health.suggest_inline_suppression {
        fallow_output::HealthActionOptions {
            omit_suppress_line: true,
            omit_reason: Some("config-disabled"),
        }
    } else {
        fallow_output::HealthActionOptions::default()
    };
    fallow_output::HealthActionContext {
        opts: action_opts,
        max_cyclomatic_threshold: max_cyclomatic,
        max_cognitive_threshold: max_cognitive,
        max_crap_threshold: max_crap,
        crap_refactor_band: config.health.crap_refactor_band,
    }
}

fn prepare_health_scope<'a, R>(
    opts: &HealthOptions<'a>,
    config: &ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    scope_inputs: HealthScopeInputs<'a, R>,
) -> HealthScope<'a, R> {
    let max_cyclomatic = opts
        .thresholds
        .max_cyclomatic
        .unwrap_or(config.health.max_cyclomatic);
    let max_cognitive = opts
        .thresholds
        .max_cognitive
        .unwrap_or(config.health.max_cognitive);
    let max_crap = opts.thresholds.max_crap.unwrap_or(config.health.max_crap);
    let ignore_set = build_ignore_set(&config.health.ignore);
    let HealthScopeInputs {
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
    } = scope_inputs;
    let file_paths = files.iter().map(|f| (f.id, &f.path)).collect();

    HealthScope {
        max_cyclomatic,
        max_cognitive,
        max_crap,
        enforce_crap: max_crap > 0.0,
        ignore_set,
        changed_files,
        diff_index,
        ws_roots,
        group_resolver,
        file_paths,
    }
}

fn load_health_coverage(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
) -> Result<Option<scoring::IstanbulCoverage>, ExitCode> {
    if let Some(coverage_path) = opts.coverage_inputs.coverage {
        return scoring::load_istanbul_coverage(
            coverage_path,
            opts.coverage_inputs.coverage_root,
            Some(&config.root),
        )
        .map(Some)
        .map_err(|e| {
            emit_error(&format!("coverage: {e}"), 2, opts.output);
            ExitCode::from(2)
        });
    }

    let Some(auto_path) = scoring::auto_detect_coverage(&config.root) else {
        return Ok(None);
    };
    if std::env::var("CI").is_ok_and(|v| !v.is_empty()) {
        eprintln!(
            "note: using auto-detected coverage at {}; pass --coverage explicitly for deterministic CI scores",
            auto_path.display()
        );
    }
    Ok(scoring::load_istanbul_coverage(
        &auto_path,
        opts.coverage_inputs.coverage_root,
        Some(&config.root),
    )
    .ok())
}

fn prepare_shared_analysis_output(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    modules: &[crate::extract::ModuleInfo],
    pre_computed: Option<crate::DeadCodeAnalysisArtifacts>,
    needed: bool,
) -> Result<Option<crate::DeadCodeAnalysisArtifacts>, ExitCode> {
    if !needed {
        return Ok(None);
    }
    if let Some(pre) = pre_computed {
        return Ok(Some(pre));
    }
    crate::analyze_with_parse_result(config, modules)
        .map(Some)
        .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, opts.output))
}

#[derive(Clone, Copy)]
struct RuntimeCoverageAnalysisScope<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    shared_analysis_output: Option<&'a crate::DeadCodeAnalysisArtifacts>,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn analyze_runtime_coverage(
    input: RuntimeCoverageAnalysisScope<'_>,
    seams: &HealthSeams<'_>,
) -> Result<Option<fallow_output::RuntimeCoverageReport>, ExitCode> {
    let Some(production_options) = input.opts.runtime_coverage.as_ref() else {
        return Ok(None);
    };
    let Some(analysis_output) = input.shared_analysis_output else {
        return Err(emit_error(
            "runtime coverage requires analysis output",
            2,
            input.opts.output,
        ));
    };
    (seams.runtime_coverage_analyzer)(
        production_options,
        RuntimeCoverageSeamInput {
            root: &input.config.root,
            modules: input.modules,
            analysis_output,
            istanbul_coverage: input.istanbul_coverage,
            file_paths: input.file_paths,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            top: input.opts.top,
            codeowners_path: input.config.codeowners.as_deref(),
            quiet: input.opts.quiet,
            output: input.opts.output,
        },
    )
    .map(Some)
}

struct HealthAnalysisData {
    runtime_coverage: Option<fallow_output::RuntimeCoverageReport>,
    score_output: Option<scoring::FileScoreOutput>,
    files_scored: Option<usize>,
    average_maintainability: Option<f64>,
    framework_health_facts: Option<FrameworkHealthFacts>,
    file_scores_ms: f64,
    git_churn_ms: f64,
    git_churn_cache_hit: bool,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
}

#[derive(Clone, Copy, Default)]
struct FrameworkHealthFacts {
    unused_load_data_keys_global_abstain: bool,
}

fn build_framework_health_diagnostics(
    config: &ResolvedConfig,
    facts: Option<FrameworkHealthFacts>,
) -> Option<fallow_output::FrameworkHealthDiagnostics> {
    let facts = facts?;
    let detected_frameworks = detect_frameworks(config);
    if detected_frameworks.is_empty() {
        return None;
    }

    let mut detectors = Vec::new();
    for framework in &detected_frameworks {
        add_framework_detectors(&mut detectors, framework, &config.rules, facts);
    }

    if detectors.is_empty() {
        return None;
    }

    Some(fallow_output::FrameworkHealthDiagnostics {
        detected_frameworks,
        detectors,
    })
}

fn detect_frameworks(config: &ResolvedConfig) -> Vec<String> {
    let mut deps = rustc_hash::FxHashSet::default();
    if let Ok(pkg) = PackageJson::load(&config.root.join("package.json")) {
        deps.extend(pkg.all_dependency_names());
    }
    for workspace in fallow_config::discover_workspaces(&config.root) {
        if let Ok(pkg) = PackageJson::load(&workspace.root.join("package.json")) {
            deps.extend(pkg.all_dependency_names());
        }
    }

    let mut frameworks = Vec::new();
    if deps.contains("react") || deps.contains("preact") || deps.contains("next") {
        frameworks.push("react".to_string());
    }
    if deps.contains("next") {
        frameworks.push("next".to_string());
    }
    if deps.contains("vue") || deps.contains("@vue/runtime-core") {
        frameworks.push("vue".to_string());
    }
    if deps.contains("nuxt") {
        frameworks.push("nuxt".to_string());
    }
    if deps.contains("svelte") || deps.contains("@sveltejs/kit") {
        frameworks.push("svelte".to_string());
    }
    if deps.contains("@sveltejs/kit") {
        frameworks.push("sveltekit".to_string());
    }
    if deps.contains("@angular/core") {
        frameworks.push("angular".to_string());
    }
    frameworks.sort_unstable();
    frameworks.dedup();
    frameworks
}

fn add_framework_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
    facts: FrameworkHealthFacts,
) {
    match framework {
        "angular" => add_angular_detectors(detectors, framework, rules),
        "next" => add_next_detectors(detectors, framework, rules),
        "nuxt" => add_nuxt_detectors(detectors, framework, rules),
        "vue" => add_vue_detectors(detectors, framework, rules),
        "react" => add_react_detectors(detectors, framework, rules),
        "svelte" => add_svelte_detectors(detectors, framework, rules),
        "sveltekit" => add_sveltekit_detectors(detectors, framework, rules, facts),
        _ => {}
    }
}

fn add_angular_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-input",
        rules.unused_component_inputs,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-output",
        rules.unused_component_outputs,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_next_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "invalid-client-export",
        rules.invalid_client_export,
    );
    add_detector(
        detectors,
        framework,
        "mixed-client-server-barrel",
        rules.mixed_client_server_barrel,
    );
    add_detector(
        detectors,
        framework,
        "misplaced-directive",
        rules.misplaced_directive,
    );
    add_detector(
        detectors,
        framework,
        "route-collision",
        rules.route_collision,
    );
    add_detector(
        detectors,
        framework,
        "dynamic-segment-name-conflict",
        rules.dynamic_segment_name_conflict,
    );
    add_detector(
        detectors,
        framework,
        "unused-server-action",
        rules.unused_server_actions,
    );
}

fn add_nuxt_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-emit",
        rules.unused_component_emits,
    );
    add_not_checked_detector(
        detectors,
        framework,
        "unprovided-inject",
        "requires_vue_runtime_dependency",
    );
}

fn add_vue_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-emit",
        rules.unused_component_emits,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_react_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(detectors, framework, "prop-drilling", rules.prop_drilling);
    add_detector(detectors, framework, "thin-wrapper", rules.thin_wrapper);
    add_detector(
        detectors,
        framework,
        "duplicate-prop-shape",
        rules.duplicate_prop_shape,
    );
}

fn add_svelte_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
) {
    add_detector(
        detectors,
        framework,
        "unrendered-component",
        rules.unrendered_components,
    );
    add_detector(
        detectors,
        framework,
        "unused-component-prop",
        rules.unused_component_props,
    );
    add_detector(
        detectors,
        framework,
        "unused-svelte-event",
        rules.unused_svelte_events,
    );
    add_detector(
        detectors,
        framework,
        "unprovided-inject",
        rules.unprovided_injects,
    );
}

fn add_sveltekit_detectors(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    rules: &fallow_config::RulesConfig,
    facts: FrameworkHealthFacts,
) {
    if facts.unused_load_data_keys_global_abstain && rules.unused_load_data_keys != Severity::Off {
        detectors.push(fallow_output::FrameworkHealthDetector {
            id: "unused-load-data-key".to_string(),
            framework: framework.to_string(),
            status: fallow_output::FrameworkHealthDetectorStatus::Abstained,
            reason: Some("unused_load_data_keys_global_abstain".to_string()),
        });
    } else {
        add_detector(
            detectors,
            framework,
            "unused-load-data-key",
            rules.unused_load_data_keys,
        );
    }
}

fn add_detector(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    id: &str,
    severity: Severity,
) {
    let (status, reason) = if severity == Severity::Off {
        (
            fallow_output::FrameworkHealthDetectorStatus::DisabledByConfig,
            Some("disabled_by_config".to_string()),
        )
    } else {
        (fallow_output::FrameworkHealthDetectorStatus::Active, None)
    };
    detectors.push(fallow_output::FrameworkHealthDetector {
        id: id.to_string(),
        framework: framework.to_string(),
        status,
        reason,
    });
}

fn add_not_checked_detector(
    detectors: &mut Vec<fallow_output::FrameworkHealthDetector>,
    framework: &str,
    id: &str,
    reason: &str,
) {
    detectors.push(fallow_output::FrameworkHealthDetector {
        id: id.to_string(),
        framework: framework.to_string(),
        status: fallow_output::FrameworkHealthDetectorStatus::NotChecked,
        reason: Some(reason.to_string()),
    });
}

struct HealthRuntimeSectionsInput<'a> {
    config: &'a ResolvedConfig,
    files: &'a [fallow_types::discover::DiscoveredFile],
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    loaded_baseline: Option<&'a HealthBaselineData>,
    findings: &'a [ComplexityViolation],
    analysis_data: HealthAnalysisData,
    has_istanbul_coverage: bool,
    needs_file_scores: bool,
}

struct HealthRuntimeSections {
    analysis_data: HealthAnalysisData,
    derived_sections: HealthDerivedSections,
    vital_data: HealthVitalData,
}

fn prepare_health_runtime_sections(
    opts: &HealthOptions<'_>,
    mut input: HealthRuntimeSectionsInput<'_>,
) -> Result<HealthRuntimeSections, ExitCode> {
    let file_scores_slice = health_file_scores_slice(input.analysis_data.score_output.as_ref());
    let derived_sections = prepare_health_derived_sections(
        opts,
        HealthDerivedSectionInput {
            config: input.config,
            files: input.files,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            file_scores: file_scores_slice,
            churn_fetch: input.analysis_data.churn_fetch.take(),
            diff_index: input.diff_index,
            score_output: input.analysis_data.score_output.as_ref(),
            loaded_baseline: input.loaded_baseline,
        },
    );

    finalize_health_runtime_outputs(
        opts,
        HealthRuntimeFinalizeInput {
            config: input.config,
            runtime_coverage: &mut input.analysis_data.runtime_coverage,
            findings: input.findings,
            targets: &derived_sections.targets,
            loaded_baseline: input.loaded_baseline,
            changed_files: input.changed_files,
            diff_index: input.diff_index,
        },
    )?;

    let vital_data = prepare_health_vital_data_from_sections(
        opts,
        &input,
        &derived_sections,
        file_scores_slice,
    )?;

    Ok(HealthRuntimeSections {
        analysis_data: input.analysis_data,
        derived_sections,
        vital_data,
    })
}

fn prepare_health_vital_data_from_sections(
    opts: &HealthOptions<'_>,
    input: &HealthRuntimeSectionsInput<'_>,
    derived_sections: &HealthDerivedSections,
    file_scores_slice: &[FileHealthScore],
) -> Result<HealthVitalData, ExitCode> {
    prepare_health_vital_data(&HealthVitalDataInput {
        opts,
        modules: input.modules,
        file_paths: input.file_paths,
        score_output: input.analysis_data.score_output.as_ref(),
        file_scores_slice,
        hotspots: &derived_sections.hotspots,
        dupes_report: derived_sections.dupes_report.as_ref(),
        candidate_paths: &derived_sections.candidate_paths,
        total_files: input.files.len(),
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        diff_index: input.diff_index,
        hotspot_summary: derived_sections.hotspot_summary.as_ref(),
        has_istanbul_coverage: input.has_istanbul_coverage,
        needs_file_scores: input.needs_file_scores,
    })
}

struct HealthAnalysisDataInput<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pre_computed_analysis: Option<crate::DeadCodeAnalysisArtifacts>,
    needs_file_scores: bool,
    seams: &'a HealthSeams<'a>,
}

fn prepare_health_analysis_data(
    input: HealthAnalysisDataInput<'_>,
) -> Result<HealthAnalysisData, ExitCode> {
    let mut input = input;
    let needs_analysis_output = input.needs_file_scores || input.opts.runtime_coverage.is_some();
    let seams = input.seams;
    let mut shared_analysis =
        prepare_shared_health_analysis(&mut input, needs_analysis_output, seams)?;

    let runtime_coverage = analyze_runtime_coverage(
        RuntimeCoverageAnalysisScope {
            opts: input.opts,
            config: input.config,
            modules: input.modules,
            shared_analysis_output: shared_analysis.output.as_ref(),
            istanbul_coverage: input.istanbul_coverage,
            file_paths: input.file_paths,
            ignore_set: input.ignore_set,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
        },
        seams,
    )?;

    let precomputed_for_scores = shared_analysis.take_for_file_scores(input.needs_file_scores);

    let (file_score_result, file_scores_ms, churn_fetch) = compute_file_scores_and_churn(
        FileScoresAndChurnInput {
            opts: input.opts,
            config: input.config,
            modules: input.modules,
            file_paths: input.file_paths,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            ignore_set: input.ignore_set,
            istanbul_coverage: input.istanbul_coverage,
            needs_file_scores: input.needs_file_scores,
        },
        precomputed_for_scores,
    )?;
    let (git_churn_ms, git_churn_cache_hit) = churn_fetch
        .as_ref()
        .map_or((0.0, false), |cf| (cf.git_log_ms, cf.cache_hit));
    let (score_output, files_scored, average_maintainability) = file_score_result;

    print_slow_churn_note(input.opts, churn_fetch.as_ref());

    Ok(HealthAnalysisData {
        runtime_coverage,
        score_output,
        files_scored,
        average_maintainability,
        framework_health_facts: shared_analysis.framework_health_facts,
        file_scores_ms,
        git_churn_ms,
        git_churn_cache_hit,
        churn_fetch,
    })
}

struct PreparedSharedHealthAnalysis {
    output: Option<crate::DeadCodeAnalysisArtifacts>,
    framework_health_facts: Option<FrameworkHealthFacts>,
}

impl PreparedSharedHealthAnalysis {
    fn take_for_file_scores(
        &mut self,
        needs_file_scores: bool,
    ) -> Option<crate::DeadCodeAnalysisArtifacts> {
        if needs_file_scores {
            self.output.take()
        } else {
            None
        }
    }
}

fn prepare_shared_health_analysis(
    input: &mut HealthAnalysisDataInput<'_>,
    needs_analysis_output: bool,
    seams: &HealthSeams<'_>,
) -> Result<PreparedSharedHealthAnalysis, ExitCode> {
    let output = prepare_shared_analysis_output(
        input.opts,
        input.config,
        input.modules,
        input.pre_computed_analysis.take(),
        needs_analysis_output,
    )?;
    let framework_health_facts = output.as_ref().map(|output| FrameworkHealthFacts {
        unused_load_data_keys_global_abstain: output.results.unused_load_data_keys_global_abstain,
    });
    if let Some(graph) = output.as_ref().and_then(|output| output.graph.as_ref()) {
        (seams.note_graph_structure)(graph.module_count(), graph.edge_count());
    }

    Ok(PreparedSharedHealthAnalysis {
        output,
        framework_health_facts,
    })
}

type FileScoresAndChurn = (FileScoreResult, f64, Option<hotspots::ChurnFetchResult>);

#[derive(Clone, Copy)]
struct FileScoresAndChurnInput<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    ignore_set: &'a globset::GlobSet,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    needs_file_scores: bool,
}

fn compute_file_scores_and_churn(
    input: FileScoresAndChurnInput<'_>,
    precomputed_for_scores: Option<crate::DeadCodeAnalysisArtifacts>,
) -> Result<FileScoresAndChurn, ExitCode> {
    let needs_churn = input.opts.hotspots || input.opts.targets;
    if input.needs_file_scores && needs_churn {
        return std::thread::scope(|s| {
            let churn_handle =
                s.spawn(|| hotspots::fetch_churn_data(input.opts, &input.config.cache_dir));
            let t = Instant::now();
            let score_result = compute_filtered_file_scores(FileScoreInput {
                config: input.config,
                modules: input.modules,
                file_paths: input.file_paths,
                changed_files: input.changed_files,
                ws_roots: input.ws_roots,
                ignore_set: input.ignore_set,
                output: input.opts.output,
                istanbul_coverage: input.istanbul_coverage,
                pre_computed: precomputed_for_scores,
            })?;
            let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
            let churn = churn_handle
                .join()
                .map_err(|_| emit_error("churn thread panicked", 2, input.opts.output))?;
            Ok((score_result, fs_ms, churn))
        });
    }

    let t = Instant::now();
    let score_result = if input.needs_file_scores {
        compute_filtered_file_scores(FileScoreInput {
            config: input.config,
            modules: input.modules,
            file_paths: input.file_paths,
            changed_files: input.changed_files,
            ws_roots: input.ws_roots,
            ignore_set: input.ignore_set,
            output: input.opts.output,
            istanbul_coverage: input.istanbul_coverage,
            pre_computed: precomputed_for_scores,
        })?
    } else {
        (None, None, None)
    };
    let fs_ms = t.elapsed().as_secs_f64() * 1000.0;
    let churn = if needs_churn {
        hotspots::fetch_churn_data(input.opts, &input.config.cache_dir)
    } else {
        None
    };
    Ok((score_result, fs_ms, churn))
}

fn print_slow_churn_note(
    opts: &HealthOptions<'_>,
    churn_fetch: Option<&hotspots::ChurnFetchResult>,
) {
    if let Some(cf) = churn_fetch
        && !cf.cache_hit
        && !opts.no_cache
        && !opts.quiet
        && cf.git_log_ms > 500.0
    {
        eprintln!(
            "{}",
            format!(
                "  note: git churn analysis took {:.1}s (cached for next run at same HEAD)",
                cf.git_log_ms / 1000.0
            )
            .dimmed()
        );
    }
}

fn count_finding_severities(findings: &[ComplexityViolation]) -> (usize, usize, usize) {
    let (mut critical, mut high, mut moderate) = (0usize, 0usize, 0usize);
    for finding in findings {
        match finding.severity {
            FindingSeverity::Critical => critical += 1,
            FindingSeverity::High => high += 1,
            FindingSeverity::Moderate => moderate += 1,
        }
    }
    (critical, high, moderate)
}

fn apply_health_baseline_and_top(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &mut Vec<ComplexityViolation>,
) -> Result<Option<HealthBaselineData>, ExitCode> {
    let loaded_baseline = if let Some(load_path) = opts.baseline {
        Some(load_health_baseline(
            load_path,
            findings,
            &config.root,
            opts.quiet,
            opts.output,
        )?)
    } else {
        None
    };
    if let Some(top) = opts.top {
        findings.truncate(top);
    }
    Ok(loaded_baseline)
}

struct FilteredHotspotInput<'a> {
    opts: &'a HealthOptions<'a>,
    config: &'a ResolvedConfig,
    file_scores_slice: &'a [FileHealthScore],
    ignore_set: &'a globset::GlobSet,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    churn_fetch: Option<hotspots::ChurnFetchResult>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn compute_filtered_hotspots(
    input: FilteredHotspotInput<'_>,
) -> (Vec<HotspotEntry>, Option<HotspotSummary>, f64) {
    let t = Instant::now();
    let (mut hotspots, hotspot_summary) = if let Some(churn_data) = input.churn_fetch {
        compute_hotspots(
            input.opts,
            input.config,
            input.file_scores_slice,
            input.ignore_set,
            input.ws_roots,
            churn_data,
        )
    } else {
        (Vec::new(), None)
    };
    if let Some(diff_index) = input.diff_index {
        filter_hotspots_by_diff(&mut hotspots, diff_index, &input.config.root);
    }
    (
        hotspots,
        hotspot_summary,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

#[derive(Clone, Copy)]
struct FilteredTargetInput<'a> {
    opts: &'a HealthOptions<'a>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores_slice: &'a [FileHealthScore],
    hotspots: &'a [HotspotEntry],
    loaded_baseline: Option<&'a HealthBaselineData>,
    config: &'a ResolvedConfig,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
}

fn compute_filtered_targets(
    input: FilteredTargetInput<'_>,
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>, f64) {
    let t = Instant::now();
    let (mut targets, target_thresholds) = compute_targets(&input);
    if let Some(diff_index) = input.diff_index {
        filter_refactoring_targets_by_diff(&mut targets, diff_index, &input.config.root);
    }
    (
        targets,
        target_thresholds,
        t.elapsed().as_secs_f64() * 1000.0,
    )
}

fn filter_runtime_coverage_report(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    report: Option<&mut fallow_output::RuntimeCoverageReport>,
    loaded_baseline: Option<&HealthBaselineData>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&fallow_output::DiffIndex>,
) {
    if let Some(report) = report {
        let ctx = RuntimeCoverageFilterContext::new(&config.root)
            .with_baseline(loaded_baseline)
            .with_top(opts.top)
            .with_changed_files(changed_files)
            .with_diff_index(diff_index);
        apply_runtime_coverage_filters(report, &ctx);
    }
}

fn save_health_baseline_if_requested(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    findings: &[ComplexityViolation],
    runtime_coverage: Option<&fallow_output::RuntimeCoverageReport>,
    targets: &[RefactoringTarget],
) -> Result<(), ExitCode> {
    if let Some(save_path) = opts.save_baseline {
        save_health_baseline(&HealthBaselineSaveInput {
            save_path,
            findings,
            runtime_coverage_findings: runtime_coverage
                .map_or(&[], |report| report.findings.as_slice()),
            targets,
            config_root: &config.root,
            quiet: opts.quiet,
            output: opts.output,
        })?;
    }
    Ok(())
}

struct HealthRuntimeFinalizeInput<'a> {
    config: &'a ResolvedConfig,
    runtime_coverage: &'a mut Option<fallow_output::RuntimeCoverageReport>,
    findings: &'a [ComplexityViolation],
    targets: &'a [RefactoringTarget],
    loaded_baseline: Option<&'a HealthBaselineData>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn finalize_health_runtime_outputs(
    opts: &HealthOptions<'_>,
    input: HealthRuntimeFinalizeInput<'_>,
) -> Result<(), ExitCode> {
    let HealthRuntimeFinalizeInput {
        config,
        runtime_coverage,
        findings,
        targets,
        loaded_baseline,
        changed_files,
        diff_index,
    } = input;

    filter_runtime_coverage_report(
        opts,
        config,
        runtime_coverage.as_mut(),
        loaded_baseline,
        changed_files,
        diff_index,
    );
    save_health_baseline_if_requested(opts, config, findings, runtime_coverage.as_ref(), targets)
}

fn prepare_health_duplication_data(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> (
    rustc_hash::FxHashSet<std::path::PathBuf>,
    Option<crate::duplicates::DuplicationReport>,
    f64,
) {
    let candidate_paths =
        collect_candidate_paths(files, config, changed_files, ws_roots, ignore_set);
    let (dupes_report, duplication_ms) =
        compute_health_duplication_report(opts, config, files, &candidate_paths);
    (candidate_paths, dupes_report, duplication_ms)
}

fn compute_health_duplication_report(
    opts: &HealthOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> (Option<crate::duplicates::DuplicationReport>, f64) {
    let t = Instant::now();
    let dupes_report = if opts.score || opts.targets {
        let scoped_files = filter_files_to_paths(files, candidate_paths);
        Some(if opts.no_cache {
            crate::duplicates::find_duplicates(&config.root, &scoped_files, &config.duplicates)
        } else {
            crate::duplicates::find_duplicates_cached(
                &config.root,
                &scoped_files,
                &config.duplicates,
                &config.cache_dir,
            )
        })
    } else {
        None
    };
    (dupes_report, t.elapsed().as_secs_f64() * 1000.0)
}

struct HealthVitalData {
    vital_signs: fallow_output::VitalSigns,
    health_score: Option<HealthScore>,
    health_trend: Option<fallow_output::HealthTrend>,
    large_functions: Vec<fallow_output::LargeFunctionEntry>,
}

struct HealthVitalDataInput<'a> {
    opts: &'a HealthOptions<'a>,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    score_output: Option<&'a scoring::FileScoreOutput>,
    file_scores_slice: &'a [FileHealthScore],
    hotspots: &'a [HotspotEntry],
    dupes_report: Option<&'a crate::duplicates::DuplicationReport>,
    candidate_paths: &'a rustc_hash::FxHashSet<std::path::PathBuf>,
    total_files: usize,
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
    hotspot_summary: Option<&'a HotspotSummary>,
    has_istanbul_coverage: bool,
    needs_file_scores: bool,
}

/// Assign the prop-drilling chain count / max depth onto the vital signs. Prop
/// drilling is a whole-project graph signal (the chains live in AnalysisResults,
/// surfaced via FileScoreOutput); only populated when the opt-in `prop-drilling`
/// rule emitted chains, so the small capped penalty stays dormant by default.
fn apply_prop_drilling_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    score_output: &scoring::FileScoreOutput,
) {
    if score_output.prop_drilling_chains.is_empty() {
        return;
    }
    vital_signs.prop_drilling_chain_count =
        u32::try_from(score_output.prop_drilling_chains.len()).ok();
    vital_signs.prop_drilling_max_depth = score_output
        .prop_drilling_chains
        .iter()
        .map(|c| c.chain.depth)
        .max();
}

/// Assign the descriptive render fan-in blast-radius metric (p95 / high-pct / max
/// distinct parents plus a located top-N list) onto the vital signs. Aggregates
/// are precomputed in core and ride on FileScoreOutput; non-React runs leave the
/// fields `None` (skip_serializing_if), so the JSON contract is unchanged.
fn apply_render_fan_in_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    score_output: &scoring::FileScoreOutput,
    config: &ResolvedConfig,
) {
    let Some(metric) = score_output.render_fan_in.as_ref() else {
        return;
    };
    vital_signs.p95_render_fan_in = metric.p95_distinct_parents;
    vital_signs.render_fan_in_high_pct = metric.high_pct;
    // The public headline (`max_render_fan_in`) is the max DISTINCT-PARENTS:
    // honest blast radius = the most distinct render LOCATIONS any one
    // component is rendered from. `render_sites` (incl. repeats) is secondary.
    vital_signs.max_render_fan_in = metric.max_distinct_parents;

    // Located top-N list so a consumer sees WHICH component carries the
    // headline fan-in, not just the number. The core carrier is sorted by
    // (path, component) for run-stability and INCLUDES rendered-nowhere `0`
    // entries (for the percentile distribution), so re-sort by
    // distinct_parents (the honest headline axis) descending, tie-break on
    // render_sites descending, and drop the `0`-fan-in entries here. Final
    // tie-break on (path, component) so the cap is deterministic. Cap at a
    // small N.
    const MAX_TOP_RENDER_FAN_IN: usize = 20;
    let mut top: Vec<&fallow_types::results::RenderFanInComponent> = metric
        .per_component
        .iter()
        .filter(|c| c.distinct_parents > 0)
        .collect();
    top.sort_by(|a, b| {
        b.distinct_parents
            .cmp(&a.distinct_parents)
            .then_with(|| b.render_sites.cmp(&a.render_sites))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.component.cmp(&b.component))
    });
    vital_signs.top_render_fan_in = top
        .into_iter()
        .take(MAX_TOP_RENDER_FAN_IN)
        .map(|c| fallow_output::RenderFanInTopComponent {
            component: c.component.clone(),
            path: c
                .file
                .strip_prefix(&config.root)
                .unwrap_or(&c.file)
                .to_path_buf(),
            render_sites: c.render_sites,
            distinct_parents: c.distinct_parents,
        })
        .collect();
}

/// Compute the scoped vital signs / counts for the candidate subset, then assign
/// the prop-drilling and render fan-in metrics onto the vital signs.
fn compute_scoped_vital_signs(
    input: &HealthVitalDataInput<'_>,
    total_files_scoped: usize,
    project_subset: &SubsetFilter<'_>,
) -> (fallow_output::VitalSigns, fallow_output::VitalSignsCounts) {
    let vital_signs_input = VitalSignsAndCountsInput {
        score_output: input.score_output,
        modules: input.modules,
        file_paths: input.file_paths,
        needs_file_scores: input.needs_file_scores,
        file_scores_slice: input.file_scores_slice,
        needs_hotspots: input.opts.hotspots || input.opts.targets,
        hotspots: input.hotspots,
        total_files: total_files_scoped,
        subset: project_subset,
    };
    let (mut vital_signs, counts) = compute_vital_signs_and_counts(&vital_signs_input);

    if let Some(score_output) = input.score_output {
        apply_prop_drilling_metrics(&mut vital_signs, score_output);
        apply_render_fan_in_metrics(&mut vital_signs, score_output, input.config);
    }
    (vital_signs, counts)
}

/// Persist the health snapshot when `--save-snapshot` was requested.
fn maybe_save_health_snapshot(
    input: &HealthVitalDataInput<'_>,
    vital_signs: &fallow_output::VitalSigns,
    counts: &fallow_output::VitalSignsCounts,
    health_score: Option<&HealthScore>,
) -> Result<(), ExitCode> {
    if let Some(ref snapshot_path) = input.opts.save_snapshot {
        save_snapshot(SnapshotInput {
            opts: input.opts,
            snapshot_path,
            vital_signs,
            counts,
            hotspot_summary: input.hotspot_summary,
            health_score,
            coverage_model: Some(active_health_coverage_model(input.has_istanbul_coverage)),
        })?;
    }
    Ok(())
}

fn prepare_health_vital_data(
    input: &HealthVitalDataInput<'_>,
) -> Result<HealthVitalData, ExitCode> {
    let project_subset = if input.candidate_paths.len() == input.total_files {
        SubsetFilter::Full
    } else {
        SubsetFilter::Paths(input.candidate_paths)
    };
    let total_files_scoped = input.candidate_paths.len();
    let (mut vital_signs, mut counts) =
        compute_scoped_vital_signs(input, total_files_scoped, &project_subset);

    let health_score = compute_health_score_metrics(
        input.opts,
        input.dupes_report,
        &mut vital_signs,
        &mut counts,
        total_files_scoped,
    );
    let large_functions = collect_filtered_large_functions(FilteredLargeFunctionInput {
        vital_signs: &vital_signs,
        modules: input.modules,
        file_paths: input.file_paths,
        config: input.config,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
        diff_index: input.diff_index,
    });
    maybe_save_health_snapshot(input, &vital_signs, &counts, health_score.as_ref())?;
    let health_trend =
        compute_health_trend(input.opts, &vital_signs, &counts, health_score.as_ref());

    Ok(HealthVitalData {
        vital_signs,
        health_score,
        health_trend,
        large_functions,
    })
}

fn compute_health_score_metrics(
    opts: &HealthOptions<'_>,
    dupes_report: Option<&crate::duplicates::DuplicationReport>,
    vital_signs: &mut fallow_output::VitalSigns,
    counts: &mut fallow_output::VitalSignsCounts,
    total_files_scoped: usize,
) -> Option<HealthScore> {
    if opts.score
        && let Some(report) = dupes_report
    {
        apply_duplication_metrics(vital_signs, counts, report);
    }
    opts.score
        .then(|| vital_signs::compute_health_score(vital_signs, total_files_scoped))
}

#[derive(Clone, Copy)]
struct FilteredLargeFunctionInput<'a> {
    vital_signs: &'a fallow_output::VitalSigns,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    config: &'a ResolvedConfig,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    diff_index: Option<&'a fallow_output::DiffIndex>,
}

fn collect_filtered_large_functions(
    input: FilteredLargeFunctionInput<'_>,
) -> Vec<fallow_output::LargeFunctionEntry> {
    let large_input = LargeFunctionInput {
        vital_signs: input.vital_signs,
        modules: input.modules,
        file_paths: input.file_paths,
        config_root: &input.config.root,
        ignore_set: input.ignore_set,
        changed_files: input.changed_files,
        ws_roots: input.ws_roots,
    };
    let mut large_functions = collect_large_functions(&large_input);
    if let Some(diff_index) = input.diff_index {
        filter_large_functions_by_diff(&mut large_functions, diff_index, &input.config.root);
    }
    large_functions
}

/// Drop complexity findings whose function body span does NOT overlap any
/// added line in the supplied diff. The function spans
/// `[line..=line + line_count - 1]`: a hotspot that starts before the
/// diff but extends into a touched line counts as overlap. `line_count`
/// of zero collapses to `[line..=line]` so older fixture rows without
/// extents do not silently match every diff.
///
/// Paths that cannot be expressed relative to `root` (different drive,
/// path-traversal escape) are RETAINED rather than silently dropped:
/// surfacing an unfilterable path is better than hiding a finding.
fn filter_complexity_findings_by_diff(
    findings: &mut Vec<ComplexityViolation>,
    diff_index: &fallow_output::DiffIndex,
    root: &std::path::Path,
) {
    findings.retain(|f| {
        let Some(rel) = relative_to_root(&f.path, root) else {
            return true;
        };
        let start = u64::from(f.line);
        let end = if f.line_count == 0 {
            start
        } else {
            start + u64::from(f.line_count) - 1
        };
        diff_index.range_overlaps_added(&rel, start, end)
    });
}

/// Drop hotspot entries whose file is not touched by the supplied diff.
/// Hotspots are per-file aggregates without a per-line position
/// (`HotspotEntry` has no `line` field), so file-level matching is the
/// only signal the diff carries. Paths outside `root` are RETAINED for
/// the same reason as [`filter_complexity_findings_by_diff`].
fn filter_hotspots_by_diff(
    hotspots: &mut Vec<fallow_output::HotspotEntry>,
    diff_index: &fallow_output::DiffIndex,
    root: &std::path::Path,
) {
    hotspots.retain(|h| match relative_to_root(&h.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop refactoring targets whose file is not touched by the diff.
/// `RefactoringTarget` is per-file (no line range on the target itself);
/// the line-anchored evidence under `target.evidence.complex_functions`
/// is left intact for downstream renderers because dropping individual
/// evidence rows could turn a multi-function recommendation into a
/// confusing zero-evidence entry.
fn filter_refactoring_targets_by_diff(
    targets: &mut Vec<fallow_output::RefactoringTarget>,
    diff_index: &fallow_output::DiffIndex,
    root: &std::path::Path,
) {
    targets.retain(|t| match relative_to_root(&t.path, root) {
        Some(rel) => diff_index.touches_file(&rel),
        None => true,
    });
}

/// Drop large-function entries whose body span does NOT overlap any added
/// line in the supplied diff. Same range semantics as
/// [`filter_complexity_findings_by_diff`].
fn filter_large_functions_by_diff(
    entries: &mut Vec<fallow_output::LargeFunctionEntry>,
    diff_index: &fallow_output::DiffIndex,
    root: &std::path::Path,
) {
    entries.retain(|e| {
        let Some(rel) = relative_to_root(&e.path, root) else {
            return true;
        };
        let start = u64::from(e.line);
        let end = if e.line_count == 0 {
            start
        } else {
            start + u64::from(e.line_count) - 1
        };
        diff_index.range_overlaps_added(&rel, start, end)
    });
}

fn collect_candidate_paths(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<std::path::PathBuf> {
    files
        .iter()
        .filter(|file| {
            path_in_health_scope(&file.path, config, changed_files, ws_roots, ignore_set)
        })
        .map(|file| file.path.clone())
        .collect()
}

fn filter_files_to_paths(
    files: &[fallow_types::discover::DiscoveredFile],
    candidate_paths: &rustc_hash::FxHashSet<std::path::PathBuf>,
) -> Vec<fallow_types::discover::DiscoveredFile> {
    files
        .iter()
        .filter(|file| candidate_paths.contains(&file.path))
        .cloned()
        .collect()
}

pub(crate) fn apply_duplication_metrics(
    vital_signs: &mut fallow_output::VitalSigns,
    counts: &mut fallow_output::VitalSignsCounts,
    dupes_report: &crate::duplicates::DuplicationReport,
) {
    let pct = dupes_report.stats.duplication_percentage;
    vital_signs.duplication_pct = Some((pct * 10.0).round() / 10.0);
    counts.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    if let Some(ref mut vc) = vital_signs.counts {
        vc.duplicated_lines = Some(dupes_report.stats.duplicated_lines);
    }
}

/// Sort findings by the specified criteria.
fn sort_findings(findings: &mut [ComplexityViolation], sort: HealthSort) {
    match sort {
        HealthSort::Severity => findings.sort_by_key(|f| {
            std::cmp::Reverse((
                exceeded_priority(f.exceeded),
                severity_priority(f.severity),
                f.crap.is_some(),
                f.cyclomatic,
                f.cognitive,
                f.line_count,
            ))
        }),
        HealthSort::Cyclomatic => findings.sort_by_key(|f| std::cmp::Reverse(f.cyclomatic)),
        HealthSort::Cognitive => findings.sort_by_key(|f| std::cmp::Reverse(f.cognitive)),
        HealthSort::Lines => findings.sort_by_key(|f| std::cmp::Reverse(f.line_count)),
    }
}

const fn exceeded_priority(exceeded: ExceededThreshold) -> u8 {
    match exceeded {
        ExceededThreshold::All => 5,
        ExceededThreshold::CyclomaticCrap | ExceededThreshold::CognitiveCrap => 4,
        ExceededThreshold::Crap => 3,
        ExceededThreshold::Both => 2,
        ExceededThreshold::Cyclomatic | ExceededThreshold::Cognitive => 1,
    }
}

const fn severity_priority(severity: FindingSeverity) -> u8 {
    match severity {
        FindingSeverity::Critical => 3,
        FindingSeverity::High => 2,
        FindingSeverity::Moderate => 1,
    }
}

/// `(score_output, files_scored, average_maintainability)`.
type FileScoreResult = (Option<scoring::FileScoreOutput>, Option<usize>, Option<f64>);

/// Compute file scores, applying workspace and ignore filters.
struct FileScoreInput<'a> {
    config: &'a ResolvedConfig,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    ignore_set: &'a globset::GlobSet,
    output: OutputFormat,
    istanbul_coverage: Option<&'a scoring::IstanbulCoverage>,
    pre_computed: Option<crate::DeadCodeAnalysisArtifacts>,
}

fn compute_filtered_file_scores(input: FileScoreInput<'_>) -> Result<FileScoreResult, ExitCode> {
    let analysis_output = if let Some(pre) = input.pre_computed {
        pre
    } else {
        crate::analyze_with_parse_result(input.config, input.modules)
            .map_err(|e| emit_error(&format!("analysis failed: {e}"), 2, input.output))?
    };
    match compute_file_scores(
        input.modules,
        input.file_paths,
        input.changed_files,
        analysis_output,
        input.istanbul_coverage,
        &input.config.root,
    ) {
        Ok(mut output) => {
            if let Some(ws) = input.ws_roots {
                output
                    .scores
                    .retain(|s| ws.iter().any(|r| s.path.starts_with(r)));
            }
            if !input.ignore_set.is_empty() {
                output.scores.retain(|s| {
                    let relative = s.path.strip_prefix(&input.config.root).unwrap_or(&s.path);
                    !input.ignore_set.is_match(relative)
                });
            }
            filter_coverage_gaps(
                &mut output.coverage.report,
                &mut output.coverage.runtime_paths,
                input.config,
                input.changed_files,
                input.ws_roots,
                input.ignore_set,
            );
            let total_scored = output.scores.len();
            let avg = if total_scored > 0 {
                let sum: f64 = output.scores.iter().map(|s| s.maintainability_index).sum();
                Some((sum / total_scored as f64 * 10.0).round() / 10.0)
            } else {
                None
            };
            Ok((Some(output), Some(total_scored), avg))
        }
        Err(e) => {
            eprintln!("Warning: failed to compute file scores: {e}");
            Ok((None, Some(0), None))
        }
    }
}

/// Compute refactoring targets when requested, applying baseline and top filters.
fn compute_targets(
    input: &FilteredTargetInput<'_>,
) -> (Vec<RefactoringTarget>, Option<TargetThresholds>) {
    if !input.opts.targets {
        return (Vec::new(), None);
    }
    let Some(output) = input.score_output else {
        return (Vec::new(), None);
    };
    let clone_siblings = input
        .dupes_report
        .map_or_else(rustc_hash::FxHashMap::default, |report| {
            targets::build_clone_sibling_evidence(report)
        });
    let target_aux = TargetAuxData::from_output(output, &clone_siblings);
    let (mut tgts, thresholds) =
        compute_refactoring_targets(input.file_scores_slice, &target_aux, input.hotspots);
    if let Some(baseline) = input.loaded_baseline {
        tgts = filter_new_health_targets(tgts, baseline, &input.config.root);
    }
    if let Some(ref effort) = input.opts.effort {
        tgts.retain(|t| t.effort == *effort);
    }
    if let Some(top) = input.opts.top {
        tgts.truncate(top);
    }
    (tgts, Some(thresholds))
}

fn path_in_health_scope(
    path: &std::path::Path,
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) -> bool {
    if let Some(changed) = changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = ws_roots
        && !ws.iter().any(|r| path.starts_with(r))
    {
        return false;
    }
    if !ignore_set.is_empty() {
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            return false;
        }
    }
    true
}

fn filter_coverage_gaps(
    coverage_gaps: &mut CoverageGaps,
    runtime_paths: &mut Vec<std::path::PathBuf>,
    config: &ResolvedConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    ignore_set: &globset::GlobSet,
) {
    runtime_paths
        .retain(|path| path_in_health_scope(path, config, changed_files, ws_roots, ignore_set));
    coverage_gaps.files.retain(|item| {
        path_in_health_scope(&item.file.path, config, changed_files, ws_roots, ignore_set)
    });
    coverage_gaps.exports.retain(|item| {
        path_in_health_scope(
            &item.export.path,
            config,
            changed_files,
            ws_roots,
            ignore_set,
        )
    });

    runtime_paths.sort();
    runtime_paths.dedup();

    let runtime_files = runtime_paths.len();
    let untested_files = coverage_gaps.files.len();
    let covered_files = runtime_files.saturating_sub(untested_files);
    coverage_gaps.summary = scoring::build_coverage_summary(
        runtime_files,
        covered_files,
        untested_files,
        coverage_gaps.exports.len(),
    );
}

/// Subset selector used when scoping `vital_signs`, `health_score`, and
/// `analysis_counts` to a workspace package or a `--group-by` bucket.
///
/// `Full` skips filtering entirely (project-wide). `Paths` matches files whose
/// absolute path is in the given set (exact match), which is what scoped
/// project runs and `--group-by` use to keep every score input on the same
/// filtered file set.
pub enum SubsetFilter<'a> {
    Full,
    Paths(&'a rustc_hash::FxHashSet<std::path::PathBuf>),
}

impl SubsetFilter<'_> {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }
    pub fn matches(&self, path: &std::path::Path) -> bool {
        match self {
            Self::Full => true,
            Self::Paths(set) => set.contains(path),
        }
    }
}

/// Build vital signs and counts for the slice of files selected by `subset`.
///
/// When `subset` is anything other than `SubsetFilter::Full`, per-module
/// aggregates (cyclomatic distribution, total LOC, unit profiles) are
/// restricted to modules in the subset, the analysis counts (`dead_files`,
/// `dead_exports`, `unused_deps`, `circular_deps`, `total_exports`) are
/// recomputed from the snapshot for the same subset, and `total_files` should
/// already reflect the subset-scoped count.
pub(crate) struct VitalSignsAndCountsInput<'a> {
    pub(crate) score_output: Option<&'a scoring::FileScoreOutput>,
    pub(crate) modules: &'a [crate::extract::ModuleInfo],
    pub(crate) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(crate) needs_file_scores: bool,
    pub(crate) file_scores_slice: &'a [FileHealthScore],
    pub(crate) needs_hotspots: bool,
    pub(crate) hotspots: &'a [HotspotEntry],
    pub(crate) total_files: usize,
    pub(crate) subset: &'a SubsetFilter<'a>,
}

pub(crate) fn compute_vital_signs_and_counts(
    input: &VitalSignsAndCountsInput<'_>,
) -> (fallow_output::VitalSigns, fallow_output::VitalSignsCounts) {
    let analysis_counts = input.score_output.map(|o| {
        o.analysis_snapshot
            .counts_for(input.subset, &o.analysis_counts)
    });
    let module_filter_set: Option<rustc_hash::FxHashSet<crate::discover::FileId>> =
        if input.subset.is_full() {
            None
        } else {
            Some(
                input
                    .modules
                    .iter()
                    .filter_map(|m| {
                        let path = input.file_paths.get(&m.file_id)?;
                        if input.subset.matches(path) {
                            Some(m.file_id)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        };
    let vs_input = vital_signs::VitalSignsInput {
        modules: input.modules,
        module_filter: module_filter_set.as_ref(),
        file_scores: if input.needs_file_scores {
            Some(input.file_scores_slice)
        } else {
            None
        },
        hotspots: if input.needs_hotspots {
            Some(input.hotspots)
        } else {
            None
        },
        total_files: input.total_files,
        analysis_counts,
    };
    let signs = vital_signs::compute_vital_signs(&vs_input);
    let counts = vital_signs::build_counts(&vs_input);
    (signs, counts)
}

/// Save a vital signs snapshot to disk if requested.
struct SnapshotInput<'a> {
    opts: &'a HealthOptions<'a>,
    snapshot_path: &'a std::path::Path,
    vital_signs: &'a fallow_output::VitalSigns,
    counts: &'a fallow_output::VitalSignsCounts,
    hotspot_summary: Option<&'a fallow_output::HotspotSummary>,
    health_score: Option<&'a fallow_output::HealthScore>,
    coverage_model: Option<fallow_output::CoverageModel>,
}

fn save_snapshot(input: SnapshotInput<'_>) -> Result<(), ExitCode> {
    let shallow = input.hotspot_summary.is_some_and(|s| s.shallow_clone);
    let snapshot = vital_signs::build_snapshot(
        input.vital_signs.clone(),
        input.counts.clone(),
        input.opts.root,
        shallow,
        input.health_score,
        input.coverage_model,
    );
    let explicit = if input.snapshot_path.as_os_str().is_empty() {
        None
    } else {
        Some(input.snapshot_path)
    };
    match vital_signs::save_snapshot(&snapshot, input.opts.root, explicit) {
        Ok(saved_path) => {
            if !input.opts.quiet {
                eprintln!("Saved vital signs snapshot to {}", saved_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(&e, 2, input.opts.output)),
    }
}

/// Compute health trend from historical snapshots if requested.
fn compute_health_trend(
    opts: &HealthOptions<'_>,
    vital_signs: &fallow_output::VitalSigns,
    counts: &fallow_output::VitalSignsCounts,
    health_score: Option<&fallow_output::HealthScore>,
) -> Option<fallow_output::HealthTrend> {
    if !opts.trend {
        return None;
    }
    if opts.changed_since.is_some() && !opts.quiet {
        eprintln!(
            "warning: --trend comparison may be inaccurate with --changed-since; \
             snapshots are typically from full-project runs"
        );
    }
    let snapshots = vital_signs::load_snapshots(opts.root);
    if snapshots.is_empty() && !opts.quiet {
        eprintln!(
            "No snapshots found. Run `fallow health --save-snapshot` to save a \
             baseline, then use --trend on subsequent runs to track progress."
        );
    }
    vital_signs::compute_trend(
        vital_signs,
        counts,
        health_score.map(|s| s.score),
        &snapshots,
    )
}

pub(crate) struct HealthReportAssembly {
    pub(crate) report_coverage_gaps: bool,
    pub(crate) findings: Vec<ComplexityViolation>,
    pub(crate) threshold_overrides: Vec<fallow_output::ThresholdOverrideState>,
    pub(crate) files_analyzed: usize,
    pub(crate) total_functions: usize,
    pub(crate) total_above_threshold: usize,
    pub(crate) max_cyclomatic: u16,
    pub(crate) max_cognitive: u16,
    pub(crate) max_crap: f64,
    pub(crate) files_scored: Option<usize>,
    pub(crate) average_maintainability: Option<f64>,
    pub(crate) vital_signs: fallow_output::VitalSigns,
    pub(crate) health_score: Option<fallow_output::HealthScore>,
    pub(crate) score_output: Option<scoring::FileScoreOutput>,
    pub(crate) hotspots: Vec<HotspotEntry>,
    pub(crate) hotspot_summary: Option<fallow_output::HotspotSummary>,
    pub(crate) targets: Vec<RefactoringTarget>,
    pub(crate) target_thresholds: Option<TargetThresholds>,
    pub(crate) health_trend: Option<fallow_output::HealthTrend>,
    pub(crate) has_istanbul_coverage: bool,
    pub(crate) runtime_coverage: Option<fallow_output::RuntimeCoverageReport>,
    pub(crate) framework_health: Option<fallow_output::FrameworkHealthDiagnostics>,
    pub(crate) large_functions: Vec<LargeFunctionEntry>,
    pub(crate) sev_critical: usize,
    pub(crate) sev_high: usize,
    pub(crate) sev_moderate: usize,
}

/// Collect functions exceeding 60 LOC when the unit size risk profile warrants it.
///
/// Only populated when `very_high_risk >= 3%` in the unit size profile (same threshold
/// that triggers showing the risk profile line). Sorted by line count descending.
struct LargeFunctionInput<'a> {
    vital_signs: &'a fallow_output::VitalSigns,
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    config_root: &'a std::path::Path,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
}

fn collect_large_functions(input: &LargeFunctionInput<'_>) -> Vec<LargeFunctionEntry> {
    let dominated = input
        .vital_signs
        .unit_size_profile
        .as_ref()
        .is_some_and(|p| p.very_high_risk >= 3.0);
    if !dominated {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = input.changed_files
            && !changed.contains(path.as_path())
        {
            continue;
        }
        if let Some(ws) = input.ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }
        for func in &module.complexity {
            if func.line_count > 60 {
                entries.push(LargeFunctionEntry {
                    path: path.clone(),
                    name: func.name.clone(),
                    line: func.line,
                    line_count: func.line_count,
                });
            }
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.line_count));
    entries
}

/// Build a glob set from health ignore patterns.
///
/// User patterns were validated at config load time
/// (see `FallowConfig::validate_user_globs`).
#[expect(
    clippy::expect_used,
    reason = "health ignore globs are validated before health analysis"
)]
fn build_ignore_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(
            globset::Glob::new(pattern)
                .expect("health.ignore pattern was validated at config load time"),
        );
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}

/// Collect health findings from parsed modules, applying ignore, changed-since,
/// and workspace filters. The returned `files_analyzed` / `total_functions`
/// counters reflect only modules that pass every filter so the rendered
/// summary matches the produced findings.
#[expect(
    clippy::too_many_arguments,
    reason = "filter pipeline mirrors compute_filtered_file_scores"
)]
#[cfg(test)]
fn collect_findings(
    modules: &[crate::extract::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    config_root: &std::path::Path,
    ignore_set: &globset::GlobSet,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&[std::path::PathBuf]>,
    max_cyclomatic: u16,
    max_cognitive: u16,
    complexity_breakdown: bool,
) -> (Vec<ComplexityViolation>, usize, usize) {
    let global = GlobalHealthThresholds {
        cyclomatic: max_cyclomatic,
        cognitive: max_cognitive,
        crap: 30.0,
    };
    let resolver = ThresholdOverrideResolver::new(&[], global);
    let mut tracker = ThresholdOverrideStateTracker::default();
    let mut input = CollectFindingsInput {
        modules,
        file_paths,
        config_root,
        ignore_set,
        changed_files,
        ws_roots,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
        complexity_breakdown,
    };
    collect_findings_with_resolver(&mut input)
}

struct CollectFindingsInput<'a> {
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    config_root: &'a std::path::Path,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    threshold_resolver: &'a ThresholdOverrideResolver,
    threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
    complexity_breakdown: bool,
}

fn collect_findings_with_resolver(
    input: &mut CollectFindingsInput<'_>,
) -> (Vec<ComplexityViolation>, usize, usize) {
    let mut files_analyzed = 0usize;
    let mut total_functions = 0usize;
    let mut findings: Vec<ComplexityViolation> = Vec::new();

    for module in input.modules {
        let Some((path, relative)) = collect_findings_module_path(input, module) else {
            continue;
        };

        files_analyzed += 1;
        // Precompute the per-function React hook profile ONCE per module from the
        // cached `hook_uses` IR (the sole reader of `module.hook_uses`). Aligned
        // by index to `module.complexity`; all-`None` at zero cost for non-React
        // files (empty `hook_uses`).
        let hook_profiles = react_hooks::build_module_hook_profiles(module);
        for (fc_idx, fc) in module.complexity.iter().enumerate() {
            total_functions += 1;
            if crate::suppress::is_suppressed(
                &module.suppressions,
                fc.line,
                crate::suppress::IssueKind::Complexity,
            ) {
                continue;
            }
            let react_hook_profile = hook_profiles.get(fc_idx).cloned().flatten();
            if let Some(finding) =
                collect_complexity_finding(input, path, relative, fc, react_hook_profile)
            {
                findings.push(finding);
            }
        }
    }

    (findings, files_analyzed, total_functions)
}

fn collect_findings_module_path<'a>(
    input: &CollectFindingsInput<'a>,
    module: &crate::extract::ModuleInfo,
) -> Option<(&'a std::path::PathBuf, &'a std::path::Path)> {
    let &path = input.file_paths.get(&module.file_id)?;
    let relative = path.strip_prefix(input.config_root).unwrap_or(path);
    if input.ignore_set.is_match(relative) {
        return None;
    }
    if let Some(changed) = input.changed_files
        && !changed.contains(path)
    {
        return None;
    }
    if let Some(ws) = input.ws_roots
        && !ws.iter().any(|root| path.starts_with(root))
    {
        return None;
    }
    Some((path, relative))
}

fn collect_complexity_finding(
    input: &mut CollectFindingsInput<'_>,
    path: &std::path::Path,
    relative: &std::path::Path,
    fc: &fallow_types::extract::FunctionComplexity,
    react_hook_profile: Option<fallow_output::ReactHookProfile>,
) -> Option<ComplexityViolation> {
    let (applied_thresholds, matched_overrides) =
        input.threshold_resolver.resolve(relative, &fc.name);
    input.threshold_state_tracker.record_complexity(
        ComplexityFunctionContext {
            path,
            function: &fc.name,
            cyclomatic: fc.cyclomatic,
            cognitive: fc.cognitive,
        },
        &matched_overrides,
        input.threshold_resolver.global,
    );
    let exceeds_cyclomatic = fc.cyclomatic > applied_thresholds.effective.max_cyclomatic;
    let exceeds_cognitive = fc.cognitive > applied_thresholds.effective.max_cognitive;
    if !exceeds_cyclomatic && !exceeds_cognitive {
        return None;
    }

    Some(ComplexityViolation {
        path: path.to_path_buf(),
        name: fc.name.clone(),
        line: fc.line,
        col: fc.col,
        cyclomatic: fc.cyclomatic,
        cognitive: fc.cognitive,
        line_count: fc.line_count,
        param_count: fc.param_count,
        react_hook_count: fc.react_hook_count,
        react_jsx_max_depth: fc.react_jsx_max_depth,
        react_prop_count: fc.react_prop_count,
        react_hook_profile,
        exceeded: ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, false),
        severity: compute_finding_severity(
            fc.cognitive,
            fc.cyclomatic,
            None,
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        ),
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: contributions_for(input.complexity_breakdown, fc),
        effective_thresholds: applied_thresholds
            .override_index
            .map(|_| applied_thresholds.effective),
        threshold_source: applied_thresholds
            .override_index
            .map(|_| fallow_output::ThresholdSource::Override),
    })
}

/// Clone the per-decision-point breakdown onto a finding only when the caller
/// opted in via `health --complexity-breakdown`; otherwise leave it empty so it
/// is omitted from JSON.
fn contributions_for(
    complexity_breakdown: bool,
    fc: &fallow_types::extract::FunctionComplexity,
) -> Vec<fallow_types::extract::ComplexityContribution> {
    if complexity_breakdown {
        fc.contributions.clone()
    } else {
        Vec::new()
    }
}

/// Merge per-function CRAP data into an existing complexity findings vector.
///
/// Functions that only exceed `--max-crap` (without exceeding cyclomatic or
/// cognitive) become new findings. Functions that already produced a finding
/// for cyclomatic/cognitive get their `crap` and `coverage_pct` fields
/// populated, and the `exceeded` discriminant plus `severity` are recomputed
/// to reflect CRAP's contribution.
struct CrapFindingMergeInput<'a> {
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    config_root: &'a std::path::Path,
    ignore_set: &'a globset::GlobSet,
    changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    ws_roots: Option<&'a [std::path::PathBuf]>,
    per_function_crap: &'a rustc_hash::FxHashMap<std::path::PathBuf, Vec<scoring::PerFunctionCrap>>,
    template_inherit_provenance: &'a rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
    complexity_breakdown: bool,
    threshold_resolver: &'a ThresholdOverrideResolver,
    threshold_state_tracker: &'a mut ThresholdOverrideStateTracker,
}

type ComplexityByPosition<'a> = rustc_hash::FxHashMap<
    &'a std::path::Path,
    rustc_hash::FxHashMap<(u32, u32), &'a fallow_types::extract::FunctionComplexity>,
>;

/// The precomputed position-keyed lookup maps shared across the CRAP merge pass:
/// existing-finding index, per-function complexity, React hook profiles, and
/// per-path suppressions.
struct CrapMergeMaps<'a> {
    finding_index: rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize>,
    complexity_by_pos: ComplexityByPosition<'a>,
    hook_profiles_by_pos: rustc_hash::FxHashMap<
        &'a std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
    >,
    suppressions_by_path:
        rustc_hash::FxHashMap<&'a std::path::Path, &'a Vec<crate::suppress::Suppression>>,
}

/// Process one path's per-function CRAP entries: record threshold state, skip
/// below-threshold / suppressed frames, then merge into an existing finding or
/// append a new one to `new_findings`.
fn process_crap_findings_for_path(
    path: &std::path::Path,
    per_fn: &[scoring::PerFunctionCrap],
    maps: &CrapMergeMaps<'_>,
    findings: &mut [ComplexityViolation],
    new_findings: &mut Vec<ComplexityViolation>,
    input: &mut CrapFindingMergeInput<'_>,
) {
    for pf in per_fn {
        let Some(fc) = maps
            .complexity_by_pos
            .get(path)
            .and_then(|m| m.get(&(pf.line, pf.col)).copied())
        else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        let (applied_thresholds, matched_overrides) =
            input.threshold_resolver.resolve(relative, &fc.name);
        input.threshold_state_tracker.record_crap(
            path,
            &fc.name,
            MeasuredThresholdMetrics {
                cyclomatic: fc.cyclomatic,
                cognitive: fc.cognitive,
                crap: pf.crap,
            },
            &matched_overrides,
            input.threshold_resolver.global,
        );
        if pf.crap < applied_thresholds.effective.max_crap
            || crap_is_suppressed(path, pf, &maps.suppressions_by_path)
        {
            continue;
        }

        if let Some(&idx) = maps
            .finding_index
            .get(&(path.to_path_buf(), pf.line, pf.col))
        {
            merge_existing_crap_finding(&mut findings[idx], path, pf, input, applied_thresholds);
        } else {
            let hook_profile = maps
                .hook_profiles_by_pos
                .get(path)
                .and_then(|m| m.get(&(pf.line, pf.col)).cloned());
            new_findings.push(new_crap_finding(
                path,
                pf,
                fc,
                hook_profile,
                input,
                applied_thresholds,
            ));
        }
    }
}

fn merge_crap_findings(
    findings: &mut Vec<ComplexityViolation>,
    input: &mut CrapFindingMergeInput<'_>,
) {
    // Copy the `'a` references out so the lookup maps and the per-function map
    // borrow the underlying analysis data, not `input`, leaving `input` free to
    // be passed mutably into the per-path processor below.
    let modules = input.modules;
    let file_paths = input.file_paths;
    let per_function_crap = input.per_function_crap;
    let maps = CrapMergeMaps {
        finding_index: build_complexity_finding_index(findings),
        complexity_by_pos: build_complexity_by_position(modules, file_paths),
        hook_profiles_by_pos: build_hook_profiles_by_position(modules, file_paths),
        suppressions_by_path: build_complexity_suppressions_by_path(modules, file_paths),
    };

    let mut new_findings: Vec<ComplexityViolation> = Vec::new();
    for (path, per_fn) in per_function_crap {
        if !crap_path_in_scope(path, input) {
            continue;
        }
        process_crap_findings_for_path(path, per_fn, &maps, findings, &mut new_findings, input);
    }
    findings.extend(new_findings);
}

fn build_complexity_finding_index(
    findings: &[ComplexityViolation],
) -> rustc_hash::FxHashMap<(std::path::PathBuf, u32, u32), usize> {
    findings
        .iter()
        .enumerate()
        .map(|(idx, f)| ((f.path.clone(), f.line, f.col), idx))
        .collect()
}

fn build_complexity_by_position<'a>(
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> ComplexityByPosition<'a> {
    let mut complexity_by_pos: ComplexityByPosition<'a> = rustc_hash::FxHashMap::default();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let entry = complexity_by_pos.entry(path.as_path()).or_default();
        for fc in &module.complexity {
            entry.insert((fc.line, fc.col), fc);
        }
    }
    complexity_by_pos
}

/// Build a `path -> (line, col) -> ReactHookProfile` map by precomputing each
/// module's per-function hook profile ONCE (the CRAP path keys findings by
/// `(line, col)`, so the profile must be addressable the same way). Frames with
/// no attributed component-scope hook are omitted; non-React modules contribute
/// nothing.
fn build_hook_profiles_by_position<'a>(
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> rustc_hash::FxHashMap<
    &'a std::path::Path,
    rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
> {
    let mut by_pos: rustc_hash::FxHashMap<
        &'a std::path::Path,
        rustc_hash::FxHashMap<(u32, u32), fallow_output::ReactHookProfile>,
    > = rustc_hash::FxHashMap::default();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let profiles = react_hooks::build_module_hook_profiles(module);
        let mut frame_profiles = rustc_hash::FxHashMap::default();
        for (fc, profile) in module.complexity.iter().zip(profiles) {
            if let Some(profile) = profile {
                frame_profiles.insert((fc.line, fc.col), profile);
            }
        }
        if !frame_profiles.is_empty() {
            by_pos.insert(path.as_path(), frame_profiles);
        }
    }
    by_pos
}

fn build_complexity_suppressions_by_path<'a>(
    modules: &'a [crate::extract::ModuleInfo],
    file_paths: &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
) -> rustc_hash::FxHashMap<&'a std::path::Path, &'a Vec<crate::suppress::Suppression>> {
    modules
        .iter()
        .filter_map(|module| {
            file_paths
                .get(&module.file_id)
                .map(|path| (path.as_path(), &module.suppressions))
        })
        .collect()
}

fn crap_path_in_scope(path: &std::path::Path, input: &CrapFindingMergeInput<'_>) -> bool {
    let relative = path.strip_prefix(input.config_root).unwrap_or(path);
    if input.ignore_set.is_match(relative) {
        return false;
    }
    if let Some(changed) = input.changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = input.ws_roots
        && !ws.iter().any(|r| path.starts_with(r))
    {
        return false;
    }
    true
}

fn crap_is_suppressed(
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    suppressions_by_path: &rustc_hash::FxHashMap<
        &std::path::Path,
        &Vec<crate::suppress::Suppression>,
    >,
) -> bool {
    suppressions_by_path.get(path).is_some_and(|sups| {
        crate::suppress::is_suppressed(sups, pf.line, crate::suppress::IssueKind::Complexity)
    })
}

fn merge_existing_crap_finding(
    finding: &mut ComplexityViolation,
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    input: &CrapFindingMergeInput<'_>,
    applied_thresholds: AppliedHealthThresholds,
) {
    finding.crap = Some(pf.crap);
    finding.coverage_pct = pf.coverage_pct;
    finding.coverage_tier = Some(pf.coverage_tier);
    finding.coverage_source = Some(pf.coverage_source);
    finding.inherited_from =
        inherited_from_for(pf.coverage_source, path, input.template_inherit_provenance);
    let exceeds_cyclomatic = finding.exceeded.includes_cyclomatic();
    let exceeds_cognitive = finding.exceeded.includes_cognitive();
    finding.exceeded = ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, true);
    if applied_thresholds.override_index.is_some() {
        finding.effective_thresholds = Some(applied_thresholds.effective);
        finding.threshold_source = Some(fallow_output::ThresholdSource::Override);
    }
    finding.severity = compute_finding_severity(
        finding.cognitive,
        finding.cyclomatic,
        Some(pf.crap),
        DEFAULT_COGNITIVE_HIGH,
        DEFAULT_COGNITIVE_CRITICAL,
        DEFAULT_CYCLOMATIC_HIGH,
        DEFAULT_CYCLOMATIC_CRITICAL,
    );
}

fn new_crap_finding(
    path: &std::path::Path,
    pf: &scoring::PerFunctionCrap,
    fc: &fallow_types::extract::FunctionComplexity,
    hook_profile: Option<fallow_output::ReactHookProfile>,
    input: &CrapFindingMergeInput<'_>,
    applied_thresholds: AppliedHealthThresholds,
) -> ComplexityViolation {
    let exceeds_cyclomatic = fc.cyclomatic > applied_thresholds.effective.max_cyclomatic;
    let exceeds_cognitive = fc.cognitive > applied_thresholds.effective.max_cognitive;
    ComplexityViolation {
        path: path.to_path_buf(),
        name: fc.name.clone(),
        line: fc.line,
        col: fc.col,
        cyclomatic: fc.cyclomatic,
        cognitive: fc.cognitive,
        line_count: fc.line_count,
        param_count: fc.param_count,
        react_hook_count: fc.react_hook_count,
        react_jsx_max_depth: fc.react_jsx_max_depth,
        react_prop_count: fc.react_prop_count,
        react_hook_profile: hook_profile,
        exceeded: ExceededThreshold::from_bools(exceeds_cyclomatic, exceeds_cognitive, true),
        severity: compute_finding_severity(
            fc.cognitive,
            fc.cyclomatic,
            Some(pf.crap),
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        ),
        crap: Some(pf.crap),
        coverage_pct: pf.coverage_pct,
        coverage_tier: Some(pf.coverage_tier),
        coverage_source: Some(pf.coverage_source),
        inherited_from: inherited_from_for(
            pf.coverage_source,
            path,
            input.template_inherit_provenance,
        ),
        component_rollup: None,
        contributions: contributions_for(input.complexity_breakdown, fc),
        effective_thresholds: applied_thresholds
            .override_index
            .map(|_| applied_thresholds.effective),
        threshold_source: applied_thresholds
            .override_index
            .map(|_| fallow_output::ThresholdSource::Override),
    }
}

/// Synthesise per-Angular-component rollup findings.
///
/// For each Angular component that has both at least one class-function
/// finding above threshold AND a synthetic `<template>` finding, emit a new
/// `<component>` `ComplexityViolation` whose `cyclomatic` / `cognitive` totals are
/// `max(class) + template`. The rollup is anchored at the worst class
/// function's `(path, line, col)` so an existing
/// `// fallow-ignore-next-line complexity` placed above that function (or
/// the `@Component` decorator on inline-template components) continues to
/// hide both the per-function finding AND the rollup. Per-function and
/// per-`<template>` findings are NOT removed; the rollup is strictly
/// additive, with [`ComponentRollup`] carrying the breakdown.
///
/// Component-owner resolution has two branches:
/// - `<template>` finding on an `.html` file: look up the owner `.ts` in
///   the inverse-`templateUrl` provenance map populated by
///   `scoring::build_template_inherit_contexts` (the same walker that drives
///   `coverage_source: "estimated_component_inherited"` for CRAP).
/// - `<template>` finding on a `.ts` / `.tsx` / `.mts` / `.cts` file
///   (inline `@Component({ template: \`...\` })` literals): the owner IS
///   the file (`template_complexity.rs` remaps inline-template line/col
///   onto the decorator on the same `.ts`).
///
/// "Class function" is approximated as any finding whose name contains a
/// `.` (the `ClassName.methodName` shape `complexity.rs` emits for class
/// methods). Free functions and anonymous arrows do not participate in
/// rollups; only methods owned by a class do.
///
/// A `.ts` file carrying TWO synthetic `<template>` findings is treated
/// defensively: rollups are skipped (a `.ts` with multiple `@Component`
/// decorators would need AST-level class attribution to map each template
/// to its owning class, which is out of scope for the first cut). Fallow
/// emits a single rollup per owner per pass.
///
/// `[`ComponentRollup`]`: fallow_output::ComponentRollup
fn append_component_rollup_findings(
    findings: &mut Vec<fallow_output::ComplexityViolation>,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>>,
    max_cyclomatic: u16,
    max_cognitive: u16,
) {
    use fallow_output::ComplexityViolation;

    let mut by_owner: rustc_hash::FxHashMap<std::path::PathBuf, (Vec<usize>, Vec<usize>)> =
        rustc_hash::FxHashMap::default();
    for (idx, f) in findings.iter().enumerate() {
        if f.name == "<template>" {
            if let Some(owner) = component_template_owner(f, template_owner_lookup) {
                by_owner.entry(owner).or_default().1.push(idx);
            }
        } else if is_component_class_finding(f) {
            by_owner.entry(f.path.clone()).or_default().0.push(idx);
        }
    }

    let mut to_push: Vec<ComplexityViolation> = Vec::new();
    for (owner, (class_idxs, template_idxs)) in by_owner {
        if class_idxs.is_empty() || template_idxs.is_empty() {
            continue;
        }
        if template_idxs.len() > 1 {
            continue;
        }
        let template = &findings[template_idxs[0]];
        let Some(worst_idx) = class_idxs
            .iter()
            .copied()
            .max_by_key(|&i| findings[i].cyclomatic)
        else {
            continue;
        };
        let worst = &findings[worst_idx];
        if let Some(rollup) =
            build_component_rollup(owner, worst, template, max_cyclomatic, max_cognitive)
        {
            to_push.push(rollup);
        }
    }
    findings.extend(to_push);
}

fn component_template_owner(
    finding: &fallow_output::ComplexityViolation,
    template_owner_lookup: Option<&rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>>,
) -> Option<std::path::PathBuf> {
    let ext = finding
        .path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("html") => template_owner_lookup
            .and_then(|m| m.get(&finding.path))
            .cloned(),
        Some("ts" | "tsx" | "mts" | "cts") => Some(finding.path.clone()),
        _ => None,
    }
}

fn is_component_class_finding(finding: &fallow_output::ComplexityViolation) -> bool {
    finding.name != "<component>"
        && finding
            .path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "ts" | "tsx" | "mts" | "cts"
                )
            })
}

/// The rolled-up cyclomatic / cognitive totals for a component (worst frame plus
/// its template) and whether each total exceeds its threshold.
struct ComponentRollupTotals {
    rollup_cyc: u16,
    rollup_cog: u16,
    exceeds_cyclomatic: bool,
    exceeds_cognitive: bool,
}

/// Assemble the synthetic `<component>` rollup finding from the precomputed
/// totals, the worst class frame, and its template frame.
fn make_component_rollup_violation(
    owner: std::path::PathBuf,
    worst: &fallow_output::ComplexityViolation,
    template: &fallow_output::ComplexityViolation,
    totals: &ComponentRollupTotals,
) -> fallow_output::ComplexityViolation {
    use fallow_output::{ComponentRollup, ExceededThreshold};

    let component = owner.file_stem().map_or_else(
        || "<unknown-component>".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    fallow_output::ComplexityViolation {
        path: owner,
        name: "<component>".to_string(),
        line: worst.line,
        col: worst.col,
        cyclomatic: totals.rollup_cyc,
        cognitive: totals.rollup_cog,
        line_count: worst.line_count.saturating_add(template.line_count),
        param_count: 0,
        exceeded: ExceededThreshold::from_bools(
            totals.exceeds_cyclomatic,
            totals.exceeds_cognitive,
            false,
        ),
        severity: compute_finding_severity(
            totals.rollup_cog,
            totals.rollup_cyc,
            None,
            DEFAULT_COGNITIVE_HIGH,
            DEFAULT_COGNITIVE_CRITICAL,
            DEFAULT_CYCLOMATIC_HIGH,
            DEFAULT_CYCLOMATIC_CRITICAL,
        ),
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        component_rollup: Some(ComponentRollup {
            component,
            class_worst_function: worst.name.clone(),
            class_cyclomatic: worst.cyclomatic,
            class_cognitive: worst.cognitive,
            template_path: template.path.clone(),
            template_cyclomatic: template.cyclomatic,
            template_cognitive: template.cognitive,
        }),
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }
}

fn build_component_rollup(
    owner: std::path::PathBuf,
    worst: &fallow_output::ComplexityViolation,
    template: &fallow_output::ComplexityViolation,
    max_cyclomatic: u16,
    max_cognitive: u16,
) -> Option<fallow_output::ComplexityViolation> {
    let rollup_cyc = worst.cyclomatic.saturating_add(template.cyclomatic);
    let rollup_cog = worst.cognitive.saturating_add(template.cognitive);
    let exceeds_cyclomatic = rollup_cyc > max_cyclomatic;
    let exceeds_cognitive = rollup_cog > max_cognitive;
    if !exceeds_cyclomatic && !exceeds_cognitive {
        return None;
    }

    let totals = ComponentRollupTotals {
        rollup_cyc,
        rollup_cog,
        exceeds_cyclomatic,
        exceeds_cognitive,
    };
    Some(make_component_rollup_violation(
        owner, worst, template, &totals,
    ))
}

/// Resolve the `inherited_from` provenance path for a CRAP finding.
///
/// Returns `Some(owner_path)` only for the
/// `CoverageSource::EstimatedComponentInherited` variant, so the field stays
/// absent on every Istanbul / regular-estimated row. Pairs with the
/// `coverage_source` discriminator: any finding carrying
/// `estimated_component_inherited` also carries `inherited_from`, and vice
/// versa.
fn inherited_from_for(
    source: fallow_output::CoverageSource,
    template_path: &std::path::Path,
    template_inherit_provenance: &rustc_hash::FxHashMap<std::path::PathBuf, std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if matches!(
        source,
        fallow_output::CoverageSource::EstimatedComponentInherited
    ) {
        template_inherit_provenance.get(template_path).cloned()
    } else {
        None
    }
}

struct HealthBaselineSaveInput<'a> {
    save_path: &'a std::path::Path,
    findings: &'a [ComplexityViolation],
    runtime_coverage_findings: &'a [fallow_output::RuntimeCoverageFinding],
    targets: &'a [RefactoringTarget],
    config_root: &'a std::path::Path,
    quiet: bool,
    output: OutputFormat,
}

/// Save health baseline to disk.
fn save_health_baseline(input: &HealthBaselineSaveInput<'_>) -> Result<(), ExitCode> {
    let HealthBaselineSaveInput {
        save_path,
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
        quiet,
        output,
    } = *input;
    let baseline = HealthBaselineData::from_findings(
        findings,
        runtime_coverage_findings,
        targets,
        config_root,
    );
    match serde_json::to_string_pretty(&baseline) {
        Ok(json) => {
            if let Some(parent) = save_path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return Err(emit_error(
                    &format!("failed to create health baseline directory: {e}"),
                    2,
                    output,
                ));
            }
            if let Err(e) = std::fs::write(save_path, json) {
                return Err(emit_error(
                    &format!("failed to save health baseline: {e}"),
                    2,
                    output,
                ));
            }
            if !quiet {
                eprintln!("Saved health baseline to {}", save_path.display());
            }
            Ok(())
        }
        Err(e) => Err(emit_error(
            &format!("failed to serialize health baseline: {e}"),
            2,
            output,
        )),
    }
}

/// Load and apply a health baseline, filtering findings to show only new ones.
fn load_health_baseline(
    baseline_path: &std::path::Path,
    findings: &mut Vec<ComplexityViolation>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<HealthBaselineData, ExitCode> {
    let json = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read health baseline: {e}"), 2, output))?;
    let baseline: HealthBaselineData = serde_json::from_str(&json)
        .map_err(|e| emit_error(&format!("failed to parse health baseline: {e}"), 2, output))?;
    let baseline_entries = baseline.finding_entry_count();
    let before = findings.len();
    let overlap_entries = baseline.overlap_entry_count(findings, root);
    *findings = filter_new_health_findings(std::mem::take(findings), &baseline, root);
    if !quiet {
        eprintln!(
            "Comparing against health baseline: {}",
            baseline_path.display()
        );
    }
    if baseline_entries > 0 && before > 0 && overlap_entries == 0 && !quiet {
        eprintln!(
            "Warning: health baseline has {baseline_entries} entries but matched \
             0 current findings. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok(baseline)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::ModuleInfo;
    use fallow_types::discover::FileId;
    use fallow_types::extract::FunctionComplexity;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::path::{Path, PathBuf};

    /// Build a minimal `ModuleInfo` with only the fields `collect_findings` needs.
    fn make_module(file_id: FileId, complexity: Vec<FunctionComplexity>) -> ModuleInfo {
        ModuleInfo {
            file_id,
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: Box::default(),
            member_accesses: vec![],
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![0],
            complexity,
            flag_uses: vec![],
            class_heritage: vec![],
            exported_factory_returns: Box::default(),
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            inline_server_action_exports: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_dynamic_dispatch: false,
        }
    }

    fn make_fc(name: &str, cyclomatic: u16, cognitive: u16, line_count: u32) -> FunctionComplexity {
        FunctionComplexity {
            name: name.to_string(),
            line: 1,
            col: 0,
            cyclomatic,
            cognitive,
            line_count,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
    }

    fn make_fc_with_contributions(
        name: &str,
        cyclomatic: u16,
        cognitive: u16,
    ) -> FunctionComplexity {
        use fallow_types::extract::{
            ComplexityContribution, ComplexityContributionKind, ComplexityMetric,
        };
        let mut fc = make_fc(name, cyclomatic, cognitive, 50);
        fc.contributions = vec![ComplexityContribution {
            line: 2,
            col: 4,
            metric: ComplexityMetric::Cyclomatic,
            kind: ComplexityContributionKind::If,
            weight: 1,
            nesting: 0,
        }];
        fc
    }

    #[test]
    fn collect_findings_omits_contributions_without_breakdown_flag() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc_with_contributions("complexFn", 25, 5)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].contributions.is_empty(),
            "contributions must be omitted without the breakdown flag"
        );
    }

    #[test]
    fn collect_findings_includes_contributions_with_breakdown_flag() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc_with_contributions("complexFn", 25, 5)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            true,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].contributions.len(),
            1,
            "contributions must flow through when the breakdown flag is set"
        );
    }

    fn threshold_resolver(
        overrides: &[fallow_config::HealthThresholdOverride],
    ) -> ThresholdOverrideResolver {
        ThresholdOverrideResolver::new(
            overrides,
            GlobalHealthThresholds {
                cyclomatic: 20,
                cognitive: 15,
                crap: 30.0,
            },
        )
    }

    #[test]
    fn collect_findings_uses_threshold_override_as_local_ceiling() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("complexFn", 25, 20, 50)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
            files: vec!["src/a.ts".to_string()],
            functions: vec!["complexFn".to_string()],
            max_cyclomatic: Some(30),
            max_cognitive: Some(25),
            max_crap: None,
            reason: Some("approved assembly".to_string()),
        }]);
        let mut tracker = ThresholdOverrideStateTracker::default();

        let mut input = CollectFindingsInput {
            modules: &modules,
            file_paths: &file_paths,
            config_root: Path::new("/project"),
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
            threshold_resolver: &resolver,
            threshold_state_tracker: &mut tracker,
            complexity_breakdown: false,
        };
        let (findings, _, _) = collect_findings_with_resolver(&mut input);

        assert!(findings.is_empty());
        let states = tracker.into_states();
        assert_eq!(states.len(), 1);
        assert!(matches!(
            states[0].status,
            fallow_output::ThresholdOverrideStatus::Active
        ));
    }

    #[test]
    fn collect_findings_reports_when_local_ceiling_is_exceeded() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("complexFn", 31, 20, 50)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
            files: vec!["src/a.ts".to_string()],
            functions: vec!["complexFn".to_string()],
            max_cyclomatic: Some(30),
            max_cognitive: Some(25),
            max_crap: None,
            reason: None,
        }]);
        let mut tracker = ThresholdOverrideStateTracker::default();

        let mut input = CollectFindingsInput {
            modules: &modules,
            file_paths: &file_paths,
            config_root: Path::new("/project"),
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
            threshold_resolver: &resolver,
            threshold_state_tracker: &mut tracker,
            complexity_breakdown: false,
        };
        let (findings, _, _) = collect_findings_with_resolver(&mut input);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].effective_thresholds.unwrap().max_cyclomatic, 30);
        assert!(matches!(
            findings[0].threshold_source,
            Some(fallow_output::ThresholdSource::Override)
        ));
    }

    #[test]
    fn collect_findings_reports_stale_override_when_under_global_thresholds() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("complexFn", 10, 8, 20)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);
        let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
            files: vec!["src/a.ts".to_string()],
            functions: vec!["complexFn".to_string()],
            max_cyclomatic: Some(30),
            max_cognitive: None,
            max_crap: None,
            reason: None,
        }]);
        let mut tracker = ThresholdOverrideStateTracker::default();

        let mut input = CollectFindingsInput {
            modules: &modules,
            file_paths: &file_paths,
            config_root: Path::new("/project"),
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
            threshold_resolver: &resolver,
            threshold_state_tracker: &mut tracker,
            complexity_breakdown: false,
        };
        let (findings, _, _) = collect_findings_with_resolver(&mut input);

        assert!(findings.is_empty());
        let states = tracker.into_states();
        assert_eq!(states.len(), 1);
        assert!(matches!(
            states[0].status,
            fallow_output::ThresholdOverrideStatus::Stale
        ));
    }

    #[test]
    fn threshold_override_tracker_reports_no_match_only_when_requested() {
        let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
            files: vec!["src/missing.ts".to_string()],
            functions: vec!["missingFn".to_string()],
            max_cyclomatic: Some(30),
            max_cognitive: None,
            max_crap: None,
            reason: None,
        }]);
        let mut tracker = ThresholdOverrideStateTracker::default();
        tracker.record_no_match_entries(&resolver, false);
        assert!(tracker.into_states().is_empty());

        let mut tracker = ThresholdOverrideStateTracker::default();
        tracker.record_no_match_entries(&resolver, true);
        let states = tracker.into_states();
        assert_eq!(states.len(), 1);
        assert!(matches!(
            states[0].status,
            fallow_output::ThresholdOverrideStatus::NoMatch
        ));
    }

    #[test]
    fn build_ignore_set_empty_patterns() {
        let set = build_ignore_set(&[]);
        assert!(set.is_empty());
    }

    #[test]
    fn build_ignore_set_matches_glob() {
        let patterns = vec!["src/generated/**".to_string()];
        let set = build_ignore_set(&patterns);
        assert!(set.is_match(Path::new("src/generated/types.ts")));
        assert!(!set.is_match(Path::new("src/utils.ts")));
    }

    #[test]
    fn build_ignore_set_multiple_patterns() {
        let patterns = vec!["*.test.ts".to_string(), "dist/**".to_string()];
        let set = build_ignore_set(&patterns);
        assert!(set.is_match(Path::new("foo.test.ts")));
        assert!(set.is_match(Path::new("dist/index.js")));
        assert!(!set.is_match(Path::new("src/index.ts")));
    }

    #[test]
    #[should_panic(expected = "validated at config load time")]
    fn build_ignore_set_panics_on_unvalidated_invalid_pattern() {
        let patterns = vec!["[invalid".to_string(), "*.js".to_string()];
        let _ = build_ignore_set(&patterns);
    }

    fn make_finding(name: &str, exceeded: ExceededThreshold) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from("/project/src/a.ts"),
            name: name.to_string(),
            line: 1,
            col: 0,
            cyclomatic: match exceeded {
                ExceededThreshold::Cyclomatic
                | ExceededThreshold::Both
                | ExceededThreshold::CyclomaticCrap
                | ExceededThreshold::All => 25,
                _ => 8,
            },
            cognitive: match exceeded {
                ExceededThreshold::Cognitive
                | ExceededThreshold::Both
                | ExceededThreshold::CognitiveCrap
                | ExceededThreshold::All => 20,
                _ => 5,
            },
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded,
            severity: FindingSeverity::Moderate,
            crap: exceeded.includes_crap().then_some(30.0),
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    #[test]
    fn sort_findings_by_severity_surfaces_crap_before_single_metric_findings() {
        let mut findings = vec![
            make_finding("cyclomatic", ExceededThreshold::Cyclomatic),
            make_finding("cognitive", ExceededThreshold::Cognitive),
            make_finding("both", ExceededThreshold::Both),
            make_finding("crap", ExceededThreshold::Crap),
            make_finding("cyclomatic_crap", ExceededThreshold::CyclomaticCrap),
            make_finding("all", ExceededThreshold::All),
        ];

        sort_findings(&mut findings, HealthSort::Severity);

        let names = findings
            .iter()
            .map(|finding| finding.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "all",
                "cyclomatic_crap",
                "crap",
                "both",
                "cyclomatic",
                "cognitive",
            ]
        );
    }

    #[test]
    fn collect_findings_empty_modules() {
        let (findings, files, functions) = collect_findings(
            &[],
            &FxHashMap::default(),
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
        assert_eq!(functions, 0);
    }

    #[test]
    fn collect_findings_below_threshold() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("doStuff", 5, 3, 10)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, files, functions) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 1);
        assert_eq!(functions, 1);
    }

    #[test]
    fn collect_findings_exceeds_cyclomatic_only() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("complexFn", 25, 5, 50)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].cyclomatic, 25);
        assert!(matches!(
            findings[0].exceeded,
            ExceededThreshold::Cyclomatic
        ));
    }

    #[test]
    fn collect_findings_exceeds_cognitive_only() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("nestedFn", 5, 20, 30)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].exceeded, ExceededThreshold::Cognitive));
    }

    #[test]
    fn collect_findings_exceeds_both() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("terribleFn", 25, 20, 100)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].exceeded, ExceededThreshold::Both));
    }

    #[test]
    fn collect_findings_multiple_functions_per_file() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![
                make_fc("ok", 5, 3, 10),
                make_fc("bad", 25, 20, 50),
                make_fc("also_bad", 21, 5, 30),
            ],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, files, functions) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(files, 1);
        assert_eq!(functions, 3);
    }

    #[test]
    fn collect_findings_ignores_matching_files() {
        let path = PathBuf::from("/project/src/generated/types.ts");
        let modules = vec![make_module(FileId(0), vec![make_fc("genFn", 25, 20, 50)])];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let ignore_set = build_ignore_set(&["src/generated/**".to_string()]);
        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &ignore_set,
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
    }

    #[test]
    fn collect_findings_filters_by_changed_files() {
        let path_a = PathBuf::from("/project/src/a.ts");
        let path_b = PathBuf::from("/project/src/b.ts");
        let modules = vec![
            make_module(FileId(0), vec![make_fc("fnA", 25, 20, 50)]),
            make_module(FileId(1), vec![make_fc("fnB", 25, 20, 50)]),
        ];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path_a);
        file_paths.insert(FileId(1), &path_b);

        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            Some(&changed),
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].name, "fnA");
        assert_eq!(files, 1);
    }

    fn build_diff(text: &str) -> fallow_output::DiffIndex {
        fallow_output::DiffIndex::from_unified_diff(text)
    }

    #[test]
    fn filter_complexity_findings_by_diff_keeps_hotspot_overlapping_diff_line() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/big.ts"),
            name: "wide_fn".into(),
            line: 10,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 110,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }];
        let diff = build_diff(
            "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn filter_complexity_findings_by_diff_drops_finding_outside_diff() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/elsewhere.ts"),
            name: "outside".into(),
            line: 10,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 5,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }];
        let diff = build_diff(
            "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert!(findings.is_empty());
    }

    #[test]
    fn filter_complexity_findings_by_diff_handles_zero_line_count() {
        let mut findings = vec![ComplexityViolation {
            path: PathBuf::from("/project/src/a.ts"),
            name: "zero_extent".into(),
            line: 5,
            col: 0,
            cyclomatic: 30,
            cognitive: 30,
            line_count: 0,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::High,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }];
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -4,1 +4,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn filter_hotspots_by_diff_uses_file_level_membership() {
        use fallow_output::HotspotEntry;
        let mut hotspots = vec![
            HotspotEntry {
                path: PathBuf::from("/project/src/touched.ts"),
                score: 90.0,
                commits: 50,
                weighted_commits: 25.0,
                lines_added: 1000,
                lines_deleted: 500,
                complexity_density: 0.4,
                fan_in: 5,
                trend: crate::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            },
            HotspotEntry {
                path: PathBuf::from("/project/src/untouched.ts"),
                score: 90.0,
                commits: 50,
                weighted_commits: 25.0,
                lines_added: 1000,
                lines_deleted: 500,
                complexity_density: 0.4,
                fan_in: 5,
                trend: crate::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            },
        ];
        let diff = build_diff(
            "diff --git a/src/touched.ts b/src/touched.ts\n\
             --- a/src/touched.ts\n\
             +++ b/src/touched.ts\n\
             @@ -0,0 +1,1 @@\n\
             +new\n",
        );
        filter_hotspots_by_diff(&mut hotspots, &diff, Path::new("/project"));
        assert_eq!(hotspots.len(), 1);
        assert_eq!(hotspots[0].path, PathBuf::from("/project/src/touched.ts"));
    }

    #[test]
    fn filter_large_functions_by_diff_uses_range_overlap() {
        use fallow_output::LargeFunctionEntry;
        let mut entries = vec![
            LargeFunctionEntry {
                path: PathBuf::from("/project/src/a.ts"),
                name: "kept".into(),
                line: 10,
                line_count: 100,
            },
            LargeFunctionEntry {
                path: PathBuf::from("/project/src/a.ts"),
                name: "dropped".into(),
                line: 500,
                line_count: 100,
            },
        ];
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -49,1 +49,2 @@\n\
              ctx\n\
             +touched\n",
        );
        filter_large_functions_by_diff(&mut entries, &diff, Path::new("/project"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "kept");
    }

    #[test]
    fn collect_findings_skips_module_without_path() {
        let modules = vec![make_module(FileId(99), vec![make_fc("orphan", 25, 20, 50)])];
        let file_paths = FxHashMap::default();

        let (findings, files, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
        assert_eq!(files, 0);
    }

    #[test]
    fn collect_findings_at_exact_threshold_not_reported() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![make_fc("borderline", 20, 15, 20)],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn collect_findings_preserves_function_metadata() {
        let path = PathBuf::from("/project/src/a.ts");
        let modules = vec![make_module(
            FileId(0),
            vec![FunctionComplexity {
                name: "processData".to_string(),
                line: 42,
                col: 8,
                cyclomatic: 25,
                cognitive: 18,
                line_count: 75,
                param_count: 2,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        )];
        let mut file_paths = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let (findings, _, _) = collect_findings(
            &modules,
            &file_paths,
            Path::new("/project"),
            &globset::GlobSet::empty(),
            None,
            None,
            20,
            15,
            false,
        );
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.name, "processData");
        assert_eq!(f.line, 42);
        assert_eq!(f.col, 8);
        assert_eq!(f.cyclomatic, 25);
        assert_eq!(f.cognitive, 18);
        assert_eq!(f.line_count, 75);
        assert_eq!(f.path, PathBuf::from("/project/src/a.ts"));
    }

    #[test]
    fn merge_crap_findings_disambiguates_same_line_functions() {
        let path = PathBuf::from("/project/src/curried.ts");
        let outer = FunctionComplexity {
            name: "handler".to_string(),
            line: 1,
            col: 23,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 11,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 1,
            col: 43,
            cyclomatic: 7,
            cognitive: 0,
            line_count: 10,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        };
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let mut findings: Vec<ComplexityViolation> = Vec::new();

        let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
            FxHashMap::default();
        per_function_crap.insert(
            path.clone(),
            vec![
                scoring::PerFunctionCrap {
                    line: inner.line,
                    col: inner.col,
                    crap: 56.0,
                    coverage_pct: None,
                    coverage_tier: fallow_output::CoverageTier::None,
                    coverage_source: fallow_output::CoverageSource::Estimated,
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 2.0,
                    coverage_pct: None,
                    coverage_tier: fallow_output::CoverageTier::None,
                    coverage_source: fallow_output::CoverageSource::Estimated,
                },
            ],
        );

        let resolver = threshold_resolver(&[]);
        let mut tracker = ThresholdOverrideStateTracker::default();
        let mut input = CrapFindingMergeInput {
            modules: &modules,
            file_paths: &file_paths,
            config_root: Path::new("/project"),
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
            per_function_crap: &per_function_crap,
            template_inherit_provenance: &FxHashMap::default(),
            complexity_breakdown: false,
            threshold_resolver: &resolver,
            threshold_state_tracker: &mut tracker,
        };
        merge_crap_findings(&mut findings, &mut input);

        assert_eq!(
            findings.len(),
            1,
            "expected one CRAP finding for inner arrow"
        );
        let f = &findings[0];
        assert_eq!(f.name, "<arrow>", "name must come from inner arrow");
        assert_eq!(f.line, 1);
        assert_eq!(f.col, 43, "col must disambiguate same-line arrows");
        assert_eq!(f.cyclomatic, 7, "cyclomatic must come from inner arrow");
        assert_eq!(f.cognitive, 0);
        assert_eq!(
            f.crap,
            Some(56.0),
            "CRAP must match the function it's reported against"
        );
        let cc = f64::from(f.cyclomatic);
        #[expect(
            clippy::suboptimal_flops,
            reason = "cc * cc + cc matches the CRAP formula specification"
        )]
        let expected_crap = cc * cc + cc;
        assert!(
            (f.crap.unwrap() - expected_crap).abs() < 0.01,
            "CRAP must be consistent with reported CC: cc={cc}, crap={:?}, expected={expected_crap}",
            f.crap,
        );
    }

    #[test]
    fn merge_crap_findings_picks_outer_when_outer_exceeds() {
        let path = PathBuf::from("/project/src/curried_outer.ts");
        let outer = FunctionComplexity {
            name: "complex".to_string(),
            line: 5,
            col: 10,
            cyclomatic: 8,
            cognitive: 0,
            line_count: 20,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        };
        let inner = FunctionComplexity {
            name: "<arrow>".to_string(),
            line: 5,
            col: 30,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 1,
            param_count: 1,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        };
        let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
        let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
        file_paths.insert(FileId(0), &path);

        let mut findings: Vec<ComplexityViolation> = Vec::new();
        let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
            FxHashMap::default();
        per_function_crap.insert(
            path.clone(),
            vec![
                scoring::PerFunctionCrap {
                    line: inner.line,
                    col: inner.col,
                    crap: 2.0,
                    coverage_pct: None,
                    coverage_tier: fallow_output::CoverageTier::None,
                    coverage_source: fallow_output::CoverageSource::Estimated,
                },
                scoring::PerFunctionCrap {
                    line: outer.line,
                    col: outer.col,
                    crap: 72.0,
                    coverage_pct: None,
                    coverage_tier: fallow_output::CoverageTier::None,
                    coverage_source: fallow_output::CoverageSource::Estimated,
                },
            ],
        );

        let resolver = threshold_resolver(&[]);
        let mut tracker = ThresholdOverrideStateTracker::default();
        let mut input = CrapFindingMergeInput {
            modules: &modules,
            file_paths: &file_paths,
            config_root: Path::new("/project"),
            ignore_set: &globset::GlobSet::empty(),
            changed_files: None,
            ws_roots: None,
            per_function_crap: &per_function_crap,
            template_inherit_provenance: &FxHashMap::default(),
            complexity_breakdown: false,
            threshold_resolver: &resolver,
            threshold_state_tracker: &mut tracker,
        };
        merge_crap_findings(&mut findings, &mut input);

        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.name, "complex");
        assert_eq!(f.col, 10);
        assert_eq!(f.cyclomatic, 8);
        assert_eq!(f.crap, Some(72.0));
    }

    fn fx_summary(
        tracked: usize,
        hit: usize,
        unhit: usize,
        untracked: usize,
    ) -> fallow_output::RuntimeCoverageSummary {
        #[expect(
            clippy::cast_precision_loss,
            reason = "test fixture totals are tiny, f64 precision is fine"
        )]
        let coverage_percent = if tracked == 0 {
            0.0
        } else {
            (hit as f64 / tracked as f64) * 100.0
        };
        fallow_output::RuntimeCoverageSummary {
            data_source: fallow_output::RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: tracked,
            functions_hit: hit,
            functions_unhit: unhit,
            functions_untracked: untracked,
            coverage_percent,
            trace_count: 512,
            period_days: 7,
            deployments_seen: 2,
            capture_quality: None,
        }
    }

    fn fx_evidence(
        static_status: &str,
        test_coverage: &str,
        v8_tracking: &str,
    ) -> fallow_output::RuntimeCoverageEvidence {
        fallow_output::RuntimeCoverageEvidence {
            static_status: static_status.to_owned(),
            test_coverage: test_coverage.to_owned(),
            v8_tracking: v8_tracking.to_owned(),
            untracked_reason: None,
            observation_days: 7,
            deployments_observed: 2,
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn runtime_coverage_top_applies_after_baseline_filtering() {
        let root = Path::new("/project");
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: std::collections::BTreeMap::new(),
            runtime_coverage_findings: vec![
                "fallow:prod:aaaaaaaa".to_owned(),
                "fallow:prod:bbbbbbbb".to_owned(),
            ],
            runtime_coverage_source_hashes: vec![],
            target_keys: vec![],
        };
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(3, 0, 2, 1),
            findings: vec![
                fallow_output::RuntimeCoverageFinding {
                    id: "fallow:prod:aaaaaaaa".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/a.ts"),
                    function: "alpha".to_owned(),
                    line: 10,
                    verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: fallow_output::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                    source_hash: None,
                    discriminators: None,
                },
                fallow_output::RuntimeCoverageFinding {
                    id: "fallow:prod:bbbbbbbb".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/b.ts"),
                    function: "beta".to_owned(),
                    line: 20,
                    verdict: fallow_output::RuntimeCoverageVerdict::CoverageUnavailable,
                    invocations: None,
                    confidence: fallow_output::RuntimeCoverageConfidence::None,
                    evidence: fx_evidence("used", "not_covered", "untracked"),
                    actions: vec![],
                    source_hash: None,
                    discriminators: None,
                },
                fallow_output::RuntimeCoverageFinding {
                    id: "fallow:prod:cccccccc".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/c.ts"),
                    function: "gamma".to_owned(),
                    line: 30,
                    verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: fallow_output::RuntimeCoverageConfidence::Medium,
                    evidence: fx_evidence("used", "not_covered", "tracked"),
                    actions: vec![],
                    source_hash: None,
                    discriminators: None,
                },
            ],
            hot_paths: vec![
                fallow_output::RuntimeCoverageHotPath {
                    id: "fallow:hot:11111111".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/hot-a.ts"),
                    function: "hotAlpha".to_owned(),
                    line: 1,
                    end_line: 5,
                    invocations: 500,
                    percentile: 99,
                    actions: vec![],
                },
                fallow_output::RuntimeCoverageHotPath {
                    id: "fallow:hot:22222222".to_owned(),
                    stable_id: None,
                    path: PathBuf::from("/project/src/hot-b.ts"),
                    function: "hotBeta".to_owned(),
                    line: 2,
                    end_line: 8,
                    invocations: 250,
                    percentile: 50,
                    actions: vec![],
                },
            ],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_baseline(Some(&baseline))
                .with_top(Some(1)),
        );

        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].function, "gamma");
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected
        );
        assert_eq!(report.summary.functions_tracked, 3);
        assert_eq!(report.summary.functions_hit, 0);
        assert_eq!(report.summary.functions_unhit, 2);
        assert_eq!(report.summary.functions_untracked, 1);
        assert!((report.summary.coverage_percent - 0.0).abs() < 0.05);
        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(report.hot_paths[0].function, "hotAlpha");
    }

    #[test]
    fn runtime_coverage_baseline_refreshes_to_clean_when_only_baselined_findings_remain() {
        let root = Path::new("/project");
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: std::collections::BTreeMap::new(),
            runtime_coverage_findings: vec!["fallow:prod:aaaaaaaa".to_owned()],
            runtime_coverage_source_hashes: vec![],
            target_keys: vec![],
        };
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:aaaaaaaa".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/a.ts"),
                function: "alpha".to_owned(),
                line: 10,
                verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: fallow_output::RuntimeCoverageConfidence::Medium,
                evidence: fx_evidence("used", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            }],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_baseline(Some(&baseline)),
        );

        assert!(report.findings.is_empty());
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::Clean
        );
        assert_eq!(report.summary.functions_tracked, 2);
        assert_eq!(report.summary.functions_hit, 1);
        assert_eq!(report.summary.functions_unhit, 1);
        assert_eq!(report.summary.functions_untracked, 0);
        assert!((report.summary.coverage_percent - 50.0).abs() < 0.05);
    }

    #[test]
    fn runtime_coverage_changed_review_uses_hot_path_verdict() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![fallow_output::RuntimeCoverageHotPath {
                id: "fallow:hot:33333333".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot.ts"),
                function: "renderHotPath".to_owned(),
                line: 7,
                end_line: 24,
                invocations: 9_500,
                percentile: 99,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_changed_review_ignores_unmodified_hot_paths() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/other.ts"));
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths: vec![fallow_output::RuntimeCoverageHotPath {
                id: "fallow:hot:44444444".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot.ts"),
                function: "renderHotPath".to_owned(),
                line: 7,
                end_line: 24,
                invocations: 9_500,
                percentile: 90,
                actions: vec![],
            }],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::Clean
        );
    }

    fn fx_runtime_coverage_report_with_hot_paths(
        hot_paths: Vec<fallow_output::RuntimeCoverageHotPath>,
    ) -> fallow_output::RuntimeCoverageReport {
        fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 2, 0, 0),
            findings: vec![],
            hot_paths,
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        }
    }

    fn fx_hot_path(
        id: &str,
        path: &str,
        line: u32,
        end_line: u32,
    ) -> fallow_output::RuntimeCoverageHotPath {
        fallow_output::RuntimeCoverageHotPath {
            id: id.to_owned(),
            stable_id: None,
            path: PathBuf::from(path),
            function: "renderHotPath".to_owned(),
            line,
            end_line,
            invocations: 9_500,
            percentile: 99,
            actions: vec![],
        }
    }

    #[test]
    fn runtime_coverage_diff_index_keeps_hot_paths_with_added_line_in_range() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touch the body\n\
                    line 11\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:01010101",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_diff_index_drops_hot_paths_when_added_line_outside_range() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // unrelated change far below the hot function\n\
                    line 51\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:02020202",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::Clean
        );
    }

    #[test]
    fn runtime_coverage_diff_index_falls_back_to_single_line_when_end_line_zero() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -7,1 +7,2 @@\n\
                    +  // exactly the function's start line\n\
                    line 8\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:03030303",
            "src/hot.ts",
            7,
            0,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_diff_index_resolves_absolute_hot_path_against_root() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touched\n\
                    line 11\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:04040404",
            "/project/src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
    }

    #[test]
    fn runtime_coverage_diff_index_authoritative_for_files_in_diff() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // outside the hot function\n\
                    line 51\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:05050505",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_changed_files(Some(&changed_files))
                .with_diff_index(Some(&diff_index)),
        );

        assert!(report.hot_paths.is_empty());
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::Clean
        );
    }

    #[test]
    fn runtime_coverage_per_file_fallback_to_changed_files_when_diff_omits_file() {
        let root = Path::new("/project");
        let diff = "diff --git a/src/other.ts b/src/other.ts\n\
                    --- a/src/other.ts\n\
                    +++ b/src/other.ts\n\
                    @@ -1,1 +1,2 @@\n\
                    +  // unrelated\n\
                    line 2\n";
        let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:0a0a0a0a",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_changed_files(Some(&changed_files))
                .with_diff_index(Some(&diff_index)),
        );

        assert_eq!(report.hot_paths.len(), 1);
        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
        );
    }

    #[test]
    fn runtime_coverage_pr_context_promotes_hot_path_touched_above_cold_code() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:cold0001".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/cold.ts"),
                function: "coldFn".to_owned(),
                line: 4,
                verdict: fallow_output::RuntimeCoverageVerdict::SafeToDelete,
                invocations: Some(0),
                confidence: fallow_output::RuntimeCoverageConfidence::High,
                evidence: fx_evidence("unused", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            }],
            hot_paths: vec![fx_hot_path("fallow:hot:0b0b0b0b", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
        );
        assert_eq!(
            report.signals,
            vec![
                fallow_output::RuntimeCoverageSignal::ColdCodeDetected,
                fallow_output::RuntimeCoverageSignal::HotPathTouched,
            ]
        );
    }

    #[test]
    fn runtime_coverage_standalone_keeps_cold_code_primary_above_unchanged_hot_paths() {
        let root = Path::new("/project");
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:cold0002".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/cold.ts"),
                function: "coldFn".to_owned(),
                line: 4,
                verdict: fallow_output::RuntimeCoverageVerdict::SafeToDelete,
                invocations: Some(0),
                confidence: fallow_output::RuntimeCoverageConfidence::High,
                evidence: fx_evidence("unused", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            }],
            hot_paths: vec![fx_hot_path("fallow:hot:0c0c0c0c", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(&mut report, &RuntimeCoverageFilterContext::new(root));

        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected
        );
        assert_eq!(
            report.signals,
            vec![fallow_output::RuntimeCoverageSignal::ColdCodeDetected]
        );
        assert_eq!(report.hot_paths.len(), 1);
    }

    #[test]
    fn runtime_coverage_license_grace_outranks_pr_context_signals() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fallow_output::RuntimeCoverageReport {
            schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
            verdict: fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace,
            signals: Vec::new(),
            summary: fx_summary(2, 1, 1, 0),
            findings: vec![],
            hot_paths: vec![fx_hot_path("fallow:hot:0d0d0d0d", "src/hot.ts", 7, 24)],
            blast_radius: vec![],
            importance: vec![],
            watermark: Some(fallow_output::RuntimeCoverageWatermark::LicenseExpiredGrace),
            warnings: vec![],
            actionable: true,
            actionability_reason: None,
            actionability_verdict: None,
            provenance: fallow_output::RuntimeCoverageProvenance::default(),
        };

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(
            report.verdict,
            fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace
        );
        assert!(
            report
                .signals
                .contains(&fallow_output::RuntimeCoverageSignal::LicenseExpiredGrace)
        );
        assert!(
            report
                .signals
                .contains(&fallow_output::RuntimeCoverageSignal::HotPathTouched)
        );
    }

    #[test]
    fn retain_hot_paths_drops_when_diff_touches_file_but_no_added_lines() {
        let root = Path::new("/project");
        let diff = fallow_output::DiffIndex::from_unified_diff(
            "diff --git a/src/hot.ts b/src/hot.ts\n\
             --- a/src/hot.ts\n\
             +++ b/src/hot.ts\n\
             @@ -10,3 +10,1 @@\n\
             -one\n\
             -two\n\
             -three\n\
             ctx\n",
        );
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:deletiononly",
            "src/hot.ts",
            10,
            12,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root)
                .with_diff_index(Some(&diff))
                .with_changed_files(Some(&changed_files)),
        );

        assert!(
            report.hot_paths.is_empty(),
            "diff touched the file with no added lines: must drop, not fall through to changed_files"
        );
    }

    #[test]
    fn runtime_coverage_changed_files_matches_relative_hot_path_against_absolute_set() {
        let root = Path::new("/project");
        let mut changed_files = FxHashSet::default();
        changed_files.insert(PathBuf::from("/project/src/hot.ts"));
        let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
            "fallow:hot:06060606",
            "src/hot.ts",
            7,
            24,
        )]);

        apply_runtime_coverage_filters(
            &mut report,
            &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
        );

        assert_eq!(report.hot_paths.len(), 1);
    }

    fn make_class_finding(
        path: &str,
        name: &str,
        line: u32,
        cyclomatic: u16,
        cognitive: u16,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from(path),
            name: name.to_string(),
            line,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 20,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::Moderate,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    fn make_template_finding(
        path: &str,
        line: u32,
        cyclomatic: u16,
        cognitive: u16,
    ) -> ComplexityViolation {
        ComplexityViolation {
            path: PathBuf::from(path),
            name: "<template>".to_string(),
            line,
            col: 0,
            cyclomatic,
            cognitive,
            line_count: 30,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded: ExceededThreshold::Both,
            severity: FindingSeverity::Moderate,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    #[test]
    fn rollup_external_template_via_provenance_lookup() {
        let component_ts = PathBuf::from("/proj/src/host-game.component.ts");
        let template_html = PathBuf::from("/proj/src/host-game.component.html");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "handleClick", 42, 3, 4),
            make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
        ];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template_html.clone(), component_ts.clone());
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

        assert_eq!(findings.len(), 3, "rollup is strictly additive");
        let rollup = findings
            .iter()
            .find(|f| f.name == "<component>")
            .expect("rollup must be present");
        assert_eq!(rollup.path, component_ts);
        assert_eq!(rollup.cyclomatic, 9, "9 = worst class 3 + template 6");
        assert_eq!(rollup.cognitive, 14, "14 = worst class 4 + template 10");
        assert_eq!(rollup.line, 42, "anchored at worst class function line");
        let breakdown = rollup.component_rollup.as_ref().expect("breakdown present");
        assert_eq!(
            breakdown.component, "host-game.component",
            "component identifier is the .ts owner's file stem"
        );
        assert_eq!(breakdown.class_worst_function, "handleClick");
        assert_eq!(breakdown.class_cyclomatic, 3);
        assert_eq!(breakdown.template_cyclomatic, 6);
        assert_eq!(breakdown.template_path, template_html);
    }

    #[test]
    fn rollup_inline_template_owner_is_same_ts_file() {
        let component_ts = PathBuf::from("/proj/src/inline.component.ts");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "ngOnInit", 25, 5, 8),
            make_template_finding(component_ts.to_str().unwrap(), 10, 4, 6),
        ];
        append_component_rollup_findings(&mut findings, None, 8, 8);

        let rollup = findings
            .iter()
            .find(|f| f.name == "<component>")
            .expect("rollup must be present for inline-template case without provenance lookup");
        assert_eq!(rollup.cyclomatic, 9);
        assert_eq!(rollup.cognitive, 14);
        let breakdown = rollup.component_rollup.as_ref().unwrap();
        assert_eq!(breakdown.template_path, component_ts);
        assert_eq!(breakdown.component, "inline.component");
    }

    #[test]
    fn rollup_picks_worst_class_function_by_cyclomatic() {
        let component_ts = PathBuf::from("/proj/src/multi.component.ts");
        let template = PathBuf::from("/proj/src/multi.component.html");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "first", 10, 3, 4),
            make_class_finding(component_ts.to_str().unwrap(), "worst", 20, 8, 9),
            make_class_finding(component_ts.to_str().unwrap(), "middle", 30, 5, 6),
            make_template_finding(template.to_str().unwrap(), 1, 4, 6),
        ];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template, component_ts);
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

        let rollup = findings.iter().find(|f| f.name == "<component>").unwrap();
        assert_eq!(rollup.cyclomatic, 12, "8 (worst.cyc) + 4 (template.cyc)");
        let breakdown = rollup.component_rollup.as_ref().unwrap();
        assert_eq!(breakdown.class_worst_function, "worst");
        assert_eq!(breakdown.class_cyclomatic, 8);
    }

    #[test]
    fn rollup_skipped_when_no_template_finding() {
        let component_ts = "/proj/src/only-class.component.ts";
        let mut findings = vec![make_class_finding(component_ts, "Foo.method", 10, 5, 7)];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(findings.len(), before, "no template means no rollup");
    }

    #[test]
    fn rollup_skipped_when_no_class_findings() {
        let template_html = PathBuf::from("/proj/src/orphan.component.html");
        let component_ts = PathBuf::from("/proj/src/orphan.component.ts");
        let mut findings = vec![make_template_finding(
            template_html.to_str().unwrap(),
            1,
            6,
            10,
        )];
        let mut lookup = rustc_hash::FxHashMap::default();
        lookup.insert(template_html, component_ts);
        let before = findings.len();
        append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);
        assert_eq!(
            findings.len(),
            before,
            "no class methods above threshold means no rollup"
        );
    }

    #[test]
    fn rollup_skipped_when_multiple_templates_on_one_owner() {
        let component_ts = PathBuf::from("/proj/src/twin.component.ts");
        let mut findings = vec![
            make_class_finding(component_ts.to_str().unwrap(), "TwinA.fn", 10, 5, 7),
            make_template_finding(component_ts.to_str().unwrap(), 5, 3, 4),
            make_template_finding(component_ts.to_str().unwrap(), 50, 4, 5),
        ];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(
            findings.len(),
            before,
            "two templates on one owner is defensively skipped"
        );
    }

    #[test]
    fn rollup_external_template_skipped_when_lookup_missing() {
        let template_html = PathBuf::from("/proj/src/no-owner.component.html");
        let component_ts = "/proj/src/no-owner.component.ts";
        let mut findings = vec![
            make_class_finding(component_ts, "NoOwner.fn", 10, 5, 7),
            make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
        ];
        let before = findings.len();
        append_component_rollup_findings(&mut findings, None, 30, 25);
        assert_eq!(findings.len(), before);
    }
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
        compute_css_analytics_report(
            files,
            HealthScanCtx {
                config: &config,
                ignore_set: &globset::GlobSet::empty(),
                changed_files: None,
                ws_roots: None,
            },
        )
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
