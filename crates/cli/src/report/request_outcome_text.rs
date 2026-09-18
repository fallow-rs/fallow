//! One reader for the envelope's `request_outcomes`, shared by every surface
//! that states what the run was asked to do.
//!
//! The sibling of [`crate::report::gate_outcome_text`], and for the same
//! reason: `fallow report --from` re-renders a saved envelope, and the
//! producing run's stderr does not survive that boundary. What must survive is
//! whether the report is scoped as requested, so the surfaces that state a
//! verdict read it here rather than each learning a request at a time. A
//! request added in a later release reaches these renderers without a change.
//!
//! The envelope arrives untyped, exactly as `report --from` holds it, so this
//! reads root keys and tolerates a name or a status it does not recognise. An
//! unknown status is never counted as applied: the value set is open, and
//! silently reading a future `partial` as "did what you asked" is the failure
//! mode this whole object exists to remove.
//!
//! Informational on purpose. An unapplied request means the report is WIDER
//! than what was asked for, which is a reviewing problem and not a build
//! failure, and the CLI's exit code is unchanged by it. Whether a CI job should
//! care is the integration's decision.

use serde_json::Value;

/// One request's fate, read off a saved or live envelope.
struct RequestLine {
    name: String,
    status: String,
    reason: Option<String>,
}

impl RequestLine {
    /// `changed-since (invalid-ref)`, or the bare name when no reason was
    /// published (which is every applied request).
    fn described(&self) -> String {
        match self.reason.as_deref() {
            Some(reason) => format!("{} ({reason})", self.name),
            None => self.name.clone(),
        }
    }

    fn applied(&self) -> bool {
        self.status == "applied"
    }
}

/// Read `request_outcomes` off an envelope root, in wire order.
///
/// Returns an empty vector when the run was asked for nothing, which is every
/// run produced before this object existed and every run that narrowed
/// nothing.
fn read_request_outcomes(envelope: &Value) -> Vec<RequestLine> {
    let Some(map) = envelope.get("request_outcomes").and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            Some(RequestLine {
                name: name.clone(),
                status: entry.get("status")?.as_str()?.to_owned(),
                reason: entry
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// A one-line inventory for the job summary, the pull-request comment and the
/// merge-request note, or `None` when the run was asked for nothing.
///
/// Reads as an inventory ("Request outcomes: not applied X; applied Y.") for
/// the same reason the gate line does: the comment already carries a heading,
/// and a line that pronounced its own verdict would sit under a contradicting
/// one. The trailing clause is added only when something was not applied,
/// because that is the case a reader is most likely to misread: the report
/// below it is complete and valid, and covers more than was asked for.
pub fn summary_line(envelope: &Value) -> Option<String> {
    let requests = read_request_outcomes(envelope);
    if requests.is_empty() {
        return None;
    }
    let (applied, unapplied): (Vec<&RequestLine>, Vec<&RequestLine>) =
        requests.iter().partition(|request| request.applied());
    let mut clauses: Vec<String> = Vec::new();
    if !unapplied.is_empty() {
        clauses.push(format!("not applied {}", join(&unapplied)));
    }
    if !applied.is_empty() {
        clauses.push(format!("applied {}", join(&applied)));
    }
    let mut line = format!("Request outcomes: {}.", clauses.join("; "));
    if !unapplied.is_empty() {
        line.push_str(" Anything not applied means this report is wider than requested.");
    }
    Some(line)
}

fn join(requests: &[&RequestLine]) -> String {
    requests
        .iter()
        .map(|request| request.described())
        .collect::<Vec<_>>()
        .join(", ")
}

/// [`summary_line`] for a live run, which holds the requests typed rather than
/// as a parsed envelope.
///
/// Routed through the same function on purpose: `fallow report --from` must
/// render byte-identically to the direct `--format` run, which is a contract
/// with its own parity suite, so the live and saved paths cannot each format
/// the line their own way.
pub fn summary_line_for_requests(
    requests: Option<&fallow_output::RequestOutcomes>,
) -> Option<String> {
    let requests = requests?;
    let envelope = serde_json::json!({ "request_outcomes": requests });
    summary_line(&envelope)
}

/// The same inventory as a GitHub workflow-command annotation, or `None` when
/// the run was asked for nothing.
///
/// Always `::notice::`, never `::warning::`. The action already owns the
/// warning for this fact, computed from the envelope it captured, and a second
/// escalation from the render would double-count it against GitHub's
/// ten-annotations-per-level budget.
pub fn annotation_line(envelope: &Value) -> Option<String> {
    let line = summary_line(envelope)?;
    Some(format!("::notice::Fallow: {line}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(requests: &Value) -> Value {
        serde_json::json!({ "kind": "health", "request_outcomes": requests })
    }

    #[test]
    fn an_envelope_without_the_object_renders_nothing() {
        let bare = serde_json::json!({ "kind": "dead-code" });
        assert!(summary_line(&bare).is_none());
        assert!(annotation_line(&bare).is_none());
    }

    #[test]
    fn an_unapplied_request_names_its_reason_and_says_the_report_widened() {
        let value = envelope(&serde_json::json!({
            "changed-since": {
                "status": "not-applied",
                "requested": "origin/main",
                "reason": "invalid-ref",
                "message": "..."
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: not applied changed-since (invalid-ref). \
             Anything not applied means this report is wider than requested."
        );
    }

    /// The positive case is the whole reason honoured requests are published:
    /// a reviewer needs "this report IS scoped to the change" to be statable.
    #[test]
    fn an_applied_request_states_the_scope_without_a_warning_clause() {
        let value = envelope(&serde_json::json!({
            "diff-filter": { "status": "applied", "requested": "--diff-file pr.diff" }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: applied diff-filter."
        );
    }

    #[test]
    fn a_mixed_run_keeps_the_two_groups_apart_in_one_line() {
        let value = envelope(&serde_json::json!({
            "changed-since": { "status": "not-applied", "requested": "x", "reason": "git-failed" },
            "diff-filter": { "status": "applied", "requested": "--diff-stdin" }
        }));
        assert_eq!(
            summary_line(&value).expect("requests were received"),
            "Request outcomes: not applied changed-since (git-failed); applied diff-filter. \
             Anything not applied means this report is wider than requested."
        );
    }

    #[test]
    fn an_unrecognised_request_name_still_reports() {
        let value = envelope(&serde_json::json!({
            "some-future-request": { "status": "not-applied", "requested": "x" }
        }));
        assert_eq!(
            annotation_line(&value).expect("a request was received"),
            "::notice::Fallow: Request outcomes: not applied some-future-request. \
             Anything not applied means this report is wider than requested."
        );
    }

    /// The status set is open, so a value this build does not know must not be
    /// read as "the run did what it was asked".
    #[test]
    fn an_unrecognised_status_is_not_applied() {
        let value = envelope(&serde_json::json!({
            "changed-since": { "status": "partial", "requested": "origin/main" }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: not applied changed-since. \
             Anything not applied means this report is wider than requested."
        );
    }
}
