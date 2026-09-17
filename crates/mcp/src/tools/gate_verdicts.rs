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
//! # What is deliberately not done
//!
//! Nothing here moves an existing member, so a consumer already reading
//! `gate_outcomes`, `baseline_staleness` or `workspace_diagnostics` sees the
//! same values in the same places. A response with nothing to report is not
//! re-serialized at all, so an ungated run is byte for byte what it was. And
//! no result changes its `isError`: an exit-1 gate stays a success carrying
//! findings, and an exit 8 security gate stays an error, now carrying a
//! sentence that says which gate produced it.

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
    let Some(ContentBlock::Text(text)) = result.content.first() else {
        return result;
    };
    let Some(annotated) = annotate_envelope(&text.text) else {
        return result;
    };
    result.content = vec![ContentBlock::text(annotated)];
    result
}

/// [`annotate_gate_verdicts`] for callers that hold the body as a string:
/// Code Mode normalizes its subprocess output before any result exists, and
/// must read the same warnings a direct tool call does.
///
/// `None` means "nothing to add", which the caller reads as "pass the original
/// bytes through" rather than as a failure.
pub(super) fn annotate_envelope(text: &str) -> Option<String> {
    let mut value = serde_json::from_str::<Value>(text).ok()?;
    let root = value.as_object_mut()?;
    let warnings = verdict_warnings(root);
    if warnings.is_empty() {
        return None;
    }

    let existing = root
        .entry("warnings".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let array = existing.as_array_mut()?;
    array.extend(warnings.into_iter().map(Value::String));

    serde_json::to_string(&value).ok()
}

/// Every verdict this envelope states, in a fixed order so two identical runs
/// produce identical responses.
fn verdict_warnings(root: &Map<String, Value>) -> Vec<String> {
    let mut warnings: Vec<String> = BASELINE_SITES
        .iter()
        .filter_map(|(path, analysis)| baseline_warning(lookup(root, path)?, *analysis, noun(root)))
        .collect();
    warnings.extend(gate_warnings(root));
    warnings.extend(degraded_analysis_warning(root));
    warnings
}

/// Where a loaded baseline's staleness sits on each envelope shape, and which
/// analysis it belongs to on the shapes that carry more than one.
///
/// `dead-code` and `dupes` publish it at the root, `health` inside `summary`,
/// and the combined envelope once per section. Naming the sites is what keeps
/// the combined case from reporting one baseline's rot against another's
/// counts; a shape that carries none of them simply contributes nothing.
const BASELINE_SITES: &[(&[&str], Option<&str>)] = &[
    (&["baseline_staleness"], None),
    (&["summary", "baseline_staleness"], None),
    (&["check", "baseline_staleness"], Some("dead-code")),
    (&["dupes", "baseline_staleness"], Some("duplication")),
    (&["health", "summary", "baseline_staleness"], Some("health")),
];

fn lookup<'a>(root: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = root.get(*first)?;
    for key in rest {
        current = current.get(key)?;
    }
    Some(current)
}

/// What this command calls the things a baseline entry describes, matching the
/// noun its own CLI advisory uses. Read from the envelope's `kind`, so a
/// combined run keeps the generic noun rather than borrowing one section's.
fn noun(root: &Map<String, Value>) -> &'static str {
    match root.get("kind").and_then(Value::as_str) {
        Some("dead-code") => "issue",
        Some("dupes") => "clone group",
        _ => "finding",
    }
}

/// One sentence for a loaded baseline that matched less than it was saved
/// with, mirroring the CLI's two advisory messages and its gate message.
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
    if advisory == "none" && !gate_trips {
        return None;
    }

    let entries = count(staleness, "baseline_entries");
    let stale = count(staleness, "stale_entries");
    let subject = analysis.map_or_else(
        || "the loaded baseline".to_string(),
        |analysis| format!("the {analysis} baseline"),
    );

    let mut message = match advisory {
        "zero-overlap" => format!(
            "Baseline staleness: {subject} has {entries} entries but matched 0 current {noun}s. \
             Paths may have changed, or the baseline was saved on a different machine."
        ),
        "partial" => format!(
            "Baseline staleness: {stale} of {entries} entries in {subject} matched no current \
             {noun}, so it protects less than what was saved."
        ),
        _ => format!(
            "Baseline staleness: {stale} of {entries} entries in {subject} matched no current \
             {noun}."
        ),
    };
    if gate_trips {
        message.push_str(" --fail-on-stale-baseline fails a run in this state.");
    }
    message.push_str(" Re-save it with the save_baseline parameter (CLI --save-baseline).");
    Some(message)
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// One sentence per gate that reported `fail` or `warn`.
///
/// `pass` and `skipped` are left out on purpose: an agent acting on a result
/// needs what stood in its way, and a gate that concluded nothing is
/// still readable in `gate_outcomes` for a caller that wants the full
/// inventory.
fn gate_warnings(root: &Map<String, Value>) -> Vec<String> {
    let Some(gates) = root.get("gate_outcomes").and_then(Value::as_object) else {
        return Vec::new();
    };
    gates
        .iter()
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
    let enforcement = match (status, enforced) {
        ("fail", true) => "enforced, so the analysis exited non-zero for it",
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

/// Render a gate number the way the gate meant it. Counts arrive as JSON
/// numbers with a zero fraction, and `observed 3` reads as a count where
/// `observed 3.0` reads as a measurement.
fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    format!("{value}")
}

/// One sentence for every degrading diagnostic together, never one per kind.
///
/// A degraded run can record more than a dozen kinds, and an agent that has to
/// read thirteen near-identical lines learns less than one that reads a list.
/// The classification stays in Rust: this reads `degrades_analysis` rather
/// than matching a kind allowlist that a later release would silently outgrow.
fn degraded_analysis_warning(root: &Map<String, Value>) -> Option<String> {
    let diagnostics = root
        .get("workspace_diagnostics")
        .and_then(Value::as_array)?;
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
    if counts.is_empty() {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn warnings_of(envelope: &Value) -> Vec<String> {
        let annotated = annotate_envelope(&envelope.to_string()).expect("envelope is annotated");
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
                "Gate health-min-score failed (observed 86.7, threshold 99); enforced, so the \
                 analysis exited non-zero for it."
                    .to_string()
            ]
        );
    }

    #[test]
    fn an_unenforced_failing_gate_says_the_exit_code_did_not_move() {
        let warnings = warnings_of(&serde_json::json!({
            "gate_outcomes": {
                "stale-baseline": { "status": "fail", "enforced": false },
            },
        }));

        assert_eq!(
            warnings,
            vec![
                "Gate stale-baseline failed; not enforced on this run, so it did not change the \
                 exit code."
                    .to_string()
            ]
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
        assert!(annotate_envelope("[1, 2, 3]").is_none());
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
}
