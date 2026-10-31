//! The verdict of one gate on one run.
//!
//! A gate is a rule that can fail a run. Each fallow tool keys these values by
//! its own gate names.

use serde::Serialize;

/// What a gate concluded on this run.
///
/// Four-valued rather than a boolean because audit's verdict has a warn tier
/// (`crates/cli/src/cli_report.rs` maps it onto three conclusions) and because
/// a gate can stand down without passing. Widening a published boolean later
/// would retype a required field and bump every carrying envelope, so the width
/// is decided here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum GateStatus {
    /// The gate ran and its condition did not hold.
    Pass,
    /// The gate ran and reached a warn tier that does not fail the run. Only
    /// `audit-verdict` can report this today.
    Warn,
    /// The gate ran and its condition held.
    Fail,
    /// The gate could not judge this run and deliberately stood down: a
    /// change-scoped baseline comparison, `health --report-only`, or a security
    /// advisory shadowed by a configured gate. Distinct from `pass`, which
    /// asserts the condition was evaluated and did not hold.
    Skipped,
}

/// One gate's verdict on one run.
///
/// `status` and `enforced` answer different questions and legitimately
/// disagree. `status` is what the rule concluded; `enforced` is whether a
/// `fail` from this gate would make the run exit non-zero. A
/// `health --report-only` run is an explicit request never to fail, so a
/// failing gate there reports `status: fail` with `enforced: false`, and a
/// stale-baseline verdict published without `--fail-on-stale-baseline` reports
/// the same pair.
///
/// **A gate fails the build when `status` is `fail` AND `enforced` is true.**
/// Neither member decides it alone: `enforced` is true on every armed gate,
/// including the ones that passed, so gating on it by itself fails every run
/// that armed anything. Read `status` on its own to decide what to say, and
/// remember that `warn` and `skipped` are neither a pass nor a failure.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GateOutcome {
    /// What the rule concluded.
    pub status: GateStatus,
    /// True when a `fail` from this gate makes the run exit non-zero. False
    /// when the verdict is published for information only: the gate was never
    /// armed, the run was told never to fail, or the combined machine formats
    /// exit 0 for the gate (bare `fallow` without `--fail-on-issues`).
    pub enforced: bool,
    /// The measured value the gate compared, when there is one: the duplication
    /// percentage, the health score, the number of findings at or above the
    /// severity floor, the number of `error` findings of `health-findings` and
    /// of `error-severity-findings`, or
    /// the number of files in `files`. Whole numbers are
    /// carried as JSON numbers, so a count of three reads as `3.0`. Absent for
    /// gates that compare no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<f64>,
    /// The configured limit `observed` was compared against, when there is one.
    /// Absent for gates that compare no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// How the limit was spelled, for a gate whose `threshold` number does not
    /// carry its own unit. `health-min-severity` sets it to the severity floor
    /// (`moderate`, `high` or `critical`); `health-findings` and
    /// `error-severity-findings` set it to `error`, the rule severity they
    /// count; `regression` sets it to the
    /// tolerance as the user wrote it (`"50%"` or `"5"`), because `threshold`
    /// there is the allowance in issues and the percentage would otherwise be
    /// unrecoverable on the grouped envelope, which carries no `regression`
    /// object. Absent for gates whose numbers speak for themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_label: Option<String>,
    /// The files the gate judged, for a gate that judges files rather than a
    /// number. Only `parse-error` sets it: one item per file that did not
    /// parse cleanly, sorted by path. Absent when the list is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GateFile>,
}

/// One file a file-judging gate names, with the reason the gate counted it.
///
/// Today only `parse-error` emits it. The item carries the same facts as the
/// `source-parse-degraded` entry in `workspace_diagnostics[]` for that file,
/// so a consumer can act on the gate without a join. With `reason` set, the
/// count and the parser outcome are placeholders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GateFile {
    /// The file path, relative to the project root, with `/` separators.
    pub path: String,
    /// The number of parser errors for the file. `1` when `reason` is set.
    pub error_count: u32,
    /// True when the parser stopped in the file instead of recovering, so the
    /// analysis saw only the part before the error. `true` when `reason` is set.
    pub panicked: bool,
    /// Why the file counts when it is not a parser error. Absent for a parser
    /// error, the only reason before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<GateFileReason>,
}

/// Why a file counts for the `parse-error` gate, other than a parser error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum GateFileReason {
    /// The file nests too deep to parse, so it was not parsed
    /// (`source-nesting-too-deep`).
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
