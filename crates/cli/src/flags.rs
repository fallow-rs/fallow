//! `fallow flags` subcommand: detect and report feature flag patterns.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_engine::clock::AnalysisClock;
use fallow_engine::flag_age::{FlagAgeRequest, PickaxeProgress, apply_flag_ages};
use fallow_engine::flag_retirement::{
    RetirementFacts, RetirementOptions, RetirementSiteInput, RetirementSort, aggregate_flags,
    finish_report, max_age_gate,
};
use fallow_engine::flag_vendor::{
    STALE_EXPORT_DAYS, VendorExport, VendorMatch, apply_vendor_state,
};
use fallow_output::codeclimate_fingerprint_hash;
use fallow_types::flag_retirement::{
    FlagAgeMode, FlagRetirementReport, RetirementFlag, RetirementFlagKind, RetirementReason,
    RetirementVendorState,
};
use fallow_types::results::{FeatureFlag, FlagKind};
use rustc_hash::FxHashSet;

use crate::error::emit_error;
use crate::regression::{
    FlagsCounts, RegressionOpts, SaveRegressionTarget, compare_flags_regression,
    print_flags_regression, save_flags_regression_baseline,
};

/// Options for the `fallow flags` subcommand.
pub struct FlagsOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    pub production: bool,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub changed_since: Option<&'a str>,
    pub explain: bool,
    pub top: Option<usize>,
    /// Retirement report options; `None` without `--retirement`.
    pub retirement: Option<RetirementArgs>,
    /// Regression gate options. The gate works only with `--retirement`.
    pub regression: crate::regression::RegressionOpts<'a>,
    /// The first regression option on the command line, for the warning
    /// that a run without `--retirement` ignores it.
    pub regression_flag: Option<&'static str>,
}

/// Options of `fallow flags --retirement`.
pub struct RetirementArgs {
    /// Keep only rows with one of these reasons.
    pub reasons: Vec<RetirementReasonArg>,
    /// Row order.
    pub sort: RetirementSortArg,
    /// How to measure flag age.
    pub flag_age: FlagAgeArg,
    /// Keep only rows at least this many days old.
    pub min_age: Option<u64>,
    /// Vendor flag export to read, from `--flag-state`.
    pub flag_state: Option<std::path::PathBuf>,
    /// Fail when a flag in scope is older than this many days.
    pub max_flag_age: Option<u64>,
}

/// CLI mirror of [`FlagAgeMode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum FlagAgeArg {
    /// `git blame` of the flag sites. The age is a lower bound.
    Blame,
    /// `git log -S` per flag name. Slower; gives the first commit.
    Pickaxe,
    /// No age.
    Off,
}

impl From<FlagAgeArg> for FlagAgeMode {
    fn from(value: FlagAgeArg) -> Self {
        match value {
            FlagAgeArg::Blame => Self::Blame,
            FlagAgeArg::Pickaxe => Self::Pickaxe,
            FlagAgeArg::Off => Self::Off,
        }
    }
}

/// CLI mirror of [`RetirementReason`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RetirementReasonArg {
    /// The flag has exactly one read site.
    SingleReadSite,
    /// Every read site is in a test, story or mock file.
    TestOnly,
    /// The flag is a `const` bound to a literal and used as a guard.
    LiteralConstant,
    /// Both branches of the guard are the same code.
    IdenticalBranches,
    /// No branch of the guard holds code, so the flag does nothing.
    EmptyBranch,
    /// The guarded block holds unused exports.
    GuardsDeadCode,
    /// The flag is defined, but no code reads it.
    DefinedNeverRead,
    /// The vendor export says the flag is rolled out or serves one variation.
    FullyRolledOut,
    /// The vendor export says the flag is archived.
    ArchivedInVendor,
    /// The code reads the flag, but the vendor export does not hold its key.
    MissingInVendor,
    /// The vendor export holds the flag, but no code reads it.
    VendorOnly,
}

impl From<RetirementReasonArg> for RetirementReason {
    fn from(value: RetirementReasonArg) -> Self {
        match value {
            RetirementReasonArg::SingleReadSite => Self::SingleReadSite,
            RetirementReasonArg::TestOnly => Self::TestOnly,
            RetirementReasonArg::LiteralConstant => Self::LiteralConstant,
            RetirementReasonArg::IdenticalBranches => Self::IdenticalBranches,
            RetirementReasonArg::EmptyBranch => Self::EmptyBranch,
            RetirementReasonArg::GuardsDeadCode => Self::GuardsDeadCode,
            RetirementReasonArg::DefinedNeverRead => Self::DefinedNeverRead,
            RetirementReasonArg::FullyRolledOut => Self::FullyRolledOut,
            RetirementReasonArg::ArchivedInVendor => Self::ArchivedInVendor,
            RetirementReasonArg::MissingInVendor => Self::MissingInVendor,
            RetirementReasonArg::VendorOnly => Self::VendorOnly,
        }
    }
}

/// CLI mirror of [`RetirementSort`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RetirementSortArg {
    /// Oldest flag first; flags without an age come last.
    Age,
    /// Fewest read sites first.
    Sites,
    /// Flag name, ascending.
    Name,
}

impl From<RetirementSortArg> for RetirementSort {
    fn from(value: RetirementSortArg) -> Self {
        match value {
            RetirementSortArg::Age => Self::Age,
            RetirementSortArg::Sites => Self::Sites,
            RetirementSortArg::Name => Self::Name,
        }
    }
}

/// Run the `fallow flags` subcommand.
pub fn run_flags(opts: &FlagsOptions<'_>) -> ExitCode {
    let start = Instant::now();
    if let Err(code) = validate_retirement_args(opts) {
        return code;
    }
    let vendor_export = match load_vendor_export(opts) {
        Ok(export) => export,
        Err(code) => return code,
    };

    let config = match load_flags_config(opts) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let session = match fallow_engine::session::AnalysisSession::from_resolved_config(config) {
        Ok(session) => session,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };
    let scan = if opts.retirement.is_some() {
        fallow_engine::flags::analyze_feature_flags_for_retirement(&session)
    } else {
        fallow_engine::flags::analyze_feature_flags_with_session(&session)
            .map(|analysis| (analysis, RetirementFacts::default()))
    };
    let (analysis, retirement_facts) = match scan {
        Ok(scan) => scan,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };
    if analysis.files_scanned == 0 {
        return emit_error("no files discovered", 2, opts.output);
    }

    let scope = match resolve_flag_scope(opts) {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    // The retirement report counts the reads outside the scope too.
    let all_flags = if opts.retirement.is_some() {
        analysis.flags.clone()
    } else {
        Vec::new()
    };
    let mut flags = analysis.flags;
    flags.retain(|flag| scope.contains(&flag.path));
    crate::requests::measure_changed_since_scope(session.files());
    // Note find-state for telemetry before any exit (issue #1650 follow-up): the
    // flags command emits a `code_quality_review` workflow event (the same label
    // as combined `fallow`), so without this its findings_present serialized as
    // null. Count the scope-filtered flags BEFORE `--top` truncation so the
    // bucket reflects the full set, not the displayed head.
    crate::telemetry::note_result_count(flags.len());
    if let Err(code) = validate_flags_output(opts.output) {
        return code;
    }
    // The report groups every site in scope, so it reads the flags before
    // `--top` truncates the per-site list.
    let retirement = opts.retirement.as_ref().map(|args| {
        build_retirement_report(
            RetirementInput {
                sites: retirement_facts.sites_for(&all_flags),
                in_scope: &|path| scope.contains(path),
                whole_project: scope.is_whole_project(),
                vendor_export: vendor_export.as_ref(),
            },
            &session,
            args,
            opts,
        )
    });
    let mut retirement = retirement;
    let gate_failed = match &mut retirement {
        Some((report, _)) => {
            match run_regression_gate(report, flags.len(), &scope, opts) {
                Ok(()) => {}
                Err(code) => return code,
            }
            gate_failed(report)
        }
        None => {
            warn_on_ignored_regression_flag(opts);
            false
        }
    };
    sort_and_limit_flags(&mut flags, opts.top);

    let elapsed = start.elapsed();
    // Read live rather than from the session snapshot: the parse stage
    // records `source-read-failure` and `source-parse-degraded` after the
    // session captured its walk, and both are reasons a flag is missing
    // from the array this envelope reports.
    let mut workspace_diagnostics = session.current_workspace_diagnostics();
    if let Some((_, age_diagnostics)) = &retirement {
        workspace_diagnostics.extend(age_diagnostics.iter().cloned());
    }

    print_flags_result(FlagsRenderInput {
        flags: &flags,
        config: session.config(),
        opts,
        elapsed,
        files_scanned: analysis.files_scanned,
        workspace_diagnostics,
        retirement: retirement.as_ref().map(|(report, _)| report),
    });
    if let Some((report, _)) = &retirement
        && matches!(opts.output, OutputFormat::Human)
    {
        print_gate_verdicts(report, opts.quiet);
    }

    if gate_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Compare the report with the regression baseline, and save a new
/// baseline when asked.
fn run_regression_gate(
    report: &mut FlagRetirementReport,
    total_flags: usize,
    scope: &FlagScope,
    opts: &FlagsOptions<'_>,
) -> Result<(), ExitCode> {
    let counts = FlagsCounts::from_summary(&report.summary, total_flags);
    let reasons: Vec<RetirementReason> = opts
        .retirement
        .as_ref()
        .map(|args| args.reasons.iter().map(|&reason| reason.into()).collect())
        .unwrap_or_default();
    let regression = RegressionOpts {
        scoped: !scope.is_whole_project(),
        ..opts.regression
    };
    report.regression = compare_flags_regression(&regression, &counts, &reasons)?;
    match regression.save_target {
        SaveRegressionTarget::None => Ok(()),
        SaveRegressionTarget::Config => Err(emit_error(
            "fallow flags --save-regression-baseline needs a PATH: \
             the config file holds no flags baseline",
            2,
            opts.output,
        )),
        SaveRegressionTarget::File(_) if regression.scoped => {
            if !opts.quiet {
                eprintln!(
                    "Warning: --changed-since or --workspace is active; the flags regression \
                     baseline was not saved (counts not comparable to full-project baseline)"
                );
            }
            Ok(())
        }
        SaveRegressionTarget::File(path) => {
            save_flags_regression_baseline(path, opts.root, &counts, opts.output)
        }
    }
}

fn gate_failed(report: &FlagRetirementReport) -> bool {
    report
        .regression
        .as_ref()
        .is_some_and(|regression| regression.exceeded)
        || report
            .max_flag_age
            .as_ref()
            .is_some_and(|gate| gate.exceeded)
}

/// Without `--retirement`, the regression options have no effect on
/// `fallow flags`. Say so, because the run still exits with code 0.
fn warn_on_ignored_regression_flag(opts: &FlagsOptions<'_>) {
    if let Some(flag) = opts.regression_flag
        && !opts.quiet
    {
        eprintln!(
            "warning: {flag} has no effect on fallow flags without --retirement. \
             Add --retirement to gate on the flag counts."
        );
    }
}

/// Number of flag names that the human age-gate line shows.
const AGE_GATE_NAMES_SHOWN: usize = 5;

/// Print the gate verdicts of a human run to stderr. A failed gate prints
/// also with `--quiet`, because it sets the exit code.
fn print_gate_verdicts(report: &FlagRetirementReport, quiet: bool) {
    if let Some(regression) = &report.regression
        && (!quiet || regression.exceeded)
    {
        print_flags_regression(regression);
    }
    let Some(gate) = &report.max_flag_age else {
        return;
    };
    if !gate.exceeded {
        if !quiet {
            eprintln!(
                "Flag age check passed: no flag is older than {} days",
                gate.max_days
            );
        }
        return;
    }
    let names: Vec<String> = gate
        .flags
        .iter()
        .take(AGE_GATE_NAMES_SHOWN)
        .map(|flag| format!("{} ({} days)", flag.flag_name, flag.age_days))
        .collect();
    let more = gate.flags.len().saturating_sub(AGE_GATE_NAMES_SHOWN);
    let tail = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    let count = if gate.flags.len() == 1 {
        "1 flag is".to_string()
    } else {
        format!("{} flags are", gate.flags.len())
    };
    eprintln!(
        "Flag age check failed: {count} older than {} days: {}{tail}",
        gate.max_days,
        names.join(", ")
    );
}

/// Stable error code of an invalid `--flag-state` file.
const FLAG_STATE_ERROR_CODE: &str = "FALLOW_FLAG_STATE_INVALID";

/// Read the `--flag-state` export before the analysis, so an invalid file
/// fails fast with exit code 2.
fn load_vendor_export(opts: &FlagsOptions<'_>) -> Result<Option<VendorExport>, ExitCode> {
    let Some(path) = opts
        .retirement
        .as_ref()
        .and_then(|args| args.flag_state.as_deref())
    else {
        return Ok(None);
    };
    fallow_engine::flag_vendor::load_flag_state(path, opts.root)
        .map(Some)
        .map_err(|error| {
            let error = fallow_api::ProgrammaticError::new(error.message, 2)
                .with_code(FLAG_STATE_ERROR_CODE)
                .with_help(error.help);
            crate::error::emit_programmatic_error(&error, opts.output, opts.json_style)
        })
}

/// The flag sites and the scope that the retirement report reads.
struct RetirementInput<'a> {
    /// Every flag site of the project.
    sites: Vec<RetirementSiteInput>,
    /// Whether a site is in the scope of the run.
    in_scope: &'a dyn Fn(&Path) -> bool,
    /// Whether the run covers the whole project.
    whole_project: bool,
    /// The `--flag-state` export, if any.
    vendor_export: Option<&'a VendorExport>,
}

/// Build the retirement report and the diagnostics of its age measurement.
fn build_retirement_report(
    input: RetirementInput<'_>,
    session: &fallow_engine::session::AnalysisSession,
    args: &RetirementArgs,
    opts: &FlagsOptions<'_>,
) -> (
    FlagRetirementReport,
    Vec<fallow_config::WorkspaceDiagnostic>,
) {
    let root = session.root();
    let code_flag_names: FxHashSet<String> = input
        .sites
        .iter()
        .map(|site| site.flag_name.clone())
        .collect();
    let mut rows = aggregate_flags(input.sites, root, session.workspaces(), input.in_scope);
    let age_mode = FlagAgeMode::from(args.flag_age);
    let print_progress = |progress: PickaxeProgress| {
        if progress.done == 0 {
            eprintln!(
                "Reading git history for {} flag names (--flag-age pickaxe)",
                progress.total
            );
        } else {
            eprintln!("  {}/{} flag names read", progress.done, progress.total);
        }
    };
    let age = apply_flag_ages(
        &mut rows,
        &FlagAgeRequest {
            root,
            mode: age_mode,
            cache_dir: (!opts.no_cache).then_some(session.config().cache_dir.as_path()),
            progress: (!opts.quiet).then_some(&print_progress),
        },
    );
    let diagnostics: Vec<fallow_config::WorkspaceDiagnostic> = age
        .diagnostics
        .into_iter()
        .map(|kind| fallow_config::WorkspaceDiagnostic::new(root, root.to_path_buf(), kind))
        .collect();
    if !opts.quiet && matches!(opts.output, OutputFormat::Human) {
        for diagnostic in &diagnostics {
            eprintln!("warning: {}", diagnostic.message);
        }
    }
    let options = RetirementOptions {
        sort: args.sort.into(),
        min_age_days: args.min_age,
        reasons: args.reasons.iter().map(|&reason| reason.into()).collect(),
        // The human section shows candidates only, so it applies `--top` to
        // the candidates itself.
        top: (!matches!(opts.output, OutputFormat::Human))
            .then_some(opts.top)
            .flatten(),
    };
    let vendor_state = input.vendor_export.map(|export| {
        let state = apply_vendor_state(
            &mut rows,
            &VendorMatch {
                export,
                key_prefix: session.config().flags.vendor_key_prefix.as_deref(),
                code_flag_names: &code_flag_names,
                add_vendor_only: input.whole_project,
                clock_epoch_secs: AnalysisClock::for_repo(root).epoch_secs(),
            },
        );
        if !opts.quiet && matches!(opts.output, OutputFormat::Human) {
            warn_on_stale_export(&state);
        }
        state
    });
    let max_flag_age = args.max_flag_age.map(|days| max_age_gate(&rows, days));
    let mut report = finish_report(rows, age_mode, age.generated_at_clock, &options);
    report.vendor_state = vendor_state;
    report.max_flag_age = max_flag_age;
    (report, diagnostics)
}

/// Warn when the vendor export is old: its states can be out of date.
fn warn_on_stale_export(state: &RetirementVendorState) {
    if let Some(days) = state
        .export_age_days
        .filter(|days| *days > STALE_EXPORT_DAYS)
    {
        eprintln!(
            "warning: the {} flag state export is {days} days old (exported_at {}). \
             Export it again to get the current states.",
            state.source, state.exported_at
        );
    }
}

fn load_flags_config(opts: &FlagsOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        crate::runtime_support::LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
    )
}

/// The files a flags run reports on, from `--changed-since`, `--workspace`
/// and `--changed-workspaces`.
struct FlagScope {
    changed: Option<rustc_hash::FxHashSet<std::path::PathBuf>>,
    workspace_roots: Option<Vec<std::path::PathBuf>>,
}

impl FlagScope {
    /// Whether the run covers the whole project.
    const fn is_whole_project(&self) -> bool {
        self.changed.is_none() && self.workspace_roots.is_none()
    }

    fn contains(&self, path: &Path) -> bool {
        self.changed
            .as_ref()
            .is_none_or(|changed| changed.contains(path))
            && self
                .workspace_roots
                .as_ref()
                .is_none_or(|roots| roots.iter().any(|root| path.starts_with(root)))
    }
}

fn resolve_flag_scope(opts: &FlagsOptions<'_>) -> Result<FlagScope, ExitCode> {
    // The recording resolver, not the printing one: an unresolvable ref widens
    // this report to the whole project, and the stderr line it prints is gone
    // under `--quiet` (issue #2734). The printed body is identical either way.
    let changed = opts
        .changed_since
        .and_then(|git_ref| crate::requests::resolve_changed_since(opts.root, git_ref));
    let workspace_roots = crate::check::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    Ok(FlagScope {
        changed,
        workspace_roots,
    })
}

fn sort_and_limit_flags(flags: &mut Vec<FeatureFlag>, top: Option<usize>) {
    flags.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.flag_name.cmp(&b.flag_name))
    });

    if let Some(top) = top {
        flags.truncate(top);
    }
}

/// `--min-age` needs an age, and `--flag-age off` measures none, so the
/// combination would drop every row.
fn validate_retirement_args(opts: &FlagsOptions<'_>) -> Result<(), ExitCode> {
    let Some(args) = &opts.retirement else {
        return Ok(());
    };
    if args.flag_age == FlagAgeArg::Off {
        let option = if args.min_age.is_some() {
            Some("--min-age")
        } else if args.max_flag_age.is_some() {
            Some("--max-flag-age")
        } else {
            None
        };
        if let Some(option) = option {
            return Err(emit_error(
                &format!("{option} needs a flag age: use --flag-age blame or pickaxe"),
                2,
                opts.output,
            ));
        }
    }
    Ok(())
}

fn validate_flags_output(output: OutputFormat) -> Result<(), ExitCode> {
    if matches!(
        output,
        OutputFormat::PrCommentGithub
            | OutputFormat::PrCommentGitlab
            | OutputFormat::ReviewGithub
            | OutputFormat::ReviewGitlab
            | OutputFormat::Badge
            | OutputFormat::GithubAnnotations
            | OutputFormat::GithubSummary
    ) {
        return Err(emit_error(
            "flags supports human, json, compact, sarif, markdown, and codeclimate output",
            2,
            output,
        ));
    }
    Ok(())
}

/// Everything the flags renderers need, kept in one struct so the run-owned
/// diagnostics travel with the findings instead of adding a seventh parameter.
struct FlagsRenderInput<'a> {
    flags: &'a [FeatureFlag],
    config: &'a ResolvedConfig,
    opts: &'a FlagsOptions<'a>,
    elapsed: std::time::Duration,
    files_scanned: usize,
    workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    retirement: Option<&'a FlagRetirementReport>,
}

/// Print feature flag results in the requested format.
fn print_flags_result(input: FlagsRenderInput<'_>) {
    let FlagsRenderInput {
        flags,
        config,
        opts,
        elapsed,
        files_scanned,
        workspace_diagnostics,
        retirement,
    } = input;
    match opts.output {
        OutputFormat::Human => {
            print_flags_human(flags, config, elapsed, opts.quiet, files_scanned);
            if let (Some(report), Some(args)) = (retirement, &opts.retirement) {
                print_retirement_section(report, args, opts.top);
            }
        }
        OutputFormat::Json => {
            print_flags_json(
                FlagsJsonInput {
                    flags,
                    config,
                    elapsed,
                    explain: opts.explain,
                    workspace_diagnostics,
                    retirement: retirement.cloned(),
                },
                opts.json_style,
            );
        }
        OutputFormat::Compact => print_flags_compact(flags, config, retirement),
        OutputFormat::Sarif => print_flags_sarif(flags, config, retirement),
        OutputFormat::Markdown => print_flags_markdown(flags, config, retirement),
        OutputFormat::CodeClimate => print_flags_codeclimate(flags, config, retirement),
        OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge
        | OutputFormat::GithubAnnotations
        | OutputFormat::GithubSummary => unreachable!("handled above"),
    }
}

/// Format a kind tag for a feature flag.
fn kind_tag(flag: &FeatureFlag) -> String {
    use colored::Colorize;
    match flag.kind {
        FlagKind::EnvironmentVariable => "(env)".dimmed().to_string(),
        FlagKind::SdkCall => {
            if let Some(ref sdk) = flag.sdk_name {
                format!("(SDK: {sdk})").dimmed().to_string()
            } else {
                "(SDK)".dimmed().to_string()
            }
        }
        FlagKind::ConfigObject => "(config, heuristic)".dimmed().to_string(),
    }
}

/// Print a file path with dimmed directory and bold filename.
fn print_file_path(display: &str) {
    use colored::Colorize;
    if let Some(parent) = std::path::Path::new(display).parent() {
        let parent_str = parent.to_string_lossy();
        let file_name = std::path::Path::new(display)
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().to_string());
        if parent_str.is_empty() {
            println!("  {}", file_name.bold());
        } else {
            println!(
                "  {}{}{}",
                parent_str.dimmed(),
                "/".dimmed(),
                file_name.bold()
            );
        }
    } else {
        println!("  {}", display.bold());
    }
}

/// When `fallow flags` finds nothing, surface the configuration surface so the
/// user can distinguish a true negative from "fallow does not recognize my SDK
/// yet". On full defaults the hint enumerates the built-in detectors (sourced
/// from `fallow-engine`, never hardcoded) and points at the config knobs. When
/// custom `flags.*` config is present, it collapses to a single terse
/// acknowledgement so users who already found the surface are not nagged. All
/// lines go to stderr, mirroring the empty-result line they follow.
fn print_empty_flags_hint(config: &ResolvedConfig, files_scanned: usize) {
    let custom_sdk = config.flags.sdk_patterns.len();
    let custom_env = config.flags.env_prefixes.len();
    let heuristics = config.flags.config_object_heuristics;
    let has_custom = custom_sdk > 0 || custom_env > 0 || heuristics;

    let files_label = if files_scanned == 1 { "file" } else { "files" };

    if has_custom {
        print_empty_flags_custom_hint(
            custom_sdk,
            custom_env,
            heuristics,
            files_scanned,
            files_label,
        );
    } else {
        print_empty_flags_default_hint(files_scanned, files_label);
    }
}

/// Terse one-line acknowledgement of an empty result when custom `flags.*`
/// config is present.
fn print_empty_flags_custom_hint(
    custom_sdk: usize,
    custom_env: usize,
    heuristics: bool,
    files_scanned: usize,
    files_label: &str,
) {
    use colored::Colorize;

    let mut parts: Vec<String> = Vec::new();
    if custom_sdk > 0 {
        parts.push(format!(
            "{custom_sdk} custom SDK pattern{}",
            if custom_sdk == 1 { "" } else { "s" }
        ));
    }
    if custom_env > 0 {
        parts.push(format!(
            "{custom_env} custom env prefix{}",
            if custom_env == 1 { "" } else { "es" }
        ));
    }
    if heuristics {
        parts.push("config-object heuristics enabled".to_string());
    }
    eprintln!(
        "  {}",
        format!(
            "Scanned {files_scanned} {files_label} with your custom flag config: {}.",
            parts.join(", ")
        )
        .dimmed()
    );
}

/// Enumerate the built-in detectors and config knobs on an empty result with a
/// full-defaults configuration.
fn print_empty_flags_default_hint(files_scanned: usize, files_label: &str) {
    use colored::Colorize;

    let env_prefixes = fallow_engine::flags::builtin_env_prefixes()
        .iter()
        .map(|p| format!("{p}*"))
        .collect::<Vec<_>>()
        .join(", ");
    let providers = fallow_engine::flags::builtin_sdk_providers().join(", ");

    eprintln!(
        "  {}",
        format!("Scanned {files_scanned} {files_label} for:").dimmed()
    );
    eprintln!(
        "    {} Env prefixes: {}",
        "\u{00b7}".dimmed(),
        env_prefixes.dimmed()
    );
    eprintln!("    {} SDKs: {}", "\u{00b7}".dimmed(), providers.dimmed());
    eprintln!(
        "  {}",
        "Using a different SDK (in-house, or one not listed)? Add it via `flags.sdkPatterns` in your config.".dimmed()
    );
    eprintln!(
        "  {}",
        "For property-access patterns (config.featureX), enable `flags.configObjectHeuristics`."
            .dimmed()
    );
    eprintln!(
        "  {}",
        "Docs: https://docs.fallow.tools/cli/flags#configuration".dimmed()
    );
}

/// Print the "Flags guarding dead code" section (human format). No-op when no
/// flag guards a statically dead export.
fn print_dead_code_flags_section(flags: &[FeatureFlag], config: &ResolvedConfig) {
    use colored::Colorize;

    let dead_code_flags: Vec<&FeatureFlag> = flags
        .iter()
        .filter(|f| !f.guarded_dead_exports.is_empty())
        .collect();
    if dead_code_flags.is_empty() {
        return;
    }

    let label = format!("Flags guarding dead code ({})", dead_code_flags.len());
    println!("{} {}", "\u{25cf}".yellow(), label.yellow().bold());

    for flag in &dead_code_flags {
        let relative = flag
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&flag.path)
            .to_string_lossy()
            .replace('\\', "/");
        print_file_path(&relative);

        let dead_count = flag.guarded_dead_exports.len();
        let guard_lines = flag
            .guard_line_start
            .and_then(|s| flag.guard_line_end.map(|e| e.saturating_sub(s) + 1))
            .unwrap_or(0);

        let detail = if guard_lines > 0 {
            format!("guards {guard_lines} lines, {dead_count} statically dead")
        } else {
            format!("{dead_count} dead exports in guarded block")
        };

        println!(
            "    {} {} {} {}",
            format!(":{}", flag.line).dimmed(),
            flag.flag_name.bold(),
            kind_tag(flag),
            format!("({detail})").dimmed(),
        );
    }
    println!();
}

/// Print the per-file "Feature flags" listing (human format), preserving the
/// order in which files first appear in `flags`.
fn print_flags_by_file_section(flags: &[FeatureFlag], config: &ResolvedConfig) {
    use colored::Colorize;

    let mut by_file: Vec<(&std::path::Path, Vec<&FeatureFlag>)> = Vec::new();
    for flag in flags {
        if let Some(entry) = by_file.iter_mut().find(|(p, _)| *p == flag.path.as_path()) {
            entry.1.push(flag);
        } else {
            by_file.push((flag.path.as_path(), vec![flag]));
        }
    }

    let label = format!("Feature flags ({})", flags.len());
    println!("{} {}", "\u{25cf}".cyan(), label.cyan().bold());

    for (file_path, file_flags) in &by_file {
        let relative = file_path.strip_prefix(&config.root).unwrap_or(file_path);
        let display = relative.to_string_lossy().replace('\\', "/");
        print_file_path(&display);

        for flag in file_flags {
            println!(
                "    {} {} {}",
                format!(":{}", flag.line).dimmed(),
                flag.flag_name.bold(),
                kind_tag(flag),
            );
        }
    }
}

/// Human-readable output for `fallow flags`.
fn print_flags_human(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    elapsed: std::time::Duration,
    quiet: bool,
    files_scanned: usize,
) {
    use colored::Colorize;

    if flags.is_empty() {
        if !quiet {
            eprintln!(
                "{} No feature flags detected ({:.2}s)",
                "\u{2713}".green().bold(),
                elapsed.as_secs_f64()
            );
            print_empty_flags_hint(config, files_scanned);
        }
        return;
    }

    print_dead_code_flags_section(flags, config);
    print_flags_by_file_section(flags, config);

    if !quiet {
        let elapsed_str = format!("{:.2}s", elapsed.as_secs_f64());
        eprintln!(
            "\n{} {} flags detected ({})",
            "\u{2713}".green().bold(),
            flags.len(),
            elapsed_str.dimmed(),
        );
    }
}

/// Print the "Retirement candidates" section (human format). `top` limits
/// the candidates, not the rows, so rows without a reason never take a slot.
fn print_retirement_section(
    report: &FlagRetirementReport,
    args: &RetirementArgs,
    top: Option<usize>,
) {
    use colored::Colorize;

    let candidates: Vec<&RetirementFlag> = report
        .flags
        .iter()
        .filter(|row| !row.reasons.is_empty())
        .collect();
    let label = format!(
        "Retirement candidates ({} of {} flags)",
        candidates.len(),
        report.summary.distinct_flags
    );
    println!();
    println!("{} {}", "\u{25cf}".yellow(), label.yellow().bold());
    if candidates.is_empty() {
        println!("  {}", retirement_empty_state(report, args).dimmed());
        return;
    }
    let shown = &candidates[..top.map_or(candidates.len(), |top| top.min(candidates.len()))];
    for row in shown {
        println!("  {}", retirement_line(row));
    }
    if shown.len() < candidates.len() {
        println!(
            "  {}",
            format!(
                "Showing {} of {} candidates (--top {}).",
                shown.len(),
                candidates.len(),
                shown.len()
            )
            .dimmed()
        );
    }
    if report.age_mode == FlagAgeMode::Blame && shown.iter().any(|row| row.age_days.is_some()) {
        println!(
            "  {}",
            "Age is a lower bound: it counts from the oldest line that still holds the flag. \
             Use --flag-age pickaxe for the first commit."
                .dimmed()
        );
    }
    println!(
        "  {}",
        "Fallow does not remove flags. Use --format json for the evidence of each reason.".dimmed()
    );
}

/// The empty-state line: it names the filters when they removed every
/// candidate.
fn retirement_empty_state(report: &FlagRetirementReport, args: &RetirementArgs) -> String {
    let mut filters = Vec::new();
    if !args.reasons.is_empty() {
        filters.push("--reason");
    }
    if args.min_age.is_some() {
        filters.push("--min-age");
    }
    if report.summary.candidates == 0 || filters.is_empty() {
        return "No flag has a retirement reason.".to_string();
    }
    format!("No retirement candidate matches {}.", filters.join(" and "))
}

/// One human line for a retirement row: name, kind, first site, age, read
/// sites and reasons.
fn retirement_line(row: &RetirementFlag) -> String {
    use colored::Colorize;

    let kind = match row.kind {
        RetirementFlagKind::EnvironmentVariable => "(env)".to_string(),
        RetirementFlagKind::SdkCall => row
            .sdk_name
            .as_ref()
            .map_or_else(|| "(SDK)".to_string(), |sdk| format!("(SDK: {sdk})")),
        RetirementFlagKind::ConfigObject => "(config)".to_string(),
        RetirementFlagKind::Constant => "(constant)".to_string(),
        RetirementFlagKind::VendorExport => "(vendor export)".to_string(),
    };
    let location = row
        .sites
        .first()
        .map(|site| format!("{}:{}", site.path, site.line))
        .or_else(|| {
            row.evidence
                .first()
                .map(|evidence| format!("{}:{}", evidence.path, evidence.line))
        })
        .unwrap_or_default();
    let reads = if row.read_sites == 1 {
        "1 read site".to_string()
    } else {
        format!("{} read sites", row.read_sites)
    };
    let reasons: Vec<&str> = row.reasons.iter().map(|reason| reason.code()).collect();
    let separator = "\u{00b7}".dimmed();
    let age = row
        .age_days
        .map(|days| {
            let unit = if days == 1 { "day" } else { "days" };
            format!(" {separator} {days} {unit}")
        })
        .unwrap_or_default();
    format!(
        "{} {} {}{age} {separator} {reads} {separator} {}",
        row.flag_name.bold(),
        kind.dimmed(),
        location.dimmed(),
        reasons.join(", ").yellow(),
    )
}

/// Compact output (one line per finding) for `fallow flags`.
///
/// Follows the established `tag:path:line:detail` convention from `compact.rs`.
fn print_flags_compact(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    retirement: Option<&FlagRetirementReport>,
) {
    for flag in flags {
        let relative = flag
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&flag.path)
            .to_string_lossy()
            .replace('\\', "/");
        let tag = match flag.kind {
            FlagKind::EnvironmentVariable => "feature-flag-env",
            FlagKind::SdkCall => "feature-flag-sdk",
            FlagKind::ConfigObject => "feature-flag-config",
        };
        println!("{tag}:{relative}:{}:{}", flag.line, flag.flag_name);
    }
    for line in retirement
        .map(crate::flags_retirement_formats::compact_lines)
        .unwrap_or_default()
    {
        println!("{line}");
    }
}

/// Helper: get relative path string for a flag.
fn relative_path(flag: &FeatureFlag, root: &std::path::Path) -> String {
    flag.path
        .strip_prefix(root)
        .unwrap_or(&flag.path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Helper: human-readable kind label.
fn kind_label(flag: &FeatureFlag) -> &'static str {
    match flag.kind {
        FlagKind::EnvironmentVariable => "environment variable",
        FlagKind::SdkCall => "SDK call",
        FlagKind::ConfigObject => "config object",
    }
}

/// SARIF output for `fallow flags`.
#[expect(
    clippy::expect_used,
    reason = "feature flag SARIF JSON is built from serializable literals"
)]
fn print_flags_sarif(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    retirement: Option<&FlagRetirementReport>,
) {
    let mut rules = vec![serde_json::json!({
        "id": "fallow/feature-flag",
        "shortDescription": { "text": "Feature flag pattern detected" },
        "helpUri": "https://docs.fallow.tools/cli/flags",
        "defaultConfiguration": { "level": "note" },
    })];

    let mut results: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
            let path = crate::report::normalize_uri(&relative_path(f, &config.root));
            let mut msg = format!("Feature flag '{}' ({})", f.flag_name, kind_label(f));
            if !f.guarded_dead_exports.is_empty() {
                use std::fmt::Write;
                let _ = write!(
                    msg,
                    " guards {} dead exports: {}",
                    f.guarded_dead_exports.len(),
                    f.guarded_dead_exports.join(", ")
                );
            }
            serde_json::json!({
                "ruleId": "fallow/feature-flag",
                "level": "note",
                "message": { "text": msg },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": path },
                        "region": { "startLine": f.line, "startColumn": f.col + 1 },
                    }
                }],
            })
        })
        .collect();

    if let Some(report) = retirement {
        rules.push(crate::flags_retirement_formats::sarif_rule());
        results.extend(crate::flags_retirement_formats::sarif_results(report));
    }

    let sarif = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": rules,
                }
            },
            "results": results,
        }]
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&sarif).expect("JSON serialization should not fail")
    );
}

/// Escape backticks in a string for safe embedding in markdown code spans.
fn escape_backticks(s: &str) -> String {
    s.replace('`', "\\`")
}

/// Markdown output for `fallow flags` (PR comments).
fn print_flags_markdown(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    retirement: Option<&FlagRetirementReport>,
) {
    print_flags_markdown_sites(flags, config);
    if let Some(report) = retirement {
        println!();
        print!(
            "{}",
            crate::flags_retirement_formats::markdown_section(report)
        );
    }
}

fn print_flags_markdown_sites(flags: &[FeatureFlag], config: &ResolvedConfig) {
    if flags.is_empty() {
        println!("## Feature flags: no flags detected");
        return;
    }

    println!("## Feature flags: {} found\n", flags.len());

    let dead_flags: Vec<&FeatureFlag> = flags
        .iter()
        .filter(|f| !f.guarded_dead_exports.is_empty())
        .collect();

    if !dead_flags.is_empty() {
        println!("### Flags guarding dead code ({})\n", dead_flags.len());
        println!("| File | Line | Flag | Dead exports |");
        println!("|------|------|------|-------------|");
        for f in &dead_flags {
            let path = escape_backticks(&relative_path(f, &config.root));
            let name = escape_backticks(&f.flag_name);
            println!(
                "| `{path}` | {} | `{name}` | `{}` |",
                f.line,
                f.guarded_dead_exports.join("`, `")
            );
        }
        println!();
    }

    println!("### Feature flags ({})\n", flags.len());
    println!("| File | Line | Flag | Kind |");
    println!("|------|------|------|------|");
    for f in flags {
        let path = escape_backticks(&relative_path(f, &config.root));
        let name = escape_backticks(&f.flag_name);
        let kind = match f.kind {
            FlagKind::EnvironmentVariable => "env".to_string(),
            FlagKind::SdkCall => f
                .sdk_name
                .as_ref()
                .map_or_else(|| "SDK".to_string(), |sdk| format!("SDK: {sdk}")),
            FlagKind::ConfigObject => "config".to_string(),
        };
        println!("| `{path}` | {} | `{name}` | {kind} |", f.line);
    }
}

/// CodeClimate output for `fallow flags` (GitLab Code Quality).
#[expect(
    clippy::expect_used,
    reason = "feature flag CodeClimate JSON is built from serializable literals"
)]
fn print_flags_codeclimate(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    retirement: Option<&FlagRetirementReport>,
) {
    let mut issues: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
            let path = crate::report::normalize_uri(&relative_path(f, &config.root));
            let mut description = format!(
                "Feature flag '{}' detected ({})",
                f.flag_name,
                kind_label(f)
            );
            if !f.guarded_dead_exports.is_empty() {
                use std::fmt::Write;
                let _ = write!(
                    description,
                    ". Guards {} dead exports",
                    f.guarded_dead_exports.len()
                );
            }
            let fingerprint = codeclimate_fingerprint_hash(&[
                "feature-flag",
                &path,
                &f.line.to_string(),
                &f.flag_name,
            ]);
            serde_json::json!({
                "type": "issue",
                "check_name": "fallow/feature-flag",
                "description": description,
                "categories": ["Clarity"],
                "severity": "info",
                "fingerprint": fingerprint,
                "location": {
                    "path": path,
                    "lines": { "begin": f.line },
                }
            })
        })
        .collect();
    if let Some(report) = retirement {
        issues.extend(crate::flags_retirement_formats::codeclimate_issues(report));
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&issues).expect("JSON serialization should not fail")
    );
}

/// Everything the JSON renderer needs.
struct FlagsJsonInput<'a> {
    flags: &'a [FeatureFlag],
    config: &'a ResolvedConfig,
    elapsed: std::time::Duration,
    explain: bool,
    workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
    retirement: Option<FlagRetirementReport>,
}

/// JSON output for `fallow flags`.
#[expect(
    clippy::expect_used,
    reason = "feature flag JSON output is built from serializable literals"
)]
fn print_flags_json(input: FlagsJsonInput<'_>, json_style: crate::json_style::JsonStyle) {
    let FlagsJsonInput {
        flags,
        config,
        elapsed,
        explain,
        workspace_diagnostics,
        retirement,
    } = input;
    let output =
        fallow_output::build_feature_flags_output(fallow_output::FeatureFlagsOutputInput {
            schema_version: fallow_output::FEATURE_FLAGS_SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed,
            flags,
            root: &config.root,
            workspace_diagnostics,
            // The `changed-since` channel only: `init_cli_diff_filter` runs
            // before dispatch for every command, so the broader reader would
            // publish an applied `diff-filter` this command never consulted.
            request_outcomes: crate::requests::changed_since_request_outcomes(),
            meta: explain.then(fallow_output::feature_flags_meta),
            retirement,
        });
    let output = fallow_output::serialize_feature_flags_json_output(
        output,
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .expect("JSON serialization should not fail");

    println!(
        "{}",
        json_style
            .serialize(&output)
            .expect("JSON serialization should not fail")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::FlagConfidence;
    use std::path::PathBuf;

    /// No explicit `--config`; static so the `&Option<PathBuf>` field borrows it.
    const NO_CONFIG: Option<PathBuf> = None;

    fn flags_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/feature-flag-suppression")
    }

    fn flag(kind: FlagKind, name: &str, path: &str) -> FeatureFlag {
        FeatureFlag {
            path: PathBuf::from(path),
            flag_name: name.to_owned(),
            kind,
            confidence: FlagConfidence::High,
            line: 3,
            col: 2,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: None,
            guard_line_end: None,
            guarded_dead_exports: Vec::new(),
        }
    }

    fn flags_opts(root: &Path, output: OutputFormat) -> FlagsOptions<'_> {
        FlagsOptions {
            root,
            config_path: &NO_CONFIG,
            output,
            json_style: crate::json_style::JsonStyle::Compact,
            no_cache: true,
            threads: 1,
            quiet: true,
            allow_remote_extends: false,
            production: false,
            workspace: None,
            changed_workspaces: None,
            changed_since: None,
            explain: false,
            top: None,
            retirement: None,
            regression: RegressionOpts {
                fail_on_regression: false,
                tolerance: crate::regression::Tolerance::Absolute(0),
                regression_baseline_file: None,
                save_target: SaveRegressionTarget::None,
                scoped: false,
                quiet: true,
                output,
            },
            regression_flag: None,
        }
    }

    #[test]
    fn escape_backticks_escapes_only_backticks() {
        assert_eq!(escape_backticks("a`b`c"), "a\\`b\\`c");
        assert_eq!(escape_backticks("no ticks"), "no ticks");
    }

    #[test]
    fn kind_label_covers_all_kinds() {
        assert_eq!(
            kind_label(&flag(FlagKind::EnvironmentVariable, "X", "a.ts")),
            "environment variable"
        );
        assert_eq!(
            kind_label(&flag(FlagKind::SdkCall, "X", "a.ts")),
            "SDK call"
        );
        assert_eq!(
            kind_label(&flag(FlagKind::ConfigObject, "X", "a.ts")),
            "config object"
        );
    }

    #[test]
    fn relative_path_strips_root_and_normalizes_separators() {
        let root = Path::new("/proj");
        let f = flag(FlagKind::EnvironmentVariable, "X", "/proj/src/index.ts");
        assert_eq!(relative_path(&f, root), "src/index.ts");
        // A path outside the root is returned as-is (normalized).
        let outside = flag(FlagKind::EnvironmentVariable, "X", "/other/file.ts");
        assert_eq!(relative_path(&outside, root), "/other/file.ts");
    }

    #[test]
    fn kind_tag_labels_sdk_with_and_without_name() {
        colored::control::set_override(false);
        let mut sdk = flag(FlagKind::SdkCall, "X", "a.ts");
        sdk.sdk_name = Some("LaunchDarkly".to_owned());
        assert_eq!(kind_tag(&sdk), "(SDK: LaunchDarkly)");
        sdk.sdk_name = None;
        assert_eq!(kind_tag(&sdk), "(SDK)");
        assert_eq!(
            kind_tag(&flag(FlagKind::EnvironmentVariable, "X", "a.ts")),
            "(env)"
        );
        assert_eq!(
            kind_tag(&flag(FlagKind::ConfigObject, "X", "a.ts")),
            "(config, heuristic)"
        );
    }

    #[test]
    fn run_flags_renders_every_supported_format() {
        colored::control::set_override(false);
        let root = flags_fixture_root();
        for output in [
            OutputFormat::Human,
            OutputFormat::Json,
            OutputFormat::Compact,
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
        ] {
            assert_eq!(
                run_flags(&flags_opts(&root, output)),
                ExitCode::SUCCESS,
                "format {output:?} should render and exit 0"
            );
        }
    }

    #[test]
    fn run_flags_with_explain_emits_json_meta() {
        let root = flags_fixture_root();
        let opts = FlagsOptions {
            explain: true,
            ..flags_opts(&root, OutputFormat::Json)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_rejects_unsupported_format() {
        let root = flags_fixture_root();
        // Badge / PR-comment / review formats are not supported by `flags`.
        assert_eq!(
            run_flags(&flags_opts(&root, OutputFormat::Badge)),
            ExitCode::from(2)
        );
    }

    #[test]
    fn run_flags_empty_default_config_surfaces_detectors_hint() {
        colored::control::set_override(false);
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/flags-none-default");
        // Non-quiet so the built-in detectors hint renders on an empty result.
        let opts = FlagsOptions {
            quiet: false,
            ..flags_opts(&root, OutputFormat::Human)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_empty_custom_config_surfaces_terse_hint() {
        colored::control::set_override(false);
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/flags-none-custom");
        let opts = FlagsOptions {
            quiet: false,
            ..flags_opts(&root, OutputFormat::Human)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_renders_sdk_call_flag_across_formats() {
        colored::control::set_override(false);
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"flags-sdk","main":"src/index.ts"}"#,
        )
        .unwrap();
        // `variation('name', ...)` is a built-in LaunchDarkly SDK flag pattern,
        // so the SDK-name branches of every renderer are exercised.
        std::fs::write(
            root.join("src/index.ts"),
            "export function boot() {\n  if (variation('checkout-flag', false)) {\n    console.log('on');\n  }\n}\n",
        )
        .unwrap();
        for output in [
            OutputFormat::Human,
            OutputFormat::Compact,
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
            OutputFormat::Json,
        ] {
            assert_eq!(
                run_flags(&flags_opts(root, output)),
                ExitCode::SUCCESS,
                "SDK-flag render for {output:?} should exit 0"
            );
        }
    }
}
