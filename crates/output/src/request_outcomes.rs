//! What happened to every narrowing or shaping request a run received.
//!
//! One shape for every command that can be asked to scope or shape its report,
//! so a consumer reads the same members whether the envelope came from
//! `dead-code`, `dupes`, `health` or `security`. Carried as `request_outcomes`
//! at the envelope root, absent whenever the run was asked for nothing.
//!
//! This is the mirror image of [`crate::GateOutcomes`]. That object answers
//! "what did the run conclude"; this one answers "did the run do what it was
//! asked". Both are keyed maps at the root with an open key set, so a consumer
//! learns one reading rule for both, and a request added in a later release
//! reaches an unchanged consumer.
//!
//! # Why the honoured case is published too
//!
//! An entry appears for every request the run RECEIVED, including the ones it
//! honoured, exactly as `gate_outcomes` publishes gates that passed. A report
//! that says "scoped to the change" positively is the reviewer question this
//! object is actually about, and a reader who only ever sees failures cannot
//! tell "the filter applied" from "nothing was asked for".
//!
//! # Why this is not a `workspace_diagnostics` entry
//!
//! `WorkspaceDiagnostic` requires a `path` naming the file or directory that
//! triggered it, and an unresolvable git ref or an oversize diff has no such
//! path. More decisively, `degrades_analysis` means "less reached the analysis
//! than the user expected", and the shipped consumer sentence built from it
//! says findings were computed over less than the whole project. These facts
//! mean the opposite: MORE was reported than was asked for. Routing them
//! through that array would make an existing warning state something false.
//!
//! # Why it carries prose when `gate_outcomes` deliberately does not
//!
//! A gate verdict is rendered from `status`, `observed` and `threshold`, so it
//! needs no sentence. Here every reason carries a different remedy (fetch the
//! ref, regenerate the diff from the repository root, check out with full
//! history), the CLI already writes those sentences to stderr, and a rendered
//! pull-request comment needs one of them in its body. Reproducing nine
//! remedies in bash and in jq, twice, is how they drift.
//! `WorkspaceDiagnostic::message` is the precedent.
//!
//! # Wire compatibility
//!
//! The key set and the `status` value set are both OPEN. A name or a status
//! this build does not recognise means "some request" and "some outcome", not
//! an error, the same tolerate-unknown-values contract
//! `workspace_diagnostics[].kind` documents. The Rust types are closed enums so
//! the emitter cannot drift, and a request added later is an additive optional
//! key that bumps no `schema_version`.

use std::collections::BTreeMap;

use serde::Serialize;

/// Which request an outcome describes.
///
/// Taken from the flags and environment channels a run can be narrowed or
/// shaped by, rather than from any integration's input list, because a request
/// a consumer cannot name is exactly the one whose fate goes missing.
/// Serialized as kebab-case and published as an open set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RequestName {
    /// `--changed-since <ref>`: scope the analysis to the files that changed
    /// since a git ref. Not applied means the report covers the whole project.
    ChangedSince,
    /// `--diff-file`, `--diff-stdin` or `$FALLOW_DIFF_FILE`: keep only the
    /// findings a supplied unified diff touches. Not applied means every
    /// finding is reported.
    DiffFilter,
}

/// What became of one request on this run.
///
/// Two-valued today. The value set is OPEN so a later `partial` needs no bump,
/// and it is deliberately not added now: nothing emits it, and a permanently
/// unused value reads as a measurement nobody takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RequestStatus {
    /// The run did what it was asked. The report is scoped or shaped as
    /// requested.
    Applied,
    /// The run could not do what it was asked and continued anyway. The report
    /// that follows is valid and complete, and WIDER than what was asked for.
    NotApplied,
}

/// One request's fate on one run.
///
/// `reason` and `message` are present exactly when `status` is not `applied`,
/// and absent otherwise, so a consumer that only wants to know whether a
/// report is scoped reads `status` alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RequestOutcome {
    /// What became of the request.
    pub status: RequestStatus,
    /// What was asked, as the user spelled it: the git ref for
    /// `changed-since`, the diff source label (`--diff-file pr.diff`,
    /// `--diff-stdin`, `$FALLOW_DIFF_FILE build/pr.diff`) for `diff-filter`.
    pub requested: String,
    /// Why the request was not applied, as a kebab-case token. Present exactly
    /// when `status` is not `applied`. The set is open per request name; the
    /// names this build can emit are listed on [`RequestOutcomes`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// One sentence naming what was asked, what happened instead, and the next
    /// step. Byte-identical to the stderr line for the same case, so a
    /// consumer that renders this never contradicts a log a human read.
    /// Present exactly when `status` is not `applied`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl RequestOutcome {
    /// A request the run honoured.
    #[must_use]
    pub fn applied(requested: impl Into<String>) -> Self {
        Self {
            status: RequestStatus::Applied,
            requested: requested.into(),
            reason: None,
            message: None,
        }
    }

    /// A request the run could not honour, with the reason token and the
    /// sentence the CLI also wrote to stderr.
    #[must_use]
    pub fn not_applied(
        requested: impl Into<String>,
        reason: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status: RequestStatus::NotApplied,
            requested: requested.into(),
            reason: Some(reason.into()),
            message: Some(message.into()),
        }
    }
}

/// Every narrowing or shaping request a run RECEIVED, keyed by name.
///
/// Received, not failed: a request the run honoured is published with
/// `status: "applied"`, so a consumer can say "scoped to the change"
/// positively. Read an absent object as "nothing was asked for", never as
/// "nothing failed".
///
/// Absent from an envelope whenever it is empty, so a run that was asked for
/// nothing is byte-identical to one produced before this object existed. An
/// empty object is never emitted: it would assert that something was asked and
/// all of it applied, which is a different and false claim.
///
/// The names this build can emit are `changed-since` and `diff-filter`. The
/// reasons are `git-missing`, `not-a-repository`, `git-failed` and
/// `invalid-ref` for `changed-since`, and `oversize`, `unreadable`,
/// `not-utf8`, `foreign-namespace` and `ambiguous-base` for `diff-filter`.
/// Both sets are OPEN: a name a consumer does not recognise means "some
/// request", not an error.
///
/// `invalid-ref` is reachable only through the programmatic API. The
/// `--changed-since` flag validates its value before a run starts and fails
/// with exit 2 and an error document, which is the right side to err on: a
/// malformed ref is invalid input rather than a report of the wrong scope.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct RequestOutcomes(BTreeMap<RequestName, RequestOutcome>);

impl RequestOutcomes {
    /// An empty set, which serializes to nothing once [`Self::into_option`]
    /// has been applied.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Record one request's outcome, replacing any previous entry for that
    /// name.
    pub fn insert(&mut self, name: RequestName, outcome: RequestOutcome) {
        self.0.insert(name, outcome);
    }

    /// Record one request's outcome when the run received it at all.
    pub fn insert_if(&mut self, name: RequestName, outcome: Option<RequestOutcome>) {
        if let Some(outcome) = outcome {
            self.0.insert(name, outcome);
        }
    }

    /// Collapse an empty set to `None`, which is how the field stays absent on
    /// a run that was asked for nothing.
    #[must_use]
    pub fn into_option(self) -> Option<Self> {
        if self.0.is_empty() { None } else { Some(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_collapses_to_absent() {
        assert!(RequestOutcomes::new().into_option().is_none());
    }

    #[test]
    fn populated_set_survives_collapse() {
        let mut requests = RequestOutcomes::new();
        requests.insert(
            RequestName::ChangedSince,
            RequestOutcome::applied("origin/main"),
        );
        assert!(requests.into_option().is_some());
    }

    /// An honoured request carries neither a reason nor a sentence, so a
    /// consumer reading `message` never renders prose about a run that did
    /// exactly what it was told.
    #[test]
    fn an_applied_request_carries_no_reason_and_no_message() {
        let mut requests = RequestOutcomes::new();
        requests.insert(
            RequestName::DiffFilter,
            RequestOutcome::applied("--diff-file pr.diff"),
        );
        let value = serde_json::to_value(&requests).expect("request outcomes serialize");
        assert_eq!(
            value,
            serde_json::json!({
                "diff-filter": { "status": "applied", "requested": "--diff-file pr.diff" }
            })
        );
    }

    #[test]
    fn names_and_statuses_serialize_as_kebab_case() {
        let mut requests = RequestOutcomes::new();
        requests.insert(
            RequestName::ChangedSince,
            RequestOutcome::not_applied("origin/main", "invalid-ref", "Ignored."),
        );
        let value = serde_json::to_value(&requests).expect("request outcomes serialize");
        assert_eq!(
            value,
            serde_json::json!({
                "changed-since": {
                    "status": "not-applied",
                    "requested": "origin/main",
                    "reason": "invalid-ref",
                    "message": "Ignored."
                }
            })
        );
    }

    /// Two requests with different fates travel in one object, so a consumer
    /// reads the scope of each channel rather than one verdict for the run.
    #[test]
    fn two_channels_keep_their_own_status_in_one_object() {
        let mut requests = RequestOutcomes::new();
        requests.insert(
            RequestName::ChangedSince,
            RequestOutcome::applied("origin/main"),
        );
        requests.insert(
            RequestName::DiffFilter,
            RequestOutcome::not_applied("--diff-stdin", "not-utf8", "Ignored."),
        );
        let value = serde_json::to_value(&requests).expect("request outcomes serialize");
        assert_eq!(value["changed-since"]["status"], "applied");
        assert_eq!(value["diff-filter"]["status"], "not-applied");
        assert_eq!(value["diff-filter"]["reason"], "not-utf8");
    }

    #[test]
    fn insert_if_skips_a_request_the_run_never_received() {
        let mut requests = RequestOutcomes::new();
        requests.insert_if(RequestName::ChangedSince, None);
        assert!(requests.into_option().is_none());
    }
}
