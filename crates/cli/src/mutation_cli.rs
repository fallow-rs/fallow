//! CLI adapter for `fallow mutation`.
//!
//! `fallow mutation --plan` and `fallow mutation inspect` run no project code.
//! `fallow mutation` without `--plan` runs the project tests against each
//! planned mutant. Only this explicit command starts a run; project config
//! cannot start one, and the side-effect override is a flag only.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand};
use fallow_api::mutation::{MAX_VERDICT_INPUT_BYTES, Operator, baseline_blocked_run_error};
use fallow_api::{
    AnalysisOptions, MutationRunOptions, MutationRunScope, ProgrammaticError,
    apply_mutation_verdicts, plan_mutation, run_mutation, run_mutation_with_progress,
    validate_mutation_verdicts,
};
use fallow_config::{MutationConfig, OutputFormat};
use fallow_output::{
    GateName, GateOutcome, MutantStatus, MutationPlanOutput, MutationReport,
    serialize_mutation_inspect_json_output, serialize_mutation_json_output,
    serialize_mutation_plan_json_output, serialize_mutation_tasks_json_output,
};

use crate::json_style::JsonStyle;
use crate::mutation_human::{
    CodeExcerpt, CodeLookup, RunReportOptions, render_inspect, render_plan, render_run,
};
use crate::mutation_progress::MutationProgressReporter;

/// The exit code of a run with a score below the minimum.
const MIN_SCORE_EXIT_CODE: u8 = 1;
/// The exit code of invalid input and execution errors.
const INPUT_EXIT_CODE: u8 = 2;

/// The values of `--record-args`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum RecordArgsFlag {
    /// Keep the types of the arguments only.
    Shapes,
    /// Keep the argument values, after the redaction of secrets.
    Values,
}

#[derive(Subcommand)]
pub enum MutationSubcommand {
    /// Show one mutant of a saved report against the current file. Runs no code.
    ///
    /// Prints the original and the mutated code, the covering tests and the
    /// killing test. When the file changed after the report, the status is
    /// `stale` and no code is shown. Without `--report`, it reads the report
    /// that the last run saved in the Fallow cache.
    Inspect {
        /// The `mutant_id` from the report.
        #[arg(value_name = "MUTANT_ID")]
        mutant_id: String,
        /// A saved `fallow mutation --format json` report or `--plan` document
        /// (default: the report of the last run).
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,
        /// Call the export of the mutated function with `--args` on the
        /// original and on the mutant, three times each, and compare the
        /// results. Runs project code. A difference that each call repeats is
        /// a witness. Equal results are "no witness found", never
        /// "equivalent".
        #[arg(long)]
        replay: bool,
        /// The arguments of the replay call as a JSON array, for example
        /// `[2, 3]`. Objects with a `$replay` tag give `undefined`, special
        /// numbers, bigints, dates, regular expressions, maps, sets and
        /// typed arrays (default: up to 3 inputs that a `fallow mutation
        /// --record-args values` run recorded for the export).
        #[arg(long = "args", value_name = "JSON", requires = "replay")]
        replay_args: Option<String>,
        /// The export to call (default: the export that holds the mutated
        /// function).
        #[arg(long = "export", value_name = "NAME", requires = "replay")]
        replay_export: Option<String>,
        /// The time limit of one replay call, for example `30s` (default:
        /// 5s, at most 10m). A call past it is `timed_out`.
        #[arg(long = "call-timeout", value_name = "DURATION", requires = "replay")]
        call_timeout: Option<String>,
    },
    /// List the open tasks of a saved report as one batch. Runs no code.
    ///
    /// A task is a `survived` or `no_coverage` mutant with the source lines
    /// before and after the change. The human and markdown output is a list
    /// that an agent or a person can use to write the missing tests. Without
    /// `--report`, it reads the report that the last run saved in the Fallow
    /// cache.
    Tasks {
        /// A saved `fallow mutation --format json` report (default: the report
        /// of the last run).
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,
    },
    /// Write a saved report in the open `mutation-testing-report-schema`
    /// format. Runs no code.
    ///
    /// Mutation dashboards and HTML report viewers read this JSON format. The
    /// document holds the source of each mutated file, so each file must be
    /// as the run saw it. Without `--report`, it reads the report that the
    /// last run saved in the Fallow cache.
    Export {
        /// A saved `fallow mutation --format json` report (default: the report
        /// of the last run).
        #[arg(long, value_name = "PATH")]
        report: Option<PathBuf>,
    },
}

/// The flags of `fallow mutation`.
#[derive(Args, Debug, Clone, Default)]
pub struct MutationArgs {
    /// Mutate every source file instead of the changed code.
    #[arg(long, conflicts_with = "paths")]
    pub all: bool,
    /// Build the plan only: select, map and rank mutants. Runs no code.
    #[arg(long)]
    pub plan: bool,
    /// Run at most N covered mutants, in rank order. Uncovered mutants do not
    /// count. The others are `pending`.
    #[arg(long, value_name = "N")]
    pub max_mutants: Option<usize>,
    /// Keep at most N mutants on each source line. The plan keeps the most
    /// productive mutants of the line (relational, condition, arithmetic and
    /// logical operators first). The others are `pending` with the reason
    /// `sampled-out` and do not run (default: the `mutation.mutantsPerLine`
    /// key, else each mutant).
    #[arg(long, value_name = "N")]
    pub mutants_per_line: Option<usize>,
    /// Widen each changed line to the whole function that holds it, so that a
    /// changed parameter or signature selects the mutants of the function.
    /// Only the changed-code scope uses it (default: the
    /// `mutation.functionScope` key, else the changed lines only).
    #[arg(long, conflicts_with_all = ["all", "paths"])]
    pub function_scope: bool,
    /// Stop starting mutants after this time, for example `90s`, `10m` or `1h`.
    /// The others are `pending`.
    #[arg(long, value_name = "DURATION")]
    pub time_budget: Option<String>,
    /// The number of companion processes (default: half the logical cores).
    #[arg(long, value_name = "N")]
    pub concurrency: Option<usize>,
    /// The JavaScript runtime: a name on PATH such as `node`, or a file path
    /// (default from PATH: `node` for node:test, `bun` for bun test, and
    /// `node`, then `bun`, for Vitest).
    #[arg(long, value_name = "PATH")]
    pub runtime: Option<PathBuf>,
    /// Run in a dirty git worktree. The run still reports the paths that it
    /// changed.
    #[arg(long)]
    pub allow_dirty: bool,
    /// Keep mutants in functions that reach a file system, process, network
    /// or database sink. A mutant can then remove a guard of a real side
    /// effect. This is a flag only, never a config key.
    #[arg(long)]
    pub allow_side_effects: bool,
    /// Record the arguments of the changed exported functions in a record pass
    /// (off by default). `shapes` keeps their types, `values` keeps the
    /// values for `fallow mutation inspect --replay`. The values stay in the
    /// gitignored cache directory for 7 days, and an input with a secret
    /// (by key name or by entropy) is not kept.
    #[arg(long, value_name = "MODE", value_enum)]
    pub record_args: Option<RecordArgsFlag>,
    /// Exit with code 1 when the mutation score is below this percentage.
    #[arg(long, value_name = "0-100")]
    pub min_score: Option<f64>,
    /// The operators to use, comma-separated. Default: the default set. The
    /// opt-in operators (block-statement, array-literal, object-literal,
    /// method-expression, switch-case, call-expression, throw-statement,
    /// regex-constructor, regex-assertion, nullish-fallback) run only when
    /// named here.
    #[arg(long, value_name = "NAMES", value_delimiter = ',')]
    pub operators: Vec<String>,
    /// A verdict document that marks mutants as equivalent. Equivalent
    /// mutants leave the score.
    #[arg(long, value_name = "PATH")]
    pub verdicts: Option<PathBuf>,
    /// Wait this long for each runner process to start, finish its dry run or
    /// reload, for example `5m` on a slow or busy host (default: `2m`).
    #[arg(long, value_name = "DURATION")]
    pub setup_timeout: Option<String>,
    /// The config file of the test runner, for example
    /// `vitest.integration.config.mjs` (default: the
    /// `mutation.runnerConfig` key, else the runner's own discovery). The file
    /// must be inside the project root. Only Vitest reads it. This is not
    /// `--config`, which selects the Fallow config.
    #[arg(long, value_name = "PATH")]
    pub runner_config: Option<PathBuf>,
    /// Type-check each survived mutant through the `--type-aware`
    /// companion. A survivor that does not type-check becomes
    /// `compile_error` and leaves the score. An optional-chaining or
    /// nullish-fallback survivor on a receiver whose type excludes `null` and
    /// `undefined` gets an advisory `equivalence_hint`. Its status and the
    /// score do not change.
    #[arg(long)]
    pub typecheck: bool,
    /// Continue an interrupted or partial run: reuse the killed and confirmed
    /// survived mutants in its journal and run only the others. A journal of
    /// other inputs exits 2.
    #[arg(long)]
    pub resume: bool,
    /// Collect all killing tests and report redundant-test candidates for
    /// individual review. Run every covering test without stopping at a failure.
    #[arg(long)]
    pub kill_matrix: bool,
    /// Check whether this test is redundant: a test file (each test in it) or
    /// a test id (`file::name`). Repeat the flag for more targets. Only the
    /// mutants that a target covers run: first the target tests without a
    /// stop at a failure, then the other covering tests up to the first
    /// failure.
    #[arg(long, value_name = "TARGET", conflicts_with = "kill_matrix")]
    pub kill_matrix_for: Vec<String>,
    /// Rerun the open tasks of a saved report (its `survived` and
    /// `no_coverage` mutants) against the current tests. The worktree may
    /// change test files only. The report keeps the earlier result of each
    /// other mutant, so its summary counts the full set. The recheck does not
    /// run those mutants again: after you remove or change a test that killed
    /// them, run `fallow mutation` without `--recheck`. The run replaces the
    /// saved report, so `fallow mutation tasks` then lists the mutants that
    /// are still open.
    #[arg(
        long,
        conflicts_with_all = ["all", "paths", "plan", "resume", "allow_dirty", "max_mutants", "mutants_per_line", "function_scope", "min_score"]
    )]
    pub recheck: bool,
    /// The saved report of `--recheck` (default: the report of the last run).
    /// A recheck without `--mutant` replaces this file and the report of the
    /// last run.
    #[arg(long, value_name = "PATH", requires = "recheck")]
    pub report: Option<PathBuf>,
    /// Recheck only these mutant ids of the report, comma-separated or
    /// repeated (default: each open task). The saved report then stays as it
    /// is, so `fallow mutation tasks` keeps the other open tasks.
    #[arg(
        long = "mutant",
        value_name = "ID",
        value_delimiter = ',',
        requires = "recheck"
    )]
    pub mutants: Vec<String>,
    /// Compare this run with a saved complete report: list each mutant that
    /// the report detects (`killed` or `timeout`) and this run does not
    /// (`survived` or `no_coverage`), with its earlier killing tests. A
    /// report or a run that is not complete exits 2.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["plan", "recheck"])]
    pub compare: Option<PathBuf>,
    /// Mutate only these files or directories instead of the changed code.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,
}

pub struct MutationCliInput<'a> {
    pub(crate) root: &'a Path,
    pub(crate) config_path: Option<&'a Path>,
    /// The `--root` flag as typed, for the suggested inspect command.
    pub(crate) root_flag: Option<&'a Path>,
    /// The `--output-file` path. The run writes it after the run, and the
    /// worktree check leaves it out.
    pub(crate) output_file: Option<&'a Path>,
    pub(crate) allow_remote_extends: bool,
    pub(crate) no_cache: bool,
    pub(crate) threads: usize,
    pub(crate) changed_since: Option<&'a str>,
    pub(crate) diff_file: Option<&'a Path>,
    pub(crate) quiet: bool,
    pub(crate) output: OutputFormat,
    pub(crate) json_style: JsonStyle,
    pub(crate) config: MutationConfig,
    pub(crate) args: MutationArgs,
    pub(crate) subcommand: Option<MutationSubcommand>,
}

fn input_error(message: impl Into<String>, context: &str) -> ProgrammaticError {
    ProgrammaticError::new(message, INPUT_EXIT_CODE)
        .with_code("FALLOW_INVALID_MUTATION_OPTIONS")
        .with_context(context)
}

/// Parses a duration: a number with the unit `ms`, `s`, `m` or `h`. A bare
/// number is seconds.
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let split = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: u64 = number.parse().ok()?;
    let duration = match unit {
        "ms" => Duration::from_millis(value),
        "" | "s" => Duration::from_secs(value),
        "m" => Duration::from_secs(value.checked_mul(60)?),
        "h" => Duration::from_secs(value.checked_mul(3600)?),
        _ => return None,
    };
    (!duration.is_zero()).then_some(duration)
}

/// The operators of the `--operators` flag. `None` when the flag is not
/// set: the API plan then applies the `mutation.operators` config key, so the
/// CLI and MCP share one rule.
fn parse_operators(names: &[String]) -> Result<Option<Vec<Operator>>, ProgrammaticError> {
    if names.is_empty() {
        return Ok(None);
    }
    names
        .iter()
        .map(|name| {
            Operator::from_name(name.trim()).ok_or_else(|| {
                let valid: Vec<&str> = Operator::ALL.iter().map(|op| op.as_str()).collect();
                input_error(
                    format!(
                        "unknown mutation operator `{name}`; valid operators: {}",
                        valid.join(", ")
                    ),
                    "mutation.operators",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn scope(input: &MutationCliInput<'_>) -> Result<MutationRunScope, ProgrammaticError> {
    let args = &input.args;
    if args.all && input.changed_since.is_some() {
        return Err(input_error(
            "--all mutates every file; it cannot be combined with --changed-since",
            "mutation.scope",
        ));
    }
    if !args.paths.is_empty() && input.changed_since.is_some() {
        return Err(input_error(
            "paths select files to mutate; they cannot be combined with --changed-since",
            "mutation.scope",
        ));
    }
    if args.all {
        return Ok(MutationRunScope::All);
    }
    if args.paths.is_empty() {
        return Ok(MutationRunScope::Changed {
            base: input.changed_since.map(str::to_owned),
        });
    }
    let mut paths = Vec::with_capacity(args.paths.len());
    for raw in &args.paths {
        let resolved = crate::scope_path::resolve_scope_path(input.root, raw)
            .map_err(|message| input_error(message, "mutation.paths"))?;
        paths.push(resolved.absolute);
    }
    Ok(MutationRunScope::Paths(paths))
}

/// The minimum score in percent: the flag, else the config.
fn min_score(input: &MutationCliInput<'_>) -> Result<Option<f64>, ProgrammaticError> {
    let value = input.args.min_score.or(input.config.min_score);
    if let Some(score) = value
        && (!score.is_finite() || !(0.0..=100.0).contains(&score))
    {
        return Err(input_error(
            format!("--min-score must be between 0 and 100 (got {score})"),
            "mutation.minScore",
        ));
    }
    Ok(value)
}

/// Builds the run options from the flags and the `mutation` config section.
/// A flag wins over its config key.
///
/// # Errors
///
/// Returns an exit-2 error for an invalid flag value or a scope conflict.
pub fn run_options(input: &MutationCliInput<'_>) -> Result<MutationRunOptions, ProgrammaticError> {
    let time_budget = match input.args.time_budget.as_deref() {
        Some(text) => Some(parse_duration(text).ok_or_else(|| {
            input_error(
                format!("--time-budget `{text}` is not a duration such as 90s, 10m or 1h"),
                "mutation.timeBudget",
            )
        })?),
        None => None,
    };
    let runner_timeout = match input.args.setup_timeout.as_deref() {
        Some(text) => Some(parse_duration(text).ok_or_else(|| {
            input_error(
                format!("--setup-timeout `{text}` is not a duration such as 90s, 10m or 1h"),
                "mutation.setupTimeout",
            )
        })?),
        None => None,
    };
    let runner_config = match input.args.runner_config.as_deref() {
        Some(raw) => Some(
            crate::scope_path::resolve_scope_path(input.root, raw)
                .map_err(|message| {
                    input_error(
                        format!("--runner-config: {message}"),
                        "mutation.runnerConfig",
                    )
                })?
                .absolute,
        ),
        None => None,
    };
    if input.args.concurrency == Some(0) {
        return Err(input_error(
            "--concurrency must be at least 1",
            "mutation.concurrency",
        ));
    }
    if input.args.max_mutants == Some(0) {
        return Err(input_error(
            "--max-mutants must be at least 1",
            "mutation.maxMutants",
        ));
    }
    if input.args.mutants_per_line == Some(0) {
        return Err(input_error(
            "--mutants-per-line must be at least 1",
            "mutation.mutantsPerLine",
        ));
    }
    Ok(MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(input.root.to_path_buf()),
            config_path: input.config_path.map(Path::to_path_buf),
            allow_remote_extends: input.allow_remote_extends,
            no_cache: input.no_cache,
            threads: Some(input.threads),
            diff_file: input.diff_file.map(Path::to_path_buf),
            ..AnalysisOptions::default()
        },
        scope: scope(input)?,
        operators: parse_operators(&input.args.operators)?,
        allow_side_effects: input.args.allow_side_effects,
        max_mutants: input.args.max_mutants,
        mutants_per_line: input.args.mutants_per_line,
        function_scope: input.args.function_scope,
        allow_dirty: input.args.allow_dirty,
        runtime: input.args.runtime.clone(),
        concurrency: input.args.concurrency.or(input.config.concurrency),
        time_budget,
        runner_timeout,
        runner_config,
        output_paths: input
            .output_file
            .into_iter()
            .chain(recheck_report_file(input))
            .map(absolute_output_path)
            .collect(),
        typecheck: input.args.typecheck || input.config.typecheck == Some(true),
        resume: input.args.resume,
        kill_matrix: input.args.kill_matrix,
        kill_matrix_targets: input.args.kill_matrix_for.clone(),
        mutant_ids: Vec::new(),
        test_changes_only: false,
        record_args: match input.args.record_args {
            None => fallow_api::mutation::RecordArgsMode::Off,
            Some(RecordArgsFlag::Shapes) => fallow_api::mutation::RecordArgsMode::Shapes,
            Some(RecordArgsFlag::Values) => fallow_api::mutation::RecordArgsMode::Values,
        },
    })
}

/// The `--output-file` path as the report writer opens it: a relative path
/// is relative to the process directory, also when `--root` names another
/// directory. The worktree check leaves out this same absolute path.
fn absolute_output_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

/// The exit code of the score of a finished run, read off the
/// `mutation-min-score` entry of `gate_outcomes`: 1 only when the entry is an
/// enforced `fail`, so the exit code and the published verdict are one rule.
/// Survivors alone never fail a run. A run with no counted mutant has a
/// `skipped` entry and passes here; a run in which baseline failures leave
/// each mutant without a test result exits 2 through
/// [`baseline_blocked_run_error`] instead.
#[must_use]
pub fn run_exit_code(report: &MutationReport) -> u8 {
    let failed = report
        .gate_outcomes
        .as_ref()
        .and_then(|gates| gates.get(&GateName::MutationMinScore))
        .is_some_and(GateOutcome::fails_run);
    if failed { MIN_SCORE_EXIT_CODE } else { 0 }
}

fn path_argument(path: &Path) -> String {
    crate::similar_code_cli::render_cli_argument(&path.to_string_lossy())
}

/// The global flags that `fallow mutation inspect` needs to find the saved
/// report of this run: the root and the config, because the config can move
/// the cache directory. Each flag starts with a space.
pub fn inspect_flags(input: &MutationCliInput<'_>) -> String {
    let mut flags = String::new();
    if let Some(root) = input.root_flag {
        flags.push_str(" --root ");
        flags.push_str(&path_argument(root));
    }
    if let Some(config) = input.config_path {
        flags.push_str(" --config ");
        flags.push_str(&path_argument(config));
    }
    flags
}

fn read_verdicts(path: &Path) -> Result<Vec<u8>, ProgrammaticError> {
    let error = |message: String| {
        ProgrammaticError::new(message, INPUT_EXIT_CODE)
            .with_code("FALLOW_MUTATION_VERDICTS_INVALID")
            .with_context("mutation.verdicts")
    };
    let file = std::fs::File::open(path)
        .map_err(|e| error(format!("failed to read {}: {e}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(MAX_VERDICT_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| error(format!("failed to read {}: {e}", path.display())))?;
    if bytes.len() > MAX_VERDICT_INPUT_BYTES {
        return Err(error(
            "the mutation verdict document is larger than 16 MiB".to_owned(),
        ));
    }
    Ok(bytes)
}

/// Adds a digest for each mutated file that the run report does not list.
///
/// The plan lists only the files that it instruments, so a file with only
/// `no_coverage` mutants has no digest, and `inspect` could not show its
/// mutants. The run refuses a dirty worktree and reports each path that
/// changed during the run, so the current file is the planned file. A file
/// that changed during the run gets no digest.
fn complete_file_digests(report: &mut MutationReport, root: &Path) {
    let known: rustc_hash::FxHashSet<String> =
        report.files.iter().map(|file| file.path.clone()).collect();
    let missing: std::collections::BTreeSet<&str> = report
        .mutants
        .iter()
        .map(|mutant| mutant.location.file.as_str())
        .filter(|file| {
            !known.contains(*file)
                && !report
                    .worktree
                    .changed_paths
                    .iter()
                    .any(|path| path == file)
        })
        .collect();
    let added: Vec<fallow_output::MutationPlanFile> = missing
        .into_iter()
        .filter_map(|file| {
            let bytes = std::fs::read(root.join(file)).ok()?;
            Some(fallow_output::MutationPlanFile {
                path: file.to_owned(),
                source_sha256: crate::mutation_inspect::sha256_hex(&bytes),
                static_test_files: Vec::new(),
            })
        })
        .collect();
    report.files.extend(added);
    report.files.sort_by(|a, b| a.path.cmp(&b.path));
}

/// The reproduced code of each survived and unreached mutant whose file
/// still has the digest of the run.
fn code_lookup(report: &MutationReport, root: &Path) -> CodeLookup {
    let mut lookup = CodeLookup::default();
    let mut sources: rustc_hash::FxHashMap<&str, Option<String>> = rustc_hash::FxHashMap::default();
    for mutant in report.mutants.iter().filter(|mutant| {
        matches!(
            mutant.status,
            MutantStatus::Survived | MutantStatus::NoCoverage
        )
    }) {
        let file = mutant.location.file.as_str();
        let source = sources.entry(file).or_insert_with(|| {
            let expected = report.files.iter().find(|entry| entry.path == file)?;
            let bytes = std::fs::read(root.join(file)).ok()?;
            (crate::mutation_inspect::sha256_hex(&bytes) == expected.source_sha256)
                .then(|| String::from_utf8(bytes).ok())
                .flatten()
        });
        let Some(source) = source.as_deref() else {
            continue;
        };
        if let Some(code) = crate::mutation_inspect::reproduce(
            source,
            file,
            &mutant.location.mutant_id,
            mutant.location.span,
            &mutant.location.operator,
        ) {
            let (before, after) =
                crate::mutation_inspect::line_context(source, mutant.location.span);
            lookup.insert(
                mutant.location.mutant_id.clone(),
                CodeExcerpt {
                    before: before.to_owned(),
                    original: code.original,
                    replacement: code.replacement,
                    after: after.to_owned(),
                },
            );
        }
    }
    lookup
}

pub fn run(input: &MutationCliInput<'_>) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    if let Some(MutationSubcommand::Tasks { report }) = &input.subcommand {
        return run_tasks(input, report.as_deref());
    }
    if let Some(MutationSubcommand::Export { report }) = &input.subcommand {
        return run_export(input, report.as_deref());
    }
    if !matches!(output, OutputFormat::Human | OutputFormat::Json) {
        return crate::error::emit_input_error_with_style(
            "`fallow mutation` supports only human and json output",
            output,
            style,
        );
    }
    if let Some(MutationSubcommand::Inspect {
        mutant_id,
        report,
        replay,
        replay_args,
        replay_export,
        call_timeout,
    }) = &input.subcommand
    {
        let replay = replay.then(|| ReplayFlags {
            args: replay_args.as_deref(),
            export: replay_export.as_deref(),
            call_timeout: call_timeout.as_deref(),
        });
        return run_inspect(input, mutant_id, report.as_deref(), replay);
    }
    match run_inner(input) {
        Ok(code) => code,
        Err(error) => crate::error::emit_programmatic_error(&error, output, style),
    }
}

/// Runs the mutation tests. Human output without `--quiet` shows the phase
/// progress on stderr. JSON output shows none, the same as the analysis
/// progress of the other commands.
fn run_with_progress(
    options: &MutationRunOptions,
    input: &MutationCliInput<'_>,
) -> Result<MutationReport, ProgrammaticError> {
    if input.quiet || !matches!(input.output, OutputFormat::Human) {
        return run_mutation(options);
    }
    let reporter = MutationProgressReporter::start();
    let report = run_mutation_with_progress(options, &|event| reporter.report(event));
    reporter.finish();
    report
}

/// Refuses a run flag together with `--plan`.
fn refuse_run_flags_in_plan(args: &MutationArgs) -> Result<(), ProgrammaticError> {
    let refused = if args.verdicts.is_some() {
        Some((
            "--verdicts joins the results of a run; a plan has no results",
            "mutation.verdicts",
        ))
    } else if args.typecheck {
        Some((
            "--typecheck checks the survivors of a run; a plan has no survivors",
            "mutation.typecheck",
        ))
    } else if args.kill_matrix {
        Some((
            "--kill-matrix requires a run; a plan collects no test outcomes",
            "mutation.killMatrix",
        ))
    } else if !args.kill_matrix_for.is_empty() {
        Some((
            "--kill-matrix-for requires a run; a plan collects no test outcomes",
            "mutation.killMatrixFor",
        ))
    } else if args.resume {
        Some((
            "--resume continues a run; a plan runs no mutant",
            "mutation.resume",
        ))
    } else {
        None
    };
    refused.map_or(Ok(()), |(message, context)| {
        Err(input_error(message, context))
    })
}

#[path = "mutation_cli_diff.rs"]
mod diff;

fn run_inner(input: &MutationCliInput<'_>) -> Result<ExitCode, ProgrammaticError> {
    let mut options = run_options(input)?;
    let recheck = recheck_selection(input)?;
    let min_score = apply_recheck(input, &mut options, recheck.as_ref())?;
    let stdin_diff = diff::prepare(&mut options, input.root)?;
    let baseline = input
        .args
        .compare
        .as_deref()
        .map(compare::load)
        .transpose()?;
    if input.args.plan {
        refuse_run_flags_in_plan(&input.args)?;
        let plan = plan_mutation(&options)
            .map_err(|error| diff::contextualize(error, stdin_diff.as_ref()))?;
        return Ok(emit_plan(plan, input));
    }
    let verdicts = input
        .args
        .verdicts
        .as_deref()
        .map(read_verdicts)
        .transpose()?;
    if let Some(verdicts) = &verdicts {
        validate_mutation_verdicts(verdicts)?;
    }
    if !input.quiet {
        eprintln!(
            "fallow: mutation testing runs the project tests against each mutant. Fallow does not write to project files. The report lists any path that the tests changed."
        );
    }
    let mut report = run_with_progress(&options, input)
        .map_err(|error| diff::contextualize(error, stdin_diff.as_ref()))?;
    complete_file_digests(&mut report, input.root);
    // Before the verdicts: an equivalent verdict for a survivor does not make
    // the run one in which no mutant has a test result. Before the merge of
    // a recheck: an earlier result is no test result of this run.
    let blocked = baseline_blocked_run_error(&report);
    report.recheck = recheck
        .as_ref()
        .map(|selection| selection.merge_into(&mut report));
    if let Some(verdicts) = verdicts {
        apply_mutation_verdicts(&mut report, &verdicts)?;
    }
    // After the verdicts: an equivalent mutant is no lost detection.
    let (compare_section, compare_blocked) = compare_with(baseline.as_ref(), &report);
    report.compare = compare_section;
    let blocked = blocked.or(compare_blocked);
    report.gate_outcomes = crate::gates::mutation_gate_outcomes(report.summary.score, min_score);
    let exit = run_exit_code(&report);
    crate::telemetry::note_result_count(report.summary.counts.survived);
    let value = serialize_mutation_json_output(report.clone()).map_err(|error| {
        ProgrammaticError::new(
            format!("failed to serialize the mutation report: {error}"),
            2,
        )
    })?;
    let saved_path = save_reports(input, &options.analysis, &value);
    let saved = saved_path.is_some();
    if saved {
        remove_journal(&options.analysis, &report);
    }
    let json = matches!(input.output, OutputFormat::Json);
    // The JSON contract of exit code 2 is the error envelope on stdout. The
    // envelope replaces the report on stdout only when the report is in a
    // file: the saved report or the `--output-file`.
    let envelope_only = blocked.is_some() && json && input.output_file.is_none() && saved;
    let blocked = blocked
        .map(|error| with_report_locations(error, saved_path.as_deref(), input.output_file, json));
    if !envelope_only {
        if let Some(path) = input.output_file
            && let Err(code) = crate::redirect_report_to_file(path, input.output)
        {
            return Ok(code);
        }
        if json {
            emit_json(&value, input.json_style)?;
        } else {
            emit_human(input, &report, min_score, saved);
        }
        if let Some(path) = input.output_file
            && let Err(code) = crate::finalize_report_file(path, input.quiet, input.output)
        {
            return Ok(code);
        }
    }
    if let Some(error) = &blocked {
        // A JSON report on stdout keeps stdout one document, so the error
        // goes to stderr in that case.
        let report_on_stdout = json && input.output_file.is_none() && !envelope_only;
        let format = if report_on_stdout {
            OutputFormat::Human
        } else {
            input.output
        };
        return Ok(crate::error::emit_programmatic_error(
            error,
            format,
            input.json_style,
        ));
    }
    Ok(ExitCode::from(exit))
}

/// Prints the human run report, then the recheck or compare section.
fn emit_human(
    input: &MutationCliInput<'_>,
    report: &MutationReport,
    min_score: Option<f64>,
    saved: bool,
) {
    let code = code_lookup(report, input.root);
    let flags = inspect_flags(input);
    let text = render_run(
        report,
        &RunReportOptions {
            min_score,
            code: Some(&code),
            inspect_flags: saved.then_some(flags.as_str()),
        },
    );
    let section_text = report
        .recheck
        .as_ref()
        .map(recheck::render)
        .or_else(|| report.compare.as_ref().map(compare::render));
    for line in text
        .lines()
        .chain(section_text.iter().flat_map(|text| text.lines()))
    {
        crate::report::sink::outln!("{line}");
    }
}

#[path = "mutation_recheck.rs"]
mod recheck;

#[path = "mutation_compare.rs"]
mod compare;

/// The compare section of a `--compare` run, or the error of a run that is
/// not complete. Both are `None` without `--compare`.
fn compare_with(
    baseline: Option<&compare::Baseline>,
    report: &MutationReport,
) -> (
    Option<fallow_output::MutationCompare>,
    Option<ProgrammaticError>,
) {
    let Some(baseline) = baseline else {
        return (None, None);
    };
    match compare::guard_run_complete(report) {
        Some(error) => (None, Some(error)),
        None => (Some(baseline.compare(&report.mutants)), None),
    }
}

/// Narrows the run options to the mutants of a recheck and gives the minimum
/// score. A recheck runs only the selected mutants, and the other results
/// come from the earlier report, so the project gate of `mutation.minScore`
/// does not apply to it.
fn apply_recheck(
    input: &MutationCliInput<'_>,
    options: &mut MutationRunOptions,
    recheck: Option<&recheck::Selection>,
) -> Result<Option<f64>, ProgrammaticError> {
    let Some(selection) = recheck else {
        return min_score(input);
    };
    options.scope = MutationRunScope::Paths(selection.files());
    options.mutant_ids = selection.mutant_ids();
    options.test_changes_only = true;
    options.mutants_per_line = None;
    options.function_scope = false;
    Ok(None)
}

/// The mutants of a `--recheck` run, from the saved report. `None` without
/// `--recheck`.
fn recheck_selection(
    input: &MutationCliInput<'_>,
) -> Result<Option<recheck::Selection>, ProgrammaticError> {
    if !input.args.recheck {
        return Ok(None);
    }
    if input.changed_since.is_some() {
        return Err(input_error(
            "--recheck takes its mutants from the report; it cannot be combined with --changed-since",
            "mutation.recheck",
        ));
    }
    let (path, root) = inspect_report_path(input, input.args.report.as_deref())?;
    recheck::select(&path, &root, &input.args.mutants).map(Some)
}

/// Saves the report for `fallow mutation inspect` and gives its path. A
/// failed save is a warning, not a run error.
fn save_report(analysis: &AnalysisOptions, value: &serde_json::Value) -> Option<PathBuf> {
    match fallow_api::mutation::save_last_mutation_report(analysis, value) {
        Ok(path) => Some(path),
        Err(error) => {
            eprintln!(
                "fallow: warning: the mutation report was not saved for `fallow mutation inspect`: {}",
                error.message
            );
            None
        }
    }
}

/// Saves the report of the run and gives the path of the saved report of
/// the last run. A recheck also replaces its `--report` file. A recheck of
/// named mutants saves nothing, so the other open tasks stay in the saved
/// reports.
fn save_reports(
    input: &MutationCliInput<'_>,
    analysis: &AnalysisOptions,
    value: &serde_json::Value,
) -> Option<PathBuf> {
    if !input.args.mutants.is_empty() {
        return None;
    }
    let saved = save_report(analysis, value);
    if let Some(path) = recheck_report_file(input) {
        replace_report_file(path, value, input.json_style, saved.as_deref());
    }
    saved
}

/// The `--report` file of a recheck, which the run replaces. The run writes
/// it after the run, so the worktree check leaves it out, as it does for the
/// `--output-file`.
fn recheck_report_file<'a>(input: &'a MutationCliInput<'_>) -> Option<&'a Path> {
    input
        .args
        .recheck
        .then_some(input.args.report.as_deref())
        .flatten()
}

/// Gives `temporary` the permissions of the file at `target`, when it
/// exists.
fn keep_permissions(target: &Path, temporary: &Path) -> std::io::Result<()> {
    match std::fs::metadata(target) {
        Ok(metadata) => std::fs::set_permissions(temporary, metadata.permissions()),
        Err(_) => Ok(()),
    }
}

/// Replaces the `--report` file of a recheck with the merged report, so a
/// later `fallow mutation tasks --report <file>` lists the open mutants. The
/// write goes to a temporary file in the directory of the target first, so
/// a reader never sees half a report. A symlink stays a symlink, and the
/// file keeps its permissions. A file that is the saved report `saved` has the
/// report already. A failed write is a warning, not a run error.
fn replace_report_file(
    path: &Path,
    value: &serde_json::Value,
    style: JsonStyle,
    saved: Option<&Path>,
) {
    let same = |saved: &Path| dunce::canonicalize(saved).ok() == dunce::canonicalize(path).ok();
    if saved.is_some_and(same) {
        return;
    }
    let result = style
        .serialize(value)
        .map_err(|error| error.to_string())
        .and_then(|json| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .ok_or_else(|| "the path has no file name".to_owned())?;
            let target = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let temporary = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
            std::fs::write(&temporary, format!("{json}\n"))
                .and_then(|()| keep_permissions(&target, &temporary))
                .and_then(|()| std::fs::rename(&temporary, &target))
                .map_err(|error| {
                    let _ = std::fs::remove_file(&temporary);
                    error.to_string()
                })
        });
    if let Err(error) = result {
        eprintln!(
            "fallow: warning: the recheck did not replace {}: {error}",
            path.display()
        );
    }
}

/// Removes the outcome journal after a complete run whose report is saved.
/// A failed removal is a warning: a later run without `--resume` replaces
/// the journal.
fn remove_journal(analysis: &AnalysisOptions, report: &MutationReport) {
    if let Err(error) = fallow_api::mutation::remove_finished_mutation_journal(analysis, report) {
        eprintln!(
            "fallow: warning: the mutation journal of the complete run was not removed: {}",
            error.message
        );
    }
}

/// Adds the locations of the report to the help of a run error, so that a
/// JSON caller that gets the error envelope knows where to read the report.
fn with_report_locations(
    error: ProgrammaticError,
    saved: Option<&Path>,
    output_file: Option<&Path>,
    json: bool,
) -> ProgrammaticError {
    let locations: Vec<String> = saved
        .into_iter()
        .chain(output_file)
        .map(|path| path.display().to_string())
        .collect();
    let location = if locations.is_empty() {
        if !json {
            return error;
        }
        "the report of this run is on stdout".to_owned()
    } else {
        format!("the report of this run is in {}", locations.join(" and "))
    };
    let help = match &error.help {
        Some(help) => format!("{help}; {location}"),
        None => location,
    };
    error.with_help(help)
}

fn emit_json(value: &serde_json::Value, style: JsonStyle) -> Result<(), ProgrammaticError> {
    let json = style.serialize(value).map_err(|error| {
        ProgrammaticError::new(format!("failed to serialize mutation JSON: {error}"), 2)
    })?;
    crate::report::sink::outln!("{json}");
    Ok(())
}

fn emit_plan(plan: MutationPlanOutput, input: &MutationCliInput<'_>) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    crate::telemetry::note_result_count(plan.counts.planned);
    if let Some(path) = input.output_file
        && let Err(code) = crate::redirect_report_to_file(path, output)
    {
        return code;
    }
    if matches!(output, OutputFormat::Json) {
        let result = serialize_mutation_plan_json_output(plan)
            .map_err(|error| {
                ProgrammaticError::new(format!("failed to serialize the mutation plan: {error}"), 2)
            })
            .and_then(|value| emit_json(&value, style));
        if let Err(error) = result {
            return crate::error::emit_programmatic_error(&error, output, style);
        }
    } else {
        for line in render_plan(&plan).lines() {
            crate::report::sink::outln!("{line}");
        }
    }
    if let Some(path) = input.output_file
        && let Err(code) = crate::finalize_report_file(path, input.quiet, output)
    {
        return code;
    }
    ExitCode::SUCCESS
}

/// The saved report that `inspect` reads and the project root of its paths.
///
/// With `--report`, the root is the given root. Without it, inspect reads the
/// report that the last run saved in the Fallow cache. When neither
/// `--root` nor `--config` is set, the search walks up from the working
/// directory to the git worktree root, so an inspect from a subdirectory
/// finds the report of a run at the project root.
fn inspect_report_path(
    input: &MutationCliInput<'_>,
    report: Option<&Path>,
) -> Result<(PathBuf, PathBuf), ProgrammaticError> {
    if let Some(report) = report {
        return Ok((report.to_path_buf(), input.root.to_path_buf()));
    }
    let saved_report = |root: &Path| {
        fallow_api::mutation::last_mutation_report_path(&AnalysisOptions {
            root: Some(root.to_path_buf()),
            config_path: input.config_path.map(Path::to_path_buf),
            allow_remote_extends: input.allow_remote_extends,
            ..AnalysisOptions::default()
        })
    };
    let path = saved_report(input.root)?;
    if path.is_file() {
        return Ok((path, input.root.to_path_buf()));
    }
    if input.root_flag.is_none() && input.config_path.is_none() {
        for ancestor in project_ancestors(input.root) {
            if let Ok(path) = saved_report(&ancestor)
                && path.is_file()
            {
                return Ok((path, ancestor));
            }
        }
    }
    Err(ProgrammaticError::new(
        "no saved mutation report: no run of `fallow mutation` saved one for this project",
        INPUT_EXIT_CODE,
    )
    .with_code("FALLOW_MUTATION_NO_REPORT")
    .with_context("mutation.report")
    .with_help(
        "pass the --root and --config of the run, or pass --report <file> with a saved mutation report or `--plan --format json` document",
    ))
}

/// The parent directories of `root`, nearest first, up to and including the
/// first one with a `.git` entry. A `root` outside a git worktree has none,
/// so the search never leaves the project.
fn project_ancestors(root: &Path) -> Vec<PathBuf> {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if root.join(".git").exists() {
        return Vec::new();
    }
    let mut ancestors = Vec::new();
    for ancestor in root.ancestors().skip(1) {
        ancestors.push(ancestor.to_path_buf());
        if ancestor.join(".git").exists() {
            return ancestors;
        }
    }
    Vec::new()
}

/// The replay flags of `fallow mutation inspect`.
#[derive(Debug, Clone, Copy)]
struct ReplayFlags<'a> {
    args: Option<&'a str>,
    export: Option<&'a str>,
    call_timeout: Option<&'a str>,
}

/// Parses the `--args` value: a JSON array of replay values. Without
/// `--args`, the replay uses the recorded inputs.
fn replay_args(text: Option<&str>) -> Result<Option<Vec<serde_json::Value>>, ProgrammaticError> {
    let Some(text) = text else {
        return Ok(None);
    };
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Array(args)) => Ok(Some(args)),
        Ok(_) => Err(input_error(
            "--args must be a JSON array, for example [2, 3]",
            "mutation.replay.args",
        )),
        Err(error) => Err(input_error(
            format!("--args is not valid JSON: {error}"),
            "mutation.replay.args",
        )),
    }
}

/// Replays the mutant of a fresh packet and adds the result. A stale packet
/// cannot replay, because the current file no longer gives the mutant of
/// the report.
fn add_replay(
    input: &MutationCliInput<'_>,
    packet: &mut fallow_output::MutationInspectOutput,
    root: &Path,
    flags: ReplayFlags<'_>,
) -> Result<(), ProgrammaticError> {
    let args = replay_args(flags.args)?;
    if packet.status != fallow_output::MutationInspectStatus::Fresh {
        return Err(ProgrammaticError::new(
            format!(
                "the mutant `{}` is stale, so it cannot replay",
                packet.location.mutant_id
            ),
            INPUT_EXIT_CODE,
        )
        .with_code("FALLOW_MUTATION_REPLAY_STALE")
        .with_context("mutation.replay")
        .with_help("run `fallow mutation` again for a report of the current files"));
    }
    let call_timeout = match flags.call_timeout {
        Some(text) => Some(parse_duration(text).ok_or_else(|| {
            input_error(
                format!("--call-timeout `{text}` is not a duration such as 500ms, 30s or 2m"),
                "mutation.replay.callTimeout",
            )
        })?),
        None => None,
    };
    let options = run_options(input)?;
    let replay = fallow_api::replay_mutation(&fallow_api::MutationReplayOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            ..options.analysis
        },
        file: packet.location.file.clone(),
        mutant_id: packet.location.mutant_id.clone(),
        span: packet.location.span,
        operator: fallow_api::mutation::Operator::from_name(&packet.location.operator),
        export: flags.export.map(str::to_owned),
        args,
        runtime: options.runtime,
        runner_config: options.runner_config,
        call_timeout,
        runner_timeout: options.runner_timeout,
    })?;
    packet.replay = Some(replay);
    Ok(())
}

fn run_inspect(
    input: &MutationCliInput<'_>,
    mutant_id: &str,
    report: Option<&Path>,
    replay: Option<ReplayFlags<'_>>,
) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    let packet = inspect_report_path(input, report).and_then(|(path, root)| {
        let saved = crate::mutation_inspect::read_saved_report(&path)?;
        let mut packet = crate::mutation_inspect::inspect(&saved, mutant_id, &root)?;
        if let Some(flags) = replay {
            add_replay(input, &mut packet, &root, flags)?;
        }
        Ok(packet)
    });
    let packet = match packet {
        Ok(packet) => packet,
        Err(error) => return crate::error::emit_programmatic_error(&error, output, style),
    };
    crate::telemetry::note_result_count(1);
    if let Some(path) = input.output_file
        && let Err(code) = crate::redirect_report_to_file(path, output)
    {
        return code;
    }
    if matches!(output, OutputFormat::Json) {
        let result = serialize_mutation_inspect_json_output(packet)
            .map_err(|error| {
                ProgrammaticError::new(
                    format!("failed to serialize the inspect packet: {error}"),
                    2,
                )
            })
            .and_then(|value| emit_json(&value, style));
        if let Err(error) = result {
            return crate::error::emit_programmatic_error(&error, output, style);
        }
    } else {
        for line in render_inspect(&packet).lines() {
            crate::report::sink::outln!("{line}");
        }
    }
    if let Some(path) = input.output_file
        && let Err(code) = crate::finalize_report_file(path, input.quiet, output)
    {
        return code;
    }
    ExitCode::SUCCESS
}

#[path = "mutation_tasks.rs"]
mod tasks;

fn run_tasks(input: &MutationCliInput<'_>, report: Option<&Path>) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    if !matches!(
        output,
        OutputFormat::Human | OutputFormat::Json | OutputFormat::Markdown
    ) {
        return crate::error::emit_input_error_with_style(
            "`fallow mutation tasks` supports only human, json and markdown output",
            output,
            style,
        );
    }
    let batch = inspect_report_path(input, report).and_then(|(path, root)| {
        let saved = crate::mutation_inspect::read_saved_report(&path)?;
        let analysis = AnalysisOptions {
            root: Some(root.clone()),
            config_path: input.config_path.map(Path::to_path_buf),
            allow_remote_extends: input.allow_remote_extends,
            ..AnalysisOptions::default()
        };
        // Markdown can go into a PR comment, so it never holds a value.
        let markdown = matches!(input.output, OutputFormat::Markdown);
        let store = (!markdown)
            .then(|| fallow_api::mutation::mutation_witness_store(&analysis))
            .flatten();
        let values = |mutant_id: &str, input_id: &str| {
            store
                .as_ref()
                .and_then(|store| store.values(mutant_id, input_id))
        };
        tasks::tasks(&saved, &root, &values)
    });
    let batch = match batch {
        Ok(batch) => batch,
        Err(error) => return crate::error::emit_programmatic_error(&error, output, style),
    };
    crate::telemetry::note_result_count(batch.tasks.len());
    if let Some(path) = input.output_file
        && let Err(code) = crate::redirect_report_to_file(path, output)
    {
        return code;
    }
    let text = match output {
        OutputFormat::Json => {
            let result = serialize_mutation_tasks_json_output(batch)
                .map_err(|error| {
                    ProgrammaticError::new(
                        format!("failed to serialize the mutation tasks: {error}"),
                        INPUT_EXIT_CODE,
                    )
                })
                .and_then(|value| emit_json(&value, style));
            if let Err(error) = result {
                return crate::error::emit_programmatic_error(&error, output, style);
            }
            None
        }
        OutputFormat::Markdown => Some(tasks::render_markdown(&batch)),
        _ => Some(tasks::render_text(&batch)),
    };
    for line in text.iter().flat_map(|text| text.lines()) {
        crate::report::sink::outln!("{line}");
    }
    if let Some(path) = input.output_file
        && let Err(code) = crate::finalize_report_file(path, input.quiet, output)
    {
        return code;
    }
    ExitCode::SUCCESS
}

#[path = "mutation_export.rs"]
mod export;

/// Writes the saved report as a `mutation-testing-report-schema` document.
/// The document is JSON with both `--format human` and `--format json`.
fn run_export(input: &MutationCliInput<'_>, report: Option<&Path>) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    if !matches!(output, OutputFormat::Human | OutputFormat::Json) {
        return crate::error::emit_input_error_with_style(
            "`fallow mutation export` writes JSON; it supports only human and json output",
            output,
            style,
        );
    }
    let document = inspect_report_path(input, report).and_then(|(path, root)| {
        let saved = crate::mutation_inspect::read_saved_report(&path)?;
        export::export(&saved, &root)
    });
    let document = match document {
        Ok(document) => document,
        Err(error) => return crate::error::emit_programmatic_error(&error, output, style),
    };
    crate::telemetry::note_result_count(document["files"].as_object().map_or(0, |files| {
        files
            .values()
            .filter_map(|file| file["mutants"].as_array())
            .map(Vec::len)
            .sum()
    }));
    if let Some(path) = input.output_file
        && let Err(code) = crate::redirect_report_to_file(path, output)
    {
        return code;
    }
    if let Err(error) = emit_json(&document, style) {
        return crate::error::emit_programmatic_error(&error, output, style);
    }
    if let Some(path) = input.output_file
        && let Err(code) = crate::finalize_report_file(path, input.quiet, output)
    {
        return code;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
#[path = "mutation_cli_tests.rs"]
mod tests;
