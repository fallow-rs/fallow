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
    mutation_error, plan_for, runtime_error,
};
use crate::mutation::companion::{COMPANION_FILES, install};
use crate::mutation::discovery::discover_runtime;
use crate::mutation::last_report::ignore_cache_dir;
use crate::mutation::manifest::manifest;
use crate::mutation::protocol::{
    InitRequest, LinkFault, ReplayRequest, ReplayResult, Request, Response,
};
use crate::mutation::recorded_args;
use crate::mutation::replay::{check_calls, output_outcome, output_side, verdict};
use crate::mutation::run_dir::{RunFile, write_run_dir};
use crate::mutation::runner_version::runner_version_help;
use crate::mutation::transport::{CompanionLink, CompanionSpawner, ProcessSpawner, SpawnFailure};
use crate::mutation::{
    MutationReplayOptions, MutationRunOptions, MutationRunScope, RecordArgsMode,
};

/// The most recorded inputs that one replay runs.
const MAX_RECORDED_INPUTS: usize = 3;
/// The calls of each side. Three calls show a value that changes from one
/// call to the next, for example from a clock that the replay does not fix.
const CALLS_PER_SIDE: u32 = 3;
/// The default time limit of one call.
const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest time limit of one call. The companion refuses a longer one.
const MAX_CALL_TIMEOUT: Duration = Duration::from_mins(10);
/// The fixed `Date.now()` of each call: 2026-01-01T00:00:00Z.
const REPLAY_CLOCK_MS: i64 = 1_767_225_600_000;
/// The seed of `Math.random` in each call.
const REPLAY_SEED: i64 = 1;
/// The time of the worker start, the setup files and the module load of one
/// replay call, on top of its call timeout.
const REPLAY_CALL_MARGIN: Duration = Duration::from_secs(20);
/// The time of the replay request outside the calls. The wall limit of the
/// whole request is the sum, so a call that blocks its worker cannot stop
/// the command.
const REPLAY_REQUEST_MARGIN: Duration = Duration::from_secs(30);

/// The wall limit of one replay request: each call with its margin, and the
/// margin of the request.
fn wall_limit(call_timeout: Duration) -> Duration {
    (call_timeout + REPLAY_CALL_MARGIN) * (2 * CALLS_PER_SIDE) + REPLAY_REQUEST_MARGIN
}

/// The facts of the mutant that the replay needs.
struct Target {
    instrumented: String,
    export: Result<String, String>,
}

fn replay_error(message: impl Into<String>) -> crate::ProgrammaticError {
    mutation_error("FALLOW_MUTATION_REPLAY_FAILED", message).with_context("mutation.replay")
}

/// The instrumentation options that can give the mutant, in order: its own
/// operator, then each operator, each over the whole file and then over the
/// span of the mutant only. A mutant id can depend on the other mutants of
/// the file: a run of another scope can give the same id to another mutant
/// with the same text, so the span selects the mutant.
fn instrument_passes(operator: Option<Operator>, span: (u32, u32)) -> Vec<MutationOptions> {
    let mut sets = Vec::new();
    if let Some(operator) = operator {
        sets.push(vec![operator]);
    }
    sets.push(Operator::ALL.to_vec());
    sets.into_iter()
        .flat_map(|operators| {
            [None, Some(vec![span])].map(|ranges| MutationOptions {
                operators: operators.clone(),
                ranges,
                record_exports: false,
            })
        })
        .collect()
}

/// The export of the function of `mutant`, or the reason why none fits.
fn export_of(
    source: &str,
    kind: SourceKind,
    options: &MutationReplayOptions,
    function: Option<&fallow_engine::mutation::FunctionRef>,
) -> Result<String, String> {
    match (&options.export, function) {
        (Some(export), _) => Ok(export.clone()),
        (None, Some(function)) => export_containing(source, kind, function.span).ok_or_else(|| {
            format!(
                "no callable export holds the function {}; pass --export to call an export of the file that runs it",
                function
                    .name
                    .as_deref()
                    .map_or_else(|| "of the mutant".to_owned(), |name| format!("`{name}`"))
            )
        }),
        (None, None) => Err(
            "the mutant is in top-level code; pass --export to call an export of the file that reads it"
                .to_owned(),
        ),
    }
}

/// Instruments `source` until a pass gives `mutant_id` at its span, and
/// finds the export to call.
fn find_target(
    source: &str,
    options: &MutationReplayOptions,
) -> ProgrammaticResult<Option<Target>> {
    let file = options.file.as_str();
    let Some(kind) = SourceKind::from_path(file) else {
        return Ok(None);
    };
    for request in instrument_passes(options.operator, options.span) {
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
            .find(|mutant| mutant.id.as_str() == options.mutant_id && mutant.span == options.span)
        else {
            continue;
        };
        let export = export_of(source, kind, options, mutant.enclosing_function.as_ref());
        return Ok(Some(Target {
            instrumented: instrumented.code,
            export,
        }));
    }
    Err(mutation_error(
        "FALLOW_MUTATION_UNKNOWN_MUTANT",
        format!(
            "the current `{file}` gives no mutant `{}` at bytes {}..{}",
            options.mutant_id, options.span.0, options.span.1
        ),
    )
    .with_context("mutation.mutantId")
    .with_help(
        "a mutant id changes when its source changes; run `fallow mutation` again for new ids",
    ))
}

/// An output with no call, for a replay that did not run or did not end.
fn without_calls(
    input: &MutationReplayInput,
    export: Option<String>,
    verdict: MutationReplayVerdict,
    reason: String,
    call_timeout: Duration,
) -> ProgrammaticResult<MutationReplayOutput> {
    let manifest = manifest()
        .map_err(|message| mutation_error("FALLOW_MUTATION_PROTOCOL_INVALID", message))?;
    Ok(MutationReplayOutput {
        contract_version: manifest.replay.contract_version,
        verdict,
        reason: Some(reason),
        export,
        input: input.clone(),
        inputs_checked: 0,
        calls_per_side: CALLS_PER_SIDE,
        call_timeout_ms: millis(call_timeout),
        clock_ms: REPLAY_CLOCK_MS,
        seed: REPLAY_SEED,
        calls: Vec::new(),
        duration_ms: 0,
    })
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
    input: &MutationReplayInput,
    export: String,
    result: ReplayResult,
    call_timeout: Duration,
) -> ProgrammaticResult<MutationReplayOutput> {
    check_calls(&result.calls, CALLS_PER_SIDE).map_err(replay_error)?;
    let verdict = verdict(&result.calls);
    let mut output = without_calls(input, Some(export), verdict, String::new(), call_timeout)?;
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
/// Each call runs in its own worker with a fresh module instance, a clock
/// that starts at a fixed time and a fixed seed of `Math.random`. The whole
/// request has a wall limit. The
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
    if call_timeout > MAX_CALL_TIMEOUT || call_timeout < Duration::from_millis(1) {
        return Err(mutation_error(
            "FALLOW_INVALID_MUTATION_OPTIONS",
            "the replay call timeout must be from 1ms to 10m",
        )
        .with_context("mutation.replay.callTimeout"));
    }
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
    let unknown = MutationReplayInput {
        source: if options.args.is_some() {
            MutationReplayInputSource::Argument
        } else {
            MutationReplayInputSource::Recorded
        },
        input_id: None,
        args: options.args.clone().unwrap_or_default(),
    };
    let not_replayable = |export: Option<String>, reason: String| {
        without_calls(
            &unknown,
            export,
            MutationReplayVerdict::NotReplayable,
            reason,
            call_timeout,
        )
    };
    let Some(target) = find_target(&source, options)? else {
        return not_replayable(
            None,
            "a component file has no module exports to call".to_owned(),
        );
    };
    let export = match target.export {
        Ok(export) => export,
        Err(reason) => return not_replayable(None, reason),
    };
    let inputs = match input_list(&planned.cache_dir, options, &export) {
        Ok(inputs) => inputs,
        Err(reason) => return not_replayable(Some(export), reason),
    };
    let mut link = start_companion(&planned, options, &target.instrumented)?;
    let replay_target = ReplayTarget {
        file: &options.file,
        mutant_id: &options.mutant_id,
        export: &export,
    };
    let output = replay_inputs(&mut link, &replay_target, &inputs, call_timeout);
    link.shutdown();
    output
}

/// Replays `inputs` on an open companion, up to the first input that ends
/// the search, and gives the result with `inputs_checked` and the rule 3
/// reason of "no witness found".
pub(super) fn replay_inputs(
    link: &mut Box<dyn CompanionLink>,
    target: &ReplayTarget<'_>,
    inputs: &[MutationReplayInput],
    call_timeout: Duration,
) -> ProgrammaticResult<MutationReplayOutput> {
    let mut outputs = Vec::new();
    for input in inputs {
        let output = replay_one(link, target, input, call_timeout)?;
        let stop = ends_search(output.verdict);
        outputs.push(output);
        if stop {
            break;
        }
    }
    let checked = u32::try_from(outputs.len()).unwrap_or(u32::MAX);
    let mut output = pick_output(outputs).ok_or_else(|| replay_error("the replay has no input"))?;
    output.inputs_checked = checked;
    if output.verdict == MutationReplayVerdict::NoWitnessFound {
        output.reason = Some(no_witness_reason(&output));
    }
    Ok(output)
}

/// True when the replay of the next input cannot change the result: a
/// witness, a call that hangs (the companion is gone), or an export that is
/// no function. An unstable input says nothing about the next input.
const fn ends_search(verdict: MutationReplayVerdict) -> bool {
    matches!(
        verdict,
        MutationReplayVerdict::Witness
            | MutationReplayVerdict::Timeout
            | MutationReplayVerdict::NotCallable
    )
}

/// The output of a replay of several inputs: the last one, unless it shows
/// no witness and an earlier input was unstable. An unstable input is then
/// the result, because it can hide a difference.
fn pick_output(outputs: Vec<MutationReplayOutput>) -> Option<MutationReplayOutput> {
    let unstable = outputs
        .iter()
        .position(|output| output.verdict == MutationReplayVerdict::Unstable);
    let last_is_plain = outputs
        .last()
        .is_some_and(|output| output.verdict == MutationReplayVerdict::NoWitnessFound);
    match (unstable, last_is_plain) {
        (Some(index), true) => outputs.into_iter().nth(index),
        _ => outputs.into_iter().last(),
    }
}

/// The wording of rule 3: the inputs, where they come from, and the calls.
fn no_witness_reason(output: &MutationReplayOutput) -> String {
    let inputs = output.inputs_checked;
    let noun = if inputs == 1 { "input" } else { "inputs" };
    let source = match output.input.source {
        MutationReplayInputSource::Argument => "--args",
        MutationReplayInputSource::Recorded => "recorded",
    };
    format!(
        "no witness found ({inputs} {noun}, {source}, {} calls of each side)",
        output.calls_per_side
    )
}

/// The inputs of the replay: the `--args` value, or the first recorded
/// inputs of the export. The error is the reason why none can run.
fn input_list(
    cache_dir: &Path,
    options: &MutationReplayOptions,
    export: &str,
) -> Result<Vec<MutationReplayInput>, String> {
    if let Some(args) = &options.args {
        return Ok(vec![MutationReplayInput {
            source: MutationReplayInputSource::Argument,
            input_id: None,
            args: args.clone(),
        }]);
    }
    recorded_inputs(cache_dir, &options.file, export)
}

/// Up to [`MAX_RECORDED_INPUTS`] recorded inputs of `file#export` with their
/// values. The error is the reason why none can run.
pub(super) fn recorded_inputs(
    cache_dir: &Path,
    file: &str,
    export: &str,
) -> Result<Vec<MutationReplayInput>, String> {
    let key = format!("{file}#{export}");
    let hint = "run `fallow mutation --record-args values` for this code, or pass --args";
    let Some(stored) = recorded_args::load(cache_dir, &key, recorded_args::now_secs()) else {
        return Err(format!(
            "no recorded input of `{export}` in the last 7 days; {hint}"
        ));
    };
    if stored.mode != RecordArgsMode::Values {
        return Err(format!(
            "the recording of `{export}` keeps argument types only; {hint}"
        ));
    }
    let inputs: Vec<MutationReplayInput> = stored
        .inputs
        .into_iter()
        .filter_map(|input| {
            Some(MutationReplayInput {
                source: MutationReplayInputSource::Recorded,
                input_id: Some(input.input_id),
                args: input.args?,
            })
        })
        .take(MAX_RECORDED_INPUTS)
        .collect();
    if inputs.is_empty() {
        let reasons: Vec<String> = stored
            .not_replayable
            .iter()
            .map(|refusal| format!("{} ({})", refusal.reason, refusal.count))
            .collect();
        let detail = if reasons.is_empty() {
            "no call of the dry run reached it".to_owned()
        } else {
            format!("the calls were not replayable: {}", reasons.join(", "))
        };
        return Err(format!(
            "no recorded input of `{export}` can run: {detail}; {hint}"
        ));
    }
    Ok(inputs)
}

/// Replays one input on an open companion. A wall-limit timeout gives the
/// verdict `timeout`, and the companion is then gone.
/// The mutant and the export of one replay.
pub(super) struct ReplayTarget<'a> {
    pub file: &'a str,
    pub mutant_id: &'a str,
    pub export: &'a str,
}

fn replay_one(
    link: &mut Box<dyn CompanionLink>,
    target: &ReplayTarget<'_>,
    input: &MutationReplayInput,
    call_timeout: Duration,
) -> ProgrammaticResult<MutationReplayOutput> {
    let export = target.export;
    let request = ReplayRequest {
        file: target.file.to_owned(),
        export: export.to_owned(),
        mutant_id: target.mutant_id.to_owned(),
        args: input.args.clone(),
        calls: CALLS_PER_SIDE,
        call_timeout_ms: millis(call_timeout),
        clock_ms: REPLAY_CLOCK_MS,
        seed: REPLAY_SEED,
    };
    let wall_limit = wall_limit(call_timeout);
    let started = Instant::now();
    match link.request(&Request::Replay(request), wall_limit) {
        Ok(Response::Replay(result)) => completed(input, export.to_owned(), result, call_timeout),
        Ok(_) => Err(replay_error(
            "the companion answered replay with another response",
        )),
        Err(LinkFault::Timeout) => {
            let mut output = without_calls(
                input,
                Some(export.to_owned()),
                MutationReplayVerdict::Timeout,
                format!(
                    "the replay did not end in its wall limit of {}s; the module load, the original or the mutant can block the worker, and a longer --call-timeout gives more time",
                    wall_limit.as_secs()
                ),
                call_timeout,
            )?;
            output.duration_ms = millis(started.elapsed());
            Ok(output)
        }
        Err(fault) => Err(link_error("replay", &fault)),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests expect to keep the setup short")]
mod tests {
    use super::*;

    const TWINS: &str =
        "export const first = (a, b) => a + b;\nexport const second = (a, b) => a + b;\n";

    fn span_of_nth(source: &str, needle: &str, nth: usize) -> (u32, u32) {
        let start = source.match_indices(needle).nth(nth).expect("needle").0;
        let start = u32::try_from(start).expect("small");
        (start, start + u32::try_from(needle.len()).expect("small"))
    }

    /// The id that a run scoped to `span` gives the mutant of that span.
    fn scoped_id(span: (u32, u32)) -> String {
        let options = MutationOptions {
            operators: Operator::ALL.to_vec(),
            ranges: Some(vec![span]),
            record_exports: false,
        };
        let instrumented = fallow_engine::mutation::instrument_source(TWINS, "src/a.js", &options)
            .expect("layout")
            .expect("instrumented");
        instrumented
            .mutants
            .iter()
            .find(|mutant| mutant.span == span)
            .expect("the scoped mutant")
            .id
            .to_string()
    }

    #[test]
    fn a_mutant_id_that_a_twin_shares_replays_the_mutant_at_its_own_span() {
        let second = span_of_nth(TWINS, "a + b", 1);
        let options = MutationReplayOptions {
            file: "src/a.js".to_owned(),
            mutant_id: scoped_id(second),
            span: second,
            ..MutationReplayOptions::default()
        };
        let target = find_target(TWINS, &options)
            .expect("no error")
            .expect("a target");
        assert_eq!(target.export.as_deref(), Ok("second"));
        assert!(target.instrumented.contains(&options.mutant_id));
    }

    #[test]
    fn the_wall_limit_leaves_room_for_the_per_call_limits_of_the_companion() {
        // The Vitest adapter gives each call its call timeout and 10 seconds
        // as the test timeout, and node:test gives it 5 seconds more. Each
        // call also starts a worker before its test timeout runs.
        const COMPANION_CALL_MARGIN: Duration = Duration::from_secs(10);
        for call_timeout in [
            Duration::from_millis(1),
            DEFAULT_CALL_TIMEOUT,
            MAX_CALL_TIMEOUT,
        ] {
            let calls = (call_timeout + COMPANION_CALL_MARGIN) * (2 * CALLS_PER_SIDE);
            assert!(wall_limit(call_timeout) >= calls + Duration::from_secs(60));
        }
    }

    fn output(verdict: MutationReplayVerdict, id: &str) -> MutationReplayOutput {
        let input = MutationReplayInput {
            source: MutationReplayInputSource::Recorded,
            input_id: Some(id.to_owned()),
            args: Vec::new(),
        };
        without_calls(
            &input,
            Some("f".to_owned()),
            verdict,
            String::new(),
            DEFAULT_CALL_TIMEOUT,
        )
        .expect("output")
    }

    #[test]
    fn an_unstable_input_does_not_end_the_search_and_stays_the_result_without_a_witness() {
        use MutationReplayVerdict::{NoWitnessFound, Unstable, Witness};
        assert!(!ends_search(Unstable));
        assert!(!ends_search(NoWitnessFound));
        assert!(ends_search(Witness));
        let picked = pick_output(vec![output(Unstable, "a"), output(Witness, "b")]).expect("one");
        assert_eq!(picked.verdict, Witness);
        let picked =
            pick_output(vec![output(Unstable, "a"), output(NoWitnessFound, "b")]).expect("one");
        assert_eq!(picked.verdict, Unstable);
        assert_eq!(picked.input.input_id.as_deref(), Some("a"));
    }

    #[test]
    fn an_id_at_another_span_is_an_unknown_mutant() {
        let second = span_of_nth(TWINS, "a + b", 1);
        let options = MutationReplayOptions {
            file: "src/a.js".to_owned(),
            mutant_id: scoped_id(second),
            span: (0, 1),
            ..MutationReplayOptions::default()
        };
        let error = find_target(TWINS, &options).err().expect("an error");
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_MUTATION_UNKNOWN_MUTANT")
        );
    }
}
