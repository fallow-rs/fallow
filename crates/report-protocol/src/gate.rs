//! The verdict of one gate on one run.
//!
//! A gate is a rule that can fail a run. Each fallow tool keys these values by
//! its own gate names.

use std::borrow::Borrow;
use std::collections::BTreeMap;

use serde::Serialize;

/// What a gate concluded on this run.
///
/// The status has four values, not two. A verdict can have a warn tier that
/// does not fail the run, and a gate can stand down without a pass. A change
/// from a boolean to a wider type later would change the type of a required
/// member and the version of each envelope that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum GateStatus {
    /// The gate ran and its condition did not hold.
    Pass,
    /// The gate ran and reached a warn tier that does not fail the run. Only a
    /// gate with a warn tier reports this value.
    Warn,
    /// The gate ran and its condition held.
    Fail,
    /// The gate could not judge this run and stood down on purpose. For
    /// example, the run was told never to fail, or a different gate decides
    /// the run. This is not `pass`: `pass` says that the gate evaluated the
    /// condition and the condition did not hold.
    Skipped,
}

/// One gate's verdict on one run.
///
/// `status` and `enforced` answer different questions, and they can disagree.
/// `status` is what the rule concluded. `enforced` tells if a `fail` from this
/// gate makes the run exit non-zero. A run that was told never to fail reports
/// a failed gate as `status: fail` with `enforced: false`.
///
/// **A gate fails the build when `status` is `fail` AND `enforced` is true.**
/// One member alone does not decide it. `enforced` is true on every armed
/// gate, also on a gate that passed. Thus a gate rule on `enforced` alone
/// fails every run that armed a gate. Read `status` alone to decide what to
/// say. `warn` and `skipped` are not a pass and not a failure.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GateOutcome {
    /// What the rule concluded.
    pub status: GateStatus,
    /// True when a `fail` from this gate makes the run exit non-zero. False
    /// when the tool publishes the verdict for information only: no flag or
    /// config armed the gate, the run was told never to fail, or the output
    /// format exits 0 for the gate.
    pub enforced: bool,
    /// The measured value that the gate compared, when there is one: for
    /// example a percentage, a score, a number of findings, or the number of
    /// files in `files`. A whole number is a JSON number, so a count of three
    /// reads as `3.0`. Absent for a gate that compares no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<f64>,
    /// The configured limit that the gate compared `observed` against, when
    /// there is one. Absent for a gate that compares no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// How the limit was spelled, for a gate whose `threshold` number does not
    /// carry its unit. Examples are a severity floor such as `high`, the rule
    /// severity that the gate counts such as `error`, or a tolerance as the
    /// user wrote it, such as `"50%"`. Each gate name documents its label.
    /// Absent for a gate whose numbers have a clear meaning without a label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_label: Option<String>,
    /// The files that the gate judged, for a gate that judges files and not a
    /// number. One item for each file, sorted by path. Absent when the list is
    /// empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GateFile>,
}

/// One file that a file-judging gate names, with the reason that the gate
/// counted it.
///
/// A gate that counts files that did not parse cleanly writes this item. When
/// `reason` is set, `error_count` and `panicked` hold fixed values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GateFile {
    /// The file path, relative to the project root, with `/` separators.
    pub path: String,
    /// The number of parser errors for the file. `1` when `reason` is set.
    pub error_count: u32,
    /// True when the parser stopped in the file and did not recover, so the
    /// analysis saw only the part before the error. `true` when `reason` is set.
    pub panicked: bool,
    /// Why the file counts when it is not a parser error. Absent for a parser
    /// error, the only reason before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<GateFileReason>,
}

/// Why a file counts for a parse gate, other than a parser error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum GateFileReason {
    /// The file nests too deep to parse, so the tool did not parse it.
    NestingTooDeep,
}

impl GateOutcome {
    /// A gate that ran, compared no number, and was armed.
    #[must_use]
    pub const fn new(status: GateStatus, enforced: bool) -> Self {
        Self {
            status,
            enforced,
            observed: None,
            threshold: None,
            threshold_label: None,
            files: Vec::new(),
        }
    }

    /// A gate that compared `observed` against `threshold`.
    #[must_use]
    pub const fn measured(
        status: GateStatus,
        enforced: bool,
        observed: f64,
        threshold: f64,
    ) -> Self {
        Self {
            status,
            enforced,
            observed: Some(observed),
            threshold: Some(threshold),
            threshold_label: None,
            files: Vec::new(),
        }
    }

    /// A gate that counted `observed` items at or above a named floor.
    #[must_use]
    pub fn counted(
        status: GateStatus,
        enforced: bool,
        observed: f64,
        threshold_label: &str,
    ) -> Self {
        Self {
            status,
            enforced,
            observed: Some(observed),
            threshold: None,
            threshold_label: Some(threshold_label.to_owned()),
            files: Vec::new(),
        }
    }

    /// A gate that judged files: `observed` is the number of files it named.
    #[must_use]
    pub fn with_files(status: GateStatus, enforced: bool, files: Vec<GateFile>) -> Self {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a file count never approaches the f64 integer limit"
        )]
        let observed = files.len() as f64;
        Self {
            status,
            enforced,
            observed: Some(observed),
            threshold: None,
            threshold_label: None,
            files,
        }
    }

    /// Whether this outcome should make the run exit non-zero.
    #[must_use]
    pub const fn fails_run(&self) -> bool {
        self.enforced && matches!(self.status, GateStatus::Fail)
    }
}

/// The verdict of every gate that a run evaluated, keyed by gate name.
///
/// Each tool defines its own gate names. The name of a gate is the JSON member
/// name, and the members are in the order of the names. A tool never writes an
/// empty object. When no gate ran, the member is absent.
///
/// The key set is OPEN. A name that a consumer does not recognize means "some
/// gate", not an error.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "GateOutcomes"))]
#[serde(transparent)]
pub struct GateOutcomes<K: Ord>(BTreeMap<K, GateOutcome>);

impl<K: Ord> Default for GateOutcomes<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord> GateOutcomes<K> {
    /// An empty set. [`Self::into_option`] makes it absent.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Record the outcome of one gate. A previous entry for that name goes.
    pub fn insert(&mut self, name: K, outcome: GateOutcome) {
        self.0.insert(name, outcome);
    }

    /// Record the outcome of one gate when the gate ran.
    pub fn insert_if(&mut self, name: K, outcome: Option<GateOutcome>) {
        if let Some(outcome) = outcome {
            self.0.insert(name, outcome);
        }
    }

    /// Read the outcome of one gate.
    #[must_use]
    pub fn get<Q>(&self, name: &Q) -> Option<&GateOutcome>
    where
        K: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.0.get(name)
    }

    /// Whether a gate of the set fails the run: its status is `fail` and it
    /// is enforced.
    #[must_use]
    pub fn fails_run(&self) -> bool {
        self.0.values().any(GateOutcome::fails_run)
    }

    /// Whether the set holds no gate.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Make an empty set `None`. This keeps the member absent on a run that
    /// evaluated no gate.
    #[must_use]
    pub fn into_option(self) -> Option<Self> {
        if self.0.is_empty() { None } else { Some(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unenforced_failure_does_not_fail_the_run() {
        let outcome = GateOutcome::new(GateStatus::Fail, false);
        assert!(!outcome.fails_run());
        let enforced = GateOutcome::new(GateStatus::Fail, true);
        assert!(enforced.fails_run());
    }

    #[test]
    fn a_skipped_gate_never_fails_the_run() {
        assert!(!GateOutcome::new(GateStatus::Skipped, true).fails_run());
        assert!(!GateOutcome::new(GateStatus::Warn, true).fails_run());
    }
}
