//! One reader for the envelope's `gate_outcomes`, shared by every surface that
//! states a verdict.
//!
//! `fallow report --from` re-renders a saved envelope, and the producing run's
//! exit code does not survive that boundary: the caller owns the status, and
//! every `fallow report` render exits 0 so a pipeline can analyze once and
//! render many without its render steps failing. What must survive is the
//! VERDICT, so the surfaces that already state one read it here rather than
//! each learning a gate at a time. That is the whole point of a gate index: one
//! rule instead of one per gate, and a gate added later reaches these renderers
//! without a change.
//!
//! The envelope arrives untyped, exactly as `report --from` holds it, so this
//! reads root keys and tolerates a name it does not recognise. A build that
//! meets a newer gate still reports that it tripped.
//!
//! Deliberately NOT wired into the pull-request check-run `conclusion`: that
//! stays the non-blocker it has been, and moving it is a separate decision with
//! its own blast radius.

use serde_json::Value;

/// One gate's verdict, read off a saved or live envelope.
pub struct GateLine {
    pub name: String,
    pub status: String,
    pub enforced: bool,
    pub observed: Option<f64>,
    pub threshold: Option<f64>,
}

impl GateLine {
    /// Whether this gate made, or would have made, the run exit non-zero.
    fn failed(&self) -> bool {
        self.status == "fail"
    }

    /// The trailing `(85 of 90)` clause, when the gate compared numbers.
    fn measured_clause(&self) -> String {
        match (self.observed, self.threshold) {
            (Some(observed), Some(threshold)) => {
                format!(" ({} against {})", trim_num(observed), trim_num(threshold))
            }
            (Some(observed), None) => format!(" ({})", trim_num(observed)),
            _ => String::new(),
        }
    }
}

/// Render a whole number without a trailing `.0`, so a count of three reads as
/// `3` in prose while the wire keeps it a JSON number.
fn trim_num(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Read `gate_outcomes` off an envelope root, in wire order.
///
/// Returns an empty vector when the run armed no gate, which is every run
/// produced before this object existed and every run that asked for nothing.
pub fn read_gate_outcomes(envelope: &Value) -> Vec<GateLine> {
    let Some(map) = envelope.get("gate_outcomes").and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            Some(GateLine {
                name: name.clone(),
                status: entry.get("status")?.as_str()?.to_owned(),
                enforced: entry.get("enforced").and_then(Value::as_bool)?,
                observed: entry.get("observed").and_then(Value::as_f64),
                threshold: entry.get("threshold").and_then(Value::as_f64),
            })
        })
        .collect()
}

/// A one-line verdict for the job summary and the pull-request comment, or
/// `None` when the run armed no gate.
///
/// Names the gates that failed, and says which of them actually fail the build,
/// because those are different questions: a stale-baseline verdict published
/// without its opt-in flag reports `fail` and enforces nothing.
pub fn summary_line(envelope: &Value) -> Option<String> {
    let gates = read_gate_outcomes(envelope);
    if gates.is_empty() {
        return None;
    }
    let failed: Vec<&GateLine> = gates.iter().filter(|gate| gate.failed()).collect();
    if failed.is_empty() {
        let names: Vec<&str> = gates.iter().map(|gate| gate.name.as_str()).collect();
        return Some(format!("Gates passed: {}.", names.join(", ")));
    }
    let described: Vec<String> = failed
        .iter()
        .map(|gate| format!("{}{}", gate.name, gate.measured_clause()))
        .collect();
    let enforced_count = failed.iter().filter(|gate| gate.enforced).count();
    let enforcement = if enforced_count == 0 {
        " Reported only: none of them fails this run.".to_owned()
    } else if enforced_count == failed.len() {
        String::new()
    } else {
        format!(" {enforced_count} of them fails this run.")
    };
    Some(format!(
        "Gates failed: {}.{enforcement}",
        described.join(", ")
    ))
}

/// The same verdict as a GitHub workflow-command annotation, or `None` when the
/// run armed no gate or every gate passed.
///
/// `::error::` only for a gate that fails the run, `::warning::` for a verdict
/// published without enforcement, so a repository that asked for nothing never
/// gets an unsilenceable red line.
pub fn annotation_line(envelope: &Value) -> Option<String> {
    let gates = read_gate_outcomes(envelope);
    let failed: Vec<&GateLine> = gates.iter().filter(|gate| gate.failed()).collect();
    if failed.is_empty() {
        return None;
    }
    let described: Vec<String> = failed
        .iter()
        .map(|gate| format!("{}{}", gate.name, gate.measured_clause()))
        .collect();
    let level = if failed.iter().any(|gate| gate.enforced) {
        "error"
    } else {
        "warning"
    };
    Some(format!(
        "::{level}::Fallow gates failed: {}",
        described.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(gates: &Value) -> Value {
        serde_json::json!({ "kind": "health", "gate_outcomes": gates })
    }

    #[test]
    fn an_envelope_without_the_object_renders_nothing() {
        let bare = serde_json::json!({ "kind": "dead-code" });
        assert!(summary_line(&bare).is_none());
        assert!(annotation_line(&bare).is_none());
    }

    #[test]
    fn a_failing_enforced_gate_is_an_error_annotation() {
        let value = envelope(&serde_json::json!({
            "health-min-score": {
                "status": "fail", "enforced": true, "observed": 85.0, "threshold": 90.0
            }
        }));
        assert_eq!(
            annotation_line(&value).expect("a gate failed"),
            "::error::Fallow gates failed: health-min-score (85 against 90)"
        );
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates failed: health-min-score (85 against 90)."
        );
    }

    #[test]
    fn an_unenforced_verdict_warns_instead_of_erroring() {
        let value = envelope(&serde_json::json!({
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        assert_eq!(
            annotation_line(&value).expect("a gate failed"),
            "::warning::Fallow gates failed: stale-baseline"
        );
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates failed: stale-baseline. Reported only: none of them fails this run."
        );
    }

    #[test]
    fn passing_gates_are_summarized_and_never_annotated() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "pass", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates passed: regression."
        );
        assert!(annotation_line(&value).is_none());
    }

    #[test]
    fn a_skipped_gate_is_not_a_failure() {
        let value = envelope(&serde_json::json!({
            "stale-baseline": { "status": "skipped", "enforced": false }
        }));
        assert!(annotation_line(&value).is_none());
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates passed: stale-baseline."
        );
    }

    #[test]
    fn an_unrecognised_gate_name_still_reports() {
        let value = envelope(&serde_json::json!({
            "some-future-gate": { "status": "fail", "enforced": true }
        }));
        assert_eq!(
            annotation_line(&value).expect("a gate failed"),
            "::error::Fallow gates failed: some-future-gate"
        );
    }

    #[test]
    fn a_mixed_run_says_how_many_actually_fail() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "fail", "enforced": true },
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("gates ran"),
            "Gates failed: regression, stale-baseline. 1 of them fails this run."
        );
    }
}
