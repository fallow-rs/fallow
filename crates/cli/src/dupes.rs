use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_engine::change_scope::{
    ChangeScope, ChangeScopeOwner, ChangeScopeRequest, PackageBaselineCache,
};
use fallow_types::duplicates::{DefaultIgnoreSkips, DuplicationReport};

use crate::baseline::{DuplicationBaselineData, filter_new_clone_groups};
use crate::check::resolve_workspace_scope;
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

/// Per-run overrides of the `duplicates` config, from one CLI parse.
///
/// `fallow dupes` fills it from its subcommand flags and the combined run
/// fills it from the global `--dupes-*` flags. Every field defaults to "not
/// set", so [`DupesOverrides::is_empty`] tells if the run uses the config as
/// it is.
#[derive(Clone, Copy, Default)]
pub struct DupesOverrides {
    /// Detection mode. `None` falls back to the config value.
    pub mode: Option<DupesMode>,
    /// Enable function-scoped near-miss clone detection.
    pub near: bool,
    /// Failure threshold percentage. `None` falls back to config (where `0.0`
    /// disables the gate).
    pub threshold: Option<f64>,
    /// Minimum token count. `None` falls back to config.
    pub min_tokens: Option<usize>,
    /// Minimum line count. `None` falls back to config.
    pub min_lines: Option<usize>,
    /// Minimum occurrence count (clone groups with fewer instances are
    /// hidden). `None` falls back to config (default 2). CLI parsing rejects
    /// `< 2`, so callers never need to clamp here.
    pub min_occurrences: Option<usize>,
    pub skip_local: bool,
    /// Omit symlinked clone instances. `None` defers to the config value
    /// (default `false`); `Some(false)` is the explicit opt-out.
    pub ignore_symlinks: Option<bool>,
    pub cross_language: bool,
    /// Exclude import declarations from clone detection. `None` defers to the
    /// config value (which defaults to `true`); `Some(false)` is the explicit
    /// opt-out.
    pub ignore_imports: Option<bool>,
}

impl DupesOverrides {
    /// The first set override, named by its global `--dupes-*` flag. Commands
    /// that do not run duplicate detection reject this flag.
    pub fn first_global_flag(&self) -> Option<&'static str> {
        [
            (self.mode.is_some(), "--dupes-mode"),
            (self.near, "--dupes-near"),
            (self.threshold.is_some(), "--dupes-threshold"),
            (self.min_tokens.is_some(), "--dupes-min-tokens"),
            (self.min_lines.is_some(), "--dupes-min-lines"),
            (self.min_occurrences.is_some(), "--dupes-min-occurrences"),
            (self.skip_local, "--dupes-skip-local"),
            (
                self.ignore_symlinks == Some(true),
                "--dupes-ignore-symlinks",
            ),
            (
                self.ignore_symlinks == Some(false),
                "--dupes-no-ignore-symlinks",
            ),
            (self.cross_language, "--dupes-cross-language"),
            (self.ignore_imports == Some(true), "--dupes-ignore-imports"),
            (
                self.ignore_imports == Some(false),
                "--dupes-no-ignore-imports",
            ),
        ]
        .into_iter()
        .find_map(|(set, flag)| set.then_some(flag))
    }

    /// Whether no override is set, so the run uses the config as it is.
    pub fn is_empty(&self) -> bool {
        self.first_global_flag().is_none()
    }
}

pub struct DupesOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    /// CLI overrides of the `duplicates` config.
    pub overrides: DupesOverrides,
    /// Positional `[PATH]` scope: root-joined absolute file or directory inside
    /// the root. Appended to the workspace-roots channel, so it composes with
    /// `--workspace` the way multiple workspace roots compose (union), and
    /// intersects with `--changed-since` / `--diff-file` like every other
    /// scope flag. `None` means whole-project scope.
    pub scope: Option<std::path::PathBuf>,
    pub top: Option<usize>,
    pub baseline_path: Option<&'a std::path::Path>,
    /// Which argument carried `baseline_path`, so the note about a baseline
    /// another command saved names an argument this run accepts. `fallow audit`
    /// passes `--dupes-baseline`; every other caller passes `--baseline`.
    pub baseline_flag: &'a str,
    pub save_baseline_path: Option<&'a std::path::Path>,
    /// Fail the run when a loaded `baseline_path` has entries that match
    /// nothing.
    pub fail_on_stale_baseline: bool,
    /// `--fail-on-issues` or `--ci`: fail the run when a clone group remains
    /// after the baseline and suppression filters.
    pub fail_on_issues: bool,
    pub production: bool,
    pub production_override: Option<bool>,
    pub trace: Option<&'a str>,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a crate::report::ci::diff_filter::DiffIndex>,
    pub use_shared_diff_index: bool,
    pub changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    /// Who owns the change scope. `audit` owns it, so its runs never read
    /// `workspaces.changedSince`.
    pub change_scope_owner: ChangeScopeOwner,
    /// `--no-package-baselines`: ignore `workspaces.changedSince` for this run.
    pub no_package_baselines: bool,
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
    /// Emit the verbatim source text on each clone instance in `--format json`
    /// output. `false` is `--no-fragments`: the five location fields still
    /// address the same code. Human and CI renderers ignore this.
    pub include_fragments: bool,
    /// Keep a copy of the detection report before the baseline and scope
    /// filters in `DupesResult::unfiltered_report`. Combined mode sets this
    /// when health can use the report in place of a second detection.
    pub retain_unfiltered_report: bool,
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
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let (trace_result, not_found) = if let Some(fp) =
        trace_spec.strip_prefix(fallow_engine::duplicates::FINGERPRINT_PREFIX)
    {
        let fingerprint = format!("{}{fp}", fallow_engine::duplicates::FINGERPRINT_PREFIX);
        let result = fallow_engine::trace::trace_clone_by_fingerprint(report, root, &fingerprint);
        (
            result,
            format!("no clone group with fingerprint {fingerprint}"),
        )
    } else {
        let (file_path, line) = match parse_trace_spec(trace_spec) {
            Ok(parsed) => parsed,
            Err(msg) => return emit_error(msg, 2, output),
        };
        let result = fallow_engine::trace::trace_clone(report, root, file_path, line);
        (result, format!("no clone found at {file_path}:{line}"))
    };
    if trace_result.matched_instance.is_none() {
        return emit_error(&not_found, 2, output);
    }
    crate::report::print_clone_trace(&trace_result, root, output, json_style);
    ExitCode::SUCCESS
}

/// Build a `DuplicatesConfig` from CLI options, merging with values from the config file.
///
/// CLI scalar fields (`mode`, `min_tokens`, `min_lines`, `threshold`) are
/// `Option<T>` so an absent flag falls through to the value declared in
/// `toml_dupes`. This is what lets users set e.g. `duplicates.minLines = 8`
/// in `.fallowrc.jsonc` and have `fallow dupes` honor it. The opt-in toggles
/// (`skip_local`, `cross_language`) use OR-merge, so any `true` (CLI or config)
/// wins. `ignore_imports` and `ignore_symlinks` have a CLI opt-out, so they
/// use precedence instead: an explicit CLI `Some(true|false)` wins over
/// config, `None` defers to the config value.
fn build_dupes_config(
    opts: &DupesOptions<'_>,
    toml_dupes: &fallow_config::DuplicatesConfig,
) -> fallow_config::DuplicatesConfig {
    let overrides = &opts.overrides;
    let mode = overrides.mode.map_or(toml_dupes.mode, |m| match m {
        DupesMode::Strict => fallow_config::DetectionMode::Strict,
        DupesMode::Mild => fallow_config::DetectionMode::Mild,
        DupesMode::Weak => fallow_config::DetectionMode::Weak,
        DupesMode::Semantic => fallow_config::DetectionMode::Semantic,
    });
    fallow_config::DuplicatesConfig {
        enabled: true,
        mode,
        near: overrides.near || toml_dupes.near,
        min_tokens: overrides.min_tokens.unwrap_or(toml_dupes.min_tokens),
        min_lines: overrides.min_lines.unwrap_or(toml_dupes.min_lines),
        min_occurrences: overrides
            .min_occurrences
            .unwrap_or(toml_dupes.min_occurrences),
        threshold: overrides.threshold.unwrap_or(toml_dupes.threshold),
        ignore: toml_dupes.ignore.clone(),
        ignored_clones: toml_dupes.ignored_clones.clone(),
        ignore_defaults: toml_dupes.ignore_defaults,
        skip_local: overrides.skip_local || toml_dupes.skip_local,
        ignore_symlinks: overrides
            .ignore_symlinks
            .unwrap_or(toml_dupes.ignore_symlinks),
        cross_language: overrides.cross_language || toml_dupes.cross_language,
        ignore_imports: overrides
            .ignore_imports
            .unwrap_or(toml_dupes.ignore_imports),
        normalization: toml_dupes.normalization.clone(),
        min_corpus_size_for_shingle_filter: toml_dupes.min_corpus_size_for_shingle_filter,
        min_corpus_size_for_token_cache: toml_dupes.min_corpus_size_for_token_cache,
    }
}

/// Check whether duplication percentage exceeds the configured threshold.
///
/// Returns `true` if the threshold is positive and the duplication percentage exceeds it.
/// The duplication threshold rule, shared by the standalone exit path, the
/// combined exit path and the `gate_outcomes` entry, so one comparison decides
/// all three. A threshold of zero is the CLI's spelling of "no limit".
pub fn exceeds_threshold(threshold: f64, duplication_percentage: f64) -> bool {
    threshold > 0.0 && duplication_percentage > threshold
}

/// Result of executing duplication analysis without printing.
pub struct DupesResult {
    pub package_baselines: Vec<fallow_api::PackageBaselineStatus>,
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
    /// Workspace, source-discovery, and analysis-stage diagnostics as THIS
    /// analysis saw them, mirroring `CheckResult::workspace_diagnostics` and
    /// the programmatic `DuplicationProgrammaticOutput`. Combined mode runs the
    /// duplication walk concurrently with the dead-code walk whenever a
    /// per-analysis `production` split stops them from sharing a file list, and
    /// each walk replaces the process registry's source-discovery set for the
    /// root, so only a by-value snapshot answers "what did the dupes walk skip"
    /// the same way on every run (issue #2366). Empty when the run reused
    /// another analysis's discovery: that analysis carries the same list.
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    /// Whether `--format json` carries the verbatim source text per clone
    /// instance. Mirrors `DupesOptions::include_fragments`.
    pub include_fragments: bool,
    /// When a baseline was loaded: this run's view of it, for the opt-in
    /// stale-baseline gate.
    pub baseline_staleness: Option<crate::baseline_gate::LoadedBaselineStaleness>,
    /// Whether `--fail-on-stale-baseline` was requested.
    pub fail_on_stale_baseline: bool,
    /// Whether `--fail-on-issues` (or `--ci`) was requested.
    pub fail_on_issues: bool,
    /// The detection report before the baseline and scope filters, when
    /// `DupesOptions::retain_unfiltered_report` was set.
    pub unfiltered_report: Option<DuplicationReport>,
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
            production_override:
                fallow_engine::project_config::ProductionFlags::single_analysis_override(
                    opts.production,
                    opts.production_override,
                ),
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
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
    change_scope: &ChangeScope,
) -> Result<(), ExitCode> {
    let diff_index = match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    };
    let mut ws_roots = resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    if let Some(scope) = opts.scope.as_ref() {
        ws_roots.get_or_insert_with(Vec::new).push(scope.clone());
    }
    fallow_engine::duplicates::apply_scope(
        report,
        &fallow_engine::duplicates::DuplicationScope {
            changes: Some(change_scope),
            diff: diff_index,
            workspace_roots: ws_roots.as_deref(),
        },
        &config.root,
    );

    if let Some(n) = opts.top {
        apply_top(report, n, &config.root);
    }
    Ok(())
}

/// Message for the one flag pair `dupes` cannot serve at once.
///
/// The refusal and the reason for it are separate strings so the terminal shows
/// the actionable half on one line. As a single sentence this soft-wrapped to
/// four lines and buried "run one or the other" at the end of the fourth.
const TOP_WITH_GROUP_BY_MESSAGE: &str = "--top and --group-by cannot be combined on dupes";

/// The `hint:` half of [`TOP_WITH_GROUP_BY_MESSAGE`].
const TOP_WITH_GROUP_BY_HINT: &str = "run one flag or the other; per-bucket stats cover every \
     clone group in the bucket, so a global top-N would leave them describing groups the listing \
     no longer holds";

/// Refuse `--top` together with `--group-by` instead of dropping one of them.
///
/// The two were previously accepted together and `--top` was silently ignored,
/// which is the failure mode the shown/omitted disclosure exists to prevent: the
/// run exited 0 reporting every clone group after being asked for N. Refusing
/// costs the caller one flag; honouring it per bucket would either recompute
/// bucket stats over a truncated set (a second contradictory scope inside one
/// object) or need a per-bucket shown/omitted split the grouped envelope does
/// not carry.
fn validate_dupes_flag_combination(opts: &DupesOptions<'_>) -> Result<(), ExitCode> {
    if opts.top.is_some() && opts.group_by.is_some() {
        return Err(crate::error::emit_error_with_hint(
            TOP_WITH_GROUP_BY_MESSAGE,
            TOP_WITH_GROUP_BY_HINT,
            2,
            opts.output,
        ));
    }
    Ok(())
}

/// The change-scope request of one `dupes` run.
fn change_scope_request<'a>(
    opts: &DupesOptions<'_>,
    files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> ChangeScopeRequest<'a> {
    ChangeScopeRequest {
        owner: opts.change_scope_owner,
        global_ref: opts.changed_since.is_some(),
        files,
        cache: Some(crate::requests::package_baseline_cache()),
        no_package_baselines: opts.no_package_baselines,
    }
}

fn resolve_change_scope(
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    request: ChangeScopeRequest<'_>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Result<ChangeScope, ExitCode> {
    let scope = ChangeScope::resolve(request, config, workspaces)
        .map_err(|err| emit_error(&format!("Workspace baseline error: {err}"), 2, opts.output))?;
    crate::requests::warn_if_package_baselines_stood_down(&scope);
    Ok(scope)
}

/// Resolve the change scope when the caller discovered the files. Workspaces
/// are discovered only when the package map needs them and no earlier
/// analysis of the run resolved it.
fn resolve_change_scope_for_pre_discovered_files(
    opts: &DupesOptions<'_>,
    config: &ResolvedConfig,
    request: ChangeScopeRequest<'_>,
) -> Result<ChangeScope, ExitCode> {
    let resolved_earlier = request.cache.is_some_and(PackageBaselineCache::is_resolved);
    if !request.reads_package_baselines(config) || resolved_earlier {
        return resolve_change_scope(opts, config, request, &[]);
    }
    let (workspaces, _) = fallow_engine::discover::discover_workspace_packages_with_diagnostics(
        &config.root,
        &config.ignore_patterns,
    )
    .map_err(|err| emit_error(&format!("Workspace discovery error: {err}"), 2, opts.output))?;
    resolve_change_scope(opts, config, request, &workspaces)
}

fn execute_dupes_inner(
    opts: &DupesOptions<'_>,
    pre_discovered: Option<Vec<fallow_types::discover::DiscoveredFile>>,
) -> Result<DupesResult, ExitCode> {
    let start = Instant::now();

    validate_dupes_flag_combination(opts)?;

    let config = load_dupes_config_for_analysis(opts)?;

    let dupes_config = build_dupes_config(opts, &config.duplicates);

    let changed_files_from_since = resolve_changed_since(opts);
    let effective_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>> =
        opts.changed_files.or(changed_files_from_since.as_ref());
    let change_scope_request = change_scope_request(opts, effective_changed_files);

    let mut workspace_diagnostics = Vec::new();
    let (mut report, default_ignore_skips, change_scope) = match pre_discovered {
        Some(files) => {
            let change_scope =
                resolve_change_scope_for_pre_discovered_files(opts, &config, change_scope_request)?;
            crate::requests::measure_changed_since_scope(&files);
            let (report, skips) = run_duplication_analysis(
                opts,
                &config,
                &files,
                &dupes_config,
                effective_changed_files,
            );
            (report, skips, change_scope)
        }
        None => {
            let session =
                match fallow_engine::session::AnalysisSession::from_resolved_config(config.clone())
                {
                    Ok(session) => session,
                    Err(err) => {
                        return Err(emit_error(
                            &format!("Analysis error: {err}"),
                            2,
                            opts.output,
                        ));
                    }
                };
            crate::requests::measure_changed_since_scope(session.files());
            let change_scope =
                resolve_change_scope(opts, &config, change_scope_request, session.workspaces())?;
            let (report, skips) = run_duplication_analysis_with_session(
                opts,
                &session,
                &dupes_config,
                effective_changed_files,
            );
            workspace_diagnostics = session.workspace_diagnostics().to_vec();
            (report, skips, change_scope)
        }
    };

    if let Some(trace_spec) = opts.trace {
        fallow_engine::duplicates::apply_scope(
            &mut report,
            &fallow_engine::duplicates::DuplicationScope {
                changes: Some(&change_scope),
                diff: None,
                workspace_roots: None,
            },
            &config.root,
        );
        // The trace view ran the full duplication analysis; record its find-state
        // for telemetry before the focused early-return so the Dupes workflow's
        // findings_present stays populated regardless of the output view (issue
        // #1650). A trace error (exit 2) is a failed run and is left unset.
        let code = run_clone_trace(
            &report,
            &config.root,
            trace_spec,
            opts.output,
            opts.json_style,
        );
        if code == ExitCode::SUCCESS {
            crate::telemetry::note_result_count(report.clone_groups.len());
        }
        return Err(code);
    }

    let unfiltered_report = opts.retain_unfiltered_report.then(|| report.clone());
    // A global ref narrows detection itself, so the baseline sees the scoped
    // report and records the scope. The package map scopes only the report
    // below: the baseline compares the full report and nothing is hidden.
    save_duplication_baseline(&report, &config, opts)?;
    let baseline_staleness =
        apply_duplication_baseline(&mut report, &config, opts, effective_changed_files)?;
    filter_dupes_report(&mut report, opts, &config, &change_scope)?;

    let elapsed = start.elapsed();

    // Report result volume to telemetry from the real result, independent of
    // the duplication threshold gate. Exact counts are bucketed before
    // serialization.
    crate::telemetry::note_result_count(report.clone_groups.len());
    crate::telemetry::note_analysis_scale(Some(report.stats.total_files), None);

    Ok(DupesResult {
        package_baselines: change_scope.package_baselines(),
        report,
        default_ignore_skips,
        config,
        elapsed,
        threshold: dupes_config.threshold,
        min_occurrences: dupes_config.min_occurrences,
        ignore_imports: dupes_config.ignore_imports,
        explain_skipped: opts.explain_skipped,
        workspace_diagnostics,
        include_fragments: opts.include_fragments,
        baseline_staleness,
        fail_on_stale_baseline: opts.fail_on_stale_baseline,
        fail_on_issues: opts.fail_on_issues,
        unfiltered_report,
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

    if let Some(refusal) = fallow_engine::baseline::refuse_baseline_kind_overwrite(
        path,
        fallow_engine::baseline::BaselineKind::Dupes,
    ) {
        return Err(emit_error(&refusal, 2, opts.output));
    }
    let json = serialize_duplication_baseline(report, config, opts.output)?;
    match fallow_engine::write_guard::write_file(
        path,
        json.as_bytes(),
        fallow_engine::write_guard::WriteTarget::Path,
    ) {
        Ok(()) => {}
        Err(e) if e.is_directory() => {
            return Err(emit_error(
                &format!("failed to create duplication baseline directory: {e}"),
                2,
                opts.output,
            ));
        }
        Err(e) => {
            return Err(emit_error(
                &format!("failed to write duplication baseline: {e}"),
                2,
                opts.output,
            ));
        }
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

fn apply_duplication_baseline(
    report: &mut DuplicationReport,
    config: &ResolvedConfig,
    opts: &DupesOptions<'_>,
    effective_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> Result<Option<crate::baseline_gate::LoadedBaselineStaleness>, ExitCode> {
    let Some(path) = opts.baseline_path else {
        return Ok(None);
    };

    let (baseline_data, saved_by, unrecognised_format) =
        read_duplication_baseline(path, opts.output)?;
    let baseline_entries = baseline_data.entry_count();
    let before = report.clone_groups.len();
    *report = filter_new_clone_groups(std::mem::take(report), &baseline_data, &config.root);
    let matched = before.saturating_sub(report.clone_groups.len());
    let scope_reasons = duplication_comparison_scope_reasons(config, effective_changed_files);
    let staleness = fallow_engine::baseline::BaselineStaleness {
        entries: baseline_entries,
        matched,
        current_findings: before,
        change_scoped: !scope_reasons.is_empty(),
    };
    if !opts.quiet {
        eprintln!("Comparing against duplication baseline: {}", path.display());
        warn_on_duplication_baseline_staleness(staleness, path);
    }
    crate::baseline_gate::note_unrecognised_baseline(
        Some(path),
        unrecognised_format,
        saved_by,
        fallow_engine::baseline::BaselineKind::Dupes,
        opts.baseline_flag,
    );

    crate::output_runtime::set_loaded_baseline(crate::output_runtime::LoadedBaselineRecheck {
        command: "dupes",
        path: path.display().to_string(),
        baseline_entries,
        scope_reasons,
    });
    Ok(Some(crate::baseline_gate::LoadedBaselineStaleness {
        staleness,
        path: path.to_path_buf(),
        scope_reasons,
        unrecognised_format,
        saved_by,
        legacy_keys: false,
    }))
}

/// Which channels narrowed the duplication comparison below the whole project.
///
/// Deliberately narrower than the dead-code equivalent. `dupes` saves and
/// compares the baseline BEFORE `filter_dupes_report` runs, so `--workspace`,
/// `--changed-workspaces`, `--diff-file` and the positional `[PATH]` scope
/// narrow only the rendered report: the comparison, and a re-save, still cover
/// the whole project and their staleness reading is honest. Only the two
/// channels that narrow the analysis itself count here: a resolved changed-file
/// set, which selects the focused analysis, and production mode, which drops
/// test, story and dev files at discovery.
///
/// The changed-file set is reported as `changed-files` rather than as the flag
/// that produced it, because by this point the flag is gone. `change_scoped` is
/// derived from the returned set, so the two cannot disagree.
fn duplication_comparison_scope_reasons(
    config: &ResolvedConfig,
    effective_changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> fallow_output::BaselineScopeReasons {
    use fallow_output::ScopeReason;

    fallow_output::BaselineScopeReasons::empty()
        .insert_if(effective_changed_files.is_some(), ScopeReason::ChangedFiles)
        .insert_if(config.production, ScopeReason::Production)
}

/// The loaded baseline, the command that saved it when the file names one other
/// than `dupes`, and whether another command saved the file.
///
/// Every field of this format has a serde default, so a foreign JSON object
/// loads as zero clone groups and is otherwise indistinguishable from a
/// baseline saved on a project with no duplication. The file's own `kind`
/// answers when it carries one, and the keys answer for a baseline saved before
/// that member existed.
fn read_duplication_baseline(
    path: &std::path::Path,
    output: OutputFormat,
) -> Result<
    (
        DuplicationBaselineData,
        Option<fallow_engine::baseline::BaselineKind>,
        bool,
    ),
    ExitCode,
> {
    let json = std::fs::read_to_string(path).map_err(|e| {
        emit_error(
            &format!("failed to read duplication baseline: {e}"),
            2,
            output,
        )
    })?;
    let data = serde_json::from_str::<DuplicationBaselineData>(&json).map_err(|e| {
        emit_error(
            &format!("failed to parse duplication baseline: {e}"),
            2,
            output,
        )
    })?;
    let classified = fallow_engine::baseline::classify_baseline_file(
        &json,
        fallow_engine::baseline::BaselineKind::Dupes,
    );
    let saved_by = classified.saved_by();
    let unrecognised_format = !matches!(classified, fallow_engine::baseline::BaselineFileKind::Own);
    Ok((data, saved_by, unrecognised_format))
}

/// Warn when a loaded duplication baseline no longer describes the current
/// project, using the same decision as `dead-code` and `health`.
fn warn_on_duplication_baseline_staleness(
    staleness: fallow_engine::baseline::BaselineStaleness,
    path: &std::path::Path,
) {
    let baseline_entries = staleness.entries;
    let stale_entries = staleness.stale_entries();
    match staleness.warning() {
        fallow_engine::baseline::BaselineStalenessWarning::None => {}
        fallow_engine::baseline::BaselineStalenessWarning::ZeroOverlap => eprintln!(
            "Warning: duplication baseline has {baseline_entries} entries but \
             matched 0 current clone groups. Your paths may have changed, or \
             the baseline was saved on a different machine. Re-save with: \
             --save-baseline {}",
            path.display(),
        ),
        fallow_engine::baseline::BaselineStalenessWarning::Partial => eprintln!(
            "Warning: duplication baseline is partially stale: {stale_entries} \
             of {baseline_entries} entries matched no current clone group, so \
             the gate protects less than what was saved. Re-save with: \
             --save-baseline {}",
            path.display(),
        ),
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
    crate::requests::resolve_changed_since(opts.root, git_ref)
}

use fallow_engine::duplicates::apply_top;

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
        fallow_engine::duplicates::find_duplicates_touching_files_with_defaults(
            &config.root,
            files,
            dupes_config,
            &changed_files,
            cache_dir,
        )
    } else {
        fallow_engine::duplicates::find_duplicates_with_defaults(
            &config.root,
            files,
            dupes_config,
            cache_dir,
        )
    };
    (analysis.report, analysis.default_ignore_skips)
}

fn run_duplication_analysis_with_session(
    opts: &DupesOptions<'_>,
    session: &fallow_engine::session::AnalysisSession,
    dupes_config: &fallow_config::DuplicatesConfig,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) -> (DuplicationReport, DefaultIgnoreSkips) {
    let cache_dir = (!opts.no_cache).then_some(session.config().cache_dir.as_path());
    let analysis = if let Some(changed_files) = changed_files {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session.find_duplicates_touching_files_with_defaults(
            dupes_config,
            &changed_files,
            cache_dir,
        )
    } else {
        session.find_duplicates_with_defaults(dupes_config, cache_dir)
    };
    (analysis.report, analysis.default_ignore_skips)
}

/// The presentation choices a mode hands the duplication renderer.
///
/// Bundled so an entry point names them rather than passing a row of
/// positional booleans; `print_dupes_result` keeps the flat signature combined
/// mode calls.
pub struct DupesRenderOptions<'a> {
    pub result: &'a DupesResult,
    pub quiet: bool,
    pub explain: bool,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
    pub json_style: crate::json_style::JsonStyle,
}

/// Print duplication results for bare `fallow` and return its exit code.
///
/// Combined mode honors the `--dupes-` prefixed globals, so notes printed
/// from here route to those. `fallow audit` parses the same globals and then
/// drops them, so it renders through [`print_audit_dupes_result`] instead.
#[expect(
    clippy::too_many_arguments,
    reason = "duplication rendering carries independent report, grouping, and presentation options"
)]
pub fn print_dupes_result(
    result: &DupesResult,
    quiet: bool,
    explain: bool,
    summary: bool,
    summary_heading: bool,
    show_explain_tip: bool,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    print_scoped_dupes_result(
        &DupesRenderOptions {
            result,
            quiet,
            explain,
            summary,
            summary_heading,
            show_explain_tip,
            json_style,
        },
        DupesOptOutScope::Combined,
    )
}

/// Print duplication results for `fallow audit` and return its exit code.
///
/// Audit builds its `DupesOptions` from `DuplicatesConfig` alone, so the
/// `--dupes-` globals parse there and then change nothing; its notes route to
/// the config keys, which are what actually move the outcome.
pub fn print_audit_dupes_result(options: &DupesRenderOptions<'_>) -> ExitCode {
    print_scoped_dupes_result(options, DupesOptOutScope::Audit)
}

fn print_scoped_dupes_result(
    options: &DupesRenderOptions<'_>,
    opt_out_scope: DupesOptOutScope,
) -> ExitCode {
    print_dupes_result_with_grouping(DupesResultGroupingInput {
        result: options.result,
        quiet: options.quiet,
        opt_out_scope,
        explain: options.explain,
        group_by: None,
        summary: options.summary,
        summary_heading: options.summary_heading,
        show_explain_tip: options.show_explain_tip,
        json_style: options.json_style,
    })
}

pub fn run_dupes(opts: &DupesOptions<'_>) -> ExitCode {
    if let Some(code) = crate::baseline_gate::refuse_save_before_analysis(
        opts.save_baseline_path,
        fallow_engine::baseline::BaselineKind::Dupes,
        opts.output,
    ) {
        return code;
    }
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
        opt_out_scope: DupesOptOutScope::Subcommand,
        explain: opts.explain,
        group_by: resolver,
        summary: opts.summary,
        summary_heading: true,
        show_explain_tip: true,
        json_style: opts.json_style,
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
    /// Which command surface is printing, so the scoped notes name a control
    /// that mode actually honors.
    opt_out_scope: DupesOptOutScope,
    explain: bool,
    group_by: Option<report::OwnershipResolver>,
    summary: bool,
    summary_heading: bool,
    show_explain_tip: bool,
    json_style: crate::json_style::JsonStyle,
}

fn print_dupes_result_with_grouping(input: DupesResultGroupingInput<'_>) -> ExitCode {
    let result = input.result;
    let baseline_staleness = result
        .baseline_staleness
        .as_ref()
        .map(|loaded| loaded.to_envelope(0));
    let gate_outcomes = crate::gates::dupes_gate_outcomes(
        result.threshold,
        result.report.stats.duplication_percentage,
        result.report.stats.clone_groups,
        result.fail_on_issues,
        baseline_staleness.as_ref(),
        result.fail_on_stale_baseline,
    );
    let ctx = report::ReportContext {
        package_baselines: &result.package_baselines,
        root: &result.config.root,
        rules: &result.config.rules,
        workspace_diagnostics: &result.workspace_diagnostics,
        elapsed: result.elapsed,
        quiet: input.quiet,
        explain: input.explain,
        type_aware: None,
        type_aware_scope: None,
        group_by: input.group_by,
        top: None,
        summary: input.summary,
        summary_heading: input.summary_heading,
        show_explain_tip: input.show_explain_tip,
        baseline_matched: None,
        baseline_staleness,
        finding_id_query: None,
        gate_outcomes,
        config_fixable: false,
        failed_parse_files: 0,
        skip_score_and_trend: false,
        css_requested: false,
        json_style: input.json_style,
        include_fragments: result.include_fragments,
    };
    print_default_ignore_note(result, input.quiet);
    print_min_occurrences_note(result, input.quiet, input.opt_out_scope);
    print_reviewed_clones_note(result, input.quiet);
    print_near_candidates_skipped_note(result, input.quiet);
    print_ignore_imports_note(result, input.quiet, input.opt_out_scope);
    let report_code = report::print_duplication_report(&result.report, &ctx, result.config.output);
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    // The threshold gate lives here, on the single shared renderer, so every
    // entry point inherits it. Standalone `dupes` and combined mode previously
    // rendered through two near-identical functions and only the combined one
    // gated, so `fallow dupes --threshold 1` exited 0 at 100% duplication (#2009).
    let threshold_exceeded =
        exceeds_threshold(result.threshold, result.report.stats.duplication_percentage);
    if threshold_exceeded {
        eprintln!(
            "Duplication ({:.1}%) exceeds threshold ({:.1}%)",
            result.report.stats.duplication_percentage, result.threshold
        );
    }

    // Evaluated even when the threshold gate already failed. The duplication
    // percentage is printed in the report, a stale baseline is not, so
    // returning on the threshold first would exit 1 without a word about the
    // baseline the user explicitly gated on.
    let stale_baseline_failed = crate::baseline_gate::gate_failed(
        result.baseline_staleness.as_ref(),
        result.fail_on_stale_baseline,
        fallow_engine::baseline::BaselineKind::Dupes,
    );

    // `--fail-on-issues` and `--ci` fail on any clone group the report shows.
    // The same builder gives the `gate_outcomes` entry, so the two agree. The
    // report already lists each group, so no extra line is printed.
    let clone_groups_failed = crate::gates::duplication_findings_outcome(
        result.report.stats.clone_groups,
        result.fail_on_issues,
    )
    .is_some_and(|outcome| outcome.fails_run());

    let code = [
        crate::exit_codes::gate_failed_exit_code(
            fallow_output::GateName::DuplicationThreshold,
            threshold_exceeded,
        ),
        crate::exit_codes::gate_failed_exit_code(
            fallow_output::GateName::DuplicationFindings,
            clone_groups_failed,
        ),
        crate::exit_codes::gate_failed_exit_code(
            fallow_output::GateName::StaleBaseline,
            stale_baseline_failed,
        ),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    // Only the standalone command owns its exit code. `audit` owns its own,
    // and the bare run prints one line for all its sections.
    if matches!(input.opt_out_scope, DupesOptOutScope::Subcommand) {
        crate::gates::print_exit_reason(&crate::gates::ExitReason {
            gates: ctx.gate_outcomes.as_ref(),
            code,
            fail_on_issues: result.fail_on_issues,
            quiet: input.quiet,
            output: result.config.output,
            // The threshold line above prints in every mode.
            own_lines: &[fallow_output::GateName::DuplicationThreshold],
        });
    }
    crate::exit_codes::run_exit_code([code])
}

/// The default-ignore note lines: how many files the built-in duplicates
/// ignores skipped, then either the per-pattern breakdown or the route to it.
///
/// Split out from the printer so the wording and the width are testable the
/// way the sibling notes are: every line has to hold under 80 columns. The
/// route sits on its own line rather than closing the clause, which rendered at
/// 89 columns inline for a single skipped file and widens a column per digit of
/// an unbounded skip count.
fn default_ignore_note_lines(skips: &DefaultIgnoreSkips, explain: bool) -> Vec<String> {
    if skips.total == 0 {
        return Vec::new();
    }

    let total = skips.total;
    let noun = if total == 1 { "file" } else { "files" };
    if !explain {
        return vec![
            format!("note: skipped {total} {noun} matching default duplicates ignores"),
            "  (--explain-skipped for the list)".to_string(),
        ];
    }

    let mut lines = vec![format!(
        "note: skipped {total} {noun} matching default duplicates ignores:"
    )];
    lines.extend(
        skips
            .by_pattern
            .iter()
            .map(|entry| format!("  {:>5}  {}", entry.count, entry.pattern)),
    );
    lines
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

    for line in default_ignore_note_lines(&result.default_ignore_skips, result.explain_skipped) {
        eprintln!("{line}");
    }
}

/// The `minOccurrences` note lines: how many clone groups the gate hid, then
/// the control the printing mode honors for seeing them.
///
/// Both numbers are unbounded `usize`, so the threshold sits on the route line
/// rather than closing the opening clause: inline the note rendered at 84
/// columns for one hidden group at `minOccurrences=2` and widened further with
/// either count. Split out from the printer so the wording and the width are
/// testable the way `default_ignore_note_lines` is.
fn min_occurrences_note_lines(hidden: usize, min: usize, scope: DupesOptOutScope) -> Vec<String> {
    if hidden == 0 {
        return Vec::new();
    }
    let noun = if hidden == 1 { "group" } else { "groups" };
    vec![
        format!("note: hid {hidden} clone {noun} below minOccurrences"),
        format!(
            "  (lower {} from {min} to see them)",
            scope.min_occurrences_control()
        ),
    ]
}

/// Emit the `minOccurrences` note for `fallow audit`, whose clean-duplication
/// path returns before the shared renderer that would otherwise print it.
pub fn print_audit_min_occurrences_note(result: &DupesResult, quiet: bool) {
    print_min_occurrences_note(result, quiet, DupesOptOutScope::Audit);
}

/// Emit a stderr note when `minOccurrences` hid clone groups. Human-format
/// only, so machine readers (JSON, SARIF, CodeClimate) never see decorative
/// stderr noise; consumers read `stats.cloneGroupsBelowMinOccurrences`
/// directly from the JSON envelope instead.
fn print_min_occurrences_note(result: &DupesResult, quiet: bool, scope: DupesOptOutScope) {
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

    for line in min_occurrences_note_lines(
        result.report.stats.clone_groups_below_min_occurrences,
        result.min_occurrences,
        scope,
    ) {
        eprintln!("{line}");
    }
}

/// The reviewed-clones note lines: how many groups `duplicates.ignoredClones`
/// hid, then the config key and how to resurface one.
///
/// The key and its instruction moved onto their own line because the inline
/// clause rendered at 96 columns for a single hidden group and grew with an
/// unbounded hidden count.
fn reviewed_clones_note_lines(hidden: usize) -> Vec<String> {
    if hidden == 0 {
        return Vec::new();
    }
    let noun = if hidden == 1 { "group" } else { "groups" };
    vec![
        format!("note: hid {hidden} reviewed clone {noun}"),
        "  (duplicates.ignoredClones: remove a key to review it again)".to_string(),
    ]
}

fn print_reviewed_clones_note(result: &DupesResult, quiet: bool) {
    if quiet || !matches!(result.config.output, OutputFormat::Human) {
        return;
    }
    for line in reviewed_clones_note_lines(result.report.stats.clone_groups_ignored) {
        eprintln!("{line}");
    }
}

/// The incomplete-near-miss warning lines: the caveat, what was dropped, then
/// the narrowing route.
///
/// Three lines rather than one because the inline sentence rendered at 148
/// columns for a single skipped comparison, the widest human line this module
/// emitted, and the skipped count is unbounded.
fn near_candidates_skipped_note_lines(skipped: usize) -> Vec<String> {
    if skipped == 0 {
        return Vec::new();
    }
    let noun = if skipped == 1 {
        "comparison"
    } else {
        "comparisons"
    };
    vec![
        "warning: near-miss results may be incomplete".to_string(),
        format!("  skipped {skipped} candidate {noun} to stay within work limits"),
        "  (narrow with --workspace or --changed-since)".to_string(),
    ]
}

fn print_near_candidates_skipped_note(result: &DupesResult, quiet: bool) {
    if quiet || !matches!(result.config.output, OutputFormat::Human) {
        return;
    }
    for line in near_candidates_skipped_note_lines(result.report.stats.near_candidates_skipped) {
        eprintln!("{line}");
    }
}

/// Which spelling of the duplication opt-outs the printing mode honors.
///
/// The three modes take three different answers, and one shared renderer prints
/// the notes, so the mode has to carry its labels in. `--no-ignore-imports` and
/// `--min-occurrences` are declared on the `dupes` subcommand without `global`,
/// so bare `fallow` and `fallow audit` answer both with "unexpected argument"
/// and need the `--dupes-` prefixed spellings. Bare `fallow` honors those
/// prefixed flags; `fallow audit` parses them and then builds its
/// `DupesOptions` from `DuplicatesConfig` alone, so only the config keys change
/// what audit reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DupesOptOutScope {
    /// `fallow dupes`, where the opt-outs are subcommand-scoped flags.
    Subcommand,
    /// Bare `fallow`, where they are `--dupes-` prefixed globals.
    Combined,
    /// `fallow audit`, where the globals parse but only config moves the
    /// outcome.
    Audit,
}

impl DupesOptOutScope {
    /// The module-wiring opt-out this mode honors.
    const fn ignore_imports_opt_out(self) -> &'static str {
        match self {
            Self::Subcommand => "--no-ignore-imports",
            Self::Combined => "--dupes-no-ignore-imports",
            Self::Audit => "duplicates.ignoreImports: false",
        }
    }

    /// The `minOccurrences` control this mode honors.
    const fn min_occurrences_control(self) -> &'static str {
        match self {
            Self::Subcommand => "--min-occurrences",
            Self::Combined => "--dupes-min-occurrences",
            Self::Audit => "duplicates.minOccurrences",
        }
    }
}

/// The module-wiring note lines: what clone detection left out, then the
/// opt-out the mode doing the printing honors.
///
/// The opt-out sits on its own line the way `default_ignore_note_lines` puts
/// its route there: the combined spelling renders at 82 columns inline, so a
/// single-line note would hold under 80 in one mode and not the other.
fn ignore_imports_note_lines(scope: DupesOptOutScope) -> Vec<String> {
    vec![
        "note: module wiring excluded from clones".to_string(),
        format!("  ({} to include it)", scope.ignore_imports_opt_out()),
    ]
}

/// Emit a stderr note when module wiring was excluded from clone detection.
/// Human-format only, so machine readers never see decorative stderr noise.
/// Fires only when clone groups were reported, so a clean run stays quiet; it
/// tells users the report excludes a category and how to opt back in.
fn print_ignore_imports_note(result: &DupesResult, quiet: bool, scope: DupesOptOutScope) {
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

    for line in ignore_imports_note_lines(scope) {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::{DuplicationBaselineData, filter_new_clone_groups, recompute_stats};
    use fallow_config::{DetectionMode, DuplicatesConfig, NormalizationConfig};
    use fallow_engine::changed_files::filter_duplication_by_changed_files as filter_by_changed_files;
    use fallow_engine::diff_scope::filter_duplication_by_diff as filter_by_diff;
    use fallow_engine::duplicates::filter_to_workspaces as filter_by_workspaces;
    use fallow_types::duplicates::{
        CloneGroup, CloneInstance, DefaultIgnoreSkipCount, DuplicationReport, DuplicationStats,
    };
    use std::path::{Path, PathBuf};

    fn instance(file: &str, start: usize, end: usize) -> CloneInstance {
        CloneInstance {
            is_symlink: false,
            file: PathBuf::from(file),
            start_line: start,
            end_line: end,
            start_col: 0,
            end_col: 0,
            fragment: format!("const cloneBody = {file:?};"),
        }
    }

    fn make_group(instances: Vec<CloneInstance>, tokens: usize, lines: usize) -> CloneGroup {
        CloneGroup {
            instances,
            token_count: tokens,
            line_count: lines,
            similarity: None,
        }
    }

    fn make_report(
        groups: Vec<CloneGroup>,
        total_files: usize,
        total_lines: usize,
    ) -> DuplicationReport {
        let clone_instances: usize = groups.iter().map(|g| g.instances.len()).sum();
        let group_count = groups.len();
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
                clone_groups: group_count,
                clone_families: 0,
                clone_instances,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
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
            json_style: crate::json_style::JsonStyle::Compact,
            no_cache: true,
            threads: 1,
            quiet: true,
            allow_remote_extends: false,
            overrides: DupesOverrides {
                mode: Some(mode),
                min_tokens: Some(50),
                min_lines: Some(5),
                min_occurrences: Some(2),
                threshold: Some(0.0),
                ..DupesOverrides::default()
            },
            top: None,
            baseline_path: None,
            baseline_flag: "--baseline",
            save_baseline_path: None,
            fail_on_stale_baseline: false,
            fail_on_issues: false,
            production: false,
            production_override: None,
            trace: None,
            changed_since: None,
            diff_index: None,
            use_shared_diff_index: true,
            changed_files: None,
            change_scope_owner: ChangeScopeOwner::Run,
            no_package_baselines: false,
            workspace: None,
            changed_workspaces: None,
            explain: false,
            explain_skipped: false,
            summary: false,
            group_by: None,
            performance: false,
            include_fragments: true,
            retain_unfiltered_report: false,
            scope: None,
        }
    }

    #[test]
    fn top_with_group_by_is_refused_as_invalid_input() {
        let root = Path::new("/project");
        let mut opts = default_opts_for_config(root, DupesMode::Mild);
        opts.top = Some(1);
        opts.group_by = Some(crate::GroupBy::Directory);

        let code = validate_dupes_flag_combination(&opts)
            .expect_err("--top with --group-by must be refused");

        assert_eq!(code, ExitCode::from(2));
        assert!(
            TOP_WITH_GROUP_BY_MESSAGE.contains("--top")
                && TOP_WITH_GROUP_BY_MESSAGE.contains("--group-by"),
            "the refusal must name both flags"
        );
        assert!(
            TOP_WITH_GROUP_BY_HINT.contains("per-bucket stats"),
            "the hint must say why the pair cannot be served"
        );
        assert!(
            TOP_WITH_GROUP_BY_MESSAGE.len() < 60,
            "the actionable half must fit one terminal line: {} chars",
            TOP_WITH_GROUP_BY_MESSAGE.len()
        );
    }

    #[test]
    fn either_flag_on_its_own_is_accepted() {
        let root = Path::new("/project");
        let mut top_only = default_opts_for_config(root, DupesMode::Mild);
        top_only.top = Some(1);
        assert!(validate_dupes_flag_combination(&top_only).is_ok());

        let mut grouped_only = default_opts_for_config(root, DupesMode::Mild);
        grouped_only.group_by = Some(crate::GroupBy::Directory);
        assert!(validate_dupes_flag_combination(&grouped_only).is_ok());

        let neither = default_opts_for_config(root, DupesMode::Mild);
        assert!(validate_dupes_flag_combination(&neither).is_ok());
    }

    #[test]
    fn parse_trace_spec_valid() {
        let (file, line) = parse_trace_spec("src/utils.ts:42").unwrap();
        assert_eq!(file, "src/utils.ts");
        assert_eq!(line, 42);
    }

    #[test]
    fn reviewed_clones_note_pluralizes_and_explains_resurfacing() {
        assert_eq!(
            reviewed_clones_note_lines(1),
            vec![
                "note: hid 1 reviewed clone group".to_string(),
                "  (duplicates.ignoredClones: remove a key to review it again)".to_string(),
            ]
        );
        assert_eq!(
            reviewed_clones_note_lines(2)[0],
            "note: hid 2 reviewed clone groups"
        );
        assert!(reviewed_clones_note_lines(0).is_empty());
    }

    #[test]
    fn min_occurrences_note_names_the_control_the_printing_mode_honors() {
        assert_eq!(
            min_occurrences_note_lines(3, 2, DupesOptOutScope::Subcommand),
            vec![
                "note: hid 3 clone groups below minOccurrences".to_string(),
                "  (lower --min-occurrences from 2 to see them)".to_string(),
            ]
        );
        assert_eq!(
            min_occurrences_note_lines(3, 2, DupesOptOutScope::Combined)[1],
            "  (lower --dupes-min-occurrences from 2 to see them)"
        );
        // Audit rebuilds its `DupesOptions` from `DuplicatesConfig`, so both
        // flag spellings are inert there and the note has to name the key.
        assert_eq!(
            min_occurrences_note_lines(3, 2, DupesOptOutScope::Audit)[1],
            "  (lower duplicates.minOccurrences from 2 to see them)"
        );
        assert_eq!(
            min_occurrences_note_lines(1, 5, DupesOptOutScope::Subcommand)[0],
            "note: hid 1 clone group below minOccurrences"
        );
        assert!(min_occurrences_note_lines(0, 2, DupesOptOutScope::Subcommand).is_empty());
    }

    #[test]
    fn default_ignore_note_pins_its_wording_in_both_branches() {
        let skips = DefaultIgnoreSkips {
            total: 1234,
            by_pattern: vec![DefaultIgnoreSkipCount {
                pattern: "**/storybook-static/**",
                count: 1234,
            }],
        };

        let lines = default_ignore_note_lines(&skips, false);
        assert_eq!(
            lines,
            vec![
                "note: skipped 1234 files matching default duplicates ignores".to_string(),
                "  (--explain-skipped for the list)".to_string(),
            ]
        );

        let explained = default_ignore_note_lines(&skips, true);
        assert_eq!(
            explained[0],
            format!("{}:", lines[0]),
            "both branches open with the same clause, so only the tail differs"
        );
        assert_eq!(explained[1], "   1234  **/storybook-static/**");
    }

    #[test]
    fn default_ignore_note_is_singular_for_one_file_and_silent_for_none() {
        assert!(default_ignore_note_lines(&DefaultIgnoreSkips::default(), false).is_empty());
        assert!(default_ignore_note_lines(&DefaultIgnoreSkips::default(), true).is_empty());

        let one = DefaultIgnoreSkips {
            total: 1,
            by_pattern: vec![DefaultIgnoreSkipCount {
                pattern: "**/*.test.*",
                count: 1,
            }],
        };
        assert_eq!(
            default_ignore_note_lines(&one, false)[0],
            "note: skipped 1 file matching default duplicates ignores"
        );
    }

    #[test]
    fn ignore_imports_note_names_the_opt_out_the_printing_mode_honors() {
        assert_eq!(
            ignore_imports_note_lines(DupesOptOutScope::Subcommand),
            vec![
                "note: module wiring excluded from clones".to_string(),
                "  (--no-ignore-imports to include it)".to_string(),
            ]
        );
        assert_eq!(
            ignore_imports_note_lines(DupesOptOutScope::Combined),
            vec![
                "note: module wiring excluded from clones".to_string(),
                "  (--dupes-no-ignore-imports to include it)".to_string(),
            ]
        );
        assert_eq!(
            ignore_imports_note_lines(DupesOptOutScope::Audit),
            vec![
                "note: module wiring excluded from clones".to_string(),
                "  (duplicates.ignoreImports: false to include it)".to_string(),
            ]
        );

        // Combined mode's spelling is the subcommand flag under a `--dupes-`
        // prefix: clap declares the subcommand one without `global`, so bare
        // `fallow` answers it with "unexpected argument". Audit deliberately
        // breaks that pattern, because the prefixed flag parses there and then
        // changes nothing; `crates/cli/tests/dupes_tests.rs` pins both against
        // the built binary.
        assert_eq!(
            DupesOptOutScope::Combined.ignore_imports_opt_out(),
            format!(
                "--dupes-{}",
                DupesOptOutScope::Subcommand
                    .ignore_imports_opt_out()
                    .trim_start_matches("--")
            )
        );
        assert!(
            !DupesOptOutScope::Audit
                .ignore_imports_opt_out()
                .starts_with('-'),
            "audit's opt-out is a config key, not a flag"
        );
    }

    const ALL_SCOPES: [DupesOptOutScope; 3] = [
        DupesOptOutScope::Subcommand,
        DupesOptOutScope::Combined,
        DupesOptOutScope::Audit,
    ];

    /// Names a scope for the width table. Exhaustive, so a fourth mode cannot
    /// reach the notes without being named here first.
    const fn scope_label(scope: DupesOptOutScope) -> &'static str {
        match scope {
            DupesOptOutScope::Subcommand => "dupes",
            DupesOptOutScope::Combined => "combined",
            DupesOptOutScope::Audit => "audit",
        }
    }

    /// Every dupes note rendered at its widest: unbounded counts pushed to
    /// `usize::MAX`, plural nouns, both branches of the default-ignore note,
    /// and every mode's spelling of the two scoped notes. A note missing from
    /// this list is a note nothing measures.
    fn widest_note_renderings() -> Vec<(String, Vec<String>)> {
        // `DefaultIgnoreSkipCount::pattern` is `&'static str` because the
        // breakdown only ever names the engine's built-in ignore list, whose
        // longest entry is `**/storybook-static/**` (DUPES_DEFAULT_IGNORES).
        let widest_skips = DefaultIgnoreSkips {
            total: usize::MAX,
            by_pattern: vec![DefaultIgnoreSkipCount {
                pattern: "**/storybook-static/**",
                count: usize::MAX,
            }],
        };
        let mut renderings = vec![
            (
                "default-ignore-route".to_string(),
                default_ignore_note_lines(&widest_skips, false),
            ),
            (
                "default-ignore-breakdown".to_string(),
                default_ignore_note_lines(&widest_skips, true),
            ),
            (
                "reviewed-clones".to_string(),
                reviewed_clones_note_lines(usize::MAX),
            ),
            (
                "near-candidates".to_string(),
                near_candidates_skipped_note_lines(usize::MAX),
            ),
        ];
        for scope in ALL_SCOPES {
            let label = scope_label(scope);
            renderings.push((
                format!("min-occurrences ({label})"),
                min_occurrences_note_lines(usize::MAX, usize::MAX, scope),
            ));
            renderings.push((
                format!("ignore-imports ({label})"),
                ignore_imports_note_lines(scope),
            ));
        }
        renderings
    }

    #[test]
    fn every_dupes_note_holds_under_eighty_columns_at_its_widest() {
        let widths: Vec<(String, Vec<usize>)> = widest_note_renderings()
            .into_iter()
            .map(|(name, lines)| {
                let columns = lines.iter().map(|line| line.chars().count()).collect();
                (name, columns)
            })
            .collect();

        // Pinned per line rather than bounded by one shared number: a line that
        // gains four columns has to fail here even when the 80-column assertion
        // below still passes because that line started narrow. The fixtures put
        // every unbounded count at its maximum, so these are the real ceilings.
        assert_eq!(
            widths,
            vec![
                ("default-ignore-route".to_string(), vec![76, 34]),
                ("default-ignore-breakdown".to_string(), vec![77, 46]),
                ("reviewed-clones".to_string(), vec![52, 61]),
                ("near-candidates".to_string(), vec![44, 79, 46]),
                ("min-occurrences (dupes)".to_string(), vec![64, 65]),
                ("ignore-imports (dupes)".to_string(), vec![40, 37]),
                ("min-occurrences (combined)".to_string(), vec![64, 71]),
                ("ignore-imports (combined)".to_string(), vec![40, 43]),
                ("min-occurrences (audit)".to_string(), vec![64, 73]),
                ("ignore-imports (audit)".to_string(), vec![40, 49]),
            ]
        );

        for (name, lines) in widest_note_renderings() {
            for line in lines {
                assert!(
                    line.chars().count() <= 80,
                    "the {name} note must hold under 80 columns: {} chars in {line:?}",
                    line.chars().count()
                );
            }
        }
    }

    #[test]
    fn near_candidates_skipped_note_pluralizes_and_gives_next_step() {
        assert_eq!(
            near_candidates_skipped_note_lines(1),
            vec![
                "warning: near-miss results may be incomplete".to_string(),
                "  skipped 1 candidate comparison to stay within work limits".to_string(),
                "  (narrow with --workspace or --changed-since)".to_string(),
            ]
        );
        assert_eq!(
            near_candidates_skipped_note_lines(2)[1],
            "  skipped 2 candidate comparisons to stay within work limits"
        );
        assert!(near_candidates_skipped_note_lines(0).is_empty());
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
        report.sort();

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
    fn apply_top_prefers_spread_when_base_scores_match() {
        let groups = vec![
            make_group(
                vec![
                    instance("/project/src/a.ts", 1, 10),
                    instance("/project/src/b.ts", 1, 10),
                ],
                100,
                10,
            ),
            make_group(
                vec![
                    instance("/project/packages/a/a.ts", 1, 10),
                    instance("/project/packages/b/b.ts", 1, 10),
                ],
                100,
                10,
            ),
        ];
        let mut report = make_report(groups, 4, 100);

        apply_top(&mut report, 1, Path::new("/project"));

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.clone_groups[0].spread(), 2);
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
    fn apply_top_keeps_all_four_stats_on_the_measured_corpus() {
        let groups = vec![
            make_group(vec![instance("a.ts", 1, 10); 5], 50, 10),
            make_group(vec![instance("b.ts", 1, 10); 3], 50, 10),
            make_group(vec![instance("c.ts", 1, 10); 2], 50, 10),
            make_group(vec![instance("d.ts", 1, 10); 2], 50, 10),
        ];
        let mut report = make_report(groups, 4, 100);
        report.stats.files_with_clones = 4;
        report.stats.duplication_percentage = 4.7268;
        report.sort();

        apply_top(&mut report, 1, Path::new("/project"));

        assert_eq!(report.clone_groups.len(), 1, "kept exactly one group");
        assert_eq!(
            report.clone_groups[0].instances.len(),
            5,
            "kept group is the 5-instance group"
        );
        assert_eq!(
            report.stats.clone_groups, 4,
            "stats.clone_groups stays on the corpus the run measured"
        );
        assert_eq!(
            report.stats.clone_instances, 12,
            "stats.clone_instances stays on the corpus the run measured"
        );
        assert_eq!(
            report.stats.files_with_clones, 4,
            "files_with_clones was never truncated and must stay corpus-wide"
        );
        assert!(
            (report.stats.duplication_percentage - 4.7268).abs() < f64::EPSILON,
            "duplication_percentage was never truncated and must stay corpus-wide"
        );
        assert_eq!(report.clone_groups_shown(), 1);
        assert_eq!(report.clone_groups_omitted(), 3);
    }

    /// `--top` rebuilds the families from the groups that survive the cap, so
    /// `clone_families[]` narrows exactly like `clone_groups[]`. The corpus
    /// family count has to survive that rebuild or the drop is unrecoverable:
    /// a consumer reading the array alone cannot tell three families from a
    /// project with three from a project with a hundred and sixty.
    #[test]
    fn apply_top_keeps_the_family_count_on_the_measured_corpus() {
        let groups = vec![
            make_group(
                vec![instance("a.ts", 1, 10), instance("b.ts", 1, 10)],
                50,
                10,
            ),
            make_group(
                vec![instance("a.ts", 30, 40), instance("b.ts", 30, 40)],
                50,
                10,
            ),
            make_group(
                vec![instance("c.ts", 1, 10), instance("d.ts", 1, 10)],
                40,
                8,
            ),
            make_group(
                vec![instance("c.ts", 30, 40), instance("d.ts", 30, 40)],
                40,
                8,
            ),
        ];
        let root = Path::new("/project");
        let mut report = make_report(groups, 4, 100);
        fallow_engine::duplicates::refresh_clone_families(&mut report, root);
        report.stats.clone_families = report.clone_families.len();
        assert_eq!(
            report.clone_families.len(),
            2,
            "the two file pairs must build two families"
        );

        apply_top(&mut report, 1, root);

        assert_eq!(
            report.stats.clone_families, 2,
            "stats.clone_families stays on the corpus the run measured"
        );
        assert_eq!(report.clone_families_shown(), report.clone_families.len());
        assert_eq!(
            report.clone_families_shown() + report.clone_families_omitted(),
            report.stats.clone_families,
            "shown plus omitted must reconstruct the corpus family count"
        );
        assert!(
            report.clone_families_omitted() > 0,
            "top 1 keeps one file pair, so it must withhold the other family"
        );
    }

    #[test]
    fn untruncated_report_omits_nothing() {
        let groups = vec![
            make_group(vec![instance("a.ts", 1, 10); 5], 50, 10),
            make_group(vec![instance("b.ts", 1, 10); 3], 50, 10),
        ];
        let report = make_report(groups, 2, 100);

        assert_eq!(report.clone_groups_shown(), 2);
        assert_eq!(report.clone_groups_omitted(), 0);
        assert_eq!(report.clone_families_omitted(), 0);
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
    fn build_config_merges_near_and_preserves_reviewed_clones() {
        let root = PathBuf::from("/project");
        let opts = default_opts_for_config(&root, DupesMode::Mild);
        let toml = DuplicatesConfig {
            near: true,
            ignored_clones: vec!["dup:12345678:2".to_string()],
            ..DuplicatesConfig::default()
        };

        let config = build_dupes_config(&opts, &toml);

        assert!(config.near);
        assert_eq!(config.ignored_clones, toml.ignored_clones);
    }

    #[test]
    fn build_config_cross_language_cli_true_overrides_toml_false() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.overrides.cross_language = true;
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
        opts.overrides.min_tokens = Some(100);
        opts.overrides.min_lines = Some(10);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert_eq!(config.min_tokens, 100);
        assert_eq!(config.min_lines, 10);
    }

    #[test]
    fn build_config_uses_cli_threshold() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.overrides.threshold = Some(7.5);
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!((config.threshold - 7.5).abs() < f64::EPSILON);
    }

    #[test]
    fn build_config_uses_cli_skip_local() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.overrides.skip_local = true;
        let toml = DuplicatesConfig::default();
        let config = build_dupes_config(&opts, &toml);
        assert!(config.skip_local);
    }

    #[test]
    fn build_config_falls_back_to_toml_min_lines_when_cli_unset() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        opts.overrides.min_lines = None;
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
        opts.overrides.min_tokens = None;
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
        opts.overrides.threshold = None;
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
        opts.overrides.mode = None;
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
        opts.overrides.min_lines = Some(3);
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
        opts.overrides.ignore_imports = Some(false);
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
        opts.overrides.ignore_imports = Some(true);
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
    fn build_config_ignore_symlinks_cli_overrides_config_both_ways() {
        let root = PathBuf::from("/project");
        let mut opts = default_opts_for_config(&root, DupesMode::Mild);
        let config_on = DuplicatesConfig {
            ignore_symlinks: true,
            ..DuplicatesConfig::default()
        };
        assert!(build_dupes_config(&opts, &config_on).ignore_symlinks);
        assert!(!build_dupes_config(&opts, &DuplicatesConfig::default()).ignore_symlinks);

        opts.overrides.ignore_symlinks = Some(false);
        assert!(
            !build_dupes_config(&opts, &config_on).ignore_symlinks,
            "--no-ignore-symlinks must win over config"
        );
        opts.overrides.ignore_symlinks = Some(true);
        assert!(
            build_dupes_config(&opts, &DuplicatesConfig::default()).ignore_symlinks,
            "--ignore-symlinks must win over config"
        );
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
        assert_eq!(stats.duplicated_tokens, 80);
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
        let baseline = result.unwrap();
        assert!(baseline.clone_groups.is_empty());
        assert!(baseline.clone_fingerprints.is_empty());
        assert_eq!(baseline.entry_count(), 0);
    }

    #[test]
    fn baseline_entry_count_prefers_fingerprints() {
        let root = Path::new("/project");
        let report = make_report(
            vec![make_group(
                vec![
                    instance("/project/src/a.ts", 1, 10),
                    instance("/project/src/b.ts", 1, 10),
                ],
                50,
                10,
            )],
            10,
            1000,
        );
        let baseline = DuplicationBaselineData::from_report(&report, root);
        assert_eq!(baseline.clone_fingerprints.len(), 1);
        assert_eq!(baseline.entry_count(), 1);
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
    fn recompute_stats_counts_redundant_token_copies() {
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
        report.stats.total_tokens = 500;
        let stats = recompute_stats(&report);
        assert_eq!(stats.duplicated_tokens, 80);
    }
}
