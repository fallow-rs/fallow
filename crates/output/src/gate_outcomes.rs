//! The machine-readable verdict of every gate a run evaluated.
//!
//! One shape for every command that can fail a build, so a consumer reads the
//! same members whether the envelope came from `dead-code`, `dupes`, `health`,
//! `audit` or `security`. Carried as `gate_outcomes` at the envelope root. The
//! CLI always emits it with the command's default exit rule, except on `dupes`,
//! which has no default rule.
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
//! The display array is DERIVED from this object for every entry in it, so
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

pub use fallow_report_protocol::{GateFile, GateFileReason, GateOutcome, GateStatus};

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
    /// `error` fails the run. This is NOT a count threshold, and
    /// `--fail-on-issues` only promotes warn-tier rules into it, so a project
    /// with a rule set to `warn` can report findings and still exit 0.
    /// `observed` is the number of findings at `error` severity and
    /// `threshold_label` is `error`.
    ErrorSeverityFindings,
    /// `--fail-on-regression`: issue counts grew past `--tolerance` compared
    /// with the regression baseline.
    Regression,
    /// `--fail-on-stale-baseline`: the loaded baseline has entries that matched
    /// nothing this run.
    StaleBaseline,
    /// `--fail-on-baseline-growth`: a loaded baseline has a key that the same
    /// file at the base ref does not have. `observed` is the number of new
    /// keys and `threshold` is zero.
    BaselineGrowth,
    /// `--threshold`: duplication exceeded the configured percentage.
    DuplicationThreshold,
    /// `--fail-on-issues` or `--ci` on `dupes` and on the bare run: at least
    /// one clone group remains after the baseline and suppression filters.
    /// `observed` is the number of clone groups and `threshold` is zero.
    DuplicationFindings,
    /// `--min-score`: the health score fell below the configured minimum.
    HealthMinScore,
    /// `--min-severity`: at least one complexity finding reached the configured
    /// severity. One branch of the findings gate; see [`Self::HealthFindings`].
    HealthMinSeverity,
    /// The health findings gate with no severity floor: a complexity finding
    /// whose `complexity-*` rule is `error` fails the run. `observed` is the
    /// number of these findings and `threshold_label` is `error`. Inert when
    /// `--min-score` is set alone, which is what "complexity findings become
    /// informational" means.
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
    /// `--fail-on-parse-error` or the `failOnParseError` config key: at least
    /// one source file did not parse cleanly (a `source-parse-degraded` or
    /// `source-nesting-too-deep` entry in `workspace_diagnostics[]`). The
    /// entry lists each such file in `files`. Never armed by default, because
    /// the parser also rejects valid syntax that is newer than the parser.
    ParseError,
}

impl GateName {
    /// Every gate name this build can emit, in declaration order.
    ///
    /// Exists so a surface that has to cover the set exhaustively, such as the
    /// pull-request decision surface's display labels, can be tested against
    /// the emitter rather than against a hand-kept list. A new variant belongs
    /// here as well as in [`Self::as_str`], whose match will not compile until
    /// it is named.
    pub const ALL: [Self; 16] = [
        Self::ErrorSeverityFindings,
        Self::Regression,
        Self::StaleBaseline,
        Self::BaselineGrowth,
        Self::DuplicationThreshold,
        Self::DuplicationFindings,
        Self::HealthMinScore,
        Self::HealthMinSeverity,
        Self::HealthFindings,
        Self::HealthCoverageGaps,
        Self::HealthRuntimeCoverage,
        Self::Security,
        Self::SecurityAdvisory,
        Self::AuditVerdict,
        Self::TypeAwareRequire,
        Self::ParseError,
    ];

    /// The kebab-case key this gate serializes as, for prose and lookups
    /// outside the JSON envelope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ErrorSeverityFindings => "error-severity-findings",
            Self::Regression => "regression",
            Self::StaleBaseline => "stale-baseline",
            Self::BaselineGrowth => "baseline-growth",
            Self::DuplicationThreshold => "duplication-threshold",
            Self::DuplicationFindings => "duplication-findings",
            Self::HealthMinScore => "health-min-score",
            Self::HealthMinSeverity => "health-min-severity",
            Self::HealthFindings => "health-findings",
            Self::HealthCoverageGaps => "health-coverage-gaps",
            Self::HealthRuntimeCoverage => "health-runtime-coverage",
            Self::Security => "security",
            Self::SecurityAdvisory => "security-advisory",
            Self::AuditVerdict => "audit-verdict",
            Self::TypeAwareRequire => "type-aware-require",
            Self::ParseError => "parse-error",
        }
    }
}

/// The verdict of every gate a run evaluated, keyed by name.
///
/// A gate is armed by a flag or by config. The default exit rule of a command
/// is always in the object, also when no flag armed a gate:
/// `error-severity-findings` on `dead-code`, `check` and the combined run
/// (with `health-findings` when the combined run analyzed health),
/// `health-findings` on `health`, `security-advisory` on `security` and
/// `audit-verdict` on `audit`. A reader of the JSON sees a failing run without
/// the exit code. `dupes` has no default exit rule, so a `dupes` run that armed
/// no gate carries no object and always exits 0.
///
/// An empty object is never emitted. The typed programmatic API runs no CLI
/// gate and leaves the object absent.
///
/// The names this build can emit are `error-severity-findings`, `regression`,
/// `stale-baseline`, `baseline-growth`, `duplication-threshold`,
/// `duplication-findings`, `health-min-score`,
/// `health-min-severity`, `health-findings`, `health-coverage-gaps`,
/// `health-runtime-coverage`, `security`, `security-advisory`, `audit-verdict`,
/// `type-aware-require` and `parse-error`. The set is OPEN: a name a consumer does not
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

    /// Whether a gate of the set fails the run: its status is `fail` and it
    /// is enforced.
    #[must_use]
    pub fn fails_run(&self) -> bool {
        self.0
            .values()
            .any(|outcome| outcome.enforced && outcome.status == GateStatus::Fail)
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
    fn a_file_judging_gate_lists_its_files_and_counts_them() {
        let mut gates = GateOutcomes::new();
        gates.insert(
            GateName::ParseError,
            GateOutcome::with_files(
                GateStatus::Fail,
                true,
                vec![GateFile {
                    path: "src/Broken.tsx".to_owned(),
                    error_count: 1,
                    panicked: true,
                    reason: None,
                }],
            ),
        );
        gates.insert(
            GateName::Regression,
            GateOutcome::new(GateStatus::Pass, true),
        );
        let value = serde_json::to_value(&gates).expect("gate outcomes serialize");
        assert_eq!(
            value,
            serde_json::json!({
                "regression": { "status": "pass", "enforced": true },
                "parse-error": {
                    "status": "fail",
                    "enforced": true,
                    "observed": 1.0,
                    "files": [
                        { "path": "src/Broken.tsx", "error_count": 1, "panicked": true }
                    ]
                }
            })
        );
    }
}
