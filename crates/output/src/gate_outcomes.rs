//! The machine-readable verdict of every gate a run evaluated.
//!
//! One shape for every command that can fail a build, so a consumer reads the
//! same members whether the envelope came from `dead-code`, `dupes`, `health`,
//! `audit` or `security`. Carried as `gate_outcomes` at the envelope root,
//! absent whenever the run evaluated no gate.
//!
//! Every entry is a projection of the rule that decides the exit code, computed
//! once and then read by the exit path, so nothing here restates a rule that
//! lives elsewhere. `stale-baseline` projects the run's `BaselineStaleness`,
//! `regression` its `RegressionResult`, and `security` its `SecurityGate`
//! verdict. That is the point of the object: a CI integration reads one member
//! instead of reimplementing a condition in jq, and a gate added in a later
//! release reaches an unchanged consumer.
//!
//! # Why this is not the `gates` array on the pull-request decision surface
//!
//! [`crate::pr_decision::PrDecisionSurface`] already publishes a `gates` array
//! whose members carry `label`, `observed` and `threshold` as display text for
//! the GitHub check run. The two answer different questions and will not
//! converge: that one is a rendering contract for a human-facing summary, this
//! one is a machine verdict a build gates on. Hence the different key
//! (`gate_outcomes`, not `gates`) and the absence of any prose member here.
//!
//! The display array is DERIVED from this object for the gates a run armed, so
//! a tripped gate reaches the check run as a named gate rather than as a failed
//! step. That is a one-way projection into display text: a consumer that needs
//! the verdict reads this object, never the rendered row.
//!
//! # Wire compatibility
//!
//! The key set is OPEN. A name this build does not recognise means "some gate",
//! not an error, the same tolerate-unknown-values contract
//! `workspace_diagnostics[].kind` documents. The Rust key type is a closed enum
//! so the emitter cannot drift, and a gate added later is an additive optional
//! key that bumps no `schema_version`.

use std::collections::BTreeMap;

use serde::Serialize;

/// Which gate an outcome describes.
///
/// Taken from the exit-code sites rather than from any integration's input
/// list, because a gate a consumer cannot name is exactly the one whose verdict
/// goes missing. Serialized as kebab-case and published as an open set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum GateName {
    /// The CLI's own severity rule: any finding whose effective severity is
    /// `error` fails the run. This is NOT a count rule, and `--fail-on-issues`
    /// only promotes warn-tier rules into it, so a project with a rule set to
    /// `warn` can report findings and still exit 0.
    ErrorSeverityFindings,
    /// `--fail-on-regression`: issue counts grew past `--tolerance` compared
    /// with the regression baseline.
    Regression,
    /// `--fail-on-stale-baseline`: the loaded baseline has entries that matched
    /// nothing this run.
    StaleBaseline,
    /// `--threshold`: duplication exceeded the configured percentage.
    DuplicationThreshold,
    /// `--min-score`: the health score fell below the configured minimum.
    HealthMinScore,
    /// `--min-severity`: at least one complexity finding reached the configured
    /// severity. One branch of the findings gate; see [`Self::HealthFindings`].
    HealthMinSeverity,
    /// The health findings gate with no severity floor: any complexity finding
    /// fails the run. Inert when `--min-score` is set alone, which is what
    /// "complexity findings become informational" means.
    HealthFindings,
    /// The coverage-gap gate, configured through `rules.coverage-gaps`.
    HealthCoverageGaps,
    /// A runtime-coverage finding whose verdict is `safe_to_delete`,
    /// `review_required` or `low_traffic`.
    HealthRuntimeCoverage,
    /// `security --gate`: the change introduced a new security-sink candidate.
    /// The only gate that exits 8 rather than 1.
    Security,
    /// The security advisory exit, which fails on the candidate backlog rather
    /// than on what the change introduced. A configured `--gate` returns before
    /// it, so this reports `skipped` on any run that set one.
    SecurityAdvisory,
    /// `fallow audit`'s rule-severity verdict, the only three-valued gate.
    AuditVerdict,
    /// `--type-aware-require complete`: semantic analysis was partial or
    /// unavailable.
    TypeAwareRequire,
}

impl GateName {
    /// Every gate name this build can emit, in declaration order.
    ///
    /// Exists so a surface that has to cover the set exhaustively, such as the
    /// pull-request decision surface's display labels, can be tested against
    /// the emitter rather than against a hand-kept list. A new variant belongs
    /// here as well as in [`Self::as_str`], whose match will not compile until
    /// it is named.
    pub const ALL: [Self; 13] = [
        Self::ErrorSeverityFindings,
        Self::Regression,
        Self::StaleBaseline,
        Self::DuplicationThreshold,
        Self::HealthMinScore,
        Self::HealthMinSeverity,
        Self::HealthFindings,
        Self::HealthCoverageGaps,
        Self::HealthRuntimeCoverage,
        Self::Security,
        Self::SecurityAdvisory,
        Self::AuditVerdict,
        Self::TypeAwareRequire,
    ];

    /// The kebab-case key this gate serializes as, for prose and lookups
    /// outside the JSON envelope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ErrorSeverityFindings => "error-severity-findings",
            Self::Regression => "regression",
            Self::StaleBaseline => "stale-baseline",
            Self::DuplicationThreshold => "duplication-threshold",
            Self::HealthMinScore => "health-min-score",
            Self::HealthMinSeverity => "health-min-severity",
            Self::HealthFindings => "health-findings",
            Self::HealthCoverageGaps => "health-coverage-gaps",
            Self::HealthRuntimeCoverage => "health-runtime-coverage",
            Self::Security => "security",
            Self::SecurityAdvisory => "security-advisory",
            Self::AuditVerdict => "audit-verdict",
            Self::TypeAwareRequire => "type-aware-require",
        }
    }
}

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
    /// when the verdict is published for information only, because the gate was
    /// never armed or because the run was told never to fail.
    pub enforced: bool,
    /// The measured value the gate compared, when there is one: the duplication
    /// percentage, the health score, or the number of findings at or above the
    /// severity floor. Whole numbers are carried as JSON numbers, so a count of
    /// three reads as `3.0`. Absent for gates that compare no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<f64>,
    /// The configured limit `observed` was compared against, when there is one.
    /// Absent for gates that compare no number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    /// How the limit was spelled, for a gate whose `threshold` number does not
    /// carry its own unit. `health-min-severity` sets it to the severity floor
    /// (`moderate`, `high` or `critical`); `regression` sets it to the
    /// tolerance as the user wrote it (`"50%"` or `"5"`), because `threshold`
    /// there is the allowance in issues and the percentage would otherwise be
    /// unrecoverable on the grouped envelope, which carries no `regression`
    /// object. Absent for gates whose numbers speak for themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_label: Option<String>,
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
        }
    }

    /// Whether this outcome should make the run exit non-zero.
    #[must_use]
    pub const fn fails_run(&self) -> bool {
        self.enforced && matches!(self.status, GateStatus::Fail)
    }
}

/// Every gate a run ARMED, keyed by name.
///
/// Armed, not evaluated: a gate is armed by a flag or by config, never merely
/// because the rule behind it exists. Fallow's default severity rules fail a
/// run with no flag at all, so a `dead-code` run can exit 1 carrying no object
/// whatsoever. Read an absent object as "no gate was asked for", never as
/// "nothing failed".
///
/// Absent from an envelope whenever it is empty, so a run that armed no gate is
/// byte-identical to one produced before this object existed. An empty object
/// is never emitted: it would assert that gates were armed and none tripped,
/// which is a different and false claim.
///
/// The names this build can emit are `error-severity-findings`, `regression`,
/// `stale-baseline`, `duplication-threshold`, `health-min-score`,
/// `health-min-severity`, `health-findings`, `health-coverage-gaps`,
/// `health-runtime-coverage`, `security`, `security-advisory`, `audit-verdict`
/// and `type-aware-require`. The set is OPEN: a name a consumer does not
/// recognise means "some gate", not an error.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct GateOutcomes(BTreeMap<GateName, GateOutcome>);

impl GateOutcomes {
    /// An empty set, which serializes to nothing once [`Self::into_option`]
    /// has been applied.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Record one gate's outcome, replacing any previous entry for that name.
    pub fn insert(&mut self, name: GateName, outcome: GateOutcome) {
        self.0.insert(name, outcome);
    }

    /// Record one gate's outcome when it ran at all.
    pub fn insert_if(&mut self, name: GateName, outcome: Option<GateOutcome>) {
        if let Some(outcome) = outcome {
            self.0.insert(name, outcome);
        }
    }

    /// Read one gate's outcome.
    #[must_use]
    pub fn get(&self, name: GateName) -> Option<&GateOutcome> {
        self.0.get(&name)
    }

    /// Whether the set holds no gate.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Collapse an empty set to `None`, which is how the field stays absent on
    /// a run that evaluated no gate.
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
        assert!(GateOutcomes::new().into_option().is_none());
    }

    #[test]
    fn populated_set_survives_collapse() {
        let mut gates = GateOutcomes::new();
        gates.insert(
            GateName::Regression,
            GateOutcome::new(GateStatus::Fail, true),
        );
        assert!(gates.into_option().is_some());
    }

    /// `ALL` is the list a consumer covering the set exhaustively is tested
    /// against, so a variant missing from it would let a new gate reach the
    /// wire with no surface knowing about it.
    #[test]
    fn every_name_in_all_is_distinct_and_spelled_as_serde_spells_it() {
        let mut names = GateName::ALL.map(GateName::as_str).to_vec();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "a duplicated entry hides one variant");

        for name in GateName::ALL {
            assert_eq!(
                serde_json::to_value(name).expect("name serializes"),
                serde_json::json!(name.as_str())
            );
        }
    }

    #[test]
    fn names_serialize_as_kebab_case() {
        let mut gates = GateOutcomes::new();
        gates.insert(
            GateName::ErrorSeverityFindings,
            GateOutcome::new(GateStatus::Pass, true),
        );
        gates.insert(
            GateName::HealthMinScore,
            GateOutcome::measured(GateStatus::Fail, true, 85.0, 90.0),
        );
        let value = serde_json::to_value(&gates).expect("gate outcomes serialize");
        assert_eq!(
            value,
            serde_json::json!({
                "error-severity-findings": { "status": "pass", "enforced": true },
                "health-min-score": {
                    "status": "fail",
                    "enforced": true,
                    "observed": 85.0,
                    "threshold": 90.0
                }
            })
        );
    }

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
