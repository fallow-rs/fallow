use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_types::duplicates::{DefaultIgnoreSkips, DuplicationReport};

use crate::baseline::{DuplicationBaselineData, filter_new_clone_groups, recompute_stats};
use crate::check::{get_changed_files, resolve_workspace_scope};
use crate::report;
use crate::{error::emit_error, load_config_for_analysis};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum DupesMode {
    Strict,
    Mild,
    Weak,
    Semantic,
}

impl From<fallow_config::DetectionMode> for DupesMode {
    fn from(mode: fallow_config::DetectionMode) -> Self {
        match mode {
            fallow_config::DetectionMode::Strict => Self::Strict,
            fallow_config::DetectionMode::Mild => Self::Mild,
            fallow_config::DetectionMode::Weak => Self::Weak,
            fallow_config::DetectionMode::Semantic => Self::Semantic,
        }
    }
}

pub struct DupesOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    /// CLI override for detection mode. `None` falls back to the value from
    /// the config file (or its default if unspecified there).
    pub mode: Option<DupesMode>,
    /// CLI override for minimum token count. `None` falls back to config.
    pub min_tokens: Option<usize>,
    /// CLI override for minimum line count. `None` falls back to config.
    pub min_lines: Option<usize>,
    /// CLI override for minimum occurrence count (clone groups with fewer
    /// instances are hidden). `None` falls back to config (default 2).
    /// CLI parsing rejects `< 2` so callers never need to clamp here.
    pub min_occurrences: Option<usize>,
    /// CLI override for failure threshold percentage. `None` falls back to
    /// config (where `0.0` disables the gate).
    pub threshold: Option<f64>,
    pub skip_local: bool,
    pub cross_language: bool,
    /// CLI/caller override for excluding import declarations from clone
    /// detection. `None` defers to the config value (which defaults to `true`);
    /// `Some(false)` is the explicit opt-out (`--no-ignore-imports`).
    pub ignore_imports: Option<bool>,
    pub top: Option<usize>,
    pub baseline_path: Option<&'a std::path::Path>,
    pub save_baseline_path: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_override: Option<bool>,
    pub trace: Option<&'a str>,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a crate::report::ci::diff_filter::DiffIndex>,
    pub use_shared_diff_index: bool,
    pub changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub explain: bool,
    pub explain_skipped: bool,
    /// When true, emit a condensed summary instead of full item-level output.
    pub summary: bool,
    /// `dupes` accepts `--group-by` for parity with `check` / `health`. The
    /// standalone report remains ungrouped, but the shared resolver still
    /// validates unsupported modes so global-flag errors are consistent.
    pub group_by: Option<crate::GroupBy>,
    /// When true, emit a timing panel after the duplication report. Mirrors
    /// the global `--performance` flag handling for `check` and `health`.
    /// Standalone `fallow dupes` reads this; combined-mode invocations rely
    /// on the bare `fallow` pipeline panel and ignore this field.
    pub performance: bool,
}

/// Parse a `--trace` spec string into (file_path, line_number).
///
/// Returns `Err` with a human-readable message on invalid input.
fn parse_trace_spec(spec: &str) -> Result<(&str, usize), &'static str> {
    let (file_path, line_str) = spec
        .rsplit_once(':')
        .ok_or("--trace requires FILE:LINE format (e.g., src/utils.ts:42)")?;
    let line: usize = match line_str.parse() {
        Ok(l) if l > 0 => l,
        _ => return Err("--trace LINE must be a positive integer"),
    };
    Ok((file_path, line))
}

/// Resolve a `--trace` spec, print the clone-trace deep-dive, and return the
/// process exit code. Two address forms: `dup:<fp>` resolves a clone group by
/// its stable content fingerprint (discoverable from the listing or
/// `--format json`); `FILE:LINE` resolves the clone(s) containing a source
/// location.
fn run_clone_trace(
    report: &fallow_types::duplicates::DuplicationReport,
    root: &std::path::Path,
    trace_spec: &str,
    output: OutputFormat,
) -> ExitCode {
    let (trace_result, not_found) =
        if let Some(fp) = trace_spec.strip_prefix(fallow_engine::FINGERPRINT_PREFIX) {
            let fingerprint = format!("{}{fp}", fallow_engine::FINGERPRINT_PREFIX);
            let result = fallow_engine::trace_clone_by_fingerprint(report, root, &fingerprint);
            (
                result,
                format!("no clone group with fingerprint {fingerprint}"),
            )
        } else {
            let (file_path, line) = match parse_trace_spec(trace_spec) {
                Ok(parsed) => parsed,
                Err(msg) => return emit_error(msg, 2, output),
            };
            let result = fallow_engine::trace_clone(report, root, file_path, line);
            (result, format!("no clone found at {file_path}:{line}"))
        };
    if trace_result.matched_instance.is_none() {
        return emit_error(&not_found, 2, output);
    }
    crate::report::print_clone_trace(&trace_result, root, output);
    ExitCode::SUCCESS
}

/// Build a `DuplicatesConfig` from CLI options, merging with values from the config file.
///
/// CLI scalar fields (`mode`, `min_tokens`, `min_lines`, `threshold`) are
/// `Option<T>` so an absent flag falls through to the value declared in
/// `toml_dupes`. This is what lets users set e.g. `duplicates.minLines = 8`
/// in `.fallowrc.jsonc` and have `fallow dupes` honor it. The opt-in toggles
/// (`skip_local`, `cross_language`) use OR-merge, so any `true` (CLI or config)
/// wins. `ignore_imports` defaults to `true` and supports opt-out, so it uses
/// precedence instead: an explicit CLI `Some(true|false)` wins over config,
/// `None` defers to the config value.
fn build_dupes_config(
    opts: &DupesOptions<'_>,
    toml_dupes: &fallow_config::DuplicatesConfig,
) -> fallow_config::DuplicatesConfig {
    let mode = opts.mode.map_or(toml_dupes.mode, |m| match m {
        DupesMode::Strict => fallow_config::DetectionMode::Strict,
        DupesMode::Mild => fallow_config::DetectionMode::Mild,
        DupesMode::Weak => fallow_config::DetectionMode::Weak,
        DupesMode::Semantic => fallow_config::DetectionMode::Semantic,
    });
    fallow_config::DuplicatesConfig {
        enabled: true,
        mode,
        min_tokens: opts.min_tokens.unwrap_or(toml_dupes.min_tokens),
        min_lines: opts.min_lines.unwrap_or(toml_dupes.min_lines),
        min_occurrences: opts.min_occurrences.unwrap_or(toml_dupes.min_occurrences),
        threshold: opts.threshold.unwrap_or(toml_dupes.threshold),
        ignore: toml_dupes.ignore.clone(),
        ignore_defaults: toml_dupes.ignore_defaults,
        skip_local: opts.skip_local || toml_dupes.skip_local,
        cross_language: opts.cross_language || toml_dupes.cross_language,
        ignore_imports: opts.ignore_imports.unwrap_or(toml_dupes.ignore_imports),
        normalization: toml_dupes.normalization.clone(),
        min_corpus_size_for_shingle_filter: toml_dupes.min_corpus_size_for_shingle_filter,
        min_corpus_size_for_token_cache: toml_dupes.min_corpus_size_for_token_cache,
    }
}

/// Check whether duplication percentage exceeds the configured threshold.
///
/// Returns `true` if the threshold is positive and the duplication percentage exceeds it.
fn exceeds_threshold(threshold: f64, duplication_percentage: f64) -> bool {
    threshold > 0.0 && duplication_percentage > threshold
}

use fallow_engine::filter_duplication_by_changed_files as filter_by_changed_files;

/// Filter a duplication report to only retain clone groups where at least one
/// instance belongs to a file under one of the given workspace roots. Mirrors
/// the `AnalysisResults` workspace-scoping behaviour in
/// `crate::check::filtering::filter_to_workspaces`: the full cross-workspace
/// graph is still built, only reported groups are narrowed.
///
/// Families and stats are rebuilt from the surviving groups so that the
/// reported duplication percentage reflects the scoped slice, not the whole
/// repo.
fn filter_by_workspaces(
    report: &mut fallow_types::duplicates::DuplicationReport,
    ws_roots: &[std::path::PathBuf],
    root: &std::path::Path,
) {
    report.clone_groups.retain(|g| {
        g.instances
            .iter()
            .any(|i| ws_roots.iter().any(|r| i.file.starts_with(r)))
    });
    fallow_engine::refresh_clone_families(report, root);
    report.stats = recompute_stats(report);
}

/// Filter a duplication report to only retain clone groups whose at least
/// one instance has its `[start_line..=end_line]` range overlap an added
/// line for that instance's file in the supplied diff. Group-level
/// retention (panel guidance for issue #424): a group is kept if ANY of
/// its instances overlaps, even when the other instances do not, so the
/// reviewer sees the full clone family in PR context. Single-instance
/// drop is fine because a clone-of-one is no longer a clone.
///
/// Families and stats are rebuilt from the surviving groups so that the
/// reported duplication percentage reflects the scoped slice.
fn filter_by_diff(
    report: &mut fallow_types::duplicates::DuplicationReport,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &std::path::Path,
) {
    use crate::report::ci::diff_filter::relative_to_diff_path;

    let instance_overlaps = |instance: &fallow_types::duplicates::CloneInstance| -> bool {
        let Some(rel) = relative_to_diff_path(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff_index.range_overlaps_added(&rel, start, end)
    };

    report
        .clone_groups
        .retain(|g| g.instances.iter().any(instance_overlaps));
    fallow_engine::refresh_clone_families(report, root);
    report.stats = recompute_stats(report);
}

/// Result of executing duplication analysis without printing.
pub struct DupesResult {
    pub report: DuplicationReport,
    pub default_ignore_skips: DefaultIgnoreSkips,
    pub config: ResolvedConfig,
    pub elapsed: Duration,
    pub threshold: f64,
    /// Effective `minOccurrences` (CLI override merged with config). Used by
    /// the human-format note to display the value that was actually applied;
    /// `config.duplicates.min_occurrences` only carries the toml value.
    pub min_occurrences: usize,
    /// Effective `ignoreImports` (CLI override merged with config). Used by the
    /// human-format note; `config.duplicates.ignore_imports` only carries the
    /// toml value, not the resolved CLI/default precedence.
    pub ignore_imports: bool,
    pub explain_skipped: bool,
}

/// Run duplication analysis, filtering, and baseline handling. Returns results without printing.
pub fn execute_dupes(opts: &DupesOptions<'_>) -> Result<DupesResult, ExitCode> {
    execute_dupes_inner(opts, None)
}

/// Run duplication analysis using a pre-discovered file list (e.g. from the dead-code
/// pipeline). Skips re-running `discover_files`, mirroring the audit/combined-mode path
/// that already shares parsed modules with health.
pub fn execute_dupes_with_files(
    opts: &DupesOptions<'_>,
    files: Vec<fallow_types::discover::DiscoveredFile>,
) -> Result<DupesResult, ExitCode> {
    execute_dupes_inner(opts, Some(files))
}

/// Load the resolved config for a duplication run, folding the CLI/config
/// production precedence into the `production_override`.
fn load_dupes_config_for_analysis(opts: &DupesOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    load_config_for_analysis(
        opts.root,
        opts.config_path,
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: opts
                .production_override
                .or_else(|| opts.production.then_some(true)),
            quiet: opts.quiet,
        },
        fallow_config::ProductionAnalysis::Dupes,
    )
}

/// Apply the changed-files, diff-index, workspace-scope, and top-N filters to
/// the duplication report in order. Mirrors the standalone scoping pipeline.
fn filter_dupes_report(
    report: &mut DuplicationReport,
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    effective_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> Result<(), ExitCode> {
    if let Some(changed) = effective_changed_files {
        filter_by_changed_files(report, changed, &config.root);
    }

    if let Some(diff_index) = match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    } {
        filter_by_diff(report, diff_index, &config.root);
    }

    if let Some(ws_roots) = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )? {
        filter_by_workspaces(report, &ws_roots, &config.root);
    }

    if let Some(n) = opts.top
        && opts.group_by.is_none()
    {
        apply_top(report, n, &config.root);
    }
    Ok(())
}

fn execute_dupes_inner(
    opts: &DupesOptions<'_>,
    pre_discovered: Option<Vec<fallow_types::discover::DiscoveredFile>>,
) -> Result<DupesResult, ExitCode> {
    let start = Instant::now();

    let config = load_dupes_config_for_analysis(opts)?;

    let dupes_config = build_dupes_config(opts, &config.duplicates);
    let files =
        pre_discovered.unwrap_or_else(|| fallow_engine::discover_files_with_plugin_scopes(&config));

    let changed_files_from_since = resolve_changed_since(opts);
    let effective_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>> =
        opts.changed_files.or(changed_files_from_since.as_ref());

    let (mut report, default_ignore_skips) = run_duplication_analysis(
        opts,
        &config,
        &files,
        &dupes_config,
        effective_changed_files,
    );

    if let Some(trace_spec) = opts.trace {
        // The trace view ran the full duplication analysis; record its find-state
        // for telemetry before the focused early-return so the Dupes workflow's
        // findings_present stays populated regardless of the output view (issue
        // #1650). A trace error (exit 2) is a failed run and is left unset.
        let code = run_clone_trace(&report, &config.root, trace_spec, opts.output);
        if code == ExitCode::SUCCESS {
            crate::telemetry::note_result_count(report.clone_groups.len());
        }
        return Err(code);
    }

    save_duplication_baseline(&report, &config, opts)?;
    apply_duplication_baseline(&mut report, &config, opts)?;
    filter_dupes_report(&mut report, opts, &config, effective_changed_files)?;

    let elapsed = start.elapsed();

    // Report result volume to telemetry from the real result, independent of
    // the duplication threshold gate. Exact counts are bucketed before
    // serialization.
    crate::telemetry::note_result_count(report.clone_groups.len());
    crate::telemetry::note_analysis_scale(Some(report.stats.total_files), None);

    Ok(DupesResult {
        report,
        default_ignore_skips,
        config,
        elapsed,
        threshold: dupes_config.threshold,
        min_occurrences: dupes_config.min_occurrences,
        ignore_imports: dupes_config.ignore_imports,
        explain_skipped: opts.explain_skipped,
    })
}

fn save_duplication_baseline(
    report: &DuplicationReport,
    config: &ResolvedConfig,
    opts: &DupesOptions<'_>,
) -> Result<(), ExitCode> {
    let Some(path) = opts.save_baseline_path else {
        return Ok(());
    };

    let json = serialize_duplication_baseline(report, config, opts.output)?;
    ensure_duplication_baseline_parent(path, opts.output)?;
    if let Err(e) = std::fs::write(path, json) {
        return Err(emit_error(
            &format!("failed to write duplication baseline: {e}"),
            2,
            opts.output,
        ));
    }
    if !opts.quiet {
        eprintln!("Saved duplication baseline to {}", path.display());
    }

    Ok(())
}

fn serialize_duplication_baseline(
    report: &DuplicationReport,
    config: &ResolvedConfig,
    output: OutputFormat,
) -> Result<String, ExitCode> {
    let baseline_data = DuplicationBaselineData::from_report(report, &config.root);
    serde_json::to_string_pretty(&baseline_data).map_err(|e| {
        emit_error(
            &format!("failed to serialize duplication baseline: {e}"),
            2,
            output,
        )
    })
}

fn ensure_duplication_baseline_parent(
    path: &std::path::Path,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|e| {
        emit_error(
            &format!("failed to create duplication baseline directory: {e}"),
            2,
            output,
        )
    })
}

fn apply_duplication_baseline(
    report: &mut DuplicationReport,
    config: &ResolvedConfig,
    opts: &DupesOptions<'_>,
) -> Result<(), ExitCode> {
    let Some(path) = opts.baseline_path else {
        return Ok(());
    };

    let baseline_data = read_duplication_baseline(path, opts.output)?;
    let baseline_entries = baseline_data.clone_groups.len();
    let before = report.clone_groups.len();
    *report = filter_new_clone_groups(std::mem::take(report), &baseline_data, &config.root);
    let matched = before.saturating_sub(report.clone_groups.len());
    if !opts.quiet {
        eprintln!("Comparing against duplication baseline: {}", path.display());
    }
    warn_unmatched_duplication_baseline(path, baseline_entries, matched, opts.quiet);

    Ok(())
}

fn read_duplication_baseline(
    path: &std::path::Path,
    output: OutputFormat,
) -> Result<DuplicationBaselineData, ExitCode> {
    let json = std::fs::read_to_string(path).map_err(|e| {
        emit_error(
            &format!("failed to read duplication baseline: {e}"),
            2,
            output,
        )
    })?;
    serde_json::from_str::<DuplicationBaselineData>(&json).map_err(|e| {
        emit_error(
            &format!("failed to parse duplication baseline: {e}"),
            2,
            output,
        )
    })
}

fn warn_unmatched_duplication_baseline(
    path: &std::path::Path,
    baseline_entries: usize,
    matched: usize,
    quiet: bool,
) {
    if baseline_entries > 0 && matched == 0 && !quiet {
        eprintln!(
            "Warning: duplication baseline has {baseline_entries} entries but \
             matched 0 current clone groups. Your paths may have changed, or \
             the baseline was saved on a different machine. Re-save with: \
             --save-baseline {}",
            path.display(),
        );
    }
}

/// Resolve `--changed-since` to a concrete file set up front so the focused
/// fast path engages (shingle prefilter + extraction-time interval pruning)
/// instead of a full-corpus scan followed by a redundant post-filter.
///
/// `opts.changed_files` is set by the audit driver; the standalone dupes CLI
/// only sets `opts.changed_since`. Returns `None` when neither apply or when
/// the git lookup fails (caller falls back to the full-corpus path).
fn resolve_changed_since(
    opts: &DupesOptions<'_>,
) -> Option<rustc_hash::FxHashSet<std::path::PathBuf>> {
    if opts.changed_files.is_some() {
        return None;
    }
    let git_ref = opts.changed_since?;
    get_changed_files(opts.root, git_ref)
}

/// Keep only the `n` clone groups with the highest instance count.
///
/// Sort by instance count desc (most-duplicated first), then by line count
/// desc (largest blocks first), then by deterministic path/line order so
/// ties don't shift between runs. Without this, the raw vector is
/// path-sorted alphabetically, so `--top 20` returned 20 arbitrary clone
/// groups instead of the 20 most-duplicated. Re-applies `DuplicationReport::sort()`
/// after truncation so user-facing render order stays deterministic.
fn apply_top(report: &mut DuplicationReport, n: usize, root: &std::path::Path) {
    report.clone_groups.sort_by(|a, b| {
        b.instances
            .len()
            .cmp(&a.instances.len())
            .then(b.line_count.cmp(&a.line_count))
            .then_with(|| match (a.instances.first(), b.instances.first()) {
                (Some(ai), Some(bi)) => ai
                    .file
                    .cmp(&bi.file)
                    .then(ai.start_line.cmp(&bi.start_line)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    report.clone_groups.truncate(n);
    fallow_engine::refresh_clone_families(report, root);
    report.stats.clone_groups = report.clone_groups.len();
    report.stats.clone_instances = report.clone_groups.iter().map(|g| g.instances.len()).sum();
    report.sort();
}

fn run_duplication_analysis(
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    dupes_config: &fallow_config::DuplicatesConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let cache_dir = (!opts.no_cache).then_some(config.cache_dir.as_path());
    let analysis = if let Some(changed_files) = changed_files {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        fallow_engine::find_duplicates_touching_files_with_defaults(
            &config.root,
            files,
            dupes_config,
            &changed_files,
            cache_dir,
        )
    } else {
        fallow_engine::find_duplicates_with_defaults(&config.root, files, dupes_config, cache_dir)
    };
    (analysis.report, analysis.default_ignore_skips)
}

/// Print duplication results and return appropriate exit code.
pub fn print_dupes_result(
    result: &DupesResult,
    quiet: bool,
    explain: bool,
    summary: bool,
    summary_heading: bool,
    show_explain_tip: bool,
) -> ExitCode {
    let ctx = report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet,
        explain,
        group_by: None,
        top: None,
        summary,
        summary_heading,
        show_explain_tip,
        baseline_matched: None,
        config_fixable: false,
        skip_score_and_trend: false,
        css_requested: false,
    };
    print_default_ignore_note(result, quiet);
    print_min_occurrences_note(result, quiet);
    print_ignore_imports_note(result, quiet);
    let report_code = report::print_duplication_report(&result.report, &ctx, result.config.output);
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    if exceeds_threshold(result.threshold, result.report.stats.duplication_percentage) {
        eprintln!(
            "Duplication ({:.1}%) exceeds threshold ({:.1}%)",
            result.report.stats.duplication_percentage, result.threshold
        );
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

pub fn run_dupes(opts: &DupesOptions<'_>) -> ExitCode {
    let result = match execute_dupes(opts) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if opts.performance {
        print_dupes_performance(&result, opts.output);
    }
    let resolver = match crate::build_ownership_resolver(
        opts.group_by,
        opts.root,
        result.config.codeowners.as_deref(),
        opts.output,
    ) {
        Ok(r) => r,
        Err(code) => return code,
    };
    print_dupes_result_with_grouping(DupesResultGroupingInput {
        result: &result,
        quiet: opts.quiet,
        explain: opts.explain,
        group_by: resolver,
        summary: opts.summary,
        summary_heading: true,
        show_explain_tip: true,
    })
}

/// Emit a stderr timing panel for `fallow dupes --performance`. Stays out of
/// stdout so JSON / SARIF / CodeClimate envelopes are not corrupted; the
/// panel renders only for human-readable formats so machine readers don't
/// see decorative ANSI art.
fn print_dupes_performance(result: &DupesResult, output: OutputFormat) {
    if !matches!(
        output,
        OutputFormat::Human
            | OutputFormat::Compact
            | OutputFormat::Markdown
            | OutputFormat::PrCommentGithub
            | OutputFormat::PrCommentGitlab
            | OutputFormat::ReviewGithub
            | OutputFormat::ReviewGitlab
    ) {
        return;
    }
    use colored::Colorize;
    let stats = &result.report.stats;
    let total_ms = result.elapsed.as_secs_f64() * 1000.0;
    let lines = [
        String::new(),
        "┌─ Duplication Performance ─────────────────────────"
            .dimmed()
            .to_string(),
        format!("│  total:            {total_ms:>8.1}ms")
            .dimmed()
            .to_string(),
        format!("│  files analyzed:   {:>8}", stats.total_files)
            .dimmed()
            .to_string(),
        format!("│  tokens analyzed:  {:>8}", stats.total_tokens)
            .dimmed()
            .to_string(),
        format!(
            "│  clone groups:     {:>8}  ({} instances)",
            stats.clone_groups, stats.clone_instances
        )
        .dimmed()
        .to_string(),
        format!(
            "│  duplicated lines: {:>8}  ({:.1}%)",
            stats.duplicated_lines, stats.duplication_percentage
        )
        .dimmed()
        .to_string(),
        "└───────────────────────────────────────────────────"
            .dimmed()
            .to_string(),
        String::new(),
    ];
    for line in lines {
        eprintln!("{line}");
    }
}

struct DupesResultGroupingInput<'a> {
    result: &'a DupesResult,
    quiet: bool,
    explain: bool,
    group_by: Option<report::OwnershipResolver>,
    summary: bool,
    summary_heading: bool,
    show_explain_tip: bool,
}

fn print_dupes_result_with_grouping(input: DupesResultGroupingInput<'_>) -> ExitCode {
    let result = input.result;
    let ctx = report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet: input.quiet,
        explain: input.explain,
        group_by: input.group_by,
        top: None,
        summary: input.summary,
        summary_heading: input.summary_heading,
        show_explain_tip: input.show_explain_tip,
        baseline_matched: None,
        config_fixable: false,
        skip_score_and_trend: false,
        css_requested: false,
    };
    print_default_ignore_note(result, input.quiet);
    print_min_occurrences_note(result, input.quiet);
    print_ignore_imports_note(result, input.quiet);
    report::print_duplication_report(&result.report, &ctx, result.config.output)
}

pub fn print_default_ignore_note(result: &DupesResult, quiet: bool) {
    if quiet
        || !matches!(
            result.config.output,
            OutputFormat::Human
                | OutputFormat::Markdown
                | OutputFormat::PrCommentGithub
                | OutputFormat::PrCommentGitlab
                | OutputFormat::ReviewGithub
                | OutputFormat::ReviewGitlab
        )
    {
        return;
    }

    let skips = &result.default_ignore_skips;
    if skips.total == 0 {
        return;
    }

    let noun = if skips.total == 1 { "file" } else { "files" };
    if result.explain_skipped {
        eprintln!(
            "note: skipped {} {noun} matching default duplicates ignores:",
            skips.total
        );
        for entry in &skips.by_pattern {
            eprintln!("  {:>5}  {}", entry.count, entry.pattern);
        }
    } else {
        eprintln!(
            "note: skipped {} {noun} matching default duplicates ignores (use --explain-skipped for the list)",
            skips.total
        );
    }
}

/// Emit a stderr note when `minOccurrences` hid clone groups. Human-format
/// only, so machine readers (JSON, SARIF, CodeClimate) never see decorative
/// stderr noise; consumers read `stats.cloneGroupsBelowMinOccurrences`
/// directly from the JSON envelope instead.
pub fn print_min_occurrences_note(result: &DupesResult, quiet: bool) {
    if quiet
        || !matches!(
            result.config.output,
            OutputFormat::Human
                | OutputFormat::Markdown
                | OutputFormat::PrCommentGithub
                | OutputFormat::PrCommentGitlab
                | OutputFormat::ReviewGithub
                | OutputFormat::ReviewGitlab
        )
    {
        return;
    }

    let hidden = result.report.stats.clone_groups_below_min_occurrences;
    if hidden == 0 {
        return;
    }

    let min = result.min_occurrences;
    let noun = if hidden == 1 { "group" } else { "groups" };
    eprintln!(
        "note: hid {hidden} clone {noun} below minOccurrences={min} (lower --min-occurrences to see them)"
    );
}

/// Emit a stderr note when module wiring was excluded from clone detection.
/// Human-format only, so machine readers never see decorative stderr noise.
/// Fires only when clone groups were reported, so a clean run stays quiet; it
/// tells users the report excludes a category and how to opt back in.
pub fn print_ignore_imports_note(result: &DupesResult, quiet: bool) {
    if quiet
        || !result.ignore_imports
        || result.report.clone_groups.is_empty()
        || !matches!(
            result.config.output,
            OutputFormat::Human
                | OutputFormat::Markdown
                | OutputFormat::PrCommentGithub
                | OutputFormat::PrCommentGitlab
                | OutputFormat::ReviewGithub
                | OutputFormat::ReviewGitlab
        )
    {
        return;
    }

    eprintln!(
        "note: module wiring excluded from clone detection (--no-ignore-imports to include it)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{DuplicationBaselineData, filter_new_clone_groups, recompute_stats};
    use fallow_config::{DetectionMode, DuplicatesConfig, NormalizationConfig};
    use fallow_types::duplicates::{
        CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
    };
    use std::path::{Path, PathBuf};

    fn instance(file: &str, start: usize, end: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn make_group(instances: Vec<CloneInstance>, tokens: usize, lines: usize) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: tokens,
            line_count: lines,
        }
    }

    fn make_report(
        groups: Vec<CloneGroup>,
        total_files: usize,
        total_lines: usize,
    ) -> DuplicationReport {
        let clone_instances: usize = groups.iter().map(|g| g.instances.len()).sum();
        DuplicationReport {
            clone_groups: groups,
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files,
                files_with_clones: 0,
                total_lines,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    /// Build a `DupesOptions` with the command-default scalars preset
    /// (`min_tokens=50`, `min_lines=5`, `threshold=0.0`). Tests that exercise
    /// CLI-override semantics still need a concrete value, so we wrap each
    /// scalar in `Some(...)` here. Tests that want the config-fallback path
    /// should mutate the returned struct's scalars to `None` before calling
    /// `build_dupes_config`.
    fn default_opts_for_config(root: &Path, mode: DupesMode) -> DupesOptions<'_> {
        DupesOptions {
            root,
            config_path: &None,
            output: OutputFormat::Human,
            no_cache: true,
            threads: 1,
            quiet: true,
            mode: Some(mode),
            min_tokens: Some(50),
            min_lines: Some(5),
            min_occurrences: Some(2),
            threshold: Some(0.0),
            skip_local: false,
            cross_language: false,
            ignore_imports: None,
            top: None,
            baseline_path: None,
            save_baseline_path: None,
            production: false,
            production_override: None,
            trace: None,
            changed_since: None,
            diff_index: None,
            use_shared_diff_index: true,
            changed_files: None,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            summary: false,
            group_by: None,
            performance: false,
        }
    }

    #[test]
    fn parse_trace_spec_valid() {
        let (file, line) = parse_trace_spec("src/utils.ts:42").unwrap();
        assert_eq!(file, "src/utils.ts");
        assert_eq!(line, 42);
    }

    #[test]
    fn parse_trace_spec_windows_path_with_drive() {
        let (file, line) = parse_trace_spec("C:\\path\\file.ts:10").unwrap();
        assert_eq!(file, "C:\\path\\file.ts");
        assert_eq!(line, 10);
    }

    #[test]
    fn parse_trace_spec_no_colon() {
        let err = parse_trace_spec("src/utils.ts").unwrap_err();
        assert!(
            err.contains("FILE:LINE"),
            "error should mention FILE:LINE format"
        );
    }

    #[test]
    fn parse_trace_spec_line_zero() {
        let err = parse_trace_spec("src/utils.ts:0").unwrap_err();
        assert!(err.contains("positive integer"));
    }

    #[test]
    fn parse_trace_spec_negative_line() {
        let err = parse_trace_spec("src/utils.ts:-1").unwrap_err();
        assert!(err.contains("positive integer"));
    }

    #[test]
    fn parse_trace_spec_non_numeric_line() {
        let err = parse_trace_spec("src/utils.ts:abc").unwrap_err();
        assert!(err.contains("positive integer"));
    }

    #[test]
    fn parse_trace_spec_empty_line() {
        let err = parse_trace_spec("src/utils.ts:").unwrap_err();
        assert!(err.contains("positive integer"));
    }

    #[test]
    fn parse_trace_spec_large_line_number() {
        let (file, line) = parse_trace_spec("src/app.ts:999999").unwrap();
        assert_eq!(file, "src/app.ts");
        assert_eq!(line, 999_999);
    }

    #[test]
    fn parse_trace_spec_file_with_colons_in_path() {
        let (file, line) = parse_trace_spec("a:b:c:10").unwrap();
        assert_eq!(file, "a:b:c");
        assert_eq!(line, 10);
    }

    #[test]
    fn threshold_zero_never_fails() {
        assert!(!exceeds_threshold(0.0, 100.0));
    }

    #[test]
    fn threshold_negative_never_fails() {
        assert!(!exceeds_threshold(-1.0, 50.0));
    }

    #[test]
    fn threshold_exceeded() {
        assert!(exceeds_threshold(5.0, 10.0));
    }

    #[test]
    fn threshold_exactly_at_boundary() {
        assert!(!exceeds_threshold(5.0, 5.0));
    }

    #[test]
    fn threshold_just_below() {
        assert!(!exceeds_threshold(5.0, 4.9));
    }

    #[test]
    fn threshold_just_above() {
        assert!(exceeds_threshold(5.0, 5.01));
    }

    #[test]
    fn threshold_zero_duplication_with_positive_threshold() {
        assert!(!exceeds_threshold(5.0, 0.0));
    }

    #[test]
    fn apply_top_keeps_the_most_duplicated_groups() {
        let groups = vec![
            make_group(vec![instance("z-most.ts", 1, 10); 33], 50, 10),
            make_group(vec![instance("y-mid.ts", 1, 10); 8], 50, 10),
            make_group(vec![instance("a-pair.ts", 1, 10); 2], 50, 10),
            make_group(vec![instance("m-triple.ts", 1, 10); 3], 50, 10),
            make_group(vec![instance("b-pair.ts", 1, 10); 2], 50, 10),
        ];
        let mut report = make_report(groups, 5, 100);
        report.sort(); // Path-sort first, mirroring the call site.

        apply_top(&mut report, 3, Path::new("/project"));

        let kept_sizes: Vec<usize> = report
            .clone_groups
            .iter()
            .map(|g| g.instances.len())
            .collect();
        assert_eq!(
            kept_sizes.iter().sum::<usize>(),
            33 + 8 + 3,
            "top 3 should keep the 33/8/3-instance groups, not the 2-instance pairs"
        );
        assert!(
            kept_sizes.contains(&33),
            "33-instance group must be kept (was alphabetically last under path-sort)"
        );
        assert!(
            !kept_sizes.contains(&2),
            "2-instance pairs must be dropped by top-3"
        );
    }

    #[test]
    fn apply_top_tiebreaks_by_line_count_desc() {
        let groups = vec![
            make_group(vec![instance("a.ts", 1, 10); 3], 50, 10),
            make_group(vec![instance("b.ts", 1, 60); 3], 200, 60),
            make_group(vec![instance("c.ts", 1, 30); 3], 100, 30),
        ];
        let mut report = make_report(groups, 3, 100);
        report.sort();

        apply_top(&mut report, 2, Path::new("/project"));

        let kept_lines: Vec<usize> = report.clone_groups.iter().map(|g| g.line_count).collect();
        assert_eq!(
            kept_lines.iter().sum::<usize>(),
            60 + 30,
            "with equal instance count, top 2 must keep the 60-line and 30-line groups"
        );
    }

    #[test]
    fn apply_top_recomputes_clone_groups_and_clone_instances_stats() {
        let groups = vec![
            make_group(vec![instance("a.ts", 1, 10); 5], 50, 10),
            make_group(vec![instance("b.ts", 1, 10); 3], 50, 10),
            make_group(vec![instance("c.ts", 1, 10); 2], 50, 10),
            make_group(vec![instance("d.ts", 1, 10); 2], 50, 10),
        ];
        let mut report = make_report(groups, 4, 100);
        report.sort();

        apply_top(&mut report, 1, Path::new("/project"));

        assert_eq!(report.clone_groups.len(), 1, "kept exactly one group");
        assert_eq!(
            report.clone_groups[0].instances.len(),
            5,
            "kept group is the 5-instance group"
        );
        assert_eq!(
            report.stats.clone_groups,
            report.clone_groups.len(),
            "stats.clone_groups must match the truncated array length"
        );
        assert_eq!(
            report.stats.clone_instances, 5,
            "stats.clone_instances must reflect the surviving instances"
        );
    }

    #[test]
    fn build_config_maps_all_modes() {
        let root = PathBuf::from("/project");
        let toml = DuplicatesConfig::default();
        for (cli_mode, expected) in [
            (DupesMode::Strict, DetectionMode::Strict),
            (DupesMode::Mild, DetectionMode::Mild),
            (DupesMode::Weak, DetectionMode::Weak),
            (DupesMode::Semantic, DetectionMode::Semantic),
        ] {
            let opts = default_opts_for_config(&root, cli_mode);
            let config = build_dupes_config(&opts, &toml);
            assert_eq!(config.mode, expected);
        }
    }

    #[test]
    fn build_config_always_enabled() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            enabled: false,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(config.enabled);
    }

    #[test]
    fn build_config_cross_language_cli_true_overrides_toml_false() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.cross_language = true;
        let toml = DuplicatesConfig::default(); // cross_language = false
        let config = build_dupes_config(&opts, &toml);
        assert!(config.cross_language);
    }

    #[test]
    fn build_config_cross_language_toml_true_with_cli_false() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild); // cross_language = false
        let toml = DuplicatesConfig {
            cross_language: true,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(config.cross_language);
    }

    #[test]
    fn build_config_cross_language_both_false() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(!config.cross_language);
    }

    #[test]
    fn build_config_inherits_ignore_from_toml() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            ignore: vec!["**/*.generated.ts".to_string()],
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(config.ignore, vec!["**/*.generated.ts"]);
    }

    #[test]
    fn build_config_inherits_normalization_from_toml() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            normalization: NormalizationConfig {
                ignore_identifiers: Some(true),
                ignore_string_values: None,
                ignore_numeric_values: Some(false),
            },
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(config.normalization.ignore_identifiers, Some(true));
        assert!(config.normalization.ignore_string_values.is_none());
        assert_eq!(config.normalization.ignore_numeric_values, Some(false));
    }

    #[test]
    fn build_config_uses_cli_min_tokens_and_lines() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.min_tokens = Some(100);
        opts.min_lines = Some(10);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(config.min_tokens, 100);
        assert_eq!(config.min_lines, 10);
    }

    #[test]
    fn build_config_uses_cli_threshold() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.threshold = Some(7.5);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!((config.threshold - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn build_config_uses_cli_skip_local() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.skip_local = true;
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(config.skip_local);
    }

    #[test]
    fn build_config_falls_back_to_toml_min_lines_when_cli_unset() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.min_lines = None;
        let toml = DuplicatesConfig {
            min_lines: 8,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(
            config.min_lines, 8,
            "config minLines must win when --min-lines is omitted"
        );
    }

    #[test]
    fn build_config_falls_back_to_toml_min_tokens_when_cli_unset() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.min_tokens = None;
        let toml = DuplicatesConfig {
            min_tokens: 200,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(
            config.min_tokens, 200,
            "config minTokens must win when --min-tokens is omitted"
        );
    }

    #[test]
    fn build_config_falls_back_to_toml_threshold_when_cli_unset() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.threshold = None;
        let toml = DuplicatesConfig {
            threshold: 12.5,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(
            (config.threshold - 12.5).abs() < f64::EPSILON,
            "config threshold must win when --threshold is omitted"
        );
    }

    #[test]
    fn build_config_falls_back_to_toml_mode_when_cli_unset() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.mode = None;
        let toml = DuplicatesConfig {
            mode: fallow_config::DetectionMode::Strict,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(
            matches!(config.mode, fallow_config::DetectionMode::Strict),
            "config mode must win when --mode is omitted"
        );
    }

    #[test]
    fn build_config_cli_min_lines_overrides_toml() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.min_lines = Some(3);
        let toml = DuplicatesConfig {
            min_lines: 8,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(
            config.min_lines, 3,
            "explicit --min-lines must override config minLines"
        );
    }

    #[test]
    fn build_config_skip_local_or_merges_with_toml() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            skip_local: true,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(
            config.skip_local,
            "config skipLocal=true must win even when --skip-local is omitted"
        );
    }

    #[test]
    fn build_config_ignore_imports_cli_none_defers_to_config_default_true() {
        // No CLI override: the config default (now true) flows through.
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(config.ignore_imports);
    }

    #[test]
    fn build_config_ignore_imports_cli_opt_out_overrides_config_true() {
        // `--no-ignore-imports` (Some(false)) wins over a config `ignoreImports: true`.
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.ignore_imports = Some(false);
        let toml = DuplicatesConfig {
            ignore_imports: true,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(!config.ignore_imports, "CLI opt-out must win over config");
    }

    #[test]
    fn build_config_ignore_imports_cli_opt_in_overrides_config_false() {
        // `--ignore-imports` (Some(true)) wins over a config `ignoreImports: false`.
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.ignore_imports = Some(true);
        let toml = DuplicatesConfig {
            ignore_imports: false,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(config.ignore_imports, "CLI opt-in must win over config");
    }

    #[test]
    fn build_config_ignore_imports_cli_none_defers_to_config_false() {
        // Config opt-out with no CLI override: imports are counted.
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            ignore_imports: false,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(!config.ignore_imports);
    }

    #[test]
    fn baseline_save_load_round_trip() {
        let root = Path::new("/project");
        let group = make_group(
            vec![
                instance("/project/src/a.ts", 1, 10),
                instance("/project/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let report = make_report(vec![group], 10, 1000);
        let baseline = DuplicationBaselineData::from_report(&report, root);

        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let loaded: DuplicationBaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.clone_groups, baseline.clone_groups);
    }

    #[test]
    fn baseline_filters_matching_groups_completely() {
        let root = Path::new("/project");
        let group = make_group(
            vec![
                instance("/project/src/a.ts", 1, 10),
                instance("/project/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let report = make_report(vec![group], 10, 1000);
        let baseline = DuplicationBaselineData::from_report(&report, root);

        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert!(filtered.clone_groups.is_empty());
        assert_eq!(filtered.stats.clone_groups, 0);
        assert_eq!(filtered.stats.clone_instances, 0);
    }

    #[test]
    fn baseline_keeps_groups_not_in_baseline() {
        let root = Path::new("/project");
        let old_group = make_group(
            vec![
                instance("/project/src/a.ts", 1, 10),
                instance("/project/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let new_group = make_group(
            vec![
                instance("/project/src/c.ts", 20, 30),
                instance("/project/src/d.ts", 20, 30),
            ],
            60,
            11,
        );

        let baseline_report = make_report(vec![old_group.clone()], 10, 1000);
        let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

        let report = make_report(vec![old_group, new_group], 10, 1000);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert_eq!(filtered.clone_groups.len(), 1);
        assert_eq!(filtered.clone_groups[0].instances.len(), 2);
        assert!(
            filtered.clone_groups[0]
                .instances
                .iter()
                .any(|i| i.file == std::path::Path::new("/project/src/c.ts"))
        );
    }

    #[test]
    fn recompute_stats_empty_report() {
        let report = DuplicationReport::default();
        let stats = recompute_stats(&report);
        assert_eq!(stats.clone_groups, 0);
        assert_eq!(stats.clone_instances, 0);
        assert_eq!(stats.duplicated_lines, 0);
        assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recompute_stats_basic() {
        let group = make_group(
            vec![
                instance("/project/src/a.ts", 1, 5),
                instance("/project/src/b.ts", 1, 5),
            ],
            30,
            5,
        );
        let mut report = make_report(vec![group], 10, 100);
        report.stats.total_lines = 100;
        let stats = recompute_stats(&report);
        assert_eq!(stats.clone_groups, 1);
        assert_eq!(stats.clone_instances, 2);
        assert_eq!(stats.duplicated_lines, 10);
        assert!((stats.duplication_percentage - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recompute_stats_deduplicates_overlapping_lines_in_same_file() {
        let group1 = make_group(
            vec![
                instance("/project/src/a.ts", 1, 5),
                instance("/project/src/b.ts", 1, 5),
            ],
            30,
            5,
        );
        let group2 = make_group(
            vec![
                instance("/project/src/a.ts", 3, 7),
                instance("/project/src/c.ts", 10, 14),
            ],
            30,
            5,
        );
        let mut report = make_report(vec![group1, group2], 10, 100);
        report.stats.total_lines = 100;
        let stats = recompute_stats(&report);
        assert_eq!(stats.duplicated_lines, 17);
        assert_eq!(stats.files_with_clones, 3);
    }

    #[test]
    fn recompute_stats_zero_total_lines_no_division_by_zero() {
        let mut report = DuplicationReport::default();
        report.stats.total_lines = 0;
        let stats = recompute_stats(&report);
        assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recompute_stats_computes_all_fields_from_groups() {
        let group1 = make_group(
            vec![
                instance("/project/src/a.ts", 1, 10),
                instance("/project/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let group2 = make_group(
            vec![
                instance("/project/src/c.ts", 20, 25),
                instance("/project/src/d.ts", 20, 25),
            ],
            30,
            6,
        );
        let mut report = make_report(vec![group1, group2], 20, 500);
        report.stats.total_lines = 500;
        report.stats.total_tokens = 10000;
        let stats = recompute_stats(&report);
        assert_eq!(stats.clone_groups, 2);
        assert_eq!(stats.clone_instances, 4);
        assert_eq!(stats.duplicated_lines, 32);
        assert_eq!(stats.duplicated_tokens, 160);
        assert_eq!(stats.files_with_clones, 4);
        assert!((stats.duplication_percentage - 6.4).abs() < f64::EPSILON);
    }

    #[test]
    fn filter_by_changed_files_retains_groups_with_at_least_one_changed_instance() {
        let group = make_group(
            vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let changed: rustc_hash::FxHashSet<PathBuf> =
            std::iter::once(PathBuf::from("src/a.ts")).collect();

        filter_by_changed_files(&mut report, &changed, Path::new(""));

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_families.len(),
            1,
            "families should be rebuilt after filtering"
        );
    }

    #[test]
    fn filter_by_changed_files_removes_groups_with_no_changed_instances() {
        let group = make_group(
            vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let changed: rustc_hash::FxHashSet<PathBuf> =
            std::iter::once(PathBuf::from("src/c.ts")).collect();

        filter_by_changed_files(&mut report, &changed, Path::new(""));

        assert!(report.clone_groups.is_empty());
    }

    #[test]
    fn filter_by_changed_files_partial_group_retention() {
        let group1 = make_group(
            vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            50,
            10,
        );
        let group2 = make_group(
            vec![instance("src/c.ts", 1, 10), instance("src/d.ts", 1, 10)],
            50,
            10,
        );
        let mut report = make_report(vec![group1, group2], 10, 1000);
        let changed: rustc_hash::FxHashSet<PathBuf> =
            std::iter::once(PathBuf::from("src/a.ts")).collect();

        filter_by_changed_files(&mut report, &changed, Path::new(""));

        assert_eq!(report.clone_groups.len(), 1);
        assert!(
            report.clone_groups[0]
                .instances
                .iter()
                .any(|i| i.file == std::path::Path::new("src/a.ts"))
        );
    }

    #[test]
    fn filter_by_changed_files_empty_changed_set_removes_all() {
        let group = make_group(
            vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let changed: rustc_hash::FxHashSet<PathBuf> = rustc_hash::FxHashSet::default();

        filter_by_changed_files(&mut report, &changed, Path::new(""));

        assert!(report.clone_groups.is_empty());
    }

    fn build_diff(text: &str) -> crate::report::ci::diff_filter::DiffIndex {
        crate::report::ci::diff_filter::DiffIndex::from_unified_diff(text)
    }

    #[test]
    fn filter_by_diff_keeps_group_when_one_of_four_instances_overlaps() {
        let group = make_group(
            vec![
                instance("src/a.ts", 1, 10),
                instance("src/b.ts", 100, 110),
                instance("src/c.ts", 200, 210),
                instance("src/d.ts", 300, 310),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -4,1 +4,2 @@\n\
              ctx\n\
             +touched\n",
        );

        filter_by_diff(&mut report, &diff, Path::new(""));

        assert_eq!(
            report.clone_groups.len(),
            1,
            "group must survive when any one instance overlaps the diff"
        );
        assert_eq!(report.clone_groups[0].instances.len(), 4);
    }

    #[test]
    fn filter_by_diff_drops_group_with_no_instance_in_diff() {
        let group = make_group(
            vec![
                instance("src/a.ts", 100, 110),
                instance("src/b.ts", 100, 110),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let diff = build_diff(
            "diff --git a/src/elsewhere.ts b/src/elsewhere.ts\n\
             --- a/src/elsewhere.ts\n\
             +++ b/src/elsewhere.ts\n\
             @@ -0,0 +1,1 @@\n\
             +noop\n",
        );

        filter_by_diff(&mut report, &diff, Path::new(""));

        assert!(report.clone_groups.is_empty());
    }

    #[test]
    fn filter_by_diff_drops_group_when_instance_path_matches_but_range_does_not() {
        let group = make_group(
            vec![
                instance("src/a.ts", 100, 110),
                instance("src/b.ts", 200, 210),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -4,1 +4,2 @@\n\
              ctx\n\
             +touched\n",
        );

        filter_by_diff(&mut report, &diff, Path::new(""));

        assert!(report.clone_groups.is_empty());
    }

    #[test]
    fn filter_by_diff_handles_long_instance_with_diff_in_middle() {
        let group = make_group(
            vec![
                instance("src/big.ts", 50, 250),
                instance("src/other.ts", 50, 250),
            ],
            500,
            200,
        );
        let mut report = make_report(vec![group], 10, 5000);
        let diff = build_diff(
            "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -149,1 +149,2 @@\n\
              ctx\n\
             +touched\n",
        );

        filter_by_diff(&mut report, &diff, Path::new(""));

        assert_eq!(report.clone_groups.len(), 1);
    }

    #[test]
    fn filter_by_workspaces_retains_group_with_instance_under_any_root() {
        let group = make_group(
            vec![
                instance("/p/packages/ui/src/a.ts", 1, 10),
                instance("/p/packages/api/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let roots = vec![PathBuf::from("/p/packages/ui")];

        filter_by_workspaces(&mut report, &roots, Path::new("/p"));

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_families.len(),
            1,
            "families rebuilt after scoping"
        );
    }

    #[test]
    fn filter_by_workspaces_drops_group_with_no_instance_under_any_root() {
        let group = make_group(
            vec![
                instance("/p/packages/legacy/src/a.ts", 1, 10),
                instance("/p/packages/legacy/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![group], 10, 1000);
        let roots = vec![PathBuf::from("/p/packages/ui")];

        filter_by_workspaces(&mut report, &roots, Path::new("/p"));

        assert!(report.clone_groups.is_empty());
    }

    #[test]
    fn filter_by_workspaces_union_of_multiple_roots() {
        let g_ui = make_group(
            vec![
                instance("/p/packages/ui/src/a.ts", 1, 10),
                instance("/p/packages/ui/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let g_api = make_group(
            vec![
                instance("/p/packages/api/src/x.ts", 1, 10),
                instance("/p/packages/api/src/y.ts", 1, 10),
            ],
            50,
            10,
        );
        let g_legacy = make_group(
            vec![
                instance("/p/packages/legacy/src/c.ts", 1, 10),
                instance("/p/packages/legacy/src/d.ts", 1, 10),
            ],
            50,
            10,
        );
        let mut report = make_report(vec![g_ui, g_api, g_legacy], 30, 3000);
        let roots = vec![
            PathBuf::from("/p/packages/ui"),
            PathBuf::from("/p/packages/api"),
        ];

        filter_by_workspaces(&mut report, &roots, Path::new("/p"));

        assert_eq!(
            report.clone_groups.len(),
            2,
            "ui + api retained, legacy dropped"
        );
    }

    #[test]
    fn filter_by_workspaces_empty_roots_drops_everything() {
        let group = make_group(vec![instance("/p/packages/ui/src/a.ts", 1, 10)], 50, 10);
        let mut report = make_report(vec![group], 10, 1000);
        let roots: Vec<PathBuf> = vec![];

        filter_by_workspaces(&mut report, &roots, Path::new("/p"));

        assert!(report.clone_groups.is_empty());
    }

    #[test]
    fn baseline_empty_json_object_uses_defaults() {
        let result = serde_json::from_str::<DuplicationBaselineData>(r#"{"clone_groups": []}"#);
        assert!(result.is_ok());
        assert!(result.unwrap().clone_groups.is_empty());
    }

    #[test]
    fn families_rebuilt_after_baseline_filter() {
        let root = Path::new("/project");
        let group1 = make_group(
            vec![
                instance("/project/src/a.ts", 1, 10),
                instance("/project/src/b.ts", 1, 10),
            ],
            50,
            10,
        );
        let group2 = make_group(
            vec![
                instance("/project/src/c.ts", 20, 30),
                instance("/project/src/d.ts", 20, 30),
            ],
            60,
            11,
        );

        let baseline_report = make_report(vec![group1.clone()], 10, 1000);
        let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

        let report = make_report(vec![group1, group2], 10, 1000);
        let filtered = filter_new_clone_groups(report, &baseline, root);

        assert_eq!(filtered.clone_groups.len(), 1);
        assert_eq!(filtered.clone_families.len(), 1);
        assert_eq!(filtered.clone_families[0].groups.len(), 1);
    }

    #[test]
    fn stats_recomputed_after_changed_since_filter() {
        let group = make_group(
            vec![instance("src/a.ts", 1, 5), instance("src/b.ts", 1, 5)],
            30,
            5,
        );
        let mut report = make_report(vec![group], 10, 100);
        report.stats.total_lines = 100;
        report.stats.total_tokens = 5000;
        report.stats.total_files = 10;

        let changed: rustc_hash::FxHashSet<PathBuf> =
            std::iter::once(PathBuf::from("src/x.ts")).collect();

        filter_by_changed_files(&mut report, &changed, Path::new(""));

        assert_eq!(report.stats.clone_groups, 0);
        assert_eq!(report.stats.clone_instances, 0);
        assert_eq!(report.stats.duplicated_lines, 0);
        assert!((report.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
        assert_eq!(report.stats.total_lines, 100);
        assert_eq!(report.stats.total_tokens, 5000);
        assert_eq!(report.stats.total_files, 10);
    }

    #[test]
    fn recompute_stats_counts_tokens_per_instance() {
        let group = make_group(
            vec![
                instance("/project/src/a.ts", 1, 5),
                instance("/project/src/b.ts", 1, 5),
                instance("/project/src/c.ts", 1, 5),
            ],
            40,
            5,
        );
        let mut report = make_report(vec![group], 10, 100);
        report.stats.total_lines = 100;
        let stats = recompute_stats(&report);
        assert_eq!(stats.duplicated_tokens, 120);
    }
}
