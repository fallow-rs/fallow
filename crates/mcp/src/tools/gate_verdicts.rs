//! The run's verdicts, restated in the tool result an agent actually reads.
//!
//! Every CLI-backed tool spawns the CLI with `--format json --quiet`, which
//! removes the stderr line a gate prints, and `non_success_result` converts
//! exit 1 into a success so the findings still reach the caller. Both are
//! deliberate. The cost is that a gated run and an ungated one arrive
//! identically: the agent is handed a report and nothing that says the CLI
//! judged it and said no (#2676, #2682).
//!
//! The envelope has carried the facts since `gate_outcomes` shipped, so this
//! module adds no verdict of its own. It restates three things the envelope
//! already decided, as plain strings on the root `warnings` array:
//! `baseline_staleness`, every `gate_outcomes` entry that reported `fail` or
//! `warn`, and whether `workspace_diagnostics` degraded the analysis.
//!
//! # Why the `warnings` array and not a text line
//!
//! The result's single content block is the CLI's stdout, and
//! `captured_output_result` substitutes `"{}"` for an empty one precisely so
//! the block always parses. A prepended prose line would make
//! `JSON.parse(result.content[0].text)` throw for every CLI-backed tool. The
//! house pattern for adding a fact to that block is to parse, mutate and
//! re-serialize, which is what `super::ensure_top_level_warnings` already does.
//!
//! # Every route, not only the subprocess
//!
//! The three sources are envelope members, not process state, so the typed
//! `fallow-api` route reads them too and is annotated at its one exit point.
//! Without that a tool would answer differently depending on whether a
//! parameter happened to force the CLI fallback, and an agent reading an empty
//! `warnings` array on the typed route would conclude the run was clean.
//!
//! # What is deliberately not done
//!
//! Nothing here moves an existing member, so a consumer already reading
//! `gate_outcomes`, `baseline_staleness` or `workspace_diagnostics` sees the
//! same values in the same places. A response with nothing to state is not
//! re-serialized at all and comes back byte for byte as the CLI wrote it.
//! And no result changes its `isError`: an exit-1 gate stays a success
//! carrying findings, and an exit 8 security gate stays an error, now carrying
//! a sentence that says which gate produced it.
//!
//! One accounting note: `max_output_bytes` bounds what the CLI wrote, not what
//! the tool returns, so an annotated body is the handful of bytes these
//! sentences cost larger than the cap that admitted it. Code Mode's two checks
//! measure different bodies for the same reason: the pre-read comparison
//! admits the raw file, while the host call is charged the annotated body,
//! because that is what the snippet reads. A call landing in the last few
//! hundred bytes of its budget can therefore be refused after the analysis
//! ran, which is the correct side to err on: the alternative is charging an
//! agent less than what enters its context.

use std::collections::BTreeMap;

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Map, Value};

/// Re-render one tool result with this run's verdicts appended to the
/// envelope's root `warnings` array.
///
/// Returns the result untouched when the body is not a JSON object, when the
/// run reported no verdict worth stating, or when `warnings` is present and is
/// not an array. `is_error` and every other member of the result are
/// preserved, because whether a run failed is decided by the exit code the
/// caller already translated, never by what a gate says here.
pub(super) fn annotate_gate_verdicts(mut result: CallToolResult) -> CallToolResult {
    let Some(ContentBlock::Text(text)) = result.content.first_mut() else {
        return result;
    };
    let Some(annotated) = annotate_envelope(&text.text) else {
        return result;
    };
    // Replace the text, not the block and not the vector: annotations, `_meta`
    // and any sibling block belong to whoever put them there.
    text.text = annotated;
    result
}

/// [`annotate_gate_verdicts`] for callers that hold the body as a string:
/// Code Mode normalizes its subprocess output before any result exists, and
/// must read the same warnings a direct tool call does.
///
/// `None` means "nothing to add", which the caller reads as "pass the original
/// bytes through" rather than as a failure.
pub(super) fn annotate_envelope(text: &str) -> Option<String> {
    // Parsing is the expensive part and most tools can never carry any of
    // these members, so rule those out on the raw bytes first: a key that is
    // absent from the text cannot be present in the parse.
    if !CARRIER_KEYS.iter().any(|key| text.contains(key)) {
        return None;
    }
    let mut value = serde_json::from_str::<Value>(text).ok()?;
    if !annotate_in_place(&mut value) {
        return None;
    }
    serde_json::to_string(&value).ok()
}

/// [`annotate_envelope`] for the typed route, which already holds the parsed
/// envelope and must not pay for a round trip through text to reach it.
///
/// This one copies, because the route owns the value it is rendering and can
/// only lend it out. The copy is paid only on a run that has something to say,
/// which is why the check comes first.
pub(super) fn annotate_value(value: &Value) -> Option<String> {
    let warnings = verdict_warnings(value.as_object()?);
    if warnings.is_empty() {
        return None;
    }
    let mut annotated = value.clone();
    append_warnings(annotated.as_object_mut()?, warnings)?;
    serde_json::to_string(&annotated).ok()
}

/// Append this run's verdicts to an owned envelope, reporting whether any were
/// added. The text route parses into a value it owns, so it mutates that
/// rather than copying it.
fn annotate_in_place(value: &mut Value) -> bool {
    let Some(root) = value.as_object() else {
        return false;
    };
    let warnings = verdict_warnings(root);
    if warnings.is_empty() {
        return false;
    }
    let Some(root) = value.as_object_mut() else {
        return false;
    };
    append_warnings(root, warnings).is_some()
}

/// Add the sentences to the root `warnings` array, creating it when absent.
///
/// `None` when `warnings` is present and is not an array, which leaves the
/// envelope untouched: nothing was inserted, because the entry only fills an
/// absent key.
fn append_warnings(root: &mut Map<String, Value>, warnings: Vec<String>) -> Option<()> {
    let existing = root
        .entry("warnings".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    existing
        .as_array_mut()?
        .extend(warnings.into_iter().map(Value::String));
    Some(())
}

/// The members that make an envelope worth parsing. Any absent from the raw
/// text means this module has nothing to say about that response.
const CARRIER_KEYS: &[&str] = &[
    "gate_outcomes",
    "baseline_staleness",
    "workspace_diagnostics",
    "request_outcomes",
];

/// Every verdict this envelope states, in a fixed order so two identical runs
/// produce identical responses.
///
/// The baseline sites are the shared list, because the pull-request comment
/// advisory reads the same envelopes and a site only one surface knows about
/// is a baseline only one surface reports. `audit` resolves its baselines from
/// config as well as from parameters, which is why the sites are keyed on the
/// envelope rather than on what the caller passed.
fn verdict_warnings(root: &Map<String, Value>) -> Vec<String> {
    let noun = noun(root);
    let mut warnings: Vec<String> = fallow_types::envelope_sites::baseline_staleness_objects(root)
        .filter_map(|(staleness, analysis)| baseline_warning(staleness, analysis, noun))
        .collect();
    let baseline_reported = !warnings.is_empty();
    warnings.extend(gate_warnings(root, baseline_reported));
    warnings.extend(degraded_analysis_warning(root));
    warnings.extend(unapplied_request_warning(root));
    warnings.extend(empty_scope_request_warning(root));
    warnings
}

/// Where a run publishes the diagnostics that say it was degraded, in the
/// order they are looked for.
///
/// The single-analysis and combined envelopes carry them at the root; `audit`
/// carries them inside its sub-analysis sections. First match wins rather than
/// summing, because a run records its diagnostics once and adding up two
/// views of the same walk would report every kind twice.
const DIAGNOSTIC_SITES: &[&[&str]] = &[
    &["workspace_diagnostics"],
    &["dead_code", "workspace_diagnostics"],
    &["duplication", "workspace_diagnostics"],
    &["complexity", "workspace_diagnostics"],
];

/// What this command calls the things a baseline entry describes, matching the
/// noun its own CLI advisory uses. Read from the envelope's `kind`, so a
/// multi-section run keeps the generic noun rather than borrowing one
/// section's; the entry names the analysis separately.
fn noun(root: &Map<String, Value>) -> &'static str {
    match root.get("kind").and_then(Value::as_str) {
        Some("dead-code") => "issue",
        Some("dupes") => "clone group",
        _ => "finding",
    }
}

/// One sentence for a loaded baseline that matched less than it was saved
/// with, mirroring the CLI's two advisory messages and its gate message, for
/// one this run was too narrow to judge at all, or for a file that is not this
/// command's baseline.
///
/// A file written by another command is read from `unrecognised_format` rather
/// than from `baseline_entries == 0`, which is also what a baseline saved on a
/// project with nothing to record carries.
///
/// The remedy names the parameter rather than a path because the envelope
/// carries no baseline path, and because `audit` resolves its three baselines
/// from config, where there was never a parameter to echo back.
fn baseline_warning(
    staleness: &Value,
    analysis: Option<&str>,
    noun: &'static str,
) -> Option<String> {
    let advisory = staleness
        .get("warning")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let gate_trips = staleness
        .get("gate_trips")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let baseline_entries = count(staleness, "baseline_entries");
    let total = entries(baseline_entries);
    let stale = count(staleness, "stale_entries");
    let subject = analysis.map_or_else(
        || "the loaded baseline".to_string(),
        |analysis| format!("the {analysis} baseline"),
    );
    if staleness
        .get("unrecognised_format")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let mut message = format!(
            "Baseline staleness: {subject} has no entries this command recognises. It may be a \
             baseline saved by another command, or an empty file. Either way it suppresses \
             nothing."
        );
        // The gate rule holds on such a file, so the clause belongs here too:
        // without it an agent reads a sentence about a harmless empty file while
        // the same run would fail a repository that armed the gate.
        if gate_trips {
            message.push_str(" --fail-on-stale-baseline fails a run in this state.");
        }
        return Some(message);
    }
    if advisory == "none" && !gate_trips {
        return unjudged_baseline_warning(staleness, &subject, &total, baseline_entries);
    }

    let mut message = match advisory {
        "zero-overlap" => format!(
            "Baseline staleness: {subject} has {total} but matched 0 current {noun}s. \
             Paths may have changed, or the baseline was saved on a different machine."
        ),
        "partial" => format!(
            "Baseline staleness: {stale} of {total} in {subject} matched no current {noun}, \
             so it protects less than what was saved."
        ),
        _ => format!(
            "Baseline staleness: {stale} of {total} in {subject} matched no current {noun}."
        ),
    };
    if gate_trips {
        message.push_str(" --fail-on-stale-baseline fails a run in this state.");
    }
    message.push_str(" Re-save it with the save_baseline parameter (CLI --save-baseline).");
    Some(message)
}

/// One sentence for a baseline this run could not judge at all, which neither
/// advisory covers.
///
/// Reached only with a non-empty baseline; a zero-entry one is reported by its
/// own sentence before this, because "nothing was judged" and "there was
/// nothing to judge" are different problems with different remedies.
///
/// A change-scoped run compares a whole-project baseline against a slice of
/// it, so the advisory and the gate both stand down by construction and a
/// rotting baseline stays silent for as long as every run is narrowed. The
/// parenthetical names `scope_reasons`; an envelope from a version that
/// predates that member drops the clause rather than guessing.
fn unjudged_baseline_warning(
    staleness: &Value,
    subject: &str,
    total: &str,
    baseline_entries: u64,
) -> Option<String> {
    if baseline_entries == 0 {
        return None;
    }
    if !staleness
        .get("change_scoped")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let reasons = scope_reasons(staleness);
    let scope = if reasons.is_empty() {
        "only part of the project".to_string()
    } else {
        format!("only part of the project ({reasons})")
    };
    Some(format!(
        "Baseline staleness: {subject} has {total} and was not judged on this run, which \
         analyzed {scope}. Run the command over the whole project to judge it."
    ))
}

/// The run's `scope_reasons` as a comma-joined list, empty when the member is
/// absent or carries nothing readable.
fn scope_reasons(staleness: &Value) -> String {
    staleness
        .get("scope_reasons")
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn entries(count: u64) -> String {
    if count == 1 {
        return "1 entry".to_string();
    }
    format!("{count} entries")
}

/// One sentence per gate that reported `fail` or `warn`.
///
/// `pass` and `skipped` are left out on purpose: an agent acting on a result
/// needs what stood in its way, and a gate that concluded nothing is still
/// readable in `gate_outcomes` for a caller that wants the full inventory.
///
/// `stale-baseline` is left out too when a baseline entry already spoke and
/// the gate was not armed, which is every MCP run that loads a baseline: no
/// tool passes `--fail-on-stale-baseline`, so the gate entry would add a
/// second sentence saying the verdict changed nothing, next to one that
/// carries the counts and the remedy. An armed gate still reports, because
/// "this run exited non-zero for it" is not in the baseline sentence.
fn gate_warnings(root: &Map<String, Value>, baseline_reported: bool) -> Vec<String> {
    let Some(gates) = root.get("gate_outcomes").and_then(Value::as_object) else {
        return Vec::new();
    };
    gates
        .iter()
        .filter(|(name, outcome)| {
            !(baseline_reported
                && name.as_str() == "stale-baseline"
                && outcome.get("enforced").and_then(Value::as_bool) != Some(true))
        })
        .filter_map(|(name, outcome)| gate_warning(name, outcome))
        .collect()
}

fn gate_warning(name: &str, outcome: &Value) -> Option<String> {
    let status = outcome.get("status").and_then(Value::as_str)?;
    let verdict = match status {
        "fail" => "failed",
        "warn" => "reported warn",
        _ => return None,
    };
    let enforced = outcome
        .get("enforced")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // An enforced failure is the sentence an agent is most likely to misread,
    // because the result it arrives in reports success. Say why both are true.
    let enforcement = match (status, enforced) {
        ("fail", true) => {
            "enforced, so the CLI exited non-zero for it, and this result still \
                           carries the full report"
        }
        (_, true) => "armed, though a warn does not fail the run",
        _ => "not enforced on this run, so it did not change the exit code",
    };
    let measurement = gate_measurement(outcome);
    Some(format!(
        "Gate {name} {verdict}{measurement}; {enforcement}."
    ))
}

/// The numbers the gate compared, when it compared any.
///
/// `threshold_label` is carried alongside `threshold` rather than instead of
/// it: on `regression` the number is the allowance in issues while the label
/// is the tolerance as the user spelled it, and dropping either loses half the
/// verdict.
fn gate_measurement(outcome: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(observed) = outcome.get("observed").and_then(Value::as_f64) {
        parts.push(format!("observed {}", number(observed)));
    }
    let threshold = outcome.get("threshold").and_then(Value::as_f64);
    let label = outcome.get("threshold_label").and_then(Value::as_str);
    match (threshold, label) {
        (Some(threshold), Some(label)) => parts.push(format!(
            "threshold {} spelled \"{label}\"",
            number(threshold)
        )),
        (Some(threshold), None) => parts.push(format!("threshold {}", number(threshold))),
        (None, Some(label)) => parts.push(format!("threshold \"{label}\"")),
        (None, None) => {}
    }
    if parts.is_empty() {
        return String::new();
    }
    format!(" ({})", parts.join(", "))
}

/// Render a gate number the way the gate meant it.
///
/// Counts arrive as JSON numbers with a zero fraction, and `observed 3` reads
/// as a count where `observed 3.0` reads as a measurement. A measured
/// percentage arrives at full `f64` precision, which is a number for computing
/// with and not for reading: `97.82608695652172` in a sentence is noise around
/// the two digits that carry the verdict. The sentence rounds; the envelope's
/// own `observed` keeps every digit for anything that computes on it.
fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    let rounded = format!("{value:.2}");
    rounded
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// One sentence for every degrading diagnostic together, never one per kind.
///
/// A degraded run can record more than a dozen kinds, and an agent that has to
/// read thirteen near-identical lines learns less than one that reads a list.
/// The classification stays in Rust: this reads `degrades_analysis` rather
/// than matching a kind allowlist that a later release would silently outgrow.
fn degraded_analysis_warning(root: &Map<String, Value>) -> Option<String> {
    let counts = DIAGNOSTIC_SITES
        .iter()
        .filter_map(|path| fallow_types::envelope_sites::lookup(root, path)?.as_array())
        .map(|diagnostics| degrading_kinds(diagnostics))
        .find(|counts| !counts.is_empty())?;
    let kinds = counts
        .iter()
        .map(|(kind, count)| format!("{kind} ({count})"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Analysis was degraded: {kinds}. Findings may be incomplete; read \
         workspace_diagnostics for what each kind changes."
    ))
}

/// One sentence for every request the run could not apply, never one per
/// request.
///
/// Reads the envelope's own `request_outcomes`, so a request added in a later
/// release reports here without a change. Honoured requests are deliberately
/// silent: an agent acting on a result needs what stood in its way, and
/// "the filter applied" is readable in `request_outcomes` for a caller that
/// wants the full inventory.
///
/// This one matters more on the MCP route than anywhere else. Every CLI-backed
/// tool spawns the CLI with `--quiet`, which removes the stderr line that used
/// to be the ONLY trace of a stand-down, and the agent is then handed a
/// whole-project report in answer to a question about a change.
///
/// Grouped by the entry's `affects`, never by its name. The object also carries
/// requests that write a file beside the report, whose failure leaves the
/// report's scope untouched, and one sentence for both classes tells the agent
/// that an unwritten file widened the analysis. A class this build does not
/// recognise is reported without either claim rather than assumed to narrow.
fn unapplied_request_warning(root: &Map<String, Value>) -> Option<String> {
    let requests = root.get("request_outcomes")?.as_object()?;
    let mut widened: Vec<String> = Vec::new();
    let mut withheld: Vec<String> = Vec::new();
    let mut unclassified: Vec<String> = Vec::new();
    for (name, outcome) in requests {
        if outcome.get("status").and_then(Value::as_str) == Some("applied") {
            continue;
        }
        let described = match outcome.get("reason").and_then(Value::as_str) {
            Some(reason) => format!("{name} ({reason})"),
            None => name.clone(),
        };
        match outcome.get("affects").and_then(Value::as_str) {
            Some("scope") => widened.push(described),
            Some("artifact") => withheld.push(described),
            _ => unclassified.push(described),
        }
    }
    let mut sentences: Vec<String> = Vec::new();
    if !widened.is_empty() {
        sentences.push(format!(
            "Requests not applied: {}. The report below is complete and WIDER than what was \
             asked for; read request_outcomes for the remedy on each.",
            widened.join(", ")
        ));
    }
    if !withheld.is_empty() {
        sentences.push(format!(
            "Output not written: {}. The report below is unaffected, but anything reading \
             that file has nothing to read; read request_outcomes for the remedy on each.",
            withheld.join(", ")
        ));
    }
    if !unclassified.is_empty() {
        sentences.push(format!(
            "Requests not applied: {}. Read request_outcomes for what each one means.",
            unclassified.join(", ")
        ));
    }
    (!sentences.is_empty()).then(|| sentences.join(" "))
}

/// One sentence for a narrowing request the run DID apply over a scope it
/// measured as empty.
///
/// The opposite shape of [`unapplied_request_warning`], and the one a clean
/// report cannot state for itself: every finding filters out of an empty scope,
/// so the agent is handed a report with nothing in it for a run that analyzed
/// nothing. Keyed on `scope_size == 0` beside `status == "applied"`, so an
/// envelope from a binary that publishes no such member says nothing here.
///
/// Restricted to `affects == "scope"`: a request that writes a file beside the
/// report never narrows what was analyzed, so a size it measured says nothing
/// about the findings.
fn empty_scope_request_warning(root: &Map<String, Value>) -> Option<String> {
    let requests = root.get("request_outcomes")?.as_object()?;
    let empty: Vec<&str> = requests
        .iter()
        .filter(|(_, outcome)| {
            outcome.get("status").and_then(Value::as_str) == Some("applied")
                && outcome.get("affects").and_then(Value::as_str) == Some("scope")
                && outcome.get("scope_size").and_then(Value::as_u64) == Some(0)
        })
        .map(|(name, _)| name.as_str())
        .collect();
    (!empty.is_empty()).then(|| {
        format!(
            "Requests applied over an empty scope: {}. No finding can survive a scope that \
             measured nothing, so this report is clean because nothing in it was analyzable, not \
             because the code is clean; check the diff or ref the run was given.",
            empty.join(", ")
        )
    })
}

fn degrading_kinds(diagnostics: &[Value]) -> BTreeMap<&str, usize> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for diagnostic in diagnostics {
        if diagnostic.get("degrades_analysis").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let kind = diagnostic
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        *counts.entry(kind).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings_of(envelope: &Value) -> Vec<String> {
        // `None` means the run had nothing to state, which is a verdict of its
        // own and must read as an empty list rather than as a broken envelope.
        let Some(annotated) = annotate_envelope(&envelope.to_string()) else {
            return Vec::new();
        };
        let value: Value = serde_json::from_str(&annotated).expect("annotated body parses");
        value["warnings"]
            .as_array()
            .expect("warnings is an array")
            .iter()
            .map(|entry| entry.as_str().expect("warning is a string").to_string())
            .collect()
    }

    fn staleness(advisory: &str, entries: u64, matched: u64, gate_trips: bool) -> Value {
        serde_json::json!({
            "baseline_entries": entries,
            "matched_entries": matched,
            "stale_entries": entries - matched,
            "current_findings": 2,
            "change_scoped": false,
            "stale": advisory != "none",
            "warning": advisory,
            "gate_trips": gate_trips,
            "moved_entries": 0,
        })
    }

    /// A narrowed run compares a whole-project baseline against a slice of it,
    /// so the advisory and the gate both stand down by construction. Without a
    /// third sentence such a baseline rots without a word for as long as every
    /// run is narrowed, which is every merge-request pipeline.
    #[test]
    fn a_baseline_no_run_could_judge_is_still_reported() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": {
                "baseline_entries": 8,
                "matched_entries": 0,
                "stale_entries": 8,
                "current_findings": 0,
                "change_scoped": true,
                "stale": false,
                "warning": "none",
                "gate_trips": false,
                "moved_entries": 0,
                "scope_reasons": ["changed-since", "production"],
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(
            warnings[0],
            "Baseline staleness: the loaded baseline has 8 entries and was not judged on this \
             run, which analyzed only part of the project (changed-since, production). Run the \
             command over the whole project to judge it."
        );
    }

    /// An envelope from a version that predates `scope_reasons` still gets the
    /// sentence, minus the clause it cannot fill.
    #[test]
    fn an_unjudged_baseline_without_scope_reasons_drops_the_clause() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dupes",
            "baseline_staleness": {
                "baseline_entries": 3,
                "matched_entries": 0,
                "stale_entries": 3,
                "current_findings": 0,
                "change_scoped": true,
                "stale": false,
                "warning": "none",
                "gate_trips": false,
                "moved_entries": 0,
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("analyzed only part of the project. Run the command"),
            "{warnings:?}"
        );
    }

    #[test]
    fn an_empty_baseline_on_a_narrowed_run_says_nothing_about_staleness() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": {
                "baseline_entries": 0,
                "matched_entries": 0,
                "stale_entries": 0,
                "current_findings": 0,
                "change_scoped": true,
                "stale": false,
                "warning": "none",
                "gate_trips": false,
                "moved_entries": 0,
                "scope_reasons": ["production"],
            },
        }));

        assert!(
            warnings
                .iter()
                .all(|warning| !warning.contains("was not judged")),
            "{warnings:?}"
        );
    }

    /// A baseline written by another command earns every green verdict below
    /// honestly and would otherwise say nothing, so an agent handed the report
    /// cannot tell a working baseline from one saved by another command.
    #[test]
    fn a_baseline_with_no_recognised_entries_is_reported() {
        let mut unrecognised = staleness("none", 0, 0, false);
        unrecognised["unrecognised_format"] = serde_json::json!(true);
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dupes",
            "baseline_staleness": unrecognised,
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(
            warnings[0],
            "Baseline staleness: the loaded baseline has no entries this command recognises. It \
             may be a baseline saved by another command, or an empty file. Either way it \
             suppresses nothing."
        );
    }

    /// The gate rule holds on such a file, so the sentence carries the clause the
    /// count sentences carry. Without it an agent reads about a harmless empty
    /// file while the same run fails a repository that armed the gate.
    #[test]
    fn a_baseline_with_no_recognised_entries_says_the_gate_fails_on_it() {
        let mut unrecognised = staleness("none", 0, 0, true);
        unrecognised["unrecognised_format"] = serde_json::json!(true);
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dupes",
            "baseline_staleness": unrecognised,
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].ends_with(" --fail-on-stale-baseline fails a run in this state."),
            "{warnings:?}"
        );
    }

    /// The same numbers with the member absent describe a baseline saved on a
    /// project that had nothing to record, which is not a mistake and must not
    /// be reported as one.
    #[test]
    fn an_empty_baseline_of_this_command_is_not_reported() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dupes",
            "baseline_staleness": staleness("none", 0, 0, false),
        }));

        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// The unrecognised sentence wins over the unjudged one: "this is not a
    /// baseline of mine" and "nothing judged it" are different problems with
    /// different remedies, and re-running unscoped would not fix the first.
    #[test]
    fn a_zero_entry_baseline_on_a_narrowed_run_reports_the_empty_baseline() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "health",
            "summary": {
                "baseline_staleness": {
                    "baseline_entries": 0,
                    "matched_entries": 0,
                    "stale_entries": 0,
                    "current_findings": 0,
                    "change_scoped": true,
                    "stale": false,
                    "warning": "none",
                    "gate_trips": false,
                    "moved_entries": 0,
                    "unrecognised_format": true,
                    "scope_reasons": ["production"],
                },
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("no entries this command recognises"),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_baseline_that_matched_nothing_names_the_advisory_and_the_remedy() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": staleness("zero-overlap", 2, 0, true),
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("has 2 entries but matched 0 current issues"));
        assert!(warnings[0].contains("--fail-on-stale-baseline fails a run in this state"));
        assert!(warnings[0].contains("save_baseline"));
    }

    #[test]
    fn a_one_entry_baseline_is_not_reported_as_entries() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": staleness("zero-overlap", 1, 0, true),
        }));

        assert!(warnings[0].contains("has 1 entry but"), "{warnings:?}");
    }

    #[test]
    fn a_partially_stale_baseline_reports_the_fraction_that_went_unmatched() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": staleness("partial", 8, 6, true),
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("2 of 8 entries in the loaded baseline matched no current issue"),
            "{warnings:?}"
        );
        assert!(warnings[0].contains("protects less than what was saved"));
    }

    /// The gate is stricter than the advisory: a rotted baseline on a cleaned
    /// project reports `warning: none` with `gate_trips: true`. That run is
    /// exactly what #2676 is about, so it must not stay silent.
    #[test]
    fn a_tripping_gate_with_no_advisory_still_produces_an_entry() {
        let warnings = warnings_of(&serde_json::json!({
            "baseline_staleness": staleness("none", 4, 1, true),
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0]
                .contains("3 of 4 entries in the loaded baseline matched no current finding"),
            "{warnings:?}"
        );
        assert!(warnings[0].contains("--fail-on-stale-baseline"));
    }

    /// No MCP tool passes `--fail-on-stale-baseline`, so the gate entry on a
    /// baselined run always reports an unarmed verdict the baseline sentence
    /// has already covered in more detail.
    #[test]
    fn an_unarmed_stale_baseline_gate_does_not_repeat_the_baseline_sentence() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": staleness("zero-overlap", 2, 0, true),
            "gate_outcomes": {
                "stale-baseline": { "status": "fail", "enforced": false },
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("Baseline staleness:"),
            "{warnings:?}"
        );
    }

    /// An armed one is kept: that it made the run exit non-zero is a fact the
    /// baseline sentence does not carry.
    #[test]
    fn an_armed_stale_baseline_gate_is_reported_beside_the_baseline_sentence() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "baseline_staleness": staleness("zero-overlap", 2, 0, true),
            "gate_outcomes": {
                "stale-baseline": { "status": "fail", "enforced": true },
            },
        }));

        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[1].starts_with("Gate stale-baseline failed"),
            "{warnings:?}"
        );
    }

    #[test]
    fn an_enforced_failing_gate_names_the_numbers_and_the_non_zero_exit() {
        let warnings = warnings_of(&serde_json::json!({
            "gate_outcomes": {
                "health-min-score": {
                    "status": "fail",
                    "enforced": true,
                    "observed": 86.7,
                    "threshold": 99.0,
                },
            },
        }));

        assert_eq!(
            warnings,
            vec![
                "Gate health-min-score failed (observed 86.7, threshold 99); enforced, so the CLI \
                 exited non-zero for it, and this result still carries the full report."
                    .to_string()
            ]
        );
    }

    #[test]
    fn an_unenforced_failing_gate_says_the_exit_code_did_not_move() {
        let warnings = warnings_of(&serde_json::json!({
            "gate_outcomes": {
                "duplication-threshold": { "status": "fail", "enforced": false },
            },
        }));

        assert_eq!(
            warnings,
            vec![
                "Gate duplication-threshold failed; not enforced on this run, so it did not \
                 change the exit code."
                    .to_string()
            ]
        );
    }

    /// A measured percentage reaches the envelope at full `f64` precision.
    /// The sentence carries the two digits that decide the verdict, and the
    /// envelope keeps the rest.
    #[test]
    fn a_measured_percentage_is_rounded_for_the_sentence() {
        let warnings = warnings_of(&serde_json::json!({
            "gate_outcomes": {
                "duplication-threshold": {
                    "status": "fail",
                    "enforced": true,
                    "observed": 97.826_086_956_521_72,
                    "threshold": 0.1,
                },
            },
        }));

        assert!(
            warnings[0].contains("observed 97.83, threshold 0.1"),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_warn_tier_gate_reports_its_tier_and_its_spelled_threshold() {
        let warnings = warnings_of(&serde_json::json!({
            "gate_outcomes": {
                "regression": {
                    "status": "warn",
                    "enforced": true,
                    "observed": 3.0,
                    "threshold": 2.0,
                    "threshold_label": "50%",
                },
            },
        }));

        assert_eq!(
            warnings,
            vec![
                "Gate regression reported warn (observed 3, threshold 2 spelled \"50%\"); armed, \
                 though a warn does not fail the run."
                    .to_string()
            ]
        );
    }

    #[test]
    fn passing_and_skipped_gates_say_nothing() {
        assert!(
            annotate_envelope(
                &serde_json::json!({
                    "gate_outcomes": {
                        "health-findings": { "status": "skipped", "enforced": false },
                        "stale-baseline": { "status": "pass", "enforced": true },
                    },
                })
                .to_string()
            )
            .is_none()
        );
    }

    #[test]
    fn degrading_diagnostics_aggregate_into_one_entry_with_counts() {
        let warnings = warnings_of(&serde_json::json!({
            "workspace_diagnostics": [
                { "kind": "node-modules-missing", "degrades_analysis": true },
                { "kind": "no-source-files-analyzed", "degrades_analysis": true },
                { "kind": "node-modules-missing", "degrades_analysis": true },
                { "kind": "boundaries-not-configured" },
            ],
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("no-source-files-analyzed (1), node-modules-missing (2)"),
            "{warnings:?}"
        );
        assert!(
            !warnings[0].contains("boundaries-not-configured"),
            "an advisory diagnostic is not a degraded run: {warnings:?}"
        );
    }

    /// The combined envelope carries one baseline per section, so the entry has
    /// to name which analysis rotted.
    #[test]
    fn a_combined_envelope_names_the_analysis_each_baseline_belongs_to() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "combined",
            "check": { "baseline_staleness": staleness("zero-overlap", 2, 0, true) },
            "health": { "summary": { "baseline_staleness": staleness("partial", 4, 3, true) } },
        }));

        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[0].contains("the dead-code baseline"),
            "{warnings:?}"
        );
        assert!(warnings[1].contains("the health baseline"), "{warnings:?}");
    }

    /// The shape below is the one `fallow audit --base <ref> --format json
    /// --quiet --health-baseline <file>` emits, measured against the binary:
    /// the diagnostics sit under `dead_code` and the only staleness object
    /// audit publishes sits under `complexity.summary`, with
    /// `change_scoped: true` because every audit is change-scoped. So a real
    /// audit reports its degraded run, and now also reports the baseline no
    /// audit can judge. A change-scoped run cannot judge a whole-project
    /// baseline, which used to be stated as silence; a baseline every run
    /// stands down on rots without a word, so it gets the unjudged sentence.
    #[test]
    fn an_audit_envelope_reports_its_degraded_run_and_its_unjudged_baseline() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "audit",
            "verdict": "pass",
            "dead_code": {
                "workspace_diagnostics": [
                    { "kind": "node-modules-missing", "degrades_analysis": true },
                ],
            },
            "complexity": {
                "summary": {
                    "baseline_staleness": {
                        "baseline_entries": 1,
                        "matched_entries": 1,
                        "stale_entries": 0,
                        "current_findings": 1,
                        "change_scoped": true,
                        "stale": false,
                        "warning": "none",
                        "gate_trips": false,
                        "moved_entries": 0,
                    },
                },
            },
        }));

        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(
            warnings[0].contains("the complexity baseline has 1 entry and was not judged"),
            "{warnings:?}"
        );
        assert!(
            warnings[1].contains("node-modules-missing (1)"),
            "{warnings:?}"
        );
    }

    /// Should a whole-project audit ever publish that object, the sentence has
    /// to name the section an agent can go and read, which is `complexity`.
    #[test]
    fn the_audit_complexity_baseline_is_named_by_its_section() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "audit",
            "complexity": {
                "summary": { "baseline_staleness": staleness("zero-overlap", 5, 0, true) },
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("the complexity baseline"),
            "{warnings:?}"
        );
    }

    /// Two sections reporting the same walk must not be added together.
    #[test]
    fn sectioned_diagnostics_are_read_once_rather_than_summed() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "audit",
            "dead_code": {
                "workspace_diagnostics": [
                    { "kind": "node-modules-missing", "degrades_analysis": true },
                ],
            },
            "duplication": {
                "workspace_diagnostics": [
                    { "kind": "node-modules-missing", "degrades_analysis": true },
                ],
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("node-modules-missing (1)"),
            "{warnings:?}"
        );
    }

    #[test]
    fn an_envelope_with_no_verdict_is_passed_through_unchanged() {
        let clean = serde_json::json!({
            "schema_version": 1,
            "total_issues": 0,
            "workspace_diagnostics": [{ "kind": "boundaries-not-configured" }],
        })
        .to_string();

        assert!(annotate_envelope(&clean).is_none());
    }

    /// An agent may be pointed at a pinned older binary whose envelope predates
    /// `gate_outcomes` and `degrades_analysis`. That is not an error, and it
    /// must not produce a fabricated verdict either.
    #[test]
    fn a_pre_gate_outcomes_envelope_produces_nothing() {
        let older = serde_json::json!({
            "schema_version": 1,
            "total_issues": 4,
            "regression": { "exceeded": true },
            "workspace_diagnostics": [{ "kind": "node-modules-missing" }],
        })
        .to_string();

        assert!(annotate_envelope(&older).is_none());
    }

    /// A body carrying none of the three members must not be parsed at all,
    /// which is what keeps the annotation off the tools that can never report
    /// a verdict.
    #[test]
    fn a_body_without_any_carrier_key_is_rejected_before_parsing() {
        let unrelated = serde_json::json!({ "file_count": 12, "entry_points": [] }).to_string();

        assert!(annotate_envelope(&unrelated).is_none());
    }

    #[test]
    fn an_existing_warnings_array_is_appended_to_rather_than_replaced() {
        let annotated = annotate_envelope(
            &serde_json::json!({
                "warnings": ["Package manager was not detected."],
                "gate_outcomes": { "security": { "status": "fail", "enforced": true } },
            })
            .to_string(),
        )
        .expect("envelope is annotated");
        let value: Value = serde_json::from_str(&annotated).expect("annotated body parses");

        let warnings = value["warnings"].as_array().expect("warnings is an array");
        assert_eq!(warnings.len(), 2, "{value}");
        assert_eq!(warnings[0], "Package manager was not detected.");
        assert!(
            warnings[1]
                .as_str()
                .is_some_and(|entry| entry.starts_with("Gate security failed")),
            "{value}"
        );
    }

    #[test]
    fn a_warnings_key_that_is_not_an_array_is_left_alone() {
        assert!(
            annotate_envelope(
                &serde_json::json!({
                    "warnings": "none",
                    "gate_outcomes": { "security": { "status": "fail", "enforced": true } },
                })
                .to_string()
            )
            .is_none()
        );
    }

    #[test]
    fn a_body_that_is_not_a_json_object_is_left_alone() {
        assert!(annotate_envelope("not json").is_none());
        assert!(annotate_envelope("[\"gate_outcomes\"]").is_none());
    }

    #[test]
    fn the_result_keeps_its_error_flag_and_gains_the_gate_sentence() {
        let body = serde_json::json!({
            "gate_outcomes": { "security": { "status": "fail", "enforced": true } },
        })
        .to_string();

        let result = annotate_gate_verdicts(CallToolResult::error(vec![ContentBlock::text(body)]));

        assert_eq!(result.is_error, Some(true));
        let ContentBlock::Text(text) = &result.content[0] else {
            panic!("expected a text block");
        };
        let value: Value = serde_json::from_str(&text.text).expect("annotated body parses");
        assert!(
            value["warnings"][0]
                .as_str()
                .is_some_and(|entry| entry.starts_with("Gate security failed")),
            "{value}"
        );
    }

    /// The typed route holds the envelope already parsed, and must reach the
    /// same sentences the subprocess route does.
    #[test]
    fn the_typed_route_reads_the_same_members_without_a_text_round_trip() {
        let envelope = serde_json::json!({
            "kind": "health",
            "summary": { "baseline_staleness": staleness("partial", 4, 3, true) },
        });

        let annotated = annotate_value(&envelope).expect("envelope is annotated");
        let value: Value = serde_json::from_str(&annotated).expect("annotated body parses");

        assert_eq!(
            value["warnings"].as_array().map(Vec::len),
            Some(1),
            "{value}"
        );
        assert!(
            value["summary"]["baseline_staleness"]["gate_trips"] == Value::Bool(true),
            "the envelope's own members must not move: {value}"
        );
    }

    /// Every CLI-backed tool spawns the CLI with `--quiet`, which removes the
    /// stderr line that used to be the only trace of a stand-down. Without
    /// this sentence an agent is handed a whole-project report in answer to a
    /// question about a change.
    #[test]
    fn a_request_the_run_could_not_apply_says_so_and_names_its_reason() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "request_outcomes": {
                "changed-since": {
                    "status": "not-applied",
                    "affects": "scope",
                    "requested": "origin/main",
                    "reason": "git-failed",
                    "message": "..."
                },
                "diff-filter": {
                    "status": "applied",
                    "affects": "scope",
                    "requested": "--diff-stdin"
                }
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("Requests not applied: changed-since (git-failed)."),
            "{warnings:?}"
        );
        assert!(warnings[0].contains("WIDER than what was"), "{warnings:?}");
        assert!(
            !warnings[0].contains("diff-filter"),
            "an honoured request needs no sentence: {warnings:?}"
        );
    }

    /// A run that applied everything it was asked adds nothing, so the object
    /// appearing on every scoped CI run cannot make every response noisy. A
    /// measured non-empty scope and an unmeasured one are both silent.
    #[test]
    fn a_run_that_applied_everything_it_was_asked_adds_no_warning() {
        let envelope = serde_json::json!({
            "kind": "dead-code",
            "request_outcomes": {
                "changed-since": {
                    "status": "applied",
                    "affects": "scope",
                    "requested": "origin/main"
                },
                "diff-filter": {
                    "status": "applied",
                    "affects": "scope",
                    "requested": "--diff-stdin",
                    "scope_size": 12
                }
            },
        });
        assert!(
            annotate_envelope(&envelope.to_string()).is_none(),
            "nothing to add must pass the original bytes through"
        );
    }

    /// The Action and the GitLab template warn on this shape, and MCP runs the
    /// CLI with `--quiet`, so without this sentence the agent is handed a clean
    /// report for a run that analyzed nothing.
    #[test]
    fn a_narrowing_request_applied_over_an_empty_scope_says_the_report_covered_nothing() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "request_outcomes": {
                "diff-filter": {
                    "status": "applied",
                    "affects": "scope",
                    "requested": "--diff-stdin",
                    "scope_size": 0
                },
                "sarif-file": {
                    "status": "applied",
                    "affects": "artifact",
                    "requested": "out.sarif",
                    "scope_size": 0
                }
            },
        }));

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("Requests applied over an empty scope: diff-filter."),
            "{warnings:?}"
        );
        assert!(
            !warnings[0].contains("sarif-file"),
            "a file written beside the report narrows nothing: {warnings:?}"
        );
        assert!(
            !warnings[0].contains("WIDER"),
            "the report is not wider than asked, it is empty: {warnings:?}"
        );
    }

    /// The status set is open: a value this build does not know must not be
    /// read as "the run did what it was asked".
    #[test]
    fn an_unrecognised_request_status_is_reported_rather_than_assumed_applied() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "request_outcomes": {
                "changed-since": {
                    "status": "partial",
                    "affects": "scope",
                    "requested": "origin/main"
                }
            },
        }));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("Requests not applied: changed-since."),
            "{warnings:?}"
        );
    }

    /// A request that writes a file beside the report narrows nothing, so the
    /// agent must not be told the report covers more than it asked for.
    #[test]
    fn an_output_file_that_was_not_written_does_not_claim_a_wider_report() {
        let warnings = warnings_of(&serde_json::json!({
            "kind": "dead-code",
            "request_outcomes": {
                "sarif-file": {
                    "status": "not-applied",
                    "affects": "artifact",
                    "requested": "out.sarif",
                    "reason": "write-failed",
                    "message": "..."
                }
            },
        }));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].starts_with("Output not written: sarif-file (write-failed)."),
            "{warnings:?}"
        );
        assert!(!warnings[0].contains("WIDER"), "{warnings:?}");
    }

    /// The typed route reads the same member, so a tool cannot answer
    /// differently depending on whether a parameter forced the CLI fallback.
    #[test]
    fn the_typed_route_reports_an_unapplied_request_too() {
        let envelope = serde_json::json!({
            "kind": "dupes",
            "request_outcomes": {
                "diff-filter": {
                    "status": "not-applied",
                    "affects": "scope",
                    "requested": "--diff-stdin",
                    "reason": "not-utf8"
                }
            },
        });
        let annotated = annotate_value(&envelope).expect("envelope is annotated");
        let value: Value = serde_json::from_str(&annotated).expect("annotated body parses");
        assert!(
            value["warnings"][0]
                .as_str()
                .is_some_and(|entry| entry.contains("diff-filter (not-utf8)")),
            "{value}"
        );
    }
}
