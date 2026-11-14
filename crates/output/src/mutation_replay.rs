//! The output types of `fallow mutation inspect --replay` and of the
//! argument recording of `fallow mutation --record-args`.

use serde::{Deserialize, Serialize};

/// The schema of a field that holds any JSON value: a primitive, an array
/// or an object.
#[cfg(feature = "schema")]
fn any_json_value(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": ["null", "boolean", "number", "string", "array", "object"]
    })
}

/// The verdict of one replay. A replay never decides that a mutant is
/// equivalent: equal results on both sides give `no_witness_found`, which
/// says only that this input shows no difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationReplayVerdict {
    /// Each call of a side gave the same result, and the two sides differ.
    /// The input is a witness: a test with this input can kill the mutant.
    Witness,
    /// Each call of both sides gave the same result for each input. These
    /// inputs show no difference. Another input can still show one.
    NoWitnessFound,
    /// The calls of one side gave different results, for example from a
    /// clock or a random source that the replay does not fix. Such a
    /// difference is never a witness.
    Unstable,
    /// A call did not end before the call timeout, or the replay did not end
    /// before its wall limit. The module load, the original or the mutant
    /// can block.
    Timeout,
    /// The export is not a function.
    NotCallable,
    /// The replay did not run: the function has no export to call, the file
    /// is a component file, or no recorded input can run. `reason` says why.
    NotReplayable,
}

/// The side of one replay call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationReplaySide {
    /// The original code: no mutant is active.
    Original,
    /// The code with the mutant active.
    Mutant,
}

/// The outcome of one replay call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationReplayOutcome {
    /// The function returned. A returned promise counts with its value.
    Returned,
    /// The function, or the load of its module, threw. A rejected promise
    /// counts too.
    Threw,
    /// The call did not end before the call timeout.
    TimedOut,
    /// The export is not a function.
    NotCallable,
}

/// Where the arguments of a replay come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationReplayInputSource {
    /// The `--args` value of the command.
    Argument,
    /// An input that the record pass of `fallow mutation --record-args values`
    /// recorded.
    Recorded,
}

/// The input of a replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReplayInput {
    /// Where the arguments come from.
    pub source: MutationReplayInputSource,
    /// The id of a recorded input. Absent for `--args`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    /// The arguments in the replay encoding: JSON values, and objects with a
    /// `$replay` tag for `undefined`, special numbers, bigints, dates,
    /// regular expressions, maps, sets and typed arrays.
    pub args: Vec<serde_json::Value>,
}

/// One call of a replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReplayCall {
    /// The side of the call.
    pub side: MutationReplaySide,
    /// The 1-based number of the call on its side.
    pub index: u32,
    /// How the call ended.
    pub outcome: MutationReplayOutcome,
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_json_value"))]
    /// The returned or thrown value in the replay encoding. An error is
    /// `{"$replay": "error", "name", "message"}`. `null` for a call that
    /// timed out.
    pub value: serde_json::Value,
    /// A short text of the value for people, at most 200 characters.
    pub text: String,
}

/// The replay of one export on the original and on one mutant (replay
/// contract 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationReplayOutput {
    /// The version of the replay contract of the runner companion.
    pub contract_version: u32,
    /// The verdict of the replay.
    pub verdict: MutationReplayVerdict,
    /// Why the replay did not run or did not end. Absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The export that the replay called. `null` when no export holds the
    /// mutant.
    pub export: Option<String>,
    /// The arguments of each call. With recorded inputs, the input of the
    /// first witness, else the last input that ran.
    pub input: MutationReplayInput,
    /// The inputs that the replay ran, in order, up to the first witness.
    pub inputs_checked: u32,
    /// The calls of each side. The sides take turns.
    pub calls_per_side: u32,
    /// The time limit of one call, in milliseconds.
    pub call_timeout_ms: u64,
    /// The fixed value of `Date.now()` in each call.
    pub clock_ms: i64,
    /// The seed of `Math.random` in each call.
    pub seed: i64,
    /// Each call, in call order.
    pub calls: Vec<MutationReplayCall>,
    /// The wall time of the replay request, in milliseconds. It differs
    /// between two replays of the same input.
    pub duration_ms: u64,
}

/// What a record pass keeps of the arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MutationRecordMode {
    /// The types of the arguments only.
    Shapes,
    /// The argument values, after the redaction of secrets.
    Values,
}

/// The calls of one reason whose arguments a recording did not keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationNotReplayable {
    /// Why the arguments cannot be replayed. One of a fixed set: `a
    /// function`, `a symbol`, `a proxy`, `a cyclic value`, `a class
    /// instance`, `a value that is too large`, `an accessor property`, `an
    /// object with a symbol key`, `an object that this runtime cannot check
    /// for a proxy`, `a value that cannot be read`, `a value under a secret
    /// key name` or `a string that looks like a secret`. It never holds text
    /// of the code or of a value.
    pub reason: String,
    /// The number of calls.
    pub count: u64,
}

/// The recorded arguments of one exported function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationRecordedFunction {
    /// The project-root-relative path of the file.
    pub file: String,
    /// The export name. `fallow mutation inspect --replay` calls it.
    pub export: String,
    /// The number of distinct inputs that the cache keeps.
    pub input_count: usize,
    /// The calls whose arguments the cache does not keep, by reason.
    pub not_replayable: Vec<MutationNotReplayable>,
}

/// The argument recording of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationRecordingOutput {
    /// What the cache keeps.
    pub mode: MutationRecordMode,
    /// True when the inputs are in the cache file
    /// `<cache dir>/mutation/recorded-args.json`.
    pub saved: bool,
    /// Why the run recorded nothing or saved nothing, for example a runner
    /// that records no arguments. Absent when the inputs are saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The recorded exported functions, sorted by file and export.
    pub functions: Vec<MutationRecordedFunction>,
}

/// The replay of a recorded input against a survived or uncovered mutant,
/// after a `fallow mutation --record-args values` run. It holds no argument
/// value: the values stay in the gitignored cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutantWitness {
    /// The verdict of the replay. Never `equivalent`.
    pub verdict: MutationReplayVerdict,
    /// The recorded input of the verdict. `fallow mutation tasks` joins its
    /// values from the cache.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    /// The recorded inputs that the replay ran.
    pub inputs_checked: u32,
    /// Why the replay gave no witness, for example `no witness found (2
    /// inputs, recorded, 3 calls of each side)`. Absent for a witness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The result of one side of a witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationWitnessSide {
    /// How the call ended.
    pub outcome: MutationReplayOutcome,
    /// The returned or thrown value in the replay encoding.
    #[cfg_attr(feature = "schema", schemars(schema_with = "any_json_value"))]
    pub value: serde_json::Value,
    /// A short text of the value for people.
    pub text: String,
}

/// The witness of a task: the reference of the report, and the values from
/// the local cache when the cache still has them. Only the verdict
/// `witness` can have `args`, `original` and `mutant`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MutationTaskWitness {
    /// The verdict of the replay. Never `equivalent`.
    pub verdict: MutationReplayVerdict,
    /// The recorded input of the verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    /// The recorded inputs that the replay ran.
    pub inputs_checked: u32,
    /// Why the replay gave no witness. Absent for a witness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The command that replays the mutant again and shows both sides.
    pub replay_command: String,
    /// The arguments of the input in the replay encoding: JSON, and objects
    /// with a `$replay` tag for other values (`{"$replay": "undefined"}`,
    /// `{"$replay": "bigint", "value": "12"}`, and `date`, `regexp`, `map`,
    /// `set` and `bytes`). Absent for another verdict, for a stale task, when
    /// the cache no longer has the input (7 days), and in the markdown
    /// output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<serde_json::Value>>,
    /// The result on the original code: the value that a new test asserts.
    /// The replay ran with a fixed clock and seed, so run the new test on the
    /// current code to confirm it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<MutationWitnessSide>,
    /// The result on the mutant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutant: Option<MutationWitnessSide>,
}
