#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary produces intentional terminal output"
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod api;
#[cfg(test)]
mod architecture_boundaries;
mod audit;
mod audit_brief;
mod audit_cache_prune;
mod audit_decision_surface;
mod audit_focus;
mod audit_walkthrough;
mod base_worktree;
mod baselines;
/// Re-exported for integration tests so they hash reusable-cache roots through
/// the exact production path (`dunce` canonicalization + platform path-identity
/// bytes) rather than an approximation that diverges on Windows.
pub use base_worktree::canonical_root_hash;
mod walkthrough_state;
use fallow_engine::baseline;
mod agent_install;
mod baseline_gate;
mod baseline_growth;
mod cache_notice;
mod check;
mod ci;
mod ci_template;
mod claude_code_hint;
mod cli_agent;
mod cli_args;
mod cli_format;
mod cli_hooks;
mod cli_impact;
mod cli_production;
mod cli_report;
mod cli_startup;
pub use fallow_engine::codeowners;
mod combined;
mod config;
mod count_delta;
mod coverage;
mod discovery_note;
mod dispatch;
mod doctor;
mod dupes;
mod exit_codes;
pub mod explain;
mod fix;
mod flags;
mod flags_retirement_formats;
mod gates;
mod guard;
mod health;
mod impact;
mod init;
mod inspect;
mod json_style;
mod license;
mod list;
mod migrate;
mod mutation_cli;
mod mutation_human;
mod mutation_inspect;
mod mutation_progress;
mod onboarding;
#[cfg(test)]
mod output_envelope;
mod output_runtime;
mod path_util;
mod plugin_check;
mod process_clock;
mod rayon_pool;
mod regression;
pub mod report;
mod requests;
mod rule_pack;
mod runtime_support;
mod schema;
mod scope_path;
mod security;
mod security_help;
mod selector;
mod setup_hooks;
mod signal;
mod similar_code_cli;
mod similar_code_help;
mod suppressions;
mod task_matrix;
mod telemetry;
mod trace_chain;
mod trace_dependency;
mod trace_error;
mod trace_path;
mod type_aware_degrade;
mod update_check;
use fallow_engine::validate;
use fallow_engine::vital_signs;
mod cli_telemetry;
mod viz;
mod watch;
mod write_scope;

use cli_args::{AuditCacheCli, Cli, Command, CoverageCli, RulePackCli};
pub use cli_args::{AuditGateArg, BaselineModeArg, EffortFilter, EmailModeArg, HealthSeverityCli};
use dispatch::{
    ResolvedHealthCoverageInputs, dispatch_subcommand, map_telemetry_subcommand,
    resolve_health_coverage_inputs,
};
/// Structured error output for CLI and JSON formats.
pub(crate) mod error;
use cli_format::FormatConfig;
#[cfg(test)]
use cli_format::parse_format_arg;
use cli_impact::ImpactCli;
use cli_production::{ProductionModes, resolve_production_modes};
#[cfg(test)]
use cli_startup::build_tracing_filter;
use cli_startup::{
    bare_combined_baseline_subcommand_error_message, bare_coverage_subcommand_error_message,
    cli_bare_combined_baseline_flag, cli_has_bare_coverage_input, parse_cli_args,
    reject_global_baseline_flags, run_pre_dispatch_checks, setup_tracing, validate_inputs,
};
#[cfg(test)]
use cli_telemetry::TelemetryRun;
#[cfg(test)]
use cli_telemetry::{fallback_failure_reason_for, telemetry_workflow_for_command};
use cli_telemetry::{record_run_epilogue, start_telemetry_run};
use error::emit_error;
pub(crate) use runtime_support::{AnalysisKind, GroupBy};
pub(crate) use runtime_support::{
    ConfigLoadOptions, build_ownership_resolver, load_config_for_analysis,
};
#[cfg(test)]
use security_help::{SECURITY_UNSUPPORTED_GLOBAL_LONGS, SecurityHelpTarget};
use security_help::{render_security_help, security_help_target};
use similar_code_help::{render_similar_code_help, similar_code_help_target};

const DEFAULT_MIN_INVOCATIONS_HOT: u64 = 100;

/// Resolve an audit baseline path using CLI > config precedence.
///
/// Both sources resolve relative paths against the project root. This keeps
/// behavior consistent in CI scripts where `--root $REPO_ROOT` differs from
/// the process CWD.
fn resolve_audit_baseline_path(
    root: &std::path::Path,
    cli: Option<&std::path::Path>,
    config: Option<&str>,
) -> Option<PathBuf> {
    let path = cli.map(std::path::Path::to_path_buf).or_else(|| {
        config.map(|p| {
            let path = PathBuf::from(p);
            if path_util::is_absolute_path_any_platform(&path) {
                path
            } else {
                root.join(path)
            }
        })
    })?;
    if path_util::is_absolute_path_any_platform(&path) {
        Some(path)
    } else {
        Some(root.join(path))
    }
}

fn emit_known_failure(
    message: &str,
    exit_code: u8,
    output: fallow_config::OutputFormat,
    reason: telemetry::FailureReason,
) -> ExitCode {
    telemetry::note_failure_reason(reason);
    emit_error(message, exit_code, output)
}

fn emit_known_failure_with_style(
    message: &str,
    exit_code: u8,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
    reason: telemetry::FailureReason,
) -> ExitCode {
    telemetry::note_failure_reason(reason);
    error::emit_error_with_style(message, exit_code, output, json_style)
}

fn unsupported_security_global(cli: &Cli) -> Option<&'static str> {
    if cli.baseline.is_some() {
        Some("--baseline")
    } else if cli.save_baseline.is_some() {
        Some("--save-baseline")
    } else if cli.fail_on_stale_baseline {
        Some("--fail-on-stale-baseline")
    } else if cli.fail_on_baseline_growth {
        Some("--fail-on-baseline-growth")
    } else if cli.baseline_base.is_some() {
        Some("--baseline-base")
    } else if cli.fail_on_parse_error {
        Some("--fail-on-parse-error")
    } else if cli.production {
        Some("--production")
    } else if cli.no_production {
        Some("--no-production")
    } else if cli.no_package_baselines {
        Some("--no-package-baselines")
    } else if cli.group_by.is_some() {
        Some("--group-by")
    } else if cli.performance {
        Some("--performance")
    } else if cli.explain_skipped {
        Some("--explain-skipped")
    } else if cli.fail_on_regression {
        Some("--fail-on-regression")
    } else if cli.regression_baseline.is_some() {
        Some("--regression-baseline")
    } else if cli.save_regression_baseline.is_some() {
        Some("--save-regression-baseline")
    } else if let Some(flag) = cli.dupes_overrides().first_global_flag() {
        Some(flag)
    } else if cli.include_entry_exports {
        Some("--include-entry-exports")
    } else if cli.show_cascade {
        Some("--show-cascade")
    } else {
        None
    }
}

struct DispatchContext<'a> {
    cli: &'a Cli,
    root: &'a std::path::Path,
    output: fallow_config::OutputFormat,
    quiet: bool,
    fail_on_issues: bool,
    json_style: json_style::JsonStyle,
    threads: usize,
    tolerance: regression::Tolerance,
    save_regression_file: Option<&'a std::path::PathBuf>,
    save_to_config: bool,
}

impl DispatchContext<'_> {
    fn production_modes(
        &self,
        dead_code: bool,
        health: bool,
        dupes: bool,
    ) -> Result<ProductionModes, ExitCode> {
        resolve_production_modes(self.cli, self.root, self.output, dead_code, health, dupes)
    }

    fn production_for(
        &self,
        analysis: fallow_config::ProductionAnalysis,
    ) -> Result<bool, ExitCode> {
        self.production_modes(false, false, false)
            .map(|modes| modes.for_analysis(analysis))
    }

    fn growth_flags(&self) -> baseline_growth::GrowthFlags<'_> {
        baseline_growth::GrowthFlags {
            enabled: self.cli.fail_on_baseline_growth,
            base: self.cli.baseline_base.as_deref(),
            changed_since: self.cli.changed_since.as_deref(),
        }
    }

    fn growth_ctx(
        &self,
        owner: baseline_growth::GrowthOwner,
    ) -> baseline_growth::GrowthContext<'_> {
        baseline_growth::GrowthContext {
            root: self.root,
            flags: self.growth_flags(),
            owner,
            output: self.output,
            json_style: self.json_style,
        }
    }

    fn regression_opts(&self, scoped: bool) -> regression::RegressionOpts<'_> {
        regression::RegressionOpts {
            fail_on_regression: self.cli.fail_on_regression,
            tolerance: self.tolerance,
            regression_baseline_file: self.cli.regression_baseline.as_deref(),
            save_target: if let Some(path) = self.save_regression_file {
                regression::SaveRegressionTarget::File(path)
            } else if self.save_to_config {
                regression::SaveRegressionTarget::Config
            } else {
                regression::SaveRegressionTarget::None
            },
            scoped,
            quiet: self.quiet,
            output: self.output,
        }
    }
}

/// Test-only helper invoked when `FALLOW_TEST_SIGNAL_HELPER=1` is set.
/// Spawns `sleep 30` via the `ScopedChild` registry so the child is
/// tracked by the signal handler, prints the child PID to stdout, then
/// busy-waits so a SIGINT/SIGTERM delivered to the parent fires the
/// signal handler (which kills the child and exits 128+signum).
///
/// When `FALLOW_TEST_SIGNAL_HELPER_GRACEFUL=1` is also set, graceful
/// mode is activated BEFORE spawning the child. In graceful mode the
/// signal handler kills the child (proving drain runs unconditionally)
/// but does NOT call `std::process::exit`, so the helper itself sees
/// `wait_with_output` return and exits 0. This is the path the
/// integration test asserts: graceful drain + clean exit.
///
/// With `FALLOW_TEST_SIGNAL_HELPER=panic`, a thread panics after the PID
/// line, so the test can assert that the panic hook kills the child. Lives in
/// `main.rs` (not tests/) because clap is already parsed below and we
/// need to intercept before that.
#[cfg(unix)]
fn signal_test_helper() -> ExitCode {
    use std::io::Write as _;
    use std::process::Command;

    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER_GRACEFUL").is_some() {
        signal::set_graceful_mode();
    }

    let mut command = Command::new("sleep");
    command.arg("30");
    let child = match signal::ScopedChild::spawn(&mut command) {
        Ok(c) => c,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "spawn sleep failed: {err}");
            return ExitCode::from(2);
        }
    };
    let pid = child.id();
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{pid}");
    let _ = lock.flush();
    drop(lock);
    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER").is_some_and(|value| value == "panic") {
        // A panic on another thread leaves this thread and its `Drop` alive,
        // so only the panic hook can end the child.
        let _ = std::thread::spawn(|| panic!("signal helper panic")).join();
    }
    let _ = child.wait_with_output();
    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER_GRACEFUL").is_some() {
        return ExitCode::SUCCESS;
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    ExitCode::SUCCESS
}

#[cfg(not(unix))]
fn signal_test_helper() -> ExitCode {
    ExitCode::from(2)
}

fn install_spawn_hooks() {
    fallow_engine::churn::set_spawn_hook(signal::scoped_child::output);
    fallow_engine::changed_files::set_spawn_hook(signal::scoped_child::output);
}

fn install_signal_handlers() {
    if let Err(err) = signal::install_handlers() {
        use std::io::Write as _;
        let stderr = std::io::stderr();
        let mut lock = stderr.lock();
        let _ = writeln!(lock, "fallow: failed to install signal handlers: {err}");
    }
}

/// Open `path` (creating parent dirs, truncating) and redirect report output
/// there via the ambient sink, forcing color off so the file carries no ANSI
/// codes even when attached to a TTY. Returns the error exit code if the file
/// cannot be created. Backs `--output-file`.
fn redirect_report_to_file(
    path: &std::path::Path,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    match fallow_engine::write_guard::create_file(
        path,
        fallow_engine::write_guard::WriteTarget::Path,
    ) {
        Ok(file) => {
            report::sink::set_file_sink(file);
            colored::control::set_override(false);
            Ok(())
        }
        Err(e) if e.is_directory() => Err(emit_error(
            &format!(
                "failed to create the directory of {} for --output-file: {e}",
                path.display()
            ),
            2,
            output,
        )),
        Err(e) => Err(emit_error(
            &format!("failed to open {} for --output-file: {e}", path.display()),
            2,
            output,
        )),
    }
}

/// Flush the report file after rendering and print the stderr confirmation
/// (suppressed by `--quiet`). Returns the error exit code on a write failure.
fn finalize_report_file(
    path: &std::path::Path,
    quiet: bool,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    if let Err(e) = report::sink::flush() {
        return Err(emit_error(
            &format!("failed to write {}: {e}", path.display()),
            2,
            output,
        ));
    }
    // Suppress the confirmation when nothing was rendered to the file (a command
    // that errored before producing output sends its error to stdout, not the
    // file), so we never claim "Report written" over an empty file.
    if !quiet && report::sink::wrote() {
        eprintln!("Report written to {}", path.display());
    }
    Ok(())
}

/// Run the full fallow CLI: parse argv, dispatch the selected command, and
/// return the process exit code. This is the crate's single entry point; the
/// `fallow` binary and the multicall `fallow-multicall` binary both delegate
/// here so there is exactly one clap tree and one dispatch path.
///
/// A stdout write error other than a closed pipe changes the exit code to 2,
/// so that a cut-off report never looks like a successful run.
pub fn run() -> ExitCode {
    report::sink::finish_stdout(run_command())
}

fn run_command() -> ExitCode {
    process_clock::mark_process_start();
    install_signal_handlers();
    signal::install_panic_drain();
    install_spawn_hooks();

    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER").is_some() {
        return signal_test_helper();
    }

    let (mut cli, fmt) = match parse_cli_args() {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if cli.pretty && !fmt.payload_is_json {
        eprintln!(
            "Error: --pretty requires JSON output. Use --format json --pretty, or remove --pretty."
        );
        return ExitCode::from(2);
    }

    if let Some(code) = reject_global_baseline_flags(&cli, &fmt) {
        return code;
    }

    if let Some(code) = run_schema_command_if_requested(&cli, fmt.json_style) {
        return code;
    }

    if let Some(code) = run_telemetry_command_if_requested(&mut cli, fmt.output, fmt.json_style) {
        return code;
    }
    if let Some(code) = run_doctor_command_if_requested(&cli, &fmt) {
        return code;
    }
    if is_impact_statusline(&cli) {
        let (root, _) = match validate_inputs(&cli, fmt.output, fmt.json_style) {
            Ok(validated) => validated,
            Err(code) => return code,
        };
        return cli_impact::render_impact_statusline(&root);
    }
    let telemetry_run = start_telemetry_run(&cli, &fmt);

    let (root, threads) = match validate_inputs(&cli, fmt.output, fmt.json_style) {
        Ok(v) => v,
        Err(code) => {
            return record_run_epilogue(telemetry_run, code, None, cli.parent_run.as_deref(), None);
        }
    };

    let FormatConfig {
        output,
        payload_is_json: _,
        quiet,
        fail_on_issues,
        json_style,
    } = fmt;

    let tolerance =
        match run_pre_dispatch_checks(&cli, &root, output, json_style, quiet, telemetry_run) {
            Ok(tolerance) => tolerance,
            Err(code) => return code,
        };

    if let Some(note) = write_scope::default_cache_dir_note(&cli, &root) {
        eprintln!("{note}");
        cli.no_cache = true;
    }

    let (save_regression_file, save_to_config) = regression_save_targets(&cli);

    // The agent and hooks commands manage the Claude Code setup themselves.
    // A plugin hint after `agent uninstall` would ask the user to install
    // again at once, so these commands never write the hint.
    let hint_root = (!matches!(
        cli.command,
        Some(Command::Agent { .. } | Command::Hooks { .. } | Command::SetupHooks { .. })
    ))
    .then_some(root.as_path());
    let command = cli.command.take();
    process_clock::record_startup();
    let dispatch = DispatchContext {
        cli: &cli,
        root: &root,
        output,
        quiet,
        fail_on_issues,
        json_style,
        threads,
        tolerance,
        save_regression_file: save_regression_file.as_ref(),
        save_to_config,
    };
    let exit_code = match dispatch_and_finalize(&dispatch, command) {
        Ok(code) => code,
        Err(code) => return code,
    };
    record_run_epilogue(
        telemetry_run,
        exit_code,
        None,
        cli.parent_run.as_deref(),
        hint_root,
    )
}

/// Benchmark hook for the production fix dry-run pipeline. This is not a
/// supported API. Rendered output is disabled so the benchmark measures
/// analysis and fix planning rather than terminal I/O.
#[doc(hidden)]
pub fn benchmark_fix_dry_run(root: &Path, threads: usize) -> (ExitCode, usize) {
    let config_path = None;
    fix::run_fix_with_count(&fix::FixOptions {
        root,
        config_path: &config_path,
        output: fallow_config::OutputFormat::Json,
        json_style: json_style::JsonStyle::Compact,
        no_cache: true,
        threads,
        quiet: true,
        emit_output: false,
        allow_remote_extends: false,
        dry_run: true,
        yes: false,
        production: false,
        no_create_config: true,
        type_aware: None,
        type_aware_projects: &[],
        type_aware_require: None,
        scope: None,
        show_cascade: false,
    })
}

/// Owned production-analysis corpus for the stable audit review benchmark.
/// This is not a supported API.
#[doc(hidden)]
pub use audit::AuditReviewBenchmarkCorpus;

/// Build audit analysis and preload external review inputs outside the timed
/// benchmark. This is not a supported API.
#[doc(hidden)]
pub fn create_audit_review_benchmark_corpus(
    root: &Path,
    changed_files: &[PathBuf],
    threads: usize,
) -> Result<AuditReviewBenchmarkCorpus, ExitCode> {
    audit::create_audit_review_benchmark_corpus(root, changed_files, threads)
}

/// Benchmark production audit review assembly and compact tagged JSON without
/// git, cache, worktree, or file I/O. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_audit_review_brief_many_changed_files_json(
    corpus: &mut AuditReviewBenchmarkCorpus,
) -> (ExitCode, usize, usize, usize, usize, usize) {
    match audit::benchmark_audit_review_brief_many_changed_files_json(corpus) {
        Ok(result) => (
            ExitCode::SUCCESS,
            result.introduced_count,
            result.inherited_count,
            result.public_api_added_count,
            result.decision_count,
            result.rendered_bytes,
        ),
        Err(code) => (code, 0, 0, 0, 0, 0),
    }
}

/// Finding counts of the audit analyses benchmark. This is not a supported API.
#[doc(hidden)]
pub use audit::AuditAnalysesBenchmarkResult;

/// Benchmark the HEAD and base analyses of one audit over two prepared trees,
/// without git. This is not a supported API.
///
/// # Errors
///
/// Returns the exit code of the failed analysis.
#[doc(hidden)]
pub fn benchmark_audit_analyses(
    head_root: &Path,
    base_root: &Path,
    changed_files: &[PathBuf],
    threads: usize,
) -> Result<AuditAnalysesBenchmarkResult, ExitCode> {
    audit::benchmark_audit_analyses(head_root, base_root, changed_files, threads)
}

#[doc(hidden)]
pub use inspect::InspectBenchmarkCorpus;

/// Build the child-response corpus outside the timed inspect benchmark. This
/// is not a supported API.
#[doc(hidden)]
pub fn create_inspect_benchmark_corpus(root: &Path, threads: usize) -> InspectBenchmarkCorpus {
    inspect::create_inspect_benchmark_corpus(root, threads)
}

/// Benchmark file inspect orchestration and compact tagged JSON rendering
/// without process startup. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_inspect_file_evidence_bundle_json(
    root: &Path,
    threads: usize,
    corpus: &InspectBenchmarkCorpus,
) -> (ExitCode, usize, usize) {
    match inspect::benchmark_inspect_file_evidence_bundle_json(root, threads, corpus) {
        Ok((child_call_count, rendered_bytes)) => {
            (ExitCode::SUCCESS, child_call_count, rendered_bytes)
        }
        Err(_) => (ExitCode::from(2), 0, 0),
    }
}

/// Benchmark hook for the production dead-code analysis and compact JSON
/// rendering pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_dead_code_json(root: &Path, threads: usize) -> (ExitCode, usize, usize) {
    match check::benchmark_dead_code_json(root, threads) {
        Ok((issue_count, rendered_bytes)) => (ExitCode::SUCCESS, issue_count, rendered_bytes),
        Err(code) => (code, 0, 0),
    }
}

/// Benchmark hook for the production security analysis and JSON rendering
/// pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_security_json(root: &Path, threads: usize) -> (ExitCode, usize, usize) {
    match security::benchmark_security_json(root, threads) {
        Ok((finding_count, rendered_bytes)) => (ExitCode::SUCCESS, finding_count, rendered_bytes),
        Err(code) => (code, 0, 0),
    }
}

#[doc(hidden)]
pub use security::{SecurityBlindSpotsBenchmarkResult, SecuritySurvivorsBenchmarkCorpus};

/// Build the explicit candidate and verifier inputs outside the timed
/// survivors benchmark. This is not a supported API.
#[doc(hidden)]
pub fn create_security_survivors_benchmark_corpus(
    root: &Path,
    threads: usize,
) -> Result<SecuritySurvivorsBenchmarkCorpus, ExitCode> {
    security::create_security_survivors_benchmark_corpus(root, threads)
}

/// Benchmark the production survivors loaders, candidate/verdict join, and
/// compact JSON serializer. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_security_survivors_json(
    corpus: &SecuritySurvivorsBenchmarkCorpus,
) -> (ExitCode, usize, usize, usize, usize, usize) {
    match security::benchmark_security_survivors_json(corpus) {
        Ok((survivors, dismissed, needs_human_review, unverdicted, rendered_bytes)) => (
            ExitCode::SUCCESS,
            survivors,
            dismissed,
            needs_human_review,
            unverdicted,
            rendered_bytes,
        ),
        Err(_) => (ExitCode::from(2), 0, 0, 0, 0, 0),
    }
}

/// Benchmark unresolved-callee normalization, blind-spot grouping, and compact
/// JSON serialization without project I/O. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_security_blind_spots_json(
    root: &Path,
    diagnostics: &[fallow_types::results::SecurityUnresolvedCalleeDiagnostic],
) -> SecurityBlindSpotsBenchmarkResult {
    security::benchmark_security_blind_spots_json(root, diagnostics)
}

/// Benchmark hook for the production list inventory and JSON rendering
/// pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_list_json(root: &Path, threads: usize) -> (ExitCode, usize, usize, usize, usize) {
    match list::benchmark_list_json(root, threads) {
        Ok((file_count, entry_point_count, workspace_count, rendered_bytes)) => (
            ExitCode::SUCCESS,
            file_count,
            entry_point_count,
            workspace_count,
            rendered_bytes,
        ),
        Err(code) => (code, 0, 0, 0, 0),
    }
}

/// Benchmark hook for the production boundaries listing and compact JSON
/// rendering pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_list_boundaries_json(
    root: &Path,
    threads: usize,
) -> (ExitCode, usize, usize, usize, usize) {
    match list::benchmark_list_boundaries_json(root, threads) {
        Ok((zone_count, rule_count, matched_file_count, rendered_bytes)) => (
            ExitCode::SUCCESS,
            zone_count,
            rule_count,
            matched_file_count,
            rendered_bytes,
        ),
        Err(code) => (code, 0, 0, 0, 0),
    }
}

/// Opaque deterministic global matcher for the watch-filter benchmark. This is
/// not a supported API.
#[doc(hidden)]
pub use watch::WatchFilterBenchmarkGlobalGitignore;

/// Build a deterministic global gitignore matcher outside the timed watch
/// benchmark. This is not a supported API.
#[doc(hidden)]
pub fn create_watch_filter_benchmark_global_gitignore() -> WatchFilterBenchmarkGlobalGitignore {
    watch::create_benchmark_global_gitignore()
}

/// Benchmark hook for production watch-filter initialization and project
/// gitignore discovery. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_watch_filter_initialization(
    config: &fallow_config::ResolvedConfig,
    global_gitignore: &WatchFilterBenchmarkGlobalGitignore,
) -> (usize, usize) {
    watch::benchmark_filter_initialization(config, global_gitignore)
}

/// Benchmark hook for the production Viz analysis, payload, and HTML
/// rendering pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_viz_html(root: &Path, threads: usize) -> (ExitCode, usize, usize, usize) {
    match viz::benchmark_viz_html(root, threads) {
        Ok((file_count, edge_count, rendered_bytes)) => {
            (ExitCode::SUCCESS, file_count, edge_count, rendered_bytes)
        }
        Err(code) => (code, 0, 0, 0),
    }
}

/// Benchmark hook for the production rule-pack analysis and JSON rendering
/// pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_rule_pack_test_json(root: &Path, threads: usize) -> (ExitCode, usize, usize) {
    match rule_pack::benchmark_rule_pack_test_json(root, threads) {
        Ok((finding_count, rendered_bytes)) => (ExitCode::SUCCESS, finding_count, rendered_bytes),
        Err(code) => (code, 0, 0),
    }
}

/// Benchmark hook for the production recommendation discovery and compact JSON
/// rendering pipeline. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_recommend_json(root: &Path) -> (ExitCode, usize, usize, bool, usize) {
    match onboarding::benchmark_recommend_json(root) {
        Ok((decision_count, framework_count, heterogeneous, rendered_bytes)) => (
            ExitCode::SUCCESS,
            decision_count,
            framework_count,
            heterogeneous,
            rendered_bytes,
        ),
        Err(_) => (ExitCode::from(2), 0, 0, false, 0),
    }
}

/// Benchmark hook for local runtime coverage analysis and compact JSON
/// rendering with an in-process sidecar response. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_runtime_coverage_analyze_json(
    root: &Path,
    runtime_coverage_path: &Path,
    response_bytes: &[u8],
    threads: usize,
) -> (ExitCode, usize, usize, usize, String) {
    match coverage::benchmark_local_json(root, runtime_coverage_path, response_bytes, threads) {
        Ok((finding_count, hot_path_count, request_bytes, rendered)) => (
            ExitCode::SUCCESS,
            finding_count,
            hot_path_count,
            request_bytes,
            rendered,
        ),
        Err(code) => (code, 0, 0, 0, String::new()),
    }
}

/// Status bars refresh frequently, so their local read path bypasses telemetry,
/// update checks, notices, and every other command epilogue.
fn is_impact_statusline(cli: &Cli) -> bool {
    matches!(
        cli.command.as_ref(),
        Some(Command::Impact {
            subcommand: Some(ImpactCli::Statusline),
            all: false,
            ..
        })
    )
}

/// Redirect the rendered report to `--output-file` (ambient sink), dispatch the
/// command, then flush+close the report file. Returns the dispatch exit code, or
/// `Err` carrying a redirect/finalize failure code for `main` to return directly.
fn dispatch_and_finalize(
    dispatch: &DispatchContext<'_>,
    command: Option<Command>,
) -> Result<ExitCode, ExitCode> {
    let cli = dispatch.cli;
    let output = dispatch.output;
    let quiet = dispatch.quiet;

    // Set up the report-file sink before dispatch so rendering lands in the file;
    // progress and the confirmation stay on stderr. `fallow mutation` opens the
    // file itself after the run, so the file never makes the worktree dirty.
    let deferred_output = matches!(command, Some(Command::Mutation { .. }));
    let output_file = cli.output_file.as_deref().filter(|_| !deferred_output);
    if let Some(path) = output_file
        && let Err(code) = redirect_report_to_file(path, output)
    {
        return Err(code);
    }

    let exit_code = if command.is_some() && cli_has_bare_coverage_input(cli) {
        emit_error(bare_coverage_subcommand_error_message(), 2, output)
    } else if command.is_some()
        && let Some(flag) = cli_bare_combined_baseline_flag(cli)
    {
        emit_error(
            &bare_combined_baseline_subcommand_error_message(flag),
            2,
            output,
        )
    } else {
        match command {
            None => dispatch_bare_command(dispatch),
            Some(cmd) => dispatch_subcommand(cmd, dispatch),
        }
    };

    if let Some(path) = output_file
        && let Err(code) = finalize_report_file(path, quiet, output)
    {
        return Err(code);
    }
    Ok(exit_code)
}

fn run_telemetry_command_if_requested(
    cli: &mut Cli,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
) -> Option<ExitCode> {
    if matches!(cli.command, Some(Command::Telemetry { .. }))
        && let Some(Command::Telemetry { subcommand }) = cli.command.take()
    {
        return Some(telemetry::run(
            map_telemetry_subcommand(subcommand),
            output,
            json_style,
        ));
    }
    None
}

/// Doctor deliberately bypasses the normal run epilogue: collecting readiness
/// must not write telemetry, flush a spool, perform an update check, or create
/// caches. Root validation stays inside the typed report so failures still
/// return the complete doctor envelope.
fn run_doctor_command_if_requested(cli: &Cli, format: &FormatConfig) -> Option<ExitCode> {
    if !matches!(cli.command, Some(Command::Doctor)) {
        return None;
    }

    if let Some(flag) = unsupported_doctor_option(cli) {
        let message = if flag == "--output-file" {
            "--output-file is not valid with `fallow doctor`; doctor is read-only and writes its report to stdout".to_string()
        } else {
            format!("{flag} is not valid with `fallow doctor`.")
        };
        return Some(crate::error::emit_error_with_style(
            &message,
            2,
            format.output,
            format.json_style,
        ));
    }
    if let Err(code) = doctor::validate_output(format.output, format.json_style) {
        return Some(code);
    }

    let root = cli.root.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });
    let config_path = cli.config.as_ref().map(|path| {
        if path_util::is_absolute_path_any_platform(path) {
            path.clone()
        } else {
            root.join(path)
        }
    });
    let report = doctor::collect_report(&root, config_path.as_deref());
    Some(doctor::render_report(
        &report,
        format.output,
        format.json_style,
    ))
}

/// Doctor accepts only project selection and presentation options. Rejecting
/// analysis flags avoids implying that a readiness check ran an analysis or
/// applied a gate that it deliberately bypasses.
fn unsupported_doctor_option(cli: &Cli) -> Option<&'static str> {
    let dupes_flag = cli.dupes_overrides().first_global_flag();
    [
        (cli.allow_remote_extends, "--allow-remote-extends"),
        (cli.no_cache, "--no-cache"),
        (cli.threads.is_some(), "--threads"),
        (cli.changed_since.is_some(), "--changed-since"),
        (cli.diff_file.is_some(), "--diff-file"),
        (cli.diff_stdin, "--diff-stdin"),
        (cli.churn_file.is_some(), "--churn-file"),
        (cli.max_file_size.is_some(), "--max-file-size"),
        (cli.baseline.is_some(), "--baseline"),
        (cli.baseline_mode.is_some(), "--baseline-mode"),
        (cli.parent_run.is_some(), "--parent-run"),
        (cli.save_baseline.is_some(), "--save-baseline"),
        (cli.production, "--production"),
        (cli.no_production, "--no-production"),
        (cli.no_package_baselines, "--no-package-baselines"),
        (cli.production_dead_code, "--production-dead-code"),
        (cli.production_health, "--production-health"),
        (cli.production_dupes, "--production-dupes"),
        (cli.workspace.is_some(), "--workspace"),
        (cli.changed_workspaces.is_some(), "--changed-workspaces"),
        (cli.group_by.is_some(), "--group-by"),
        (cli.performance, "--performance"),
        (cli.explain, "--explain"),
        (cli.explain_skipped, "--explain-skipped"),
        (cli.summary, "--summary"),
        (cli.ci, "--ci"),
        (cli.fail_on_issues, "--fail-on-issues"),
        (cli.sarif_file.is_some(), "--sarif-file"),
        (cli.output_file.is_some(), "--output-file"),
        (cli.report_path_prefix.is_some(), "--report-path-prefix"),
        (cli.fail_on_regression, "--fail-on-regression"),
        (cli.fail_on_stale_baseline, "--fail-on-stale-baseline"),
        (cli.fail_on_baseline_growth, "--fail-on-baseline-growth"),
        (cli.baseline_base.is_some(), "--baseline-base"),
        (cli.fail_on_parse_error, "--fail-on-parse-error"),
        (cli.tolerance != "0", "--tolerance"),
        (cli.regression_baseline.is_some(), "--regression-baseline"),
        (
            cli.save_regression_baseline.is_some(),
            "--save-regression-baseline",
        ),
        (!cli.only.is_empty(), "--only"),
        (!cli.skip.is_empty(), "--skip"),
        (dupes_flag.is_some(), dupes_flag.unwrap_or_default()),
        (cli.score, "--score"),
        (cli.trend, "--trend"),
        (cli.trend_from.is_some(), "--trend-from"),
        (cli.save_snapshot.is_some(), "--save-snapshot"),
        (cli.coverage.is_some(), "--coverage"),
        (cli.coverage_root.is_some(), "--coverage-root"),
        (cli.dupes_baseline.is_some(), "--dupes-baseline"),
        (cli.health_baseline.is_some(), "--health-baseline"),
        (cli.include_entry_exports, "--include-entry-exports"),
        (cli.show_cascade, "--show-cascade"),
        (cli.type_aware, "--type-aware"),
        (cli.no_type_aware, "--no-type-aware"),
        (!cli.type_aware_project.is_empty(), "--type-aware-project"),
        (cli.type_aware_require.is_some(), "--type-aware-require"),
    ]
    .into_iter()
    .find_map(|(used, flag)| used.then_some(flag))
}

fn run_schema_command_if_requested(
    cli: &Cli,
    json_style: json_style::JsonStyle,
) -> Option<ExitCode> {
    match cli.command {
        Some(Command::Schema) => Some(schema::run_schema(json_style)),
        Some(Command::ConfigSchema) => Some(init::run_config_schema(json_style)),
        Some(Command::PluginSchema) => Some(init::run_plugin_schema(json_style)),
        Some(Command::RulePackSchema) => Some(init::run_rule_pack_schema(json_style)),
        _ => None,
    }
}

fn regression_save_targets(cli: &Cli) -> (Option<std::path::PathBuf>, bool) {
    let save_file = cli.save_regression_baseline.as_ref().and_then(|opt| {
        opt.as_ref()
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from)
    });
    let save_to_config = cli.save_regression_baseline.is_some() && save_file.is_none();
    (save_file, save_to_config)
}

fn dispatch_bare_command(dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let (run_check, run_dupes, run_health) = combined::resolve_analyses(&cli.only, &cli.skip);
    if !(run_check || run_dupes || run_health) {
        return emit_error(
            "--skip leaves no analysis to run: pass at most two of dead-code, dupes and health",
            2,
            dispatch.output,
        );
    }
    let production = match dispatch.production_modes(
        cli.production_dead_code,
        cli.production_health,
        cli.production_dupes,
    ) {
        Ok(production) => production,
        Err(code) => return code,
    };
    // Coverage only feeds health scoring, and resolving it validates the
    // winning root. A bare run that excludes health (`--only check`,
    // `--skip health`) must neither load config for coverage nor reject a
    // relative `health.coverageRoot` it never reads.
    let coverage_inputs = if run_health {
        match resolve_health_coverage_inputs(
            dispatch,
            cli.coverage.as_deref(),
            cli.coverage_root.as_deref(),
        ) {
            Ok(inputs) => inputs,
            Err(code) => return code,
        }
    } else {
        ResolvedHealthCoverageInputs::default()
    };
    run_bare_combined(
        dispatch,
        production,
        &coverage_inputs,
        BareAnalyses {
            run_check,
            run_dupes,
            run_health,
        },
    )
}

/// Which analyses the bare `fallow` run executes (resolved from `--only`/`--skip`).
#[derive(Clone, Copy)]
struct BareAnalyses {
    run_check: bool,
    run_dupes: bool,
    run_health: bool,
}

/// Build `CombinedOptions` for a bare `fallow` invocation and run the combined
/// pipeline.
fn run_bare_combined(
    dispatch: &DispatchContext<'_>,
    production: ProductionModes,
    coverage_inputs: &ResolvedHealthCoverageInputs,
    analyses: BareAnalyses,
) -> ExitCode {
    let cli = dispatch.cli;
    let output = dispatch.output;
    if cli.fail_on_parse_error && !analyses.run_check && !analyses.run_health {
        return error::emit_error_with_style(
            "--fail-on-parse-error needs the dead-code or health analysis, and this run analyzes neither. Include dead-code or health in --only or --skip, or remove the flag.",
            2,
            output,
            dispatch.json_style,
        );
    }
    let scope = match crate::scope_path::resolve_command_scope(
        dispatch.root,
        dispatch.output,
        cli.path.clone(),
    ) {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };
    let growth_targets = baseline_growth::targets(&[
        (
            cli.baseline.as_deref().filter(|_| analyses.run_check),
            fallow_engine::baseline::BaselineKind::DeadCode,
        ),
        (
            cli.dupes_baseline.as_deref().filter(|_| analyses.run_dupes),
            fallow_engine::baseline::BaselineKind::Dupes,
        ),
        (
            cli.health_baseline
                .as_deref()
                .filter(|_| analyses.run_health),
            fallow_engine::baseline::BaselineKind::Health,
        ),
    ]);
    baseline_growth::run_gated(
        &dispatch.growth_ctx(baseline_growth::GrowthOwner::Combined),
        &growth_targets,
        || run_combined_scoped(dispatch, production, coverage_inputs, analyses, scope),
    )
}

fn run_combined_scoped(
    dispatch: &DispatchContext<'_>,
    production: ProductionModes,
    coverage_inputs: &ResolvedHealthCoverageInputs,
    analyses: BareAnalyses,
    scope: Option<PathBuf>,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let scoped_run = scope.is_some();
    let architecture = combined::resolve_architecture_selection(&cli.only, &cli.skip);
    // A narrowed dead-code section saves narrowed regression counts.
    let narrowed_report = architecture != check::ArchitectureSelection::All;
    combined::run_combined(&combined::CombinedOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        no_package_baselines: cli.no_package_baselines,
        churn_file: cli.churn_file.as_deref(),
        baseline: cli.baseline.as_deref(),
        save_baseline: cli.save_baseline.as_deref(),
        dupes_baseline: cli.dupes_baseline.as_deref(),
        health_baseline: cli.health_baseline.as_deref(),
        health_baseline_mode: cli.baseline_mode.unwrap_or_default().into(),
        health_baseline_mode_explicit: cli.baseline_mode.is_some(),
        fail_on_stale_baseline: cli.fail_on_stale_baseline,
        production: cli.production,
        production_dead_code: Some(production.dead_code),
        production_health: Some(production.health),
        production_dupes: Some(production.dupes),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        group_by: cli.group_by,
        type_aware: cli.type_aware_override(),
        type_aware_projects: &cli.type_aware_project,
        type_aware_require: cli.type_aware_require.map(Into::into),
        explain: cli.explain,
        explain_skipped: cli.explain_skipped,
        performance: cli.performance,
        summary: cli.summary,
        run_check: analyses.run_check,
        run_dupes: analyses.run_dupes,
        run_health: analyses.run_health,
        architecture,
        dupes: cli.dupes_overrides(),
        score: cli.score || cli.trend || cli.trend_from.is_some(),
        trend: cli.trend || cli.trend_from.is_some(),
        trend_from: cli.trend_from.as_deref(),
        save_snapshot: cli.save_snapshot.as_ref(),
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        include_entry_exports: cli.include_entry_exports,
        show_cascade: cli.show_cascade,
        fail_on_parse_error: cli.fail_on_parse_error,
        scope,
        regression_opts: dispatch.regression_opts(
            cli.changed_since.is_some()
                || cli.workspace.is_some()
                || cli.changed_workspaces.is_some()
                || scoped_run
                || narrowed_report,
        ),
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli_args::TOP_LEVEL_AFTER_LONG_HELP;
    use crate::cli_format::Format;
    use crate::dispatch::{
        architecture_issue_filters, check_issue_filters, type_aware_status_output,
    };

    /// Validates that the CLI definition has no flag name collisions, missing
    /// fields, or other structural errors. Catches issues like a global alias
    /// `--base` colliding with a subcommand's `--base` flag.
    #[test]
    fn cli_definition_has_no_flag_collisions() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn impact_statusline_subcommand_parses() {
        use clap::Parser;

        let cli = Cli::try_parse_from(["fallow", "impact", "statusline"]).expect("argv parses");
        assert!(matches!(
            cli.command,
            Some(Command::Impact {
                subcommand: Some(ImpactCli::Statusline),
                ..
            })
        ));
    }

    #[test]
    fn impact_statusline_bypasses_command_epilogue() {
        use clap::Parser;

        let statusline =
            Cli::try_parse_from(["fallow", "impact", "statusline"]).expect("argv parses");
        assert!(is_impact_statusline(&statusline));

        let status = Cli::try_parse_from(["fallow", "impact", "status"]).expect("argv parses");
        assert!(!is_impact_statusline(&status));

        let all_statusline =
            Cli::try_parse_from(["fallow", "impact", "--all", "statusline"]).expect("argv parses");
        assert!(!is_impact_statusline(&all_statusline));
    }

    #[test]
    fn regression_baseline_help_explains_the_default_destination() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("Omit PATH to update regression.baseline"));
        assert!(help.contains("discovered fallow config"));
        assert!(help.contains("create .fallowrc.json when none exists"));
    }

    /// The root `--help` cheat sheet is a static const that cannot call the
    /// shared renderer, so this test is the only guard that it stays in sync
    /// with `TASK_MATRIX`. Every row's command string must appear verbatim.
    #[test]
    fn after_help_lists_every_task_matrix_command() {
        for row in crate::task_matrix::TASK_MATRIX {
            assert!(
                TOP_LEVEL_AFTER_LONG_HELP.contains(row.command),
                "root --help cheat sheet is missing task-matrix command '{}'; \
                 update the top_level_task_cheat_sheet! fragment to match TASK_MATRIX",
                row.command
            );
        }
    }

    /// The curated command groups replace clap's auto-generated subcommand
    /// list, so a new subcommand stays invisible in `fallow --help` unless it
    /// is added here. Substring matching is not enough (e.g. `--trace`
    /// contains `trace`), so each name must lead a group line. The full
    /// `--help` surface is the one that must stay complete; `-h` is the
    /// curated progressive subset.
    #[test]
    fn after_help_lists_every_visible_subcommand() {
        use clap::CommandFactory;

        for sub in Cli::command().get_subcommands() {
            if sub.is_hide_set() {
                continue;
            }
            let name = sub.get_name();
            let listed = TOP_LEVEL_AFTER_LONG_HELP
                .lines()
                .any(|line| line.split_whitespace().next() == Some(name));
            assert!(
                listed,
                "root --help command list is missing subcommand '{name}'; \
                 add it to a top_level_*_command_groups! section"
            );
        }
    }

    /// A new visible subcommand must say whether it writes an analysis report.
    /// A row in `COMMAND_ENVELOPES` binds it to the schema, `report --from`,
    /// MCP and drift tests; the other list records why it has no row.
    #[test]
    fn every_visible_subcommand_is_classified_for_its_machine_contract() {
        use clap::CommandFactory;
        use fallow_types::command_surfaces::{
            COMMAND_ENVELOPES, COMMANDS_WITHOUT_ANALYSIS_ENVELOPE,
        };

        let visible: Vec<String> = Cli::command()
            .get_subcommands()
            .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
            .map(|sub| sub.get_name().to_string())
            .collect();
        for name in &visible {
            let rows = COMMAND_ENVELOPES
                .iter()
                .filter(|row| row.command == name)
                .count();
            let others = COMMANDS_WITHOUT_ANALYSIS_ENVELOPE
                .iter()
                .filter(|(command, _)| command == name)
                .count();
            assert_eq!(
                rows + others,
                1,
                "subcommand `{name}` must be in exactly one of COMMAND_ENVELOPES and \
                 COMMANDS_WITHOUT_ANALYSIS_ENVELOPE (crates/types/src/command_surfaces.rs)"
            );
        }
        let listed = COMMAND_ENVELOPES.iter().map(|row| row.command).chain(
            COMMANDS_WITHOUT_ANALYSIS_ENVELOPE
                .iter()
                .map(|(command, _)| *command),
        );
        for command in listed {
            assert!(
                visible.iter().any(|name| name == command),
                "`{command}` is listed in command_surfaces.rs but is not a visible subcommand"
            );
        }
    }

    /// `fallow architecture` selects the same issue types as the deprecated
    /// `dead-code` structure flags, so the two produce the same report.
    #[test]
    fn architecture_flags_map_to_the_dead_code_structure_filters() {
        use clap::Parser;

        let structure = |argv: &[&str]| {
            let cli = Cli::try_parse_from(argv).expect("argv parses");
            check_issue_filters(&cli.command.expect("subcommand"))
        };
        let architecture =
            |cycles, boundaries, policy| architecture_issue_filters(cycles, boundaries, policy);
        assert_eq!(
            architecture(false, false, false),
            structure(&[
                "fallow",
                "dead-code",
                "--circular-deps",
                "--re-export-cycles",
                "--package-cycles",
                "--boundary-violations",
                "--policy-violations",
            ])
        );
        assert_eq!(
            architecture(true, false, false),
            structure(&[
                "fallow",
                "dead-code",
                "--circular-deps",
                "--re-export-cycles",
                "--package-cycles",
            ])
        );
        assert_eq!(
            architecture(false, true, false),
            structure(&["fallow", "dead-code", "--boundary-violations"])
        );
        assert_eq!(
            architecture(false, false, true),
            structure(&["fallow", "dead-code", "--policy-violations"])
        );
        assert!(
            Cli::try_parse_from([
                "fallow",
                "architecture",
                "--cycles",
                "--boundaries",
                "--policy"
            ])
            .is_ok()
        );
    }

    /// `-h` is the progressive entry point: it must stay scannable (ecosystem
    /// norm is 40-80 lines) while leading with the task cheat sheet and
    /// closing with the pointer to the complete `--help` surface.
    #[test]
    fn short_help_stays_scannable_with_cheat_sheet_and_pointer() {
        use clap::CommandFactory;

        let help = Cli::command().render_help().to_string();
        let lines = help.lines().count();
        assert!(
            lines < 90,
            "root -h grew to {lines} lines; keep the short surface under 90 \
             (curate hide_short_help and the short after-help instead)"
        );
        assert!(help.contains("When the agent is about to..."));
        assert!(help.contains("Run fallow --help for the complete command list."));
    }

    /// The high-value and coarse admin commands each get a distinct telemetry
    /// workflow instead of the `Unknown` catch-all, so command families stay
    /// answerable without uploading raw command lines.
    #[test]
    fn high_value_commands_route_to_distinct_workflows() {
        use clap::Parser;
        use fallow_config::OutputFormat;

        let distinct = [
            (vec!["fallow", "impact"], telemetry::Workflow::Impact),
            (vec!["fallow", "security"], telemetry::Workflow::Security),
            (vec!["fallow", "fix"], telemetry::Workflow::Fix),
            (
                vec!["fallow", "explain", "unused-exports"],
                telemetry::Workflow::Explain,
            ),
            (
                vec!["fallow", "watch"],
                telemetry::Workflow::CodeQualityReview,
            ),
            (
                vec!["fallow", "list"],
                telemetry::Workflow::ProjectInventory,
            ),
            (
                vec!["fallow", "workspaces"],
                telemetry::Workflow::ProjectInventory,
            ),
            (
                vec!["fallow", "schema"],
                telemetry::Workflow::ProjectInventory,
            ),
            (vec!["fallow", "init"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "hooks", "install", "--target", "git"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "config-schema"], telemetry::Workflow::Setup),
            (vec!["fallow", "plugin-schema"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "rule-pack-schema"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "config"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "ci-template", "gitlab"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "migrate"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "telemetry", "status"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "setup-hooks"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "audit-cache", "remove", "--root", "."],
                telemetry::Workflow::Setup,
            ),
            (
                vec!["fallow", "license", "status"],
                telemetry::Workflow::License,
            ),
            (
                vec!["fallow", "architecture", "--cycles"],
                telemetry::Workflow::Architecture,
            ),
        ];
        for (argv, expected) in distinct {
            let cli = Cli::try_parse_from(&argv).expect("argv parses");
            assert_eq!(
                telemetry_workflow_for_command(cli.command.as_ref(), OutputFormat::Json),
                expected,
                "{argv:?} should map to {expected:?}"
            );
        }
    }

    /// `-v`, `-V`, and `--version` must all trigger clap's Version action so
    /// the version prints regardless of which spelling the user reaches for
    /// (issue #916). clap surfaces a Version action from `try_get_matches_from`
    /// as the `DisplayVersion` error kind.
    #[test]
    fn version_flag_accepts_lower_v_upper_v_and_long() {
        use clap::CommandFactory;
        for argv in [["fallow", "-v"], ["fallow", "-V"], ["fallow", "--version"]] {
            let err = Cli::command()
                .try_get_matches_from(argv)
                .expect_err("version flag should short-circuit parsing");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::DisplayVersion,
                "{argv:?} should trigger the Version action"
            );
        }
    }

    /// Guard against deferred-work wording leaking into clap-rendered help.
    /// `stub`, `placeholder`, and `not yet` framings tell users the feature
    /// is broken or pending; they belong in tracked issues, not in `--help`.
    /// Walk every (sub)command and assert each rendered long-help is clean.
    #[test]
    fn cli_help_text_contains_no_implementation_status_wording() {
        use clap::CommandFactory;
        let mut root = Cli::command();
        let mut violations: Vec<(String, String)> = Vec::new();
        visit_help(&mut root, "fallow", &mut violations);
        assert!(
            violations.is_empty(),
            "found implementation-status wording in --help output:\n{}",
            violations
                .iter()
                .map(|(cmd, line)| format!("  {cmd}: {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn dependency_override_help_is_package_manager_neutral() {
        use clap::CommandFactory;
        let help = Cli::command()
            .find_subcommand_mut("dead-code")
            .expect("dead-code command")
            .render_long_help()
            .to_string();

        assert!(help.contains("Only report unused package-manager dependency overrides"));
        assert!(help.contains("Only report misconfigured package-manager dependency overrides"));
        assert!(!help.contains("unused pnpm dependency overrides"));
        assert!(!help.contains("misconfigured pnpm dependency overrides"));
    }

    #[test]
    fn top_level_help_groups_commands_by_workflow() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        let expected_order = [
            "Analysis:",
            "  dead-code",
            "  dupes",
            "  health",
            "  flags",
            "  security",
            "  audit",
            "Workflow:",
            "  watch",
            "  fix",
            "Project inspection:",
            "  list",
            "  workspaces",
            "  explain",
            "  impact",
            "  viz",
            "Setup and configuration:",
            "  init",
            "  recommend",
            "  migrate",
            "  config",
            "  config-schema",
            "  plugin-schema",
            "  plugin-check",
            "  rule-pack-schema",
            "Automation and CI:",
            "  ci",
            "  ci-template",
            "  hooks",
            "  setup-hooks",
            "Runtime coverage:",
            "  coverage",
            "  license",
            "Reference:",
            "  schema",
            "  help",
            "Options:",
        ];
        let mut cursor = 0;
        for needle in expected_order {
            let Some(offset) = help[cursor..].find(needle) else {
                panic!("top-level help missing `{needle}` after byte {cursor}:\n{help}");
            };
            cursor += offset + needle.len();
        }
    }

    /// Every global `--dupes-*` override flag only applies to a duplication
    /// run. `security`, `doctor` and `similar-code` must reject each one, and
    /// the security help must hide each one.
    #[test]
    fn every_global_dupes_override_is_rejected_where_dupes_does_not_run() {
        let longs: Vec<String> = <Cli as clap::CommandFactory>::command()
            .get_arguments()
            .filter_map(|arg| arg.get_long())
            .filter(|long| long.starts_with("dupes-") && *long != "dupes-baseline")
            .map(str::to_owned)
            .collect();
        assert!(
            longs.len() > 1,
            "expected global --dupes-* flags, got {longs:?}"
        );
        let security_help = render_security_help(SecurityHelpTarget::Parent);

        for long in &longs {
            let flag = format!("--{long}");
            let takes_value = <Cli as clap::CommandFactory>::command()
                .get_arguments()
                .find(|arg| arg.get_long() == Some(long.as_str()))
                .is_some_and(|arg| arg.get_action().takes_values());
            let value = if long == "dupes-mode" { "weak" } else { "3" };
            let argv_for = |command: &[&str]| {
                let mut argv = vec!["fallow"];
                argv.extend_from_slice(command);
                argv.push(flag.as_str());
                if takes_value {
                    argv.push(value);
                }
                Cli::try_parse_from(argv).expect("global dupes flag parses")
            };

            let security = argv_for(&["security"]);
            assert_eq!(unsupported_security_global(&security), Some(flag.as_str()));
            let doctor = argv_for(&["doctor"]);
            assert_eq!(unsupported_doctor_option(&doctor), Some(flag.as_str()));
            let similar_code = argv_for(&["similar-code", "status"]);
            assert_eq!(
                similar_code_help::unsupported_similar_code_option(&similar_code),
                Some(flag.as_str())
            );
            assert!(
                !help_contains_long_flag(&security_help, long),
                "security help must hide unsupported {flag}:\n{security_help}"
            );
        }
    }

    #[test]
    fn security_help_hides_globals_rejected_by_security_validator() {
        let help = render_security_help(SecurityHelpTarget::Parent);

        for long in SECURITY_UNSUPPORTED_GLOBAL_LONGS {
            assert!(
                !help_contains_long_flag(&help, long),
                "security help must hide unsupported --{long}:\n{help}"
            );
        }

        for long in [
            "root",
            "config",
            "format",
            "quiet",
            "no-cache",
            "threads",
            "changed-since",
            "diff-file",
            "diff-stdin",
            "workspace",
            "changed-workspaces",
            "ci",
            "fail-on-issues",
            "sarif-file",
            "summary",
            "output-file",
            "max-file-size",
            "explain",
            "surface",
        ] {
            assert!(
                help_contains_long_flag(&help, long),
                "security help must keep supported --{long}:\n{help}"
            );
        }
    }

    #[test]
    fn security_help_detection_covers_subcommand_and_help_alias_forms() {
        assert_eq!(
            security_help_target(["security", "--help"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["security", "-h"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["--format", "json", "security", "--help"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["help", "security"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["security", "survivors", "--help"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["security", "survivors", "-h"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["help", "security", "survivors"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["security", "blind-spots", "--help"]),
            Some(SecurityHelpTarget::BlindSpots)
        );
        assert_eq!(
            security_help_target(["help", "security", "blind-spots"]),
            Some(SecurityHelpTarget::BlindSpots)
        );
        assert_eq!(security_help_target(["health", "--help"]), None);
        assert_eq!(security_help_target(["help", "health"]), None);
    }

    #[test]
    fn security_unsupported_global_validator_matches_hidden_help_contract() {
        for (argv, expected) in [
            (vec!["fallow", "security", "--performance"], "--performance"),
            (
                vec!["fallow", "security", "--no-package-baselines"],
                "--no-package-baselines",
            ),
            (
                vec!["fallow", "security", "--baseline", "base.json"],
                "--baseline",
            ),
            (
                vec!["fallow", "security", "--fail-on-stale-baseline"],
                "--fail-on-stale-baseline",
            ),
            (
                vec!["fallow", "security", "--dupes-mode", "weak"],
                "--dupes-mode",
            ),
            (vec!["fallow", "security", "--dupes-near"], "--dupes-near"),
        ] {
            let cli = Cli::try_parse_from(argv).expect("security global parses before validation");
            assert_eq!(unsupported_security_global(&cli), Some(expected));
        }

        let explain = Cli::try_parse_from(["fallow", "security", "--explain"])
            .expect("security --explain parses");
        assert_eq!(unsupported_security_global(&explain), None);
    }

    #[test]
    fn programmatic_common_options_track_analysis_affecting_cli_globals() {
        use clap::CommandFactory;

        let cli_flags: std::collections::BTreeSet<String> = Cli::command()
            .get_arguments()
            .filter(|arg| arg.is_global_set())
            .filter_map(|arg| arg.get_long().map(str::to_owned))
            .filter(|name| {
                matches!(
                    name.as_str(),
                    "root"
                        | "config"
                        | "allow-remote-extends"
                        | "no-cache"
                        | "threads"
                        | "changed-since"
                        | "diff-file"
                        | "production"
                        | "workspace"
                        | "changed-workspaces"
                        | "explain"
                )
            })
            .collect();
        let programmatic_flags: std::collections::BTreeSet<String> =
            fallow_api::COMMON_ANALYSIS_OPTION_FLAGS
                .iter()
                .map(|flag| (*flag).to_owned())
                .collect();

        assert_eq!(programmatic_flags, cli_flags);
    }

    #[test]
    fn dead_code_registry_filter_flags_are_exposed_by_clap() {
        use clap::CommandFactory;

        let cli = Cli::command();
        let dead_code = cli
            .get_subcommands()
            .find(|command| command.get_name() == "dead-code")
            .expect("dead-code subcommand is registered");
        let cli_flags: std::collections::BTreeSet<String> = dead_code
            .get_arguments()
            .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
            .collect();

        for flag in fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS.iter() {
            assert!(
                cli_flags.contains(*flag),
                "registry filter flag {flag} is missing from dead-code clap args"
            );
        }
    }

    fn help_contains_long_flag(help: &str, long: &str) -> bool {
        let flag = format!("--{long}");
        help.split(|c: char| c.is_whitespace() || c == ',' || c == '[' || c == ']')
            .any(|token| token == flag)
    }

    fn visit_help(cmd: &mut clap::Command, path: &str, violations: &mut Vec<(String, String)>) {
        let help = cmd.render_long_help().to_string();
        for line in scan_forbidden(&help) {
            violations.push((path.to_owned(), line));
        }
        let names: Vec<String> = cmd
            .get_subcommands()
            .map(|sub| sub.get_name().to_owned())
            .collect();
        for name in names {
            if name == "help" {
                continue;
            }
            if let Some(sub) = cmd.find_subcommand_mut(&name) {
                let sub_path = format!("{path} {name}");
                visit_help(sub, &sub_path, violations);
            }
        }
    }

    fn scan_forbidden(s: &str) -> Vec<String> {
        let lower = s.to_ascii_lowercase();
        let mut out = Vec::new();
        for word in ["stub", "placeholder"] {
            if let Some(idx) = find_whole_word(&lower, word) {
                out.push(extract_line(s, idx));
            }
        }
        if let Some(idx) = lower.find("not yet") {
            out.push(extract_line(s, idx));
        }
        out
    }

    fn find_whole_word(haystack: &str, word: &str) -> Option<usize> {
        let bytes = haystack.as_bytes();
        let mut start = 0;
        while let Some(rel) = haystack[start..].find(word) {
            let abs = start + rel;
            let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
            let after_idx = abs + word.len();
            let after_ok = after_idx >= bytes.len() || !bytes[after_idx].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(abs);
            }
            start = abs + word.len();
        }
        None
    }

    fn extract_line(s: &str, byte_idx: usize) -> String {
        let line_start = s[..byte_idx].rfind('\n').map_or(0, |i| i + 1);
        let line_end = s[byte_idx..].find('\n').map_or(s.len(), |i| byte_idx + i);
        s[line_start..line_end].trim().to_owned()
    }

    #[test]
    fn emit_error_returns_given_exit_code() {
        let code = emit_error("test error", 2, fallow_config::OutputFormat::Human);
        assert_eq!(code, ExitCode::from(2));
    }

    fn telemetry_run_for_mode(mode: telemetry::AnalysisMode) -> TelemetryRun {
        TelemetryRun {
            workflow: telemetry::Workflow::Health,
            output: fallow_config::OutputFormat::Json,
            quiet: true,
            start: std::time::Instant::now(),
            context: telemetry::WorkflowContext {
                run_scope: telemetry::RunScope::FullProject,
                config_shape: telemetry::ConfigShape::Default,
                output_destination: telemetry::OutputDestination::Stdout,
                analysis_mode: mode,
            },
        }
    }

    #[test]
    fn fallback_failure_reason_skips_success_and_findings() {
        let run = telemetry_run_for_mode(telemetry::AnalysisMode::Static);

        assert_eq!(fallback_failure_reason_for(&run, ExitCode::SUCCESS), None);
        assert_eq!(fallback_failure_reason_for(&run, ExitCode::from(1)), None);
    }

    #[test]
    fn fallback_failure_reason_classifies_network_auth_and_analysis() {
        let static_run = telemetry_run_for_mode(telemetry::AnalysisMode::Static);
        let cloud_run = telemetry_run_for_mode(telemetry::AnalysisMode::ProductionCoverage);

        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(api::NETWORK_EXIT_CODE)),
            Some(telemetry::FailureReason::Network),
        );
        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(12)),
            Some(telemetry::FailureReason::Auth),
        );
        assert_eq!(
            fallback_failure_reason_for(&cloud_run, ExitCode::from(3)),
            Some(telemetry::FailureReason::Auth),
        );
        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(2)),
            Some(telemetry::FailureReason::Analysis),
        );
    }

    #[test]
    fn bare_coverage_flags_parse_without_subcommand() {
        let cli = Cli::try_parse_from([
            "fallow",
            "--coverage",
            "coverage/coverage-final.json",
            "--coverage-root",
            "/ci/workspace",
        ])
        .expect("bare combined coverage flags should parse");
        assert!(cli.command.is_none());
        assert_eq!(
            cli.coverage.as_deref(),
            Some(std::path::Path::new("coverage/coverage-final.json"))
        );
        assert_eq!(
            cli.coverage_root.as_deref(),
            Some(std::path::Path::new("/ci/workspace"))
        );
    }

    #[test]
    fn bare_coverage_before_subcommand_is_detectable() {
        let cli = Cli::try_parse_from([
            "fallow",
            "--coverage",
            "coverage/coverage-final.json",
            "dead-code",
        ])
        .expect("clap should parse pre-subcommand bare coverage for custom rejection");
        assert!(cli.command.is_some());
        assert!(cli_has_bare_coverage_input(&cli));
        let message = bare_coverage_subcommand_error_message();
        assert!(message.contains("bare combined-mode flags"));
        assert!(message.contains("fallow health --coverage <coverage-final.json>"));
    }

    #[test]
    fn bare_combined_baseline_before_subcommand_is_detectable() {
        for flag in ["--dupes-baseline", "--health-baseline"] {
            let cli = Cli::try_parse_from(["fallow", flag, "x.json", "dead-code"])
                .expect("clap should parse a pre-subcommand combined baseline");
            assert!(cli.command.is_some());
            assert_eq!(cli_bare_combined_baseline_flag(&cli), Some(flag));
            let message = bare_combined_baseline_subcommand_error_message(flag);
            assert!(message.contains(flag));
            assert!(message.contains("omit the subcommand"));
        }
        let bare = Cli::try_parse_from(["fallow", "--dupes-baseline", "x.json"])
            .expect("bare combined baseline should parse");
        assert!(bare.command.is_none());
    }

    #[test]
    fn subcommand_coverage_flag_keeps_regular_clap_error() {
        let Err(err) = Cli::try_parse_from(["fallow", "dead-code", "--coverage"]) else {
            panic!("dead-code --coverage should fail to parse");
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn type_aware_flags_parse_for_semantic_analysis() {
        let cli = Cli::try_parse_from([
            "fallow",
            "dead-code",
            "--unused-class-members",
            "--type-aware",
            "--type-aware-project",
            "tsconfig.json",
            "--type-aware-project",
            "packages/web/tsconfig.json",
        ])
        .expect("type-aware flag should parse");
        assert!(cli.type_aware);
        assert_eq!(
            cli.type_aware_project,
            [
                PathBuf::from("tsconfig.json"),
                PathBuf::from("packages/web/tsconfig.json")
            ]
        );
        let Some(Command::Check {
            unused_class_members,
            ..
        }) = cli.command
        else {
            panic!("dead-code should parse as the check command");
        };
        assert!(unused_class_members);
    }

    #[test]
    fn no_type_aware_conflicts_with_type_aware() {
        let Err(err) = Cli::try_parse_from(["fallow", "audit", "--type-aware", "--no-type-aware"])
        else {
            panic!("--no-type-aware must conflict with --type-aware");
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn no_type_aware_forces_semantic_analysis_off() {
        let cli = Cli::try_parse_from(["fallow", "audit", "--no-type-aware"])
            .expect("--no-type-aware should parse on audit");
        assert_eq!(cli.type_aware_override(), Some(false));

        let cli = Cli::try_parse_from(["fallow", "dead-code", "--type-aware"])
            .expect("--type-aware should parse");
        assert_eq!(cli.type_aware_override(), Some(true));

        let cli = Cli::try_parse_from(["fallow", "dead-code"]).expect("bare command should parse");
        assert_eq!(cli.type_aware_override(), None);
    }

    #[test]
    fn type_aware_status_output_hides_host_paths() {
        let root = Path::new("/private/work/project");
        let output = type_aware_status_output(
            root,
            fallow_api::TypeAwareStatus {
                available: false,
                discovery_source: Some("environment-override"),
                companion_path: Some(PathBuf::from("/private/tools/fallow-type-aware")),
                package_version: None,
                protocol_version: 7,
                backend_family: None,
                backend_version: None,
                remediation: Some(
                    "failed to launch /private/tools/fallow-type-aware from /private/work/project"
                        .to_string(),
                ),
            },
        );

        assert_eq!(
            output.schema_version.0,
            fallow_output::TYPE_AWARE_STATUS_SCHEMA_VERSION
        );
        assert_eq!(output.companion_path.as_deref(), Some("fallow-type-aware"));
        let remediation = output.remediation.expect("remediation");
        assert!(!remediation.contains("/private/"));
        assert!(remediation.contains("fallow-type-aware"));
    }

    #[test]
    fn format_parsing_covers_all_variants() {
        assert!(matches!(parse_format_arg("json"), Some(Format::Json)));
        assert!(matches!(parse_format_arg("JSON"), Some(Format::Json)));
        assert!(matches!(parse_format_arg("human"), Some(Format::Human)));
        assert!(matches!(parse_format_arg("sarif"), Some(Format::Sarif)));
        assert!(matches!(parse_format_arg("compact"), Some(Format::Compact)));
        assert!(matches!(
            parse_format_arg("markdown"),
            Some(Format::Markdown)
        ));
        assert!(matches!(parse_format_arg("md"), Some(Format::Markdown)));
        assert!(matches!(
            parse_format_arg("codeclimate"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("gitlab-codequality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("gitlab-code-quality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("pr-comment-github"),
            Some(Format::PrCommentGithub)
        ));
        assert!(matches!(
            parse_format_arg("pr-comment-gitlab"),
            Some(Format::PrCommentGitlab)
        ));
        assert!(matches!(
            parse_format_arg("review-github"),
            Some(Format::ReviewGithub)
        ));
        assert!(matches!(
            parse_format_arg("review-gitlab"),
            Some(Format::ReviewGitlab)
        ));
        assert!(matches!(parse_format_arg("badge"), Some(Format::Badge)));
        assert!(parse_format_arg("xml").is_none());
        assert!(parse_format_arg("").is_none());
    }

    #[test]
    fn quiet_parsing_logic() {
        let parse = |s: &str| -> bool { s == "1" || s.eq_ignore_ascii_case("true") };
        assert!(parse("1"));
        assert!(parse("true"));
        assert!(parse("TRUE"));
        assert!(parse("True"));
        assert!(!parse("0"));
        assert!(!parse("false"));
        assert!(!parse("yes"));
    }

    #[test]
    fn tracing_filter_defaults_to_warn_without_env() {
        let filter = build_tracing_filter(None);
        assert!(filter.would_enable("fallow_core", &tracing::Level::WARN));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::INFO));
    }

    #[test]
    fn tracing_filter_respects_explicit_env_directives() {
        let filter = build_tracing_filter(Some("info"));
        assert!(filter.would_enable("fallow_core", &tracing::Level::INFO));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::DEBUG));

        let filter = build_tracing_filter(Some("fallow=debug"));
        assert!(filter.would_enable("fallow_core::lib", &tracing::Level::DEBUG));
        assert!(!filter.would_enable("oxc_resolver", &tracing::Level::ERROR));
    }

    #[test]
    fn tracing_filter_skips_a_directive_that_it_cannot_read() {
        let filter = build_tracing_filter(Some("fallow[span{id=1}]=trace,fallow_graph=debug,warn"));
        assert!(filter.would_enable("fallow_graph", &tracing::Level::DEBUG));
        assert!(filter.would_enable("oxc_resolver", &tracing::Level::WARN));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::TRACE));
    }

    #[test]
    fn tracing_filter_treats_empty_env_as_off() {
        for value in ["", "   "] {
            let filter = build_tracing_filter(Some(value));
            assert!(!filter.would_enable("fallow_core", &tracing::Level::ERROR));
        }
    }
}
