//! [`replay_mutation`]: one export of a source file, called with one input
//! on the original and on one mutant, through the replay request of the
//! runner companion (replay contract 1).

use std::path::Path;
use std::time::{Duration, Instant};

use fallow_engine::mutation::{
    MutationError, MutationOptions, Operator, SourceKind, export_containing,
};
use fallow_output::{
    MutationReplayCall, MutationReplayInput, MutationReplayInputSource, MutationReplayOutput,
    MutationReplayVerdict,
};

use super::mutation::{
    DEFAULT_RUNNER_TIMEOUT, Planned, ProgrammaticResult, guard_cache_dir, init_options, millis,
    mutation_error, plan_for, runner_version_help, runtime_error,
};
use crate::mutation::companion::{COMPANION_FILES, install};
use crate::mutation::discovery::discover_runtime;
use crate::mutation::last_report::ignore_cache_dir;
use crate::mutation::manifest::manifest;
use crate::mutation::protocol::{
    InitRequest, LinkFault, ReplayRequest, ReplayResult, Request, Response,
};
use crate::mutation::replay::{check_calls, output_outcome, output_side, verdict};
use crate::mutation::run_dir::{RunFile, write_run_dir};
use crate::mutation::transport::{CompanionLink, CompanionSpawner, ProcessSpawner, SpawnFailure};
use crate::mutation::{MutationReplayOptions, MutationRunOptions, MutationRunScope};

/// The calls of each side. Three calls show a value that changes from one
/// call to the next, for example from a clock that the replay does not fix.
const CALLS_PER_SIDE: u32 = 3;
/// The default time limit of one call.
const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The fixed `Date.now()` of each call: 2026-01-01T00:00:00Z.
const REPLAY_CLOCK_MS: i64 = 1_767_225_600_000;
/// The seed of `Math.random` in each call.
const REPLAY_SEED: i64 = 1;
/// The time of the process starts and the module loads of a replay request,
/// on top of the call timeouts. The wall limit of the whole request is the
/// sum, so a mutant that blocks its worker cannot stop the command.
const REPLAY_REQUEST_MARGIN: Duration = Duration::from_secs(30);

/// The facts of the mutant that the replay needs.
struct Target {
    instrumented: String,
    export: Result<String, String>,
}

fn replay_error(message: impl Into<String>) -> crate::ProgrammaticError {
    mutation_error("FALLOW_MUTATION_REPLAY_FAILED", message).with_context("mutation.replay")
}

/// The operator sets that can give the mutant: its own operator, then each
/// operator. A mutant id can depend on the other mutants of the file, so the
/// second set is a fallback.
fn operator_passes(operator: Option<Operator>) -> Vec<Vec<Operator>> {
    let mut passes = Vec::new();
    if let Some(operator) = operator {
        passes.push(vec![operator]);
    }
    passes.push(Operator::ALL.to_vec());
    passes
}

/// Instruments `source` until a pass gives `mutant_id`, and finds the
/// export to call.
fn find_target(
    source: &str,
    options: &MutationReplayOptions,
) -> ProgrammaticResult<Option<Target>> {
    let file = options.file.as_str();
    let Some(kind) = SourceKind::from_path(file) else {
        return Ok(None);
    };
    for operators in operator_passes(options.operator) {
        let request = MutationOptions {
            operators,
            ranges: None,
        };
        let instrumented = match fallow_engine::mutation::instrument_source(source, file, &request)
        {
            Some(Ok(instrumented)) => instrumented,
            Some(Err(MutationError::Parse { messages })) => {
                return Err(replay_error(format!(
                    "`{file}` does not parse: {}",
                    messages.join("; ")
                )));
            }
            Some(Err(error)) => {
                return Err(replay_error(format!(
                    "cannot instrument `{file}`: {error:?}"
                )));
            }
            None => return Ok(None),
        };
        let Some(mutant) = instrumented
            .mutants
            .iter()
            .find(|mutant| mutant.id.as_str() == options.mutant_id)
        else {
            continue;
        };
        let export = match (&options.export, &mutant.enclosing_function) {
            (Some(export), _) => Ok(export.clone()),
            (None, Some(function)) => export_containing(source, kind, function.span).ok_or_else(|| {
                format!(
                    "no export holds the function {}; pass --export to call an export of the file that runs it",
                    function.name.as_deref().map_or_else(|| "of the mutant".to_owned(), |name| format!("`{name}`"))
                )
            }),
            (None, None) => Err(
                "the mutant is in top-level code; pass --export to call an export of the file that reads it"
                    .to_owned(),
            ),
        };
        return Ok(Some(Target {
            instrumented: instrumented.code,
            export,
        }));
    }
    Err(mutation_error(
        "FALLOW_MUTATION_UNKNOWN_MUTANT",
        format!(
            "the current `{file}` gives no mutant `{}`",
            options.mutant_id
        ),
    )
    .with_context("mutation.mutantId")
    .with_help(
        "a mutant id changes when its source changes; run `fallow mutation` again for new ids",
    ))
}

/// An output with no call, for a replay that did not run or did not end.
fn without_calls(
    options: &MutationReplayOptions,
    export: Option<String>,
    verdict: MutationReplayVerdict,
    reason: String,
    call_timeout: Duration,
) -> MutationReplayOutput {
    MutationReplayOutput {
        contract_version: manifest().map_or(1, |manifest| manifest.replay.contract_version),
        verdict,
        reason: Some(reason),
        export,
        input: MutationReplayInput {
            source: MutationReplayInputSource::Argument,
            args: options.args.clone(),
        },
        calls_per_side: CALLS_PER_SIDE,
        call_timeout_ms: millis(call_timeout),
        clock_ms: REPLAY_CLOCK_MS,
        seed: REPLAY_SEED,
        calls: Vec::new(),
        duration_ms: 0,
    }
}

/// Sends `init` and checks the runner version and the replay capability.
fn start(
    link: &mut Box<dyn CompanionLink>,
    init: InitRequest,
    runner_name: &str,
    timeout: Duration,
) -> ProgrammaticResult<()> {
    let manifest = manifest()
        .map_err(|message| mutation_error("FALLOW_MUTATION_PROTOCOL_INVALID", message))?;
    let range = manifest.runner(runner_name).ok_or_else(|| {
        mutation_error(
            "FALLOW_MUTATION_PROTOCOL_INVALID",
            format!("the protocol manifest has no `{runner_name}` runner"),
        )
    })?;
    let ready = match link.request(&Request::Init(init), timeout) {
        Ok(Response::Ready(ready)) => ready,
        Ok(_) => return Err(replay_error("the companion answered init with no ready")),
        Err(fault) => return Err(link_error("init", &fault)),
    };
    if !range.contains(&ready.runner_version) {
        return Err(mutation_error(
            "FALLOW_MUTATION_RUNNER_UNSUPPORTED",
            format!(
                "{} {} is not supported; the supported range is >={} <{}",
                range.package, ready.runner_version, range.min_version, range.max_version_exclusive
            ),
        )
        .with_help(runner_version_help(range)));
    }
    if !ready.capabilities.replay {
        return Err(mutation_error(
            "FALLOW_MUTATION_REPLAY_UNSUPPORTED",
            format!("the {runner_name} runner has no replay"),
        )
        .with_context("mutation.replay")
        .with_help("replay supports the Vitest and node:test runners"));
    }
    Ok(())
}

fn link_error(stage: &str, fault: &LinkFault) -> crate::ProgrammaticError {
    let error = replay_error(format!("the replay companion failed at {stage}: {fault}"));
    match fault.stderr_tail() {
        Some(tail) => error.with_help(format!("companion stderr:\n{}", tail.trim_end())),
        None => error,
    }
}

/// The source of the file of the mutant, without a byte order mark.
fn read_source(root: &Path, file: &str) -> ProgrammaticResult<String> {
    let bytes = std::fs::read(root.join(file))
        .map_err(|error| replay_error(format!("cannot read `{file}`: {error}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| replay_error(format!("`{file}` is not UTF-8 text")))?;
    Ok(text
        .strip_prefix('\u{feff}')
        .map_or_else(|| text.clone(), str::to_owned))
}

/// Writes the run directory with the instrumented file, starts the
/// companion, and sends `init`.
fn start_companion(
    planned: &Planned,
    options: &MutationReplayOptions,
    instrumented: &str,
) -> ProgrammaticResult<Box<dyn CompanionLink>> {
    let manifest = manifest()
        .map_err(|message| mutation_error("FALLOW_MUTATION_PROTOCOL_INVALID", message))?;
    let runner = planned.runner;
    let runtime = discover_runtime(options.runtime.as_deref(), runner.runtimes())
        .map_err(|message| runtime_error(runner, message))?;
    let cache_error =
        |message: String| mutation_error("FALLOW_MUTATION_CACHE_WRITE_FAILED", message);
    ignore_cache_dir(&planned.cache_dir).map_err(|error| cache_error(error.to_string()))?;
    let mutation_cache = planned.cache_dir.join("mutation");
    let companion = install(&mutation_cache, COMPANION_FILES)
        .map_err(|message| mutation_error("FALLOW_MUTATION_COMPANION_INTEGRITY", message))?;
    let files = [RunFile {
        path: &options.file,
        code: instrumented,
    }];
    let run_dir = write_run_dir(&mutation_cache, &files, Some(&companion)).map_err(cache_error)?;
    let root = planned.resolved.root().to_path_buf();
    let init = InitRequest {
        root: root.to_string_lossy().into_owned(),
        runner: runner.name().to_owned(),
        config_path: planned.runner_config.clone(),
        run_dir: run_dir.dir.to_string_lossy().into_owned(),
        path_map: run_dir.path_map,
        options: init_options(runner, &root, &planned.plan.test_files),
    };
    let spawner = ProcessSpawner {
        runtime: runtime.path,
        companion,
        root,
        wire_version: manifest.wire_protocol_version,
        session_token_env: manifest.session_token_env.clone(),
    };
    let mut link = spawner.spawn().map_err(|failure| match failure {
        SpawnFailure::Integrity(message) => {
            mutation_error("FALLOW_MUTATION_COMPANION_INTEGRITY", message)
        }
        SpawnFailure::Fault(fault) => link_error("start", &fault),
    })?;
    let runner_timeout = options.runner_timeout.unwrap_or(DEFAULT_RUNNER_TIMEOUT);
    if let Err(error) = start(&mut link, init, runner.name(), runner_timeout) {
        link.shutdown();
        return Err(error);
    }
    Ok(link)
}

/// The reason of a verdict that needs one.
fn verdict_reason(verdict: MutationReplayVerdict, export: &str) -> Option<String> {
    match verdict {
        MutationReplayVerdict::NotCallable => Some(format!("the export `{export}` is not a function")),
        MutationReplayVerdict::Timeout => {
            Some("a call did not end before the call timeout".to_owned())
        }
        MutationReplayVerdict::Unstable => Some(
            "the calls of one side gave different results; a value that the replay does not fix (for example crypto random values or the network) can cause this".to_owned(),
        ),
        _ => None,
    }
}

/// The output of a replay result that keeps the contract.
fn completed(
    options: &MutationReplayOptions,
    export: String,
    result: ReplayResult,
    call_timeout: Duration,
) -> ProgrammaticResult<MutationReplayOutput> {
    check_calls(&result.calls, CALLS_PER_SIDE).map_err(replay_error)?;
    let verdict = verdict(&result.calls);
    let mut output = without_calls(options, Some(export), verdict, String::new(), call_timeout);
    output.reason = verdict_reason(verdict, output.export.as_deref().unwrap_or_default());
    output.duration_ms = result.duration_ms;
    output.calls = result
        .calls
        .into_iter()
        .map(|call| MutationReplayCall {
            side: output_side(call.side),
            index: call.index,
            outcome: output_outcome(call.outcome),
            value: call.value,
            text: call.text,
        })
        .collect();
    Ok(output)
}

/// Calls one export of a source file with one input on the original and on
/// one mutant, three times each, in turns, and gives the verdict. It runs
/// project code: the module of the file and each module that it imports.
///
/// Each call has a fresh module instance, a fixed clock and a fixed seed of
/// `Math.random`. A mutant runs in its own worker with a wall limit. The
/// verdict is never `equivalent`: equal results give `no_witness_found`.
///
/// # Errors
///
/// Returns an error when the file does not give the mutant, the runtime or
/// the runner is missing, the runner has no replay, or the companion fails.
pub fn replay_mutation(
    options: &MutationReplayOptions,
) -> ProgrammaticResult<MutationReplayOutput> {
    let call_timeout = options.call_timeout.unwrap_or(DEFAULT_CALL_TIMEOUT);
    let run_options = MutationRunOptions {
        analysis: options.analysis.clone(),
        scope: MutationRunScope::Paths(vec![options.file.clone().into()]),
        runtime: options.runtime.clone(),
        runner_config: options.runner_config.clone(),
        allow_dirty: true,
        ..MutationRunOptions::default()
    };
    let planned = plan_for(&run_options, guard_cache_dir)?;
    let source = read_source(planned.resolved.root(), &options.file)?;
    let not_replayable = |reason: String| {
        without_calls(
            options,
            None,
            MutationReplayVerdict::NotReplayable,
            reason,
            call_timeout,
        )
    };
    let Some(target) = find_target(&source, options)? else {
        return Ok(not_replayable(
            "a component file has no module exports to call".to_owned(),
        ));
    };
    let export = match target.export {
        Ok(export) => export,
        Err(reason) => return Ok(not_replayable(reason)),
    };
    let mut link = start_companion(&planned, options, &target.instrumented)?;
    let request = ReplayRequest {
        file: options.file.clone(),
        export: export.clone(),
        mutant_id: options.mutant_id.clone(),
        args: options.args.clone(),
        calls: CALLS_PER_SIDE,
        call_timeout_ms: millis(call_timeout),
        clock_ms: REPLAY_CLOCK_MS,
        seed: REPLAY_SEED,
    };
    let wall_limit = call_timeout * (2 * CALLS_PER_SIDE) + REPLAY_REQUEST_MARGIN;
    let started = Instant::now();
    let response = link.request(&Request::Replay(request), wall_limit);
    link.shutdown();
    match response {
        Ok(Response::Replay(result)) => completed(options, export, result, call_timeout),
        Ok(_) => Err(replay_error(
            "the companion answered replay with another response",
        )),
        Err(LinkFault::Timeout) => {
            let mut output = without_calls(
                options,
                Some(export),
                MutationReplayVerdict::Timeout,
                format!(
                    "the replay did not end in its wall limit of {}s; a call blocks its worker, so the mutant can hang",
                    wall_limit.as_secs()
                ),
                call_timeout,
            );
            output.duration_ms = millis(started.elapsed());
            Ok(output)
        }
        Err(fault) => Err(link_error("replay", &fault)),
    }
}
