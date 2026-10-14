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
use fallow_api::mutation::{MAX_VERDICT_INPUT_BYTES, Operator};
use fallow_api::{
    AnalysisOptions, MutationRunOptions, MutationRunScope, ProgrammaticError,
    apply_mutation_verdicts, plan_mutation, run_mutation, validate_mutation_verdicts,
};
use fallow_config::{MutationConfig, OutputFormat};
use fallow_output::{
    MutantStatus, MutationPlanOutput, MutationReport, serialize_mutation_inspect_json_output,
    serialize_mutation_json_output, serialize_mutation_plan_json_output,
};

use crate::json_style::JsonStyle;
use crate::mutation_human::{
    CodeExcerpt, CodeLookup, RunReportOptions, render_inspect, render_plan, render_run,
};

/// The exit code of a run with a score below the minimum.
const MIN_SCORE_EXIT_CODE: u8 = 1;
/// The exit code of invalid input and execution errors.
const INPUT_EXIT_CODE: u8 = 2;

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
    /// Run at most N mutants, in rank order. The others are `pending`.
    #[arg(long, value_name = "N")]
    pub max_mutants: Option<usize>,
    /// Stop starting mutants after this time, for example `90s`, `10m` or `1h`.
    /// The others are `pending`.
    #[arg(long, value_name = "DURATION")]
    pub time_budget: Option<String>,
    /// The number of companion processes (default: half the logical cores).
    #[arg(long, value_name = "N")]
    pub concurrency: Option<usize>,
    /// The JavaScript runtime (default: `node`, then `bun`, from PATH).
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
    /// Exit with code 1 when the mutation score is below this percentage.
    #[arg(long, value_name = "0-100")]
    pub min_score: Option<f64>,
    /// The operators to use, comma-separated (default: each operator).
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
    /// Type-check each survived mutant through the `--type-aware`
    /// companion. A survivor that does not type-check becomes
    /// `compile_error` and leaves the score.
    #[arg(long)]
    pub typecheck: bool,
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
        allow_dirty: input.args.allow_dirty,
        runtime: input.args.runtime.clone(),
        concurrency: input.args.concurrency.or(input.config.concurrency),
        time_budget,
        runner_timeout,
        output_paths: input
            .output_file
            .map(absolute_output_path)
            .into_iter()
            .collect(),
        typecheck: input.args.typecheck || input.config.typecheck == Some(true),
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

/// The exit code of a finished run: 1 only when a minimum score is set and
/// the measured score is lower. Survivors alone never fail a run. A run with no counted mutant
/// has no score and passes.
#[must_use]
pub fn run_exit_code(report: &MutationReport, min_score: Option<f64>) -> u8 {
    match (min_score, report.summary.score) {
        (Some(minimum), Some(score)) if score * 100.0 < minimum => MIN_SCORE_EXIT_CODE,
        _ => 0,
    }
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
    if !matches!(output, OutputFormat::Human | OutputFormat::Json) {
        return crate::error::emit_error_with_style(
            "`fallow mutation` supports only human and json output",
            INPUT_EXIT_CODE,
            output,
            style,
        );
    }
    if let Some(MutationSubcommand::Inspect { mutant_id, report }) = &input.subcommand {
        return run_inspect(input, mutant_id, report.as_deref());
    }
    match run_inner(input) {
        Ok(code) => code,
        Err(error) => crate::error::emit_programmatic_error(&error, output, style),
    }
}

fn run_inner(input: &MutationCliInput<'_>) -> Result<ExitCode, ProgrammaticError> {
    let options = run_options(input)?;
    let min_score = min_score(input)?;
    if input.args.plan {
        if input.args.verdicts.is_some() {
            return Err(input_error(
                "--verdicts joins the results of a run; a plan has no results",
                "mutation.verdicts",
            ));
        }
        if input.args.typecheck {
            return Err(input_error(
                "--typecheck checks the survivors of a run; a plan has no survivors",
                "mutation.typecheck",
            ));
        }
        let plan = plan_mutation(&options)?;
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
    let mut report = run_mutation(&options)?;
    complete_file_digests(&mut report, input.root);
    if let Some(verdicts) = verdicts {
        apply_mutation_verdicts(&mut report, &verdicts)?;
    }
    let exit = run_exit_code(&report, min_score);
    crate::telemetry::note_result_count(report.summary.counts.survived);
    let value = serialize_mutation_json_output(report.clone()).map_err(|error| {
        ProgrammaticError::new(
            format!("failed to serialize the mutation report: {error}"),
            2,
        )
    })?;
    let saved = match fallow_api::mutation::save_last_mutation_report(&options.analysis, &value) {
        Ok(_) => true,
        Err(error) => {
            eprintln!(
                "fallow: warning: the mutation report was not saved for `fallow mutation inspect`: {}",
                error.message
            );
            false
        }
    };
    if let Some(path) = input.output_file
        && let Err(code) = crate::redirect_report_to_file(path, input.output)
    {
        return Ok(code);
    }
    if matches!(input.output, OutputFormat::Json) {
        emit_json(&value, input.json_style)?;
    } else {
        let code = code_lookup(&report, input.root);
        let flags = inspect_flags(input);
        let text = render_run(
            &report,
            &RunReportOptions {
                min_score,
                code: Some(&code),
                inspect_flags: saved.then_some(flags.as_str()),
            },
        );
        for line in text.lines() {
            crate::report::sink::outln!("{line}");
        }
    }
    if let Some(path) = input.output_file
        && let Err(code) = crate::finalize_report_file(path, input.quiet, input.output)
    {
        return Ok(code);
    }
    Ok(ExitCode::from(exit))
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

fn run_inspect(input: &MutationCliInput<'_>, mutant_id: &str, report: Option<&Path>) -> ExitCode {
    let (output, style) = (input.output, input.json_style);
    let packet = inspect_report_path(input, report).and_then(|(path, root)| {
        let saved = crate::mutation_inspect::read_saved_report(&path)?;
        crate::mutation_inspect::inspect(&saved, mutant_id, &root)
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

#[cfg(test)]
#[path = "mutation_cli_tests.rs"]
mod tests;
