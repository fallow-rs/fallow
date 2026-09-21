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
//! Informational on purpose. An unapplied request is a reviewing problem and
//! not a build failure, and the CLI's exit code is unchanged by it. Whether a
//! CI job should care is the integration's decision.
//!
//! What an unapplied request means is read off the entry's `affects`, never off
//! its name: a narrowing request that stood down leaves the report WIDER than
//! asked for, while a secondary artifact that was not written says nothing
//! about the report's scope at all. One sentence for both classes would state
//! something false about whichever one did not happen.
//!
//! An applied entry may also carry `scope_size`, and `0` means the narrowing it
//! performed left nothing to analyze. That is stated as well: a report with no
//! findings reads as a clean result, and nothing else in the body says the scope
//! it was computed over was empty.

use serde_json::Value;

/// One request's fate, read off a saved or live envelope.
struct RequestLine {
    name: String,
    status: String,
    affects: Option<String>,
    reason: Option<String>,
    scope_size: Option<u64>,
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

    /// Whether this entry describes the report's scope rather than a file
    /// written beside it. Unapplied, that means the report widened; applied over
    /// an empty scope, that it covers nothing.
    ///
    /// An entry that does not say is not assumed to narrow: the class travels
    /// with every entry this build emits, so a missing or unrecognised value
    /// comes from a producer this build does not know, and claiming a widened
    /// report on its behalf is the defect `affects` exists to remove.
    fn affects_scope(&self) -> bool {
        self.affects.as_deref() == Some("scope")
    }

    /// Whether an unapplied entry means a requested file was not written.
    fn withholds_artifact(&self) -> bool {
        self.affects.as_deref() == Some("artifact")
    }

    /// Whether this entry narrowed the run to a scope it measured as empty.
    ///
    /// The opposite shape of a scope request that stood down, and the case a
    /// clean report cannot state for itself: every finding filters out of an
    /// empty scope, so a reader takes the emptiness for a clean result. An
    /// envelope from a producer that measures no size says nothing here, which
    /// keeps a saved envelope written before the member existed rendering as it
    /// always did.
    fn applied_over_empty_scope(&self) -> bool {
        self.applied() && self.affects_scope() && self.scope_size == Some(0)
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
                affects: entry
                    .get("affects")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                reason: entry
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                scope_size: entry.get("scope_size").and_then(Value::as_u64),
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
    // One sentence per class, and only for a class that actually has an
    // unapplied entry. A single sentence for the whole object told the reader
    // that a SARIF file which failed to write had widened the analysis, which
    // is the opposite of true and the thing a reviewer acts on.
    if unapplied.iter().any(|request| request.affects_scope()) {
        line.push_str(" Anything not applied means this report is wider than requested.");
    }
    if unapplied.iter().any(|request| request.withholds_artifact()) {
        line.push_str(
            " A requested output file was not written, so anything reading it has nothing \
             to read; the report itself is unaffected.",
        );
    }
    // The applied half of the same misreading: a narrowing request that DID
    // apply, over a scope it measured as empty. Every finding filters out, so
    // the report under this line is clean because nothing reached the analysis,
    // and the Action and the merge-request template state the same fact from the
    // envelope they captured (issue #2734).
    if applied
        .iter()
        .any(|request| request.applied_over_empty_scope())
    {
        line.push_str(
            " A narrowing request applied over an empty scope, so this report is clean \
             because nothing in it was analyzable; check the diff or ref this run was given.",
        );
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

/// [`summary_line`] for a body `fallow report --from` is about to write, where
/// the diff filter was resolved again in THIS process.
///
/// `changed-since` and `sarif-file` belong to the run that produced the
/// envelope: a re-render scopes no analysis and writes no SARIF file, so the
/// saved record is the only truth about them and reading it from the envelope
/// is right. `diff-filter` is not like that. Both shipped integrations download
/// the pull-request diff in their comment and review steps and then re-render
/// with `report --from`, and the filter that decides which findings become
/// inline comments is resolved in that second process, from that diff
/// (`filter_issues_from_env`). When it stands down, the body is written at full
/// scope while the saved entry describes a different diff, resolved by a
/// different process, and under `--quiet` (which both integrations pass) the
/// stand-down reaches no other channel at all.
///
/// So a stand-down in THIS process is overlaid on the saved object, and the
/// body states it rather than claiming a scope the render never achieved
/// (issue #2688).
///
/// Only a stand-down. A filter that applied here says nothing the body does not
/// already say ("N inline comments on the changed lines"), while the saved entry
/// may still record that the findings themselves were computed at full scope,
/// which is the more useful of the two facts and the one the reader would lose.
/// The rule is therefore "a stand-down anywhere in the chain is reported, and
/// nothing new is claimed": a re-render never turns a producing run's
/// stand-down into a scoped report, and a healthy render adds no sentence that
/// was not there before.
pub fn summary_line_for_saved_render(envelope: &Value) -> Option<String> {
    summary_line_with_live_diff_filter(
        envelope,
        crate::report::ci::diff_filter::shared_diff_request_outcome(),
    )
}

fn summary_line_with_live_diff_filter(
    envelope: &Value,
    live: Option<&fallow_output::RequestOutcome>,
) -> Option<String> {
    let stood_down = live
        .filter(|outcome| outcome.status == fallow_output::RequestStatus::NotApplied)
        .and_then(|outcome| serde_json::to_value(outcome).ok());
    let Some(live) = stood_down else {
        return summary_line(envelope);
    };
    // Collected through a sorted map so the merged object keeps the wire order
    // a live render produces, which is what the live/saved parity suite pins.
    let mut merged: std::collections::BTreeMap<String, Value> = envelope
        .get("request_outcomes")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(name, entry)| (name.clone(), entry.clone()))
                .collect()
        })
        .unwrap_or_default();
    merged.insert("diff-filter".to_owned(), live);
    let merged: serde_json::Map<String, Value> = merged.into_iter().collect();
    summary_line(&serde_json::json!({ "request_outcomes": Value::Object(merged) }))
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
                "affects": "scope",
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
            "diff-filter": {
                "status": "applied",
                "affects": "scope",
                "requested": "--diff-file pr.diff"
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: applied diff-filter."
        );
    }

    /// A measured, non-empty scope adds nothing, so a healthy scoped run reads
    /// exactly as it did before the member existed.
    #[test]
    fn a_measured_non_empty_scope_adds_no_clause() {
        let value = envelope(&serde_json::json!({
            "diff-filter": {
                "status": "applied",
                "affects": "scope",
                "requested": "--diff-file pr.diff",
                "scope_size": 12
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: applied diff-filter."
        );
    }

    /// The fact a clean report cannot state for itself: the narrowing applied,
    /// and left nothing for the analysis to see.
    #[test]
    fn an_applied_request_over_an_empty_scope_says_the_report_covered_nothing() {
        let value = envelope(&serde_json::json!({
            "diff-filter": {
                "status": "applied",
                "affects": "scope",
                "requested": "--diff-file pr.diff",
                "scope_size": 0
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: applied diff-filter. \
             A narrowing request applied over an empty scope, so this report is clean \
             because nothing in it was analyzable; check the diff or ref this run was given."
        );
    }

    /// A file written beside the report narrows nothing, so a size measured on
    /// one of those entries says nothing about what the findings cover.
    #[test]
    fn an_empty_scope_on_an_artifact_request_claims_nothing() {
        let value = envelope(&serde_json::json!({
            "sarif-file": {
                "status": "applied",
                "affects": "artifact",
                "requested": "out.sarif",
                "scope_size": 0
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: applied sarif-file."
        );
    }

    #[test]
    fn a_mixed_run_keeps_the_two_groups_apart_in_one_line() {
        let value = envelope(&serde_json::json!({
            "changed-since": {
                "status": "not-applied",
                "affects": "scope",
                "requested": "x",
                "reason": "git-failed"
            },
            "diff-filter": {
                "status": "applied",
                "affects": "scope",
                "requested": "--diff-stdin"
            }
        }));
        assert_eq!(
            summary_line(&value).expect("requests were received"),
            "Request outcomes: not applied changed-since (git-failed); applied diff-filter. \
             Anything not applied means this report is wider than requested."
        );
    }

    /// A secondary artifact that was not written says so, and says nothing
    /// about the scope of the report it travels in.
    #[test]
    fn an_unwritten_artifact_does_not_claim_the_report_widened() {
        let value = envelope(&serde_json::json!({
            "sarif-file": {
                "status": "not-applied",
                "affects": "artifact",
                "requested": "out.sarif",
                "reason": "write-failed"
            }
        }));
        let line = summary_line(&value).expect("a request was received");
        assert!(
            !line.contains("wider than requested"),
            "an unwritten file did not widen the report: {line}"
        );
        assert!(
            line.contains("A requested output file was not written"),
            "{line}"
        );
    }

    /// Both classes failing in one run states both facts, in a fixed order.
    #[test]
    fn a_run_that_widened_and_withheld_states_both() {
        let value = envelope(&serde_json::json!({
            "changed-since": {
                "status": "not-applied",
                "affects": "scope",
                "requested": "origin/main",
                "reason": "git-failed"
            },
            "sarif-file": {
                "status": "not-applied",
                "affects": "artifact",
                "requested": "out.sarif",
                "reason": "write-failed"
            }
        }));
        let line = summary_line(&value).expect("requests were received");
        let widened = line.find("wider than requested").expect("the scope clause");
        let withheld = line
            .find("A requested output file was not written")
            .expect("the artifact clause");
        assert!(widened < withheld, "{line}");
    }

    #[test]
    fn an_unrecognised_request_name_still_reports() {
        let value = envelope(&serde_json::json!({
            "some-future-request": {
                "status": "not-applied",
                "affects": "scope",
                "requested": "x"
            }
        }));
        assert_eq!(
            annotation_line(&value).expect("a request was received"),
            "::notice::Fallow: Request outcomes: not applied some-future-request. \
             Anything not applied means this report is wider than requested."
        );
    }

    /// A class this build does not know is not read as a narrowing request:
    /// claiming a widened report on its behalf is the failure `affects` exists
    /// to remove, and the entry is still reported by name.
    #[test]
    fn an_unrecognised_class_claims_neither_sentence() {
        let value = envelope(&serde_json::json!({
            "some-future-request": {
                "status": "not-applied",
                "affects": "some-future-class",
                "requested": "x"
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: not applied some-future-request."
        );
    }

    /// A re-render that resolved its own diff states that diff's fate, not the
    /// producing run's: the body it is about to write was filtered by this
    /// process, and under `--quiet` nothing else says so.
    #[test]
    fn a_re_render_states_the_diff_filter_it_resolved_itself() {
        let saved = envelope(&serde_json::json!({
            "diff-filter": {
                "status": "applied",
                "affects": "scope",
                "requested": "--diff-stdin"
            }
        }));
        let live = fallow_output::RequestOutcome::not_applied(
            fallow_output::RequestName::DiffFilter,
            "$FALLOW_DIFF_FILE pr.diff",
            "oversize",
            "...",
        );
        assert_eq!(
            summary_line_with_live_diff_filter(&saved, Some(&live))
                .expect("a request was received"),
            "Request outcomes: not applied diff-filter (oversize). \
             Anything not applied means this report is wider than requested."
        );
    }

    /// The other channels belong to the producing run, and a re-render that
    /// stood its own filter down must not drop them or disturb the wire order.
    #[test]
    fn the_overlay_keeps_the_producing_run_channels_and_the_wire_order() {
        let saved = envelope(&serde_json::json!({
            "changed-since": {
                "status": "applied",
                "affects": "scope",
                "requested": "origin/main"
            },
            "sarif-file": {
                "status": "not-applied",
                "affects": "artifact",
                "requested": "out.sarif",
                "reason": "write-failed"
            }
        }));
        let live = fallow_output::RequestOutcome::not_applied(
            fallow_output::RequestName::DiffFilter,
            "$FALLOW_DIFF_FILE pr.diff",
            "foreign-namespace",
            "...",
        );
        let line = summary_line_with_live_diff_filter(&saved, Some(&live))
            .expect("requests were received");
        assert_eq!(
            line,
            "Request outcomes: not applied diff-filter (foreign-namespace), \
             sarif-file (write-failed); applied changed-since. \
             Anything not applied means this report is wider than requested. \
             A requested output file was not written, so anything reading it has nothing \
             to read; the report itself is unaffected."
        );
    }

    /// A filter that applied in the re-render claims nothing: the body already
    /// states its own scope, and the saved record still carries the scope the
    /// FINDINGS were computed at, which is the fact a reader would lose.
    #[test]
    fn a_filter_that_applied_here_does_not_overwrite_the_saved_stand_down() {
        let saved = envelope(&serde_json::json!({
            "diff-filter": {
                "status": "not-applied",
                "affects": "scope",
                "requested": "--diff-stdin",
                "reason": "not-utf8"
            }
        }));
        let live = fallow_output::RequestOutcome::applied(
            fallow_output::RequestName::DiffFilter,
            "$FALLOW_DIFF_FILE pr.diff",
        );
        assert_eq!(
            summary_line_with_live_diff_filter(&saved, Some(&live)),
            summary_line(&saved)
        );
    }

    /// And it adds no clause to a body that had none, so a healthy render is
    /// byte-identical to what it produced before.
    #[test]
    fn a_filter_that_applied_here_adds_no_clause_of_its_own() {
        let bare = serde_json::json!({ "kind": "dead-code" });
        let live = fallow_output::RequestOutcome::applied(
            fallow_output::RequestName::DiffFilter,
            "$FALLOW_DIFF_FILE pr.diff",
        );
        assert!(summary_line_with_live_diff_filter(&bare, Some(&live)).is_none());
    }

    /// A process that resolved no diff overlays nothing, so a re-render of a
    /// saved envelope stays byte-identical to the live render of the same run.
    #[test]
    fn no_live_diff_leaves_the_saved_object_alone() {
        let saved = envelope(&serde_json::json!({
            "diff-filter": {
                "status": "not-applied",
                "affects": "scope",
                "requested": "--diff-stdin",
                "reason": "not-utf8"
            }
        }));
        assert_eq!(
            summary_line_with_live_diff_filter(&saved, None),
            summary_line(&saved)
        );
    }

    /// The status set is open, so a value this build does not know must not be
    /// read as "the run did what it was asked".
    #[test]
    fn an_unrecognised_status_is_not_applied() {
        let value = envelope(&serde_json::json!({
            "changed-since": {
                "status": "partial",
                "affects": "scope",
                "requested": "origin/main"
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a request was received"),
            "Request outcomes: not applied changed-since. \
             Anything not applied means this report is wider than requested."
        );
    }
}
