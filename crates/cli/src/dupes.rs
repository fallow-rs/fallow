use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_core::duplicates::{DefaultIgnoreSkips, DuplicationReport};

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
    /// CLI override for failure threshold percentage. `None` falls back to
    /// config (where `0.0` disables the gate).
    pub threshold: Option<f64>,
    pub skip_local: bool,
    pub cross_language: bool,
    pub ignore_imports: bool,
    pub top: Option<usize>,
    pub baseline_path: Option<&'a std::path::Path>,
    pub save_baseline_path: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_override: Option<bool>,
    pub trace: Option<&'a str>,
    pub changed_since: Option<&'a str>,
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

/// Build a `DuplicatesConfig` from CLI options, merging with values from the config file.
///
/// CLI scalar fields (`mode`, `min_tokens`, `min_lines`, `threshold`) are
/// `Option<T>` so an absent flag falls through to the value declared in
/// `toml_dupes`. This is what lets users set e.g. `duplicates.minLines = 8`
/// in `.fallowrc.jsonc` and have `fallow dupes` honor it. Boolean toggles
/// (`skip_local`, `cross_language`, `ignore_imports`) use OR-merge, so any
/// `true` (CLI or config) wins.
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
        threshold: opts.threshold.unwrap_or(toml_dupes.threshold),
        ignore: toml_dupes.ignore.clone(),
        ignore_defaults: toml_dupes.ignore_defaults,
        skip_local: opts.skip_local || toml_dupes.skip_local,
        cross_language: opts.cross_language || toml_dupes.cross_language,
        ignore_imports: opts.ignore_imports || toml_dupes.ignore_imports,
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

// Changed-file filtering for duplication reports lives in
// `fallow_core::changed_files` so the LSP can reuse it. Re-export here under
// the existing local name so call sites in this crate stay readable.
use fallow_core::changed_files::filter_duplication_by_changed_files as filter_by_changed_files;

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
    report: &mut fallow_core::duplicates::DuplicationReport,
    ws_roots: &[std::path::PathBuf],
    root: &std::path::Path,
) {
    report.clone_groups.retain(|g| {
        g.instances
            .iter()
            .any(|i| ws_roots.iter().any(|r| i.file.starts_with(r)))
    });
    report.clone_families =
        fallow_core::duplicates::families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories = fallow_core::duplicates::families::detect_mirrored_directories(
        &report.clone_families,
        root,
    );
    report.stats = recompute_stats(report);
}

/// Result of executing duplication analysis without printing.
pub struct DupesResult {
    pub report: DuplicationReport,
    pub default_ignore_skips: DefaultIgnoreSkips,
    pub config: ResolvedConfig,
    pub elapsed: Duration,
    pub threshold: f64,
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

fn execute_dupes_inner(
    opts: &DupesOptions<'_>,
    pre_discovered: Option<Vec<fallow_types::discover::DiscoveredFile>>,
) -> Result<DupesResult, ExitCode> {
    let start = Instant::now();

    let config = load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_override
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Dupes,
    )?;

    let dupes_config = build_dupes_config(opts, &config.duplicates);
    let files = pre_discovered
        .unwrap_or_else(|| fallow_core::discover::discover_files_with_plugin_scopes(&config));

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

    // Handle trace (diagnostic mode — early return)
    if let Some(trace_spec) = opts.trace {
        let (file_path, line) = match parse_trace_spec(trace_spec) {
            Ok(parsed) => parsed,
            Err(msg) => return Err(emit_error(msg, 2, opts.output)),
        };
        let trace_result = fallow_core::trace::trace_clone(&report, &config.root, file_path, line);
        if trace_result.matched_instance.is_none() {
            return Err(emit_error(
                &format!("no clone found at {file_path}:{line}"),
                2,
                opts.output,
            ));
        }
        crate::report::print_clone_trace(&trace_result, &config.root, opts.output);
        return Err(ExitCode::SUCCESS);
    }

    // Save baseline
    if let Some(path) = opts.save_baseline_path {
        let baseline_data = DuplicationBaselineData::from_report(&report, &config.root);
        match serde_json::to_string_pretty(&baseline_data) {
            Ok(json) => {
                if let Some(parent) = path.parent()
                    && !parent.as_os_str().is_empty()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    return Err(emit_error(
                        &format!("failed to create duplication baseline directory: {e}"),
                        2,
                        opts.output,
                    ));
                }
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
            }
            Err(e) => {
                return Err(emit_error(
                    &format!("failed to serialize duplication baseline: {e}"),
                    2,
                    opts.output,
                ));
            }
        }
    }

    // Filter against baseline
    if let Some(path) = opts.baseline_path {
        match std::fs::read_to_string(path) {
            Ok(json) => match serde_json::from_str::<DuplicationBaselineData>(&json) {
                Ok(baseline_data) => {
                    let baseline_entries = baseline_data.clone_groups.len();
                    let before = report.clone_groups.len();
                    report = filter_new_clone_groups(report, &baseline_data, &config.root);
                    let matched = before.saturating_sub(report.clone_groups.len());
                    if !opts.quiet {
                        eprintln!("Comparing against duplication baseline: {}", path.display());
                    }
                    if baseline_entries > 0 && matched == 0 && !opts.quiet {
                        eprintln!(
                            "Warning: duplication baseline has {baseline_entries} entries but \
                             matched 0 current clone groups. Your paths may have changed, or \
                             the baseline was saved on a different machine. Re-save with: \
                             --save-baseline {}",
                            path.display(),
                        );
                    }
                }
                Err(e) => {
                    return Err(emit_error(
                        &format!("failed to parse duplication baseline: {e}"),
                        2,
                        opts.output,
                    ));
                }
            },
            Err(e) => {
                return Err(emit_error(
                    &format!("failed to read duplication baseline: {e}"),
                    2,
                    opts.output,
                ));
            }
        }
    }

    // Filter to only changed files. Focused mode in `run_duplication_analysis`
    // already prunes groups that don't touch a changed file when
    // `effective_changed_files` is set; this pass is a safety net (no-op when
    // the focused path was used).
    if let Some(changed) = effective_changed_files {
        filter_by_changed_files(&mut report, changed, &config.root);
    }

    // Workspace scoping (either --workspace or --changed-workspaces).
    // Applied AFTER --changed-since so both can compose: in combined mode
    // the user might pass --changed-workspaces origin/main (auto-derived
    // workspace set) plus --changed-since origin/main (per-file filter
    // within those workspaces).
    if let Some(ws_roots) = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )? {
        filter_by_workspaces(&mut report, &ws_roots, &config.root);
    }

    // Apply --top.
    // Skip when --group-by is active: per-group stats must be computed over
    // the full bucket (not a globally-truncated subset), and the human/JSON
    // grouped renderers apply their own per-bucket caps at render time.
    if let Some(n) = opts.top
        && opts.group_by.is_none()
    {
        report.clone_groups.truncate(n);
        report.clone_families = fallow_core::duplicates::families::group_into_families(
            &report.clone_groups,
            &config.root,
        );
        report.mirrored_directories =
            fallow_core::duplicates::families::detect_mirrored_directories(
                &report.clone_families,
                &config.root,
            );
    }

    let elapsed = start.elapsed();

    Ok(DupesResult {
        report,
        default_ignore_skips,
        config,
        elapsed,
        // Use the merged threshold so the failure gate honors `.fallowrc.jsonc`
        // when `--threshold` is omitted on the CLI.
        threshold: dupes_config.threshold,
        explain_skipped: opts.explain_skipped,
    })
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

fn run_duplication_analysis(
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
    dupes_config: &fallow_config::DuplicatesConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    if let Some(changed_files) = changed_files {
        if opts.no_cache {
            fallow_core::duplicates::find_duplicates_touching_files_with_default_ignore_skips(
                &config.root,
                files,
                dupes_config,
                changed_files,
            )
        } else {
            fallow_core::duplicates::find_duplicates_touching_files_cached_with_default_ignore_skips(
                &config.root,
                files,
                dupes_config,
                changed_files,
                &config.cache_dir,
            )
        }
    } else if opts.no_cache {
        fallow_core::duplicates::find_duplicates_with_default_ignore_skips(
            &config.root,
            files,
            dupes_config,
        )
    } else {
        fallow_core::duplicates::find_duplicates_cached_with_default_ignore_skips(
            &config.root,
            files,
            dupes_config,
            &config.cache_dir,
        )
    }
}

/// Print duplication results and return appropriate exit code.
pub fn print_dupes_result(
    result: &DupesResult,
    quiet: bool,
    explain: bool,
    summary: bool,
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
        show_explain_tip,
        baseline_matched: None,
        health_action_opts: report::HealthActionOptions::default(),
    };
    print_default_ignore_note(result, quiet);
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
    print_dupes_result_with_grouping(
        &result,
        opts.quiet,
        opts.explain,
        resolver,
        opts.summary,
        true,
    )
}

/// Emit a stderr timing panel for `fallow dupes --performance`. Stays out of
/// stdout so JSON / SARIF / CodeClimate envelopes are not corrupted; the
/// panel renders only for human-readable formats so machine readers don't
/// see decorative ANSI art.
fn print_dupes_performance(result: &DupesResult, output: OutputFormat) {
    if !matches!(
        output,
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown
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

fn print_dupes_result_with_grouping(
    result: &DupesResult,
    quiet: bool,
    explain: bool,
    group_by: Option<report::OwnershipResolver>,
    summary: bool,
    show_explain_tip: bool,
) -> ExitCode {
    let ctx = report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet,
        explain,
        group_by,
        top: None,
        summary,
        show_explain_tip,
        baseline_matched: None,
        health_action_opts: report::HealthActionOptions::default(),
    };
    print_default_ignore_note(result, quiet);
    report::print_duplication_report(&result.report, &ctx, result.config.output)
}

pub fn print_default_ignore_note(result: &DupesResult, quiet: bool) {
    if quiet
        || !matches!(
            result.config.output,
            OutputFormat::Human | OutputFormat::Markdown
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{DuplicationBaselineData, filter_new_clone_groups, recompute_stats};
    use fallow_config::{DetectionMode, DuplicatesConfig, NormalizationConfig};
    use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
    use std::path::{Path, PathBuf};

    // ── Helpers ──────────────────────────────────────────────────────

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
            },
        }
    }

    /// Build a `DupesOptions` with the legacy CLI-default scalars preset
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
            threshold: Some(0.0),
            skip_local: false,
            cross_language: false,
            ignore_imports: false,
            top: None,
            baseline_path: None,
            save_baseline_path: None,
            production: false,
            production_override: None,
            trace: None,
            changed_since: None,
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

    // ── parse_trace_spec ─────────────────────────────────────────────

    #[test]
    fn parse_trace_spec_valid() {
        let (file, line) = parse_trace_spec("src/utils.ts:42").unwrap();
        assert_eq!(file, "src/utils.ts");
        assert_eq!(line, 42);
    }

    #[test]
    fn parse_trace_spec_windows_path_with_drive() {
        // The rsplit_once(':') should split on the LAST colon, so
        // C:\path\file.ts:10 -> file = "C:\path\file.ts", line = 10
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
        // "-1" cannot parse as usize, so it hits the catch-all error
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
        // "src/utils.ts:" -> line_str = ""
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
        // Edge case: file path contains colons (e.g., absolute path on Windows or unusual naming)
        // rsplit_once splits at the LAST colon, so "a:b:c:10" -> ("a:b:c", "10")
        let (file, line) = parse_trace_spec("a:b:c:10").unwrap();
        assert_eq!(file, "a:b:c");
        assert_eq!(line, 10);
    }

    // ── exceeds_threshold ────────────────────────────────────────────

    #[test]
    fn threshold_zero_never_fails() {
        // When threshold is 0.0 (disabled), even 100% duplication should pass
        assert!(!exceeds_threshold(0.0, 100.0));
    }

    #[test]
    fn threshold_negative_never_fails() {
        // Negative threshold is nonsensical but should not trigger failure
        assert!(!exceeds_threshold(-1.0, 50.0));
    }

    #[test]
    fn threshold_exceeded() {
        assert!(exceeds_threshold(5.0, 10.0));
    }

    #[test]
    fn threshold_exactly_at_boundary() {
        // Duplication == threshold should NOT exceed (the condition is strict >)
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

    // ── build_dupes_config ───────────────────────────────────────────

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
        // The dupes command always enables duplication detection
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
        // OR semantics: toml.cross_language || opts.cross_language
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

    // ── Config-fallback tests ────────────────────────────────────────
    // These regression tests cover the bug where CLI scalars wiped out
    // the values declared in `.fallowrc.jsonc`. With `Option<T>` opts,
    // a `None` must fall through to the toml value.

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
    fn build_config_ignore_imports_cli_true_overrides_toml_false() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.ignore_imports = true;
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(config.ignore_imports);
    }

    #[test]
    fn build_config_ignore_imports_toml_true_with_cli_false() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            ignore_imports: true,
            ..DuplicatesConfig::default()
        };
        let config = build_dupes_config(&opts, &toml);
        assert!(config.ignore_imports, "OR semantics: toml true should win");
    }

    #[test]
    fn build_config_ignore_imports_both_false() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(!config.ignore_imports);
    }

    // ── DuplicationBaselineData integration ──────────────────────────

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

        // Serialize and deserialize
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
        // The remaining group should be the new one (c.ts, d.ts)
        assert_eq!(filtered.clone_groups[0].instances.len(), 2);
        assert!(
            filtered.clone_groups[0]
                .instances
                .iter()
                .any(|i| i.file == std::path::Path::new("/project/src/c.ts"))
        );
    }

    // ── recompute_stats ──────────────────────────────────────────────

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
        // 5 lines in a.ts + 5 lines in b.ts = 10 duplicated lines
        assert_eq!(stats.duplicated_lines, 10);
        assert!((stats.duplication_percentage - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recompute_stats_deduplicates_overlapping_lines_in_same_file() {
        // Two groups both mark lines 3-7 as cloned in the same file
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
        // a.ts: lines 1-5 + lines 3-7 = lines 1-7 = 7 unique lines
        // b.ts: lines 1-5 = 5 unique lines
        // c.ts: lines 10-14 = 5 unique lines
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
        // Computed: 2 groups
        assert_eq!(stats.clone_groups, 2);
        // Computed: 4 instances total
        assert_eq!(stats.clone_instances, 4);
        // Computed: a.ts 10 + b.ts 10 + c.ts 6 + d.ts 6 = 32 duplicated lines
        assert_eq!(stats.duplicated_lines, 32);
        // Computed: (50*2) + (30*2) = 160 duplicated tokens
        assert_eq!(stats.duplicated_tokens, 160);
        // Computed: 4 unique files with clones
        assert_eq!(stats.files_with_clones, 4);
        // Computed: 32/500 * 100 = 6.4%
        assert!((stats.duplication_percentage - 6.4).abs() < f64::EPSILON);
    }

    // ── filter_by_changed_files ─────────────────────────────────────

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
        // Group 1: a.ts <-> b.ts (a.ts is changed)
        let group1 = make_group(
            vec![instance("src/a.ts", 1, 10), instance("src/b.ts", 1, 10)],
            50,
            10,
        );
        // Group 2: c.ts <-> d.ts (neither is changed)
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
        // The retained group should be the one containing a.ts
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

    // ── filter_by_workspaces ────────────────────────────────────────

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
        // An empty JSON object should deserialize with empty clone_groups
        // (this tests that the format is forward-compatible)
        let result = serde_json::from_str::<DuplicationBaselineData>(r#"{"clone_groups": []}"#);
        assert!(result.is_ok());
        assert!(result.unwrap().clone_groups.is_empty());
    }

    // ── Families rebuilt after filtering ──────────────────────────────

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

        // Baseline only knows about group1
        let baseline_report = make_report(vec![group1.clone()], 10, 1000);
        let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

        // Full report has both groups
        let report = make_report(vec![group1, group2], 10, 1000);
        let filtered = filter_new_clone_groups(report, &baseline, root);

        // Families should be rebuilt from the remaining group(s)
        assert_eq!(filtered.clone_groups.len(), 1);
        assert_eq!(filtered.clone_families.len(), 1);
        assert_eq!(filtered.clone_families[0].groups.len(), 1);
    }

    // ── Stats after changed_since filter ─────────────────────────────

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

        // All groups filtered out, stats should reflect that
        assert_eq!(report.stats.clone_groups, 0);
        assert_eq!(report.stats.clone_instances, 0);
        assert_eq!(report.stats.duplicated_lines, 0);
        assert!((report.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
        // Pass-through fields are preserved from the original stats
        assert_eq!(report.stats.total_lines, 100);
        assert_eq!(report.stats.total_tokens, 5000);
        assert_eq!(report.stats.total_files, 10);
    }

    // ── recompute_stats token counting ───────────────────────────────

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
        // 40 tokens * 3 instances = 120
        assert_eq!(stats.duplicated_tokens, 120);
    }
}
