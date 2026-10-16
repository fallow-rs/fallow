//! Orchestration entry points of mutation testing for CLI, MCP and NAPI.
//!
//! [`plan_mutation`] runs only the static half and never executes user code.
//! [`run_mutation`] plans through the engine, then runs the tests of the
//! project against each planned mutant through the embedded companion.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fallow_engine::mutation::{
    DiffIndex, MutantDisposition, MutationPlan, MutationPlanRequest, MutationScope,
    plan_mutations_for_session_with,
};
use fallow_engine::session::AnalysisSession;
use fallow_output::{
    MutationCacheOutput, MutationPlanOutput, MutationPlanSchemaVersion, MutationReport,
    MutationRunnerOutput, MutationRuntimeOutput, MutationSchemaVersion, MutationScopeKind,
    MutationScopeOutput, MutationTimings, MutationWorktreeCheck,
};
use fallow_types::envelope::ElapsedMs;
use rustc_hash::FxHashSet;

use crate::ProgrammaticError;
use crate::analysis_context::{ProgrammaticAnalysisContext, resolve_programmatic_analysis_context};
use crate::mutation::companion::{COMPANION_FILES, install};
use crate::mutation::discovery::discover_runtime;
use crate::mutation::last_report::ignore_cache_dir;
use crate::mutation::manifest::{RunnerRange, manifest};
use crate::mutation::node_args::node_test_args;
use crate::mutation::protocol::{InitOptions, InitRequest};
use crate::mutation::report::{
    CompletionFacts, baseline_failures, completion, function_results, issues, mutant_results,
    plan_counts, plan_files, plan_mutants, skipped_files, summary, tool_version,
};
use crate::mutation::result_cache::{KeyFiles, NOT_KEYED, ResultCache};
use crate::mutation::run_dir::{RunFile, write_run_dir};
use crate::mutation::runner_select::{Runner, select_runner};
use crate::mutation::scheduler::{
    ResultLookup, ScheduleConfig, ScheduleError, ScheduleIssue, ScheduleOutcome, SchedulePlan,
    ScheduledMutant, run_schedule,
};
use crate::mutation::transport::ProcessSpawner;
use crate::mutation::typecheck::typecheck_survivors;
use crate::mutation::worktree::{Exclusions, WorktreeSnapshot, canonical_path, snapshot};
use crate::mutation::{MutationRunOptions, MutationRunScope};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// The default timeout of `init` and the dry run (`protocol.md` section 8).
const DEFAULT_RUNNER_TIMEOUT: Duration = Duration::from_secs(120);

fn mutation_error(code: &str, message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2).with_code(code)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a run lasts far less than 2^64 milliseconds"
)]
const fn millis(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

fn validate(options: &MutationRunOptions) -> ProgrammaticResult<()> {
    if options.concurrency == Some(0) {
        return Err(mutation_error(
            "FALLOW_INVALID_MUTATION_OPTIONS",
            "the mutation concurrency must be at least 1",
        )
        .with_context("mutation.concurrency"));
    }
    if options.max_mutants == Some(0) {
        return Err(mutation_error(
            "FALLOW_INVALID_MUTATION_OPTIONS",
            "the mutant limit must be at least 1",
        )
        .with_context("mutation.maxMutants"));
    }
    if options.time_budget.is_some_and(|budget| budget.is_zero())
        || options
            .runner_timeout
            .is_some_and(|timeout| timeout.is_zero())
    {
        return Err(mutation_error(
            "FALLOW_INVALID_MUTATION_OPTIONS",
            "a mutation time limit must be more than zero",
        ));
    }
    Ok(())
}

fn load_session(resolved: &ProgrammaticAnalysisContext) -> ProgrammaticResult<AnalysisSession> {
    let mut project = fallow_engine::project_config::config_for_project_with_load_options(
        resolved.root(),
        resolved.config_path().as_deref(),
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: resolved.allow_remote_extends,
        },
    )
    .map_err(|error| {
        ProgrammaticError::new(format!("failed to load config: {error}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    project.config.no_cache = resolved.no_cache;
    project.config.threads = resolved.threads;
    Ok(AnalysisSession::from_config(project))
}

fn scope_output(options: &MutationRunOptions, base_ref: Option<String>) -> MutationScopeOutput {
    MutationScopeOutput {
        kind: match options.scope {
            MutationRunScope::Changed { .. } => MutationScopeKind::Changed,
            MutationRunScope::All => MutationScopeKind::All,
            MutationRunScope::Paths(_) => MutationScopeKind::Paths,
        },
        base_ref,
    }
}

/// The engine scope, and the base ref of a changed scope.
fn engine_scope(
    options: &MutationRunOptions,
    resolved: &ProgrammaticAnalysisContext,
) -> ProgrammaticResult<(MutationScope, Option<String>)> {
    let root = resolved.root();
    match &options.scope {
        MutationRunScope::All => Ok((MutationScope::All, None)),
        MutationRunScope::Paths(paths) => Ok((MutationScope::Paths(paths.clone()), None)),
        MutationRunScope::Changed { base } => {
            if let Some(diff) = &resolved.diff {
                return Ok((MutationScope::ChangedSinceBase(diff.clone()), None));
            }
            let resolved_base = crate::audit_run::resolve_audit_base(root, base.as_deref())
                .map_err(|error| {
                    let message = match error {
                        crate::audit_run::AuditBaseError::InvalidRef { value, reason, .. } => {
                            format!("invalid base ref `{value}`: {reason}")
                        }
                        crate::audit_run::AuditBaseError::NotDetected => {
                            "no base branch to compare against; pass a base ref or mutate all files"
                                .to_owned()
                        }
                    };
                    mutation_error("FALLOW_MUTATION_BASE_NOT_FOUND", message)
                        .with_context("mutation.base")
                })?;
            let text =
                fallow_engine::changed_files::try_get_changed_diff(root, &resolved_base.git_ref)
                    .map_err(|error| {
                        mutation_error(
                            "FALLOW_CHANGED_FILES_FAILED",
                            format!(
                                "cannot diff against `{}`: {}",
                                resolved_base.git_ref,
                                error.describe()
                            ),
                        )
                    })?;
            let label = format!("git diff {}", resolved_base.git_ref);
            let index = fallow_engine::diff_source::place_diff(
                DiffIndex::from_unified_diff(&text),
                root,
                &fallow_engine::diff_source::diff_base_candidates(root),
                &label,
            )
            .map_err(|stand_down| {
                let (_, message) = stand_down.into_parts();
                mutation_error("FALLOW_CHANGED_FILES_FAILED", message)
            })?;
            Ok((
                MutationScope::ChangedSinceBase(index),
                Some(resolved_base.git_ref),
            ))
        }
    }
}

/// The operators of the `mutation.operators` config key. The CLI and MCP
/// callers pass `None` when the user names no operator, so both apply this
/// one rule.
fn config_operators(
    session: &AnalysisSession,
) -> ProgrammaticResult<Option<Vec<fallow_engine::mutation::Operator>>> {
    let Some(names) = &session.config().mutation.operators else {
        return Ok(None);
    };
    names
        .iter()
        .map(|name| {
            fallow_engine::mutation::Operator::from_name(name.as_str()).ok_or_else(|| {
                mutation_error(
                    "FALLOW_INVALID_MUTATION_OPTIONS",
                    format!("unknown mutation operator `{}`", name.as_str()),
                )
                .with_context("mutation.operators")
            })
        })
        .collect::<ProgrammaticResult<Vec<_>>>()
        .map(Some)
}

struct Planned {
    resolved: ProgrammaticAnalysisContext,
    cache_dir: PathBuf,
    /// The test runner, selected from the config value and the first plan.
    runner: Runner,
    plan: MutationPlan,
    base_ref: Option<String>,
}

fn plan_for(
    options: &MutationRunOptions,
    before_plan: impl FnOnce(&Path, &Path) -> ProgrammaticResult<()>,
) -> ProgrammaticResult<Planned> {
    validate(options)?;
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    let session = load_session(&resolved)?;
    let cache_dir = session.config().cache_dir.clone();
    let configured = session.config().mutation.runner;
    before_plan(resolved.root(), &cache_dir)?;
    let (scope, base_ref) = engine_scope(options, &resolved)?;
    let operators = match &options.operators {
        Some(operators) => Some(operators.clone()),
        None => config_operators(&session)?,
    };
    let request = MutationPlanRequest {
        scope,
        operators,
        allow_side_effects: options.allow_side_effects,
        max_mutants: options.max_mutants,
        skip_component_files: false,
    };
    // The runner decision reads the test files of the first plan. A second
    // plan without the component files keeps that decision.
    let mut selected = None;
    let plan = resolved
        .install(|| {
            plan_mutations_for_session_with(&session, &request, |plan| {
                let runner = select_runner(configured, resolved.root(), &plan.test_files);
                selected = Some(runner);
                !runner.loads_component_files()
            })
        })
        .map_err(|error| mutation_error("FALLOW_MUTATION_PLAN_FAILED", error.to_string()))?;
    let runner =
        selected.unwrap_or_else(|| select_runner(configured, resolved.root(), &plan.test_files));
    Ok(Planned {
        resolved,
        cache_dir,
        runner,
        plan,
        base_ref,
    })
}

/// Builds the static mutation plan. It runs no user code.
///
/// # Errors
///
/// Returns an error for invalid options, a base ref that does not resolve,
/// or a failed analysis.
pub fn plan_mutation(options: &MutationRunOptions) -> ProgrammaticResult<MutationPlanOutput> {
    let started = Instant::now();
    let planned = plan_for(options, |_, _| Ok(()))?;
    let plan = &planned.plan;
    Ok(MutationPlanOutput {
        schema_version: MutationPlanSchemaVersion::V1,
        version: tool_version(),
        elapsed_ms: ElapsedMs(millis(started.elapsed())),
        scope: scope_output(options, planned.base_ref.clone()),
        counts: plan_counts(plan),
        mutants: plan_mutants(plan),
        files: plan_files(plan),
        test_files: plan.test_files.clone(),
        no_test_files: plan.no_test_files,
        framework_config_checked: plan.framework_config_checked,
        skipped_files: skipped_files(plan),
        unmatched_paths: plan.unmatched_paths.clone(),
    })
}

fn unsafe_cache_dir(message: String) -> ProgrammaticError {
    mutation_error("FALLOW_MUTATION_UNSAFE_CACHE_DIR", message)
        .with_help(
            "set cache.dir or FALLOW_CACHE_DIR to a directory that holds no project file, such as .fallow",
        )
        .with_context("cache.dir")
}

/// Refuses a cache directory that is the root or a directory above it. The
/// worktree check leaves the mutation cache out, and the analysis writes a
/// `.gitignore` that ignores the whole cache directory, so such a cache
/// directory hides each stray write from the check.
fn guard_cache_dir(root: &Path, cache_dir: &Path) -> ProgrammaticResult<()> {
    if !canonical_path(root).starts_with(canonical_path(cache_dir)) {
        return Ok(());
    }
    Err(unsafe_cache_dir(format!(
        "the cache directory `{}` is the project root or a directory above it, so the worktree check cannot see stray writes",
        cache_dir.display()
    )))
}

/// Refuses a cache directory that holds a planned source or test file.
fn guard_cache_holds_no_planned_file(
    root: &Path,
    cache_dir: &Path,
    plan: &MutationPlan,
) -> ProgrammaticResult<()> {
    let root = canonical_path(root);
    let cache = canonical_path(cache_dir);
    let inside = plan
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(plan.test_files.iter().map(String::as_str))
        .find(|path| root.join(path).starts_with(&cache));
    let Some(path) = inside else {
        return Ok(());
    };
    Err(unsafe_cache_dir(format!(
        "the cache directory `{}` holds the project file `{path}`, so the worktree check cannot see stray writes",
        cache_dir.display()
    )))
}

fn take_worktree_snapshot(
    root: &Path,
    cache_dir: &Path,
    output_paths: &[PathBuf],
    start: Option<&WorktreeSnapshot>,
) -> ProgrammaticResult<Option<WorktreeSnapshot>> {
    // The caller writes a relative output path against the process
    // directory, which can be below the root.
    let cwd = std::env::current_dir().unwrap_or_else(|_| root.to_path_buf());
    let outputs = output_paths.iter().map(|path| cwd.join(path));
    // Only the mutation cache is left out of the status check. The analysis
    // caches next to it are git-ignored, and an ignored file changes only
    // in the ignored-file check, so the whole cache is left out there.
    let exclusions = Exclusions {
        paths: std::iter::once(cache_dir.join("mutation"))
            .chain(outputs)
            .collect(),
        ignored: vec![cache_dir.to_path_buf()],
    };
    snapshot(root, &exclusions, start)
        .map_err(|message| mutation_error("FALLOW_MUTATION_WORKTREE_CHECK_FAILED", message))
}

/// The worktree facts at the end of a run.
struct WorktreeEnd {
    /// The paths that changed since the start snapshot.
    changed_paths: Vec<String>,
    /// Why the end snapshot failed. Stray writes are then unknown.
    check_error: Option<String>,
}

/// Compares the worktree with `start`. A failed end snapshot is a fact of
/// the report, never a reason to drop the run result or its error.
fn settle_worktree(
    start: Option<&WorktreeSnapshot>,
    take_end: impl FnOnce(&WorktreeSnapshot) -> ProgrammaticResult<Option<WorktreeSnapshot>>,
) -> WorktreeEnd {
    let Some(start) = start else {
        return WorktreeEnd {
            changed_paths: Vec::new(),
            check_error: None,
        };
    };
    match take_end(start) {
        Ok(end) => WorktreeEnd {
            changed_paths: end.map(|end| start.changed_paths(&end)).unwrap_or_default(),
            check_error: None,
        },
        Err(error) => WorktreeEnd {
            changed_paths: Vec::new(),
            check_error: Some(error.message),
        },
    }
}

/// Adds the worktree facts to the error of a failed run.
fn with_worktree_facts(mut error: ProgrammaticError, end: &WorktreeEnd) -> ProgrammaticError {
    if !end.changed_paths.is_empty() {
        error.message = format!(
            "{}; the run changed these worktree paths: {}",
            error.message,
            end.changed_paths.join(", ")
        );
    }
    if let Some(check_error) = &end.check_error {
        error.message = format!(
            "{}; the end-of-run worktree check failed: {check_error}",
            error.message
        );
    }
    error
}

const fn path_noun(count: usize) -> &'static str {
    if count == 1 { "path" } else { "paths" }
}

fn guard_worktree(allow_dirty: bool, start: Option<&WorktreeSnapshot>) -> ProgrammaticResult<()> {
    if allow_dirty {
        return Ok(());
    }
    let Some(start) = start else {
        return Err(mutation_error(
            "FALLOW_MUTATION_NOT_A_REPOSITORY",
            "mutation testing runs the tests against changed code and needs a git worktree to detect stray writes",
        )
        .with_help("run inside a git worktree, or pass --allow-dirty"));
    };
    let dirty = start.dirty_paths();
    if dirty.is_empty() {
        return Ok(());
    }
    let shown: Vec<&str> = dirty.iter().take(10).map(String::as_str).collect();
    Err(mutation_error(
        "FALLOW_MUTATION_DIRTY_WORKTREE",
        format!(
            "the git worktree has {} uncommitted {}: {}",
            dirty.len(),
            path_noun(dirty.len()),
            shown.join(", ")
        ),
    )
    .with_help("commit or stash the changes, or pass --allow-dirty"))
}

/// The help of a runner version outside `range`. It names the supported
/// range as an install step. When the minimum is not a `.0.0` release, it
/// also says that a minor update inside the major version is enough.
fn runner_version_help(range: &RunnerRange) -> String {
    let RunnerRange {
        package,
        min_version,
        max_version_exclusive,
        ..
    } = range;
    let install =
        format!("install {package} {min_version} or a later version below {max_version_exclusive}");
    match min_version.split_once('.') {
        Some((major, rest)) if rest != "0.0" => {
            format!("{install}; a {package} {major}.x release from {min_version} is in the range")
        }
        _ => install,
    }
}

/// Maps a schedule error to its public error. A setup watchdog fault names
/// `--setup-timeout`, because a slow or busy host is the usual cause. A
/// runner version error names the supported range of `runner`.
fn schedule_error(
    error: &ScheduleError,
    runner: &RunnerRange,
    setup_timeout: Duration,
) -> ProgrammaticError {
    let (code, stderr_tail) = match error {
        ScheduleError::Integrity(_) => ("FALLOW_MUTATION_COMPANION_INTEGRITY", None),
        ScheduleError::RunnerVersion(_) => ("FALLOW_MUTATION_RUNNER_UNSUPPORTED", None),
        ScheduleError::Setup { stderr_tail, .. } => {
            ("FALLOW_MUTATION_RUNNER_FAILED", stderr_tail.clone())
        }
        ScheduleError::UnknownHit(_) => ("FALLOW_MUTATION_PLAN_MISMATCH", None),
        ScheduleError::Baseline(_) => ("FALLOW_MUTATION_BASELINE_FAILED", None),
    };
    let mut help = Vec::new();
    if matches!(error, ScheduleError::RunnerVersion(_)) {
        help.push(runner_version_help(runner));
    }
    if matches!(
        error,
        ScheduleError::Setup {
            timed_out: true,
            ..
        }
    ) {
        help.push(format!(
            "the runner setup took longer than {}s; on a slow or busy host, pass --setup-timeout with a longer duration, for example 5m",
            setup_timeout.as_secs()
        ));
    }
    if let Some(tail) = stderr_tail.filter(|tail| !tail.trim().is_empty()) {
        help.push(format!("companion stderr:\n{}", tail.trim_end()));
    }
    let public = mutation_error(code, error.to_string());
    if help.is_empty() {
        public
    } else {
        public.with_help(help.join("\n"))
    }
}

fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .div_ceil(2)
        .max(1)
}

fn runtime_error(runner: Runner, message: String) -> ProgrammaticError {
    mutation_error("FALLOW_MUTATION_RUNTIME_NOT_FOUND", message)
        .with_help(runner.runtime_help())
        .with_context("mutation.runtime")
}

fn scheduled(plan: &MutationPlan, disposition: MutantDisposition) -> Vec<ScheduledMutant> {
    plan.mutants
        .iter()
        .filter(|mutant| mutant.disposition == disposition)
        .map(|mutant| ScheduledMutant {
            id: mutant.mutant.id.as_str().to_owned(),
            file: mutant.file.clone(),
        })
        .collect()
}

fn schedule_plan(plan: &MutationPlan) -> SchedulePlan {
    SchedulePlan {
        mutants: scheduled(plan, MutantDisposition::Planned),
        unreached: scheduled(plan, MutantDisposition::NoCoverage),
        pending: scheduled(plan, MutantDisposition::Pending),
        known_ids: plan
            .mutants
            .iter()
            .map(|mutant| mutant.mutant.id.as_str().to_owned())
            .collect::<FxHashSet<_>>(),
        test_files: plan.test_files.clone(),
        file_test_files: plan
            .files
            .iter()
            .map(|file| (file.path.clone(), file.static_test_files.clone()))
            .collect(),
    }
}

/// The facts of a started run that the report reads.
struct Execution {
    outcome: Option<ScheduleOutcome>,
    runner: Option<MutationRunnerOutput>,
    cache: Option<MutationCacheOutput>,
    setup_ms: u64,
}

/// The result cache of a run. `None` with `--no-cache`.
fn open_result_cache(
    no_cache: bool,
    mutation_cache: &Path,
    root: &Path,
    runner: &str,
    plan: &MutationPlan,
) -> Option<Arc<ResultCache>> {
    (!no_cache).then(|| {
        Arc::new(ResultCache::open(
            mutation_cache,
            root,
            runner,
            KeyFiles::from_plan(plan),
        ))
    })
}

/// Stores the new results of `outcome` in `cache` and writes the cache file.
/// A failed write adds a `cache-write-failed` issue and keeps the run result.
fn store_results(cache: &ResultCache, outcome: &mut ScheduleOutcome) -> MutationCacheOutput {
    let counts = cache.record(outcome);
    if let Err(message) = cache.save() {
        outcome.issues.push(ScheduleIssue {
            code: "cache-write-failed",
            message: format!("the mutation result cache was not saved: {message}"),
            mutant_id: None,
            stderr_tail: None,
        });
    }
    MutationCacheOutput {
        hits: counts.hits,
        stored: counts.stored,
        not_keyed: NOT_KEYED.iter().map(|&item| item.to_owned()).collect(),
    }
}

/// The `init` options of the runner. The `node:test` runner gets the module
/// loading flags of the `node --test` command in the project scripts.
fn init_options(runner: Runner, root: &Path) -> InitOptions {
    let node_args = match runner {
        Runner::NodeTest => node_test_args(root),
        Runner::Vitest | Runner::BunTest => Vec::new(),
    };
    InitOptions {
        node_args,
        ..InitOptions::default()
    }
}

/// The public error of a schedule that stopped the run.
fn execute(
    options: &MutationRunOptions,
    planned: &Planned,
    deadline: Option<Instant>,
) -> ProgrammaticResult<Execution> {
    let plan = &planned.plan;
    let schedule = schedule_plan(plan);
    // With no planned mutant the dry run still runs the static test files,
    // because a hit on a statically unreached mutant is a static map miss.
    if schedule.mutants.is_empty()
        && (schedule.unreached.is_empty() || schedule.test_files.is_empty())
    {
        return Ok(Execution {
            outcome: None,
            runner: None,
            cache: None,
            setup_ms: 0,
        });
    }
    let setup_started = Instant::now();
    let manifest = manifest()
        .map_err(|message| mutation_error("FALLOW_MUTATION_PROTOCOL_INVALID", message))?;
    let runner = planned.runner;
    let name = runner.name();
    let range = manifest.runner(name).ok_or_else(|| {
        mutation_error(
            "FALLOW_MUTATION_PROTOCOL_INVALID",
            format!("the protocol manifest has no `{name}` runner"),
        )
    })?;
    let runtime = discover_runtime(options.runtime.as_deref(), runner.runtimes())
        .map_err(|message| runtime_error(runner, message))?;
    let cache_error =
        |message: String| mutation_error("FALLOW_MUTATION_CACHE_WRITE_FAILED", message);
    ignore_cache_dir(&planned.cache_dir).map_err(|error| cache_error(error.to_string()))?;
    let mutation_cache = planned.cache_dir.join("mutation");
    let companion = install(&mutation_cache, COMPANION_FILES)
        .map_err(|message| mutation_error("FALLOW_MUTATION_COMPANION_INTEGRITY", message))?;
    let files: Vec<RunFile<'_>> = plan
        .files
        .iter()
        .map(|file| RunFile {
            path: &file.path,
            code: &file.instrumented_code,
        })
        .collect();
    let run_dir = write_run_dir(&mutation_cache, &files, Some(&companion)).map_err(cache_error)?;
    let resolved = &planned.resolved;
    let root = resolved.root().to_path_buf();
    let result_cache = open_result_cache(resolved.no_cache, &mutation_cache, &root, name, plan);
    let config = ScheduleConfig {
        init: InitRequest {
            root: root.to_string_lossy().into_owned(),
            runner: name.to_owned(),
            config_path: None,
            run_dir: run_dir.dir.to_string_lossy().into_owned(),
            path_map: run_dir.path_map.clone(),
            options: init_options(runner, &root),
        },
        runner: range.clone(),
        concurrency: options.concurrency.unwrap_or_else(default_concurrency),
        runner_timeout: options.runner_timeout.unwrap_or(DEFAULT_RUNNER_TIMEOUT),
        deadline,
        cache: result_cache
            .clone()
            .map(|cache| cache as Arc<dyn ResultLookup>),
    };
    let spawner = ProcessSpawner {
        runtime: runtime.path.clone(),
        companion,
        root,
        wire_version: manifest.wire_protocol_version,
        session_token_env: manifest.session_token_env.clone(),
    };
    let setup_ms = millis(setup_started.elapsed());
    let mut outcome = run_schedule(&spawner, &schedule, &config)
        .map_err(|error| schedule_error(&error, &config.runner, config.runner_timeout))?;
    let runner = outcome.ready.as_ref().map(|ready| MutationRunnerOutput {
        name: name.to_owned(),
        version: ready.runner_version.clone(),
        protocol_version: manifest.wire_protocol_version,
        runtime: MutationRuntimeOutput {
            name: runtime.kind.name().to_owned(),
            version: runtime.version.clone(),
        },
        concurrency: outcome.workers,
        notes: ready
            .notes
            .iter()
            .filter(|note| manifest.ready_notes.contains(note))
            .cloned()
            .collect(),
    });
    let cache = match (&result_cache, outcome.ready.is_some()) {
        (Some(result_cache), true) => Some(store_results(result_cache, &mut outcome)),
        _ => None,
    };
    Ok(Execution {
        outcome: Some(outcome),
        runner,
        cache,
        setup_ms,
    })
}

/// The errors of a finished run: the scheduler issues and the worktree facts.
fn run_issues(
    outcome: Option<&ScheduleOutcome>,
    changed_paths: &[String],
    check_error: Option<String>,
) -> Vec<fallow_output::MutationRunIssue> {
    let mut errors = issues(outcome);
    if !changed_paths.is_empty() {
        errors.push(fallow_output::MutationRunIssue {
            code: "worktree-changed".to_owned(),
            message: format!(
                "the run changed {} worktree {}; a mutant or a test wrote to the project",
                changed_paths.len(),
                path_noun(changed_paths.len())
            ),
            mutant_id: None,
            stderr_tail: None,
        });
    }
    if let Some(message) = check_error {
        errors.push(fallow_output::MutationRunIssue {
            code: "worktree-check-failed".to_owned(),
            message: format!(
                "the end-of-run worktree check failed, so stray writes are unknown: {message}"
            ),
            mutant_id: None,
            stderr_tail: None,
        });
    }
    errors
}

/// Refuses a run with the survivor type check before the tests start when no
/// trusted type-aware companion exists.
fn guard_typecheck_companion(enabled: bool, root: &Path) -> ProgrammaticResult<()> {
    if !enabled {
        return Ok(());
    }
    crate::type_aware::check_typecheck_companion(root).map_err(|message| {
        mutation_error("FALLOW_MUTATION_TYPECHECK_UNAVAILABLE", message)
            .with_help("install the fallow-type-aware companion, or run without --typecheck")
            .with_context("mutation.typecheck")
    })
}

/// Plans the mutants, runs the project tests against each planned mutant,
/// and reports the result.
///
/// The run refuses a dirty git worktree unless
/// [`MutationRunOptions::allow_dirty`] is set. After the run it compares the
/// worktree with the start state and reports each changed path.
///
/// # Errors
///
/// Returns an error for invalid options, a dirty worktree, a failed plan, a
/// missing runtime, a companion that does not start or load the runner, a
/// companion file that changed after install, or a test suite that fails
/// with no active mutant outside each test file.
pub fn run_mutation(options: &MutationRunOptions) -> ProgrammaticResult<MutationReport> {
    let started = Instant::now();
    let deadline = options.time_budget.map(|budget| started + budget);
    let mut start_snapshot = None;
    let planned = plan_for(options, |root, cache_dir| {
        guard_cache_dir(root, cache_dir)?;
        start_snapshot = take_worktree_snapshot(root, cache_dir, &options.output_paths, None)?;
        guard_worktree(options.allow_dirty, start_snapshot.as_ref())
    })?;
    guard_cache_holds_no_planned_file(planned.resolved.root(), &planned.cache_dir, &planned.plan)?;
    guard_typecheck_companion(options.typecheck, planned.resolved.root())?;
    let plan_ms = millis(started.elapsed());
    let execution = execute(options, &planned, deadline);
    let worktree_end = settle_worktree(start_snapshot.as_ref(), |start| {
        take_worktree_snapshot(
            planned.resolved.root(),
            &planned.cache_dir,
            &options.output_paths,
            Some(start),
        )
    });
    let execution = execution.map_err(|error| with_worktree_facts(error, &worktree_end))?;
    let WorktreeEnd {
        changed_paths,
        check_error,
    } = worktree_end;
    let plan = &planned.plan;
    let outcome = execution.outcome.as_ref();
    let mut results = mutant_results(plan, outcome);
    let typecheck = typecheck_survivors(
        options.typecheck,
        planned.resolved.root(),
        plan,
        &mut results,
    );
    let typecheck_ms = millis(typecheck.elapsed);
    let baseline = baseline_failures(outcome);
    let completion = completion(&CompletionFacts {
        plan,
        results: &results,
        baseline_failures: &baseline,
        worktree_changed: !changed_paths.is_empty(),
        worktree_unchecked: check_error.is_some(),
        unretried: outcome.is_some_and(|outcome| !outcome.unretried.is_empty()),
        unloaded_paths: outcome.map_or(&[], |outcome| outcome.unloaded_paths.as_slice()),
        typecheck_failed: typecheck.issue.is_some(),
        promoted: outcome.map_or(&[], |outcome| outcome.promoted.as_slice()),
    });
    let mut errors = run_issues(outcome, &changed_paths, check_error);
    errors.extend(typecheck.issue);
    let timings = outcome.map_or_else(
        || MutationTimings {
            plan_ms,
            typecheck_ms,
            ..MutationTimings::default()
        },
        |outcome| MutationTimings {
            plan_ms,
            setup_ms: execution.setup_ms + millis(outcome.timings.setup),
            dry_run_ms: millis(outcome.timings.dry_run),
            mutants_ms: millis(outcome.timings.mutants),
            retry_ms: millis(outcome.timings.retry),
            typecheck_ms,
        },
    );
    Ok(MutationReport {
        schema_version: MutationSchemaVersion::V1,
        version: tool_version(),
        elapsed_ms: ElapsedMs(millis(started.elapsed())),
        completion,
        scope: scope_output(options, planned.base_ref.clone()),
        runner: execution.runner,
        cache: execution.cache,
        summary: summary(&results),
        functions: function_results(plan, &results),
        mutants: results,
        no_test_files: plan.no_test_files,
        baseline_failures: baseline,
        skipped_files: skipped_files(plan),
        unmatched_paths: plan.unmatched_paths.clone(),
        unloaded_paths: outcome
            .map(|outcome| outcome.unloaded_paths.clone())
            .unwrap_or_default(),
        worktree: MutationWorktreeCheck {
            checked: start_snapshot.is_some(),
            allow_dirty: options.allow_dirty,
            dirty_at_start: start_snapshot
                .as_ref()
                .map(WorktreeSnapshot::dirty_paths)
                .unwrap_or_default(),
            changed_paths,
        },
        errors,
        timings,
        files: plan_files(plan),
        verdicts: None,
        typecheck: typecheck.output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vitest_range() -> &'static RunnerRange {
        manifest()
            .expect("manifest")
            .runner("vitest")
            .expect("vitest range")
    }

    #[test]
    fn a_runner_version_error_names_the_supported_range_as_the_install_step() {
        let error = schedule_error(
            &ScheduleError::RunnerVersion(
                "vitest 4.0.18 is not supported; the supported range is >=4.1.0 <6.0.0".to_owned(),
            ),
            vitest_range(),
            Duration::from_secs(120),
        );
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_RUNNER_UNSUPPORTED")
        );
        assert_eq!(
            error.help.as_deref(),
            Some(
                "install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range"
            )
        );

        let help: Vec<String> = manifest()
            .expect("manifest")
            .runners
            .iter()
            .map(runner_version_help)
            .collect();
        assert_eq!(
            help,
            [
                "install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range",
                "install node 22.18.0 or a later version below 27.0.0; a node 22.x release from 22.18.0 is in the range",
                "install bun 1.4.0 or a later version below 2.0.0; a bun 1.x release from 1.4.0 is in the range",
            ]
        );

        let major_start = RunnerRange {
            name: "vitest".to_owned(),
            package: "vitest".to_owned(),
            min_version: "5.0.0".to_owned(),
            max_version_exclusive: "6.0.0".to_owned(),
        };
        assert_eq!(
            runner_version_help(&major_start),
            "install vitest 5.0.0 or a later version below 6.0.0"
        );
    }

    #[test]
    fn a_setup_watchdog_error_names_the_setup_timeout_flag() {
        let error = schedule_error(
            &ScheduleError::Setup {
                message: "the mutation companion dry_run failed".to_owned(),
                stderr_tail: Some("vitest started\n".to_owned()),
                timed_out: true,
            },
            vitest_range(),
            Duration::from_secs(120),
        );
        let help = error.help.as_deref().unwrap_or_default();
        assert!(help.contains("longer than 120s"), "{help}");
        assert!(help.contains("--setup-timeout"), "{help}");
        assert!(help.contains("companion stderr:\nvitest started"), "{help}");

        let other = schedule_error(
            &ScheduleError::Setup {
                message: "the mutation companion init failed".to_owned(),
                stderr_tail: None,
                timed_out: false,
            },
            vitest_range(),
            Duration::from_secs(120),
        );
        assert_eq!(other.help, None);
    }

    #[test]
    fn a_failed_end_check_keeps_the_execute_error_and_reports_the_check_error() {
        let start = WorktreeSnapshot::default();
        let end = settle_worktree(Some(&start), |_| {
            Err(mutation_error(
                "FALLOW_MUTATION_WORKTREE_CHECK_FAILED",
                "git status failed",
            ))
        });
        assert!(end.changed_paths.is_empty());
        assert_eq!(end.check_error.as_deref(), Some("git status failed"));

        let error = with_worktree_facts(
            mutation_error(
                "FALLOW_MUTATION_SETUP_FAILED",
                "the companion did not start",
            ),
            &end,
        );
        assert_eq!(error.code.as_deref(), Some("FALLOW_MUTATION_SETUP_FAILED"));
        assert!(
            error.message.starts_with("the companion did not start"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("git status failed"),
            "{}",
            error.message
        );
    }

    #[test]
    fn every_config_operator_name_maps_to_an_engine_operator() {
        for operator in fallow_engine::mutation::Operator::ALL {
            let name: fallow_config::MutationOperatorName =
                serde_json::from_value(serde_json::json!(operator.as_str()))
                    .expect("a config operator name");
            assert_eq!(
                fallow_engine::mutation::Operator::from_name(name.as_str()),
                Some(operator)
            );
        }
    }

    #[test]
    fn no_cache_opens_no_result_cache() {
        let dir = tempfile::tempdir().expect("dir");
        let plan = MutationPlan {
            files: Vec::new(),
            mutants: Vec::new(),
            test_files: Vec::new(),
            reach_files: Vec::new(),
            test_file_reach: std::collections::BTreeMap::new(),
            support_files: Vec::new(),
            counts: fallow_engine::mutation::MutationPlanCounts::default(),
            no_test_files: false,
            framework_config_checked: false,
            skipped_files: Vec::new(),
            unmatched_paths: Vec::new(),
        };
        assert!(open_result_cache(true, dir.path(), dir.path(), "vitest", &plan).is_none());
        assert!(open_result_cache(false, dir.path(), dir.path(), "vitest", &plan).is_some());
    }

    fn empty_plan() -> MutationPlan {
        MutationPlan {
            files: Vec::new(),
            mutants: Vec::new(),
            test_files: Vec::new(),
            reach_files: Vec::new(),
            test_file_reach: std::collections::BTreeMap::new(),
            support_files: Vec::new(),
            counts: fallow_engine::mutation::MutationPlanCounts::default(),
            no_test_files: false,
            framework_config_checked: false,
            skipped_files: Vec::new(),
            unmatched_paths: Vec::new(),
        }
    }

    #[test]
    fn a_cache_dir_at_or_above_the_root_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join("src")).expect("dir");
        for cache in [root.clone(), root.join("."), dir.path().to_path_buf()] {
            let error = guard_cache_dir(&root, &cache).expect_err("refused");
            assert_eq!(
                error.code.as_deref(),
                Some("FALLOW_MUTATION_UNSAFE_CACHE_DIR")
            );
            assert_eq!(error.exit_code, 2);
            assert_eq!(error.context.as_deref(), Some("cache.dir"));
        }
        assert!(guard_cache_dir(&root, &root.join(".fallow")).is_ok());
        assert!(guard_cache_dir(&root, &root.join("src")).is_ok());
    }

    #[test]
    fn a_cache_dir_that_holds_a_planned_file_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let root = dir.path();
        let mut plan = empty_plan();
        plan.files.push(fallow_engine::mutation::PlannedFile {
            path: "src/a.ts".to_owned(),
            source_sha256: String::new(),
            instrumented_code: String::new(),
            static_test_files: Vec::new(),
        });
        plan.test_files.push("test/a.test.ts".to_owned());

        let error = guard_cache_holds_no_planned_file(root, &root.join("src"), &plan)
            .expect_err("the cache holds a planned source file");
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_UNSAFE_CACHE_DIR")
        );
        assert!(error.message.contains("src/a.ts"), "{}", error.message);
        let error = guard_cache_holds_no_planned_file(root, &root.join("test"), &plan)
            .expect_err("the cache holds a planned test file");
        assert!(
            error.message.contains("test/a.test.ts"),
            "{}",
            error.message
        );
        assert!(guard_cache_holds_no_planned_file(root, &root.join(".fallow"), &plan).is_ok());
    }

    #[test]
    fn no_start_snapshot_needs_no_end_check() {
        let end = settle_worktree(None, |_| panic!("no end check without a start snapshot"));
        assert!(end.changed_paths.is_empty());
        assert!(end.check_error.is_none());
    }
}
