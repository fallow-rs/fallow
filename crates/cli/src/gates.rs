//! Builders for the envelope's `gate_outcomes`, one per command.
//!
//! Every `GateOutcome` of a CLI envelope is built in this module. A command
//! passes the values its exit path already computed, and the rule that a
//! standalone command and the combined run share (for example
//! `error-severity-findings` or `health-findings`) has one builder here.
//!
//! Each builder projects the rules the command's exit path already applies, so
//! the published verdict and the process status cannot disagree. Nothing here
//! evaluates a condition of its own: every outcome reads the same value the
//! gate reads, which is why the identity tests in `crates/cli/tests` can assert
//! `gate_outcomes[g].status` against the feature-local field beside it.
//!
//! # When an entry appears
//!
//! A gate entry appears when the gate was ARMED on this run: `--fail-on-issues`
//! arms [`GateName::ErrorSeverityFindings`] as an enforced rule, a loaded
//! baseline arms [`GateName::StaleBaseline`], `--threshold` arms
//! [`GateName::DuplicationThreshold`], and so on.
//!
//! The default exit rule of a command is always in its object, also when no
//! flag armed a gate: `error-severity-findings` on `dead-code`, `check` and the
//! combined run, `health-findings` on `health`, `security-advisory` on
//! `security` and `audit-verdict` on `audit`. A JSON reader then sees a failing
//! run without the exit code. `dupes` has no default rule, so a `dupes` run
//! that armed nothing publishes no object and always exits 0.
//!
//! A gate that is armed but cannot be enforced publishes its verdict with
//! `enforced: false` rather than hiding it: `health --report-only`, a
//! change-scoped baseline comparison and the combined machine formats.

use std::path::Path;

use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_output::{GateFile, GateName, GateOutcome, GateOutcomes, GateStatus};

/// `pass` or `fail` from a plain predicate.
pub const fn status_of(failed: bool) -> GateStatus {
    if failed {
        GateStatus::Fail
    } else {
        GateStatus::Pass
    }
}

/// The regression gate's outcome, `None` when no regression comparison ran.
///
/// `Skipped` maps to [`GateStatus::Skipped`] rather than to `pass`: the
/// comparison stood down (a change-scoped run, typically), and reporting that
/// as a pass would assert a judgement nothing made.
pub fn regression_outcome(
    outcome: Option<&crate::regression::RegressionOutcome>,
    enforced: bool,
) -> Option<GateOutcome> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "issue counts never approach the f64 integer limit"
    )]
    fn delta(baseline: usize, current: usize) -> f64 {
        current as f64 - baseline as f64
    }

    let outcome = outcome?;
    Some(match outcome {
        crate::regression::RegressionOutcome::Pass {
            baseline_total,
            current_total,
        } => GateOutcome {
            status: GateStatus::Pass,
            enforced,
            observed: Some(delta(*baseline_total, *current_total)),
            // A pass records no tolerance, matching `regression.tolerance`,
            // which is null on the same outcome.
            threshold: None,
            threshold_label: None,
            files: Vec::new(),
        },
        crate::regression::RegressionOutcome::Exceeded {
            baseline_total,
            current_total,
            tolerance,
            ..
        } => GateOutcome {
            status: GateStatus::Fail,
            enforced,
            observed: Some(delta(*baseline_total, *current_total)),
            // The allowance rather than the tolerance's own number: a
            // percentage tolerance beside an absolute delta renders "12 against
            // 50" for a 50% allowance on a baseline of 3, which is a comparison
            // the reader cannot make. `allowed_delta` mirrors the gate's own
            // rule, so the two numbers are the ones the gate compared.
            threshold: Some(tolerance.allowed_delta(*baseline_total)),
            // The spelling the user passed, so the unit survives onto the
            // grouped envelope, which carries no `regression` object to read
            // `tolerance_kind` from.
            threshold_label: Some(tolerance.label()),
            files: Vec::new(),
        },
        crate::regression::RegressionOutcome::Skipped { .. } => {
            GateOutcome::new(GateStatus::Skipped, enforced)
        }
    })
}

/// The stale-baseline gate's outcome, `None` when no baseline was loaded.
///
/// Reads `gate_trips` off the envelope object the run already publishes, so the
/// two can never drift. `enforced` is the opt-in flag: the verdict is published
/// either way, and only the flag decides whether it fails the run.
///
/// A narrowed run cannot judge a whole-project baseline and stands down, with one
/// exception: telling a file this command never wrote from one of its own needs
/// no whole-project run, so an unrecognised baseline is judged at any scope. That
/// keeps this status equal to `gate_trips`, which is the identity the object
/// exists for.
pub const fn stale_baseline_outcome(
    staleness: Option<&fallow_output::BaselineStaleness>,
    fail_on_stale_baseline: bool,
) -> Option<GateOutcome> {
    let Some(staleness) = staleness else {
        return None;
    };
    if staleness.change_scoped && !staleness.unrecognised_format {
        return Some(GateOutcome::new(GateStatus::Skipped, false));
    }
    Some(GateOutcome::new(
        status_of(staleness.gate_trips),
        fail_on_stale_baseline,
    ))
}

/// The type-aware completeness gate's outcome, `None` unless the `complete`
/// policy was requested.
pub fn type_aware_outcome(
    require: fallow_config::TypeAwareRequire,
    meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> Option<GateOutcome> {
    if require != fallow_config::TypeAwareRequire::Complete {
        return None;
    }
    let incomplete = crate::report::ci::required_type_aware_incomplete(meta);
    Some(GateOutcome::new(status_of(incomplete), true))
}

/// The type-aware completeness gate's outcome, read from the metadata of the
/// type-aware pass. `None` unless the metadata records the `complete` policy.
///
/// `health` and `audit` record the policy they resolved in
/// `required_completeness`, and their exit path reads the same predicate,
/// [`crate::report::ci::required_type_aware_incomplete`], so the entry and the
/// exit code cannot disagree.
pub fn type_aware_meta_outcome(
    meta: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> Option<GateOutcome> {
    let required = meta?.required_completeness
        == Some(fallow_types::semantic::SemanticCompletenessRequirement::Complete);
    required.then(|| {
        GateOutcome::new(
            status_of(crate::report::ci::required_type_aware_incomplete(meta)),
            true,
        )
    })
}

/// The files of a run that did not parse cleanly, sorted by path, for the
/// `parse-error` gate.
///
/// Reads the `source-parse-degraded` entries of the run's workspace
/// diagnostics, so the gate and `workspace_diagnostics[]` name the same files.
/// Other kinds that set `degrades_analysis` do not count: several of them fire
/// on clean projects.
pub fn parse_degraded_files(root: &Path, diagnostics: &[WorkspaceDiagnostic]) -> Vec<GateFile> {
    let mut files: Vec<GateFile> = diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic.kind {
            WorkspaceDiagnosticKind::SourceParseDegraded {
                error_count,
                panicked,
            } => Some(GateFile {
                path: fallow_types::path_util::display_relative(root, &diagnostic.path),
                error_count,
                panicked,
            }),
            _ => None,
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    files
}

/// The `parse-error` gate's outcome, `None` unless `--fail-on-parse-error` or
/// the `failOnParseError` config key armed it.
///
/// The exit path reads [`GateOutcome::fails_run`] on this same value, so the
/// entry and the exit code cannot disagree.
pub fn parse_error_outcome(armed: bool, enforced: bool, files: &[GateFile]) -> Option<GateOutcome> {
    armed.then(|| GateOutcome::with_files(status_of(!files.is_empty()), enforced, files.to_vec()))
}

/// The `parse-error` gate of a run with several sections (the bare run and
/// `audit`), `None` unless a section's config arms it.
///
/// Each section is its resolved config and its workspace diagnostics. The flag
/// reaches the config of each section, so the gate is armed when any section
/// holds it, and the files are the union of the sections' degraded files. The
/// entry keeps `enforced: true`, because both callers apply the gate in every
/// output format.
pub fn sections_parse_error_outcome(
    sections: &[(&fallow_config::ResolvedConfig, &[WorkspaceDiagnostic])],
) -> Option<GateOutcome> {
    if !sections
        .iter()
        .any(|(config, _)| config.fail_on_parse_error)
    {
        return None;
    }
    let (first, _) = sections.first()?;
    let diagnostics: Vec<WorkspaceDiagnostic> = sections
        .iter()
        .flat_map(|(_, diagnostics)| diagnostics.iter().cloned())
        .collect();
    parse_error_outcome(true, true, &parse_degraded_files(&first.root, &diagnostics))
}

/// The one-line reason of a degraded file, for human text: the number of
/// parser errors and whether the parser stopped.
pub fn parse_error_reason(file: &GateFile) -> String {
    let errors = if file.error_count == 1 {
        "1 parser error".to_owned()
    } else {
        format!("{} parser errors", file.error_count)
    };
    let outcome = if file.panicked {
        "the parser stopped"
    } else {
        "the parser recovered"
    };
    format!("{errors}, {outcome}")
}

/// Print the stderr lines of a failed `parse-error` gate: one status line, then
/// one line per file with its parser outcome.
///
/// Printed also under `--quiet`, like the stale-baseline note: `--ci` implies
/// `--quiet`, and SARIF and CodeClimate have no place for the verdict, so
/// without these lines a CI log shows exit 1 and no reason.
pub fn print_parse_error_gate_failure(files: &[GateFile]) {
    if files.is_empty() {
        return;
    }
    let noun = if files.len() == 1 { "file" } else { "files" };
    eprintln!(
        "{}",
        crate::report::human_status_line(
            crate::report::HumanStatus::Failure,
            format_args!(
                "Parse-error gate failed: fallow could not parse {} {noun} cleanly.",
                files.len()
            )
        )
    );
    for file in files {
        eprintln!("  {}: {}", file.path, parse_error_reason(file));
    }
}

/// The severity rule that decides a dead-code, check or combined run's exit
/// code, whether or not `--fail-on-issues` was passed.
///
/// Severity-aware rather than a count: a project with a rule set to `warn`
/// reports findings and still passes, and `--fail-on-issues` is what promotes
/// those warns into the rule. It is always `enforced` on the standalone
/// commands, because this rule always decides their exit code.
///
/// It is the default exit rule, so it is in every `dead-code` and `check`
/// object. Without it, a run gated only on `--fail-on-regression` could exit 1
/// for an error-severity finding while every entry reported a pass.
pub const fn error_severity_outcome(has_error_severity: bool, enforced: bool) -> GateOutcome {
    GateOutcome::new(status_of(has_error_severity), enforced)
}

/// The default exit rule of `health`: a complexity finding whose
/// `complexity-*` rule is `error` fails the run.
/// Also the verdict of the health section of the combined run.
pub const fn health_findings_outcome(has_findings: bool, enforced: bool) -> GateOutcome {
    GateOutcome::new(status_of(has_findings), enforced)
}

/// The duplication threshold gate, `None` when no threshold was configured.
///
/// A threshold of zero means "no limit", which is the CLI's own reading, so it
/// arms nothing.
pub fn duplication_threshold_outcome(
    threshold: f64,
    duplication_percentage: f64,
    enforced: bool,
) -> Option<GateOutcome> {
    if threshold <= 0.0 {
        return None;
    }
    Some(GateOutcome::measured(
        status_of(crate::dupes::exceeds_threshold(
            threshold,
            duplication_percentage,
        )),
        // Arming, not the verdict: `enforced` answers "would a failure here
        // fail the run", so a passing threshold gate on a command that exits
        // on it still reports true, exactly as the stale-baseline gate does.
        enforced,
        duplication_percentage,
        threshold,
    ))
}

/// Collect the gates a dead-code or check run evaluated.
pub struct CheckGateInputs<'a> {
    pub has_error_severity: bool,
    pub regression: Option<&'a crate::regression::RegressionOutcome>,
    pub baseline_staleness: Option<&'a fallow_output::BaselineStaleness>,
    pub fail_on_stale_baseline: bool,
    pub type_aware_require: fallow_config::TypeAwareRequire,
    pub type_aware_meta: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    /// The `parse-error` gate, when it is armed. See [`parse_error_outcome`].
    pub parse_error: Option<GateOutcome>,
}

/// Build the dead-code envelope's `gate_outcomes`. Always present, because the
/// severity rule always decides the exit code.
pub fn check_gate_outcomes(input: &CheckGateInputs<'_>) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    gates.insert_if(
        GateName::Regression,
        regression_outcome(input.regression, true),
    );
    gates.insert_if(
        GateName::StaleBaseline,
        stale_baseline_outcome(input.baseline_staleness, input.fail_on_stale_baseline),
    );
    gates.insert_if(
        GateName::TypeAwareRequire,
        type_aware_outcome(input.type_aware_require, input.type_aware_meta),
    );
    gates.insert_if(GateName::ParseError, input.parse_error.clone());
    gates.insert(
        GateName::ErrorSeverityFindings,
        error_severity_outcome(input.has_error_severity, true),
    );
    gates.into_option()
}

/// The gates a duplication run evaluated.
///
/// The threshold verdict is the same [`crate::dupes::exceeds_threshold`] call
/// that the exit path makes, so the published status and the process status
/// cannot disagree. `dupes` has no default rule: a run that arms no gate
/// publishes no object.
pub fn dupes_gate_outcomes(
    threshold: f64,
    duplication_percentage: f64,
    baseline_staleness: Option<&fallow_output::BaselineStaleness>,
    fail_on_stale_baseline: bool,
) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    gates.insert_if(
        GateName::DuplicationThreshold,
        // Armed, not the verdict: the standalone command exits on this gate,
        // so a passing threshold still reports `enforced: true`.
        duplication_threshold_outcome(threshold, duplication_percentage, true),
    );
    gates.insert_if(
        GateName::StaleBaseline,
        stale_baseline_outcome(baseline_staleness, fail_on_stale_baseline),
    );
    gates.into_option()
}

/// What a health run armed, for [`health_gate_outcomes`].
pub struct HealthGateInputs<'a> {
    /// `--report-only`: every entry keeps its verdict with `enforced: false`.
    pub report_only: bool,
    /// `--min-score` with the computed score, when the gate is armed. A
    /// missing score means that the caller asked for the gate without the
    /// score it compares.
    pub min_score: Option<(f64, Option<f64>)>,
    /// `--min-severity` with the number of blocking findings at or above the
    /// floor.
    pub min_severity: Option<(fallow_output::FindingSeverity, usize)>,
    /// The coverage-gap gate when it is armed: whether the run has gaps.
    pub coverage_gaps: Option<bool>,
    /// The runtime-coverage gate when a report is present: whether a finding
    /// fails it.
    pub runtime_coverage: Option<bool>,
    /// The loaded baseline, when one was loaded.
    pub baseline_staleness: Option<&'a fallow_output::BaselineStaleness>,
    /// `--fail-on-stale-baseline`.
    pub fail_on_stale_baseline: bool,
    /// Whether the run has a complexity finding whose `complexity-*` rule is
    /// `error`.
    pub has_findings: bool,
    /// The metadata of the type-aware coupling pass, when it ran.
    pub type_aware_meta: Option<&'a fallow_types::envelope::TypeAwareMeta>,
    /// The `parse-error` gate, when it is armed, built with `enforced: true`.
    /// `--report-only` clamps it like every health gate.
    pub parse_error: Option<GateOutcome>,
}

/// The gates a health run armed.
///
/// Every entry reads the same predicate the exit path reads. `--report-only`
/// makes the run exit 0 before any gate runs, so it clamps `enforced` to false
/// on every entry and keeps each verdict. `health-findings` is the default
/// exit rule, so it is always in the object, except when `--min-severity`
/// replaces it. The type-aware completeness gate is not a health gate:
/// `--report-only` does not stop it, so its entry stays `enforced`.
pub fn health_gate_outcomes(input: &HealthGateInputs<'_>) -> Option<GateOutcomes> {
    let enforced = !input.report_only;
    let mut gates = GateOutcomes::new();
    gates.insert_if(
        GateName::TypeAwareRequire,
        type_aware_meta_outcome(input.type_aware_meta),
    );

    if let Some((threshold, score)) = input.min_score {
        gates.insert(
            GateName::HealthMinScore,
            score.map_or_else(
                || GateOutcome::new(GateStatus::Skipped, false),
                |score| {
                    GateOutcome::measured(status_of(score < threshold), enforced, score, threshold)
                },
            ),
        );
    }

    if let Some((floor, reached)) = input.min_severity {
        gates.insert(
            GateName::HealthMinSeverity,
            GateOutcome::counted(
                status_of(reached > 0),
                enforced,
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a finding count never approaches the f64 integer limit"
                )]
                {
                    reached as f64
                },
                severity_floor_label(floor),
            ),
        );
    }

    if let Some(has_gaps) = input.coverage_gaps {
        gates.insert(
            GateName::HealthCoverageGaps,
            GateOutcome::new(status_of(has_gaps), enforced),
        );
    }

    // Armed by `--runtime-coverage`, so it sits with the flag-armed gates:
    // without it, a run whose only gate is runtime coverage exits 1 and
    // publishes nothing.
    if let Some(failing) = input.runtime_coverage {
        gates.insert(
            GateName::HealthRuntimeCoverage,
            GateOutcome::new(status_of(failing), enforced),
        );
    }

    gates.insert_if(
        GateName::StaleBaseline,
        stale_baseline_outcome(
            input.baseline_staleness,
            input.fail_on_stale_baseline && enforced,
        ),
    );

    gates.insert_if(
        GateName::ParseError,
        input.parse_error.clone().map(|mut outcome| {
            outcome.enforced &= enforced;
            outcome
        }),
    );

    // With `--min-severity` the findings gate IS the severity gate, recorded
    // above under its own name.
    if input.min_severity.is_none() {
        gates.insert(
            GateName::HealthFindings,
            if input.min_score.is_some() {
                // `--min-score` alone turns the findings branch off: complexity
                // findings become informational.
                GateOutcome::new(GateStatus::Skipped, false)
            } else {
                health_findings_outcome(input.has_findings, enforced)
            },
        );
    }

    gates.into_option()
}

/// The wire spelling of a severity floor, for `threshold_label`.
const fn severity_floor_label(severity: fallow_output::FindingSeverity) -> &'static str {
    match severity {
        fallow_output::FindingSeverity::Moderate => "moderate",
        fallow_output::FindingSeverity::High => "high",
        fallow_output::FindingSeverity::Critical => "critical",
    }
}

/// The gates a security run evaluated.
///
/// `gate_failed` is the verdict of `--gate` when it is set. A configured gate
/// decides the exit code before the advisory, so the advisory is recorded as
/// `skipped`: a passing gate that hides an advisory backlog is visible.
/// Without `--gate`, the advisory is the default exit rule and is always in the
/// object. It is `enforced` only when `--fail-on-issues` or an `error` rule
/// severity lets it fail the run.
pub fn security_gate_outcomes(
    gate_failed: Option<bool>,
    advisory_failed: bool,
    advisory_enforced: bool,
) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    if let Some(gate_failed) = gate_failed {
        gates.insert(
            GateName::Security,
            GateOutcome::new(status_of(gate_failed), true),
        );
        gates.insert(
            GateName::SecurityAdvisory,
            GateOutcome::new(GateStatus::Skipped, false),
        );
        return gates.into_option();
    }
    gates.insert(
        GateName::SecurityAdvisory,
        GateOutcome::new(status_of(advisory_failed), advisory_enforced),
    );
    gates.into_option()
}

/// The audit verdict, for the envelope's `gate_outcomes`.
///
/// The only three-valued gate: the warn tier reports `warn` and does not
/// collapse onto `pass`. Always present, because `fallow audit` always reaches
/// a verdict. A loaded baseline adds a `skipped`, unenforced `stale-baseline`
/// entry: every audit narrows to the changed files, so a whole-project baseline
/// cannot be judged. One entry stands for up to three baselines. A dead-code
/// pass that ran under the `complete` type-aware policy adds a
/// `type-aware-require` entry, because that gate also decides the exit code.
/// An armed `parse-error` gate adds its entry for the same reason.
pub fn audit_gate_outcomes(
    verdict: GateStatus,
    loaded_any_baseline: bool,
    type_aware_meta: Option<&fallow_types::envelope::TypeAwareMeta>,
    parse_error: Option<GateOutcome>,
) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    gates.insert(GateName::AuditVerdict, GateOutcome::new(verdict, true));
    gates.insert_if(
        GateName::TypeAwareRequire,
        type_aware_meta_outcome(type_aware_meta),
    );
    gates.insert_if(GateName::ParseError, parse_error);
    if loaded_any_baseline {
        gates.insert(
            GateName::StaleBaseline,
            GateOutcome::new(GateStatus::Skipped, false),
        );
    }
    gates.into_option()
}

/// What the sections of a bare `fallow` run concluded, for
/// [`combined_gate_outcomes`].
pub struct CombinedGateInputs<'a> {
    /// The regression outcome of the dead-code section.
    pub regression: Option<&'a crate::regression::RegressionOutcome>,
    /// The baselines each section loaded: dead code, dupes, health.
    pub baselines: [Option<fallow_output::BaselineStaleness>; 3],
    /// `--fail-on-stale-baseline`.
    pub fail_on_stale_baseline: bool,
    /// The type-aware completeness gate when the run requested it: whether
    /// it failed.
    pub type_aware_failed: Option<bool>,
    /// The duplication threshold and the measured percentage, when the dupes
    /// section ran.
    pub duplication: Option<(f64, f64)>,
    /// Whether the dead-code section holds an error-severity finding, when it
    /// ran.
    pub has_error_severity: Option<bool>,
    /// Whether the health section holds a finding whose `complexity-*` rule
    /// is `error`, when it ran.
    pub health_has_findings: Option<bool>,
    /// The `parse-error` gate, when it is armed. The combined run applies it
    /// in every output format, so it keeps `enforced: true`.
    pub parse_error: Option<GateOutcome>,
}

/// The verdicts of a bare `fallow` run, merged into one root-level object.
///
/// It carries the default exit rule of each section that ran:
/// `error-severity-findings` for dead code and `health-findings` for health.
/// Dupes has no default rule. With these rules in it, the `status` members say
/// whether the human run of the same flags fails.
///
/// The combined machine renderers exit 0 for every gate except the
/// stale-baseline gate, the regression gate, the type-aware completeness gate
/// and the parse-error gate, so every other entry here reports
/// `enforced: false`.
pub fn combined_gate_outcomes(input: &CombinedGateInputs<'_>) -> Option<GateOutcomes> {
    let mut gates = GateOutcomes::new();
    gates.insert_if(
        GateName::Regression,
        regression_outcome(input.regression, true),
    );
    gates.insert_if(
        GateName::StaleBaseline,
        merged_stale_baseline_outcome(&input.baselines, input.fail_on_stale_baseline),
    );
    if let Some(failed) = input.type_aware_failed {
        gates.insert(
            GateName::TypeAwareRequire,
            GateOutcome::new(status_of(failed), true),
        );
    }
    gates.insert_if(GateName::ParseError, input.parse_error.clone());
    if let Some((threshold, percentage)) = input.duplication {
        gates.insert_if(
            GateName::DuplicationThreshold,
            duplication_threshold_outcome(threshold, percentage, false),
        );
    }
    if let Some(has_error_severity) = input.has_error_severity {
        gates.insert(
            GateName::ErrorSeverityFindings,
            error_severity_outcome(has_error_severity, false),
        );
    }
    if let Some(has_findings) = input.health_has_findings {
        gates.insert(
            GateName::HealthFindings,
            health_findings_outcome(has_findings, false),
        );
    }
    gates.into_option()
}

/// One stale-baseline verdict for several loaded baselines: `fail` when any
/// baseline trips, else `pass` when any was judged, else the first stand-down.
fn merged_stale_baseline_outcome(
    baselines: &[Option<fallow_output::BaselineStaleness>],
    fail_on_stale_baseline: bool,
) -> Option<GateOutcome> {
    let outcomes: Vec<GateOutcome> = baselines
        .iter()
        .filter_map(|staleness| stale_baseline_outcome(staleness.as_ref(), fail_on_stale_baseline))
        .collect();
    let judged = |status: GateStatus| {
        outcomes
            .iter()
            .find(|outcome| outcome.status == status)
            .cloned()
    };
    judged(GateStatus::Fail)
        .or_else(|| judged(GateStatus::Pass))
        .or_else(|| outcomes.first().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_that_arms_nothing_still_states_the_default_rule() {
        let gates = check_gate_outcomes(&CheckGateInputs {
            has_error_severity: true,
            regression: None,
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            type_aware_require: fallow_config::TypeAwareRequire::BestEffort,
            type_aware_meta: None,
            parse_error: None,
        })
        .expect("the default exit rule is always published");
        let outcome = gates
            .get(GateName::ErrorSeverityFindings)
            .expect("the default exit rule");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert!(outcome.fails_run());
    }

    /// The object has to explain the exit code it sits beside, so once it
    /// exists the rule that actually decides that exit code is in it.
    #[test]
    fn the_object_always_carries_the_rule_that_decides_the_exit_code() {
        let gates = check_gate_outcomes(&CheckGateInputs {
            has_error_severity: true,
            regression: Some(&crate::regression::RegressionOutcome::Pass {
                baseline_total: 1,
                current_total: 1,
            }),
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            type_aware_require: fallow_config::TypeAwareRequire::BestEffort,
            type_aware_meta: None,
            parse_error: None,
        })
        .expect("the regression gate armed the object");
        assert_eq!(
            gates.get(GateName::Regression).expect("armed").status,
            GateStatus::Pass
        );
        let outcome = gates
            .get(GateName::ErrorSeverityFindings)
            .expect("the default exit rule joins the object");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert!(outcome.enforced);
    }

    #[test]
    fn a_published_baseline_verdict_is_unenforced_without_the_flag() {
        let staleness = fallow_output::BaselineStaleness {
            baseline_entries: 8,
            matched_entries: 3,
            stale_entries: 5,
            current_findings: 0,
            change_scoped: false,
            stale: false,
            warning: fallow_output::BaselineStalenessAdvisory::None,
            gate_trips: true,
            moved_entries: 0,
            unrecognised_format: false,
            saved_by: None,
            scope_reasons: fallow_output::BaselineScopeReasons::empty(),
        };
        let unarmed = stale_baseline_outcome(Some(&staleness), false).expect("verdict published");
        assert_eq!(unarmed.status, GateStatus::Fail);
        assert!(!unarmed.enforced);
        assert!(!unarmed.fails_run());

        let armed = stale_baseline_outcome(Some(&staleness), true).expect("verdict published");
        assert!(armed.fails_run());
    }

    #[test]
    fn a_change_scoped_baseline_run_stands_down() {
        let staleness = fallow_output::BaselineStaleness {
            baseline_entries: 8,
            matched_entries: 0,
            stale_entries: 8,
            current_findings: 0,
            change_scoped: true,
            stale: false,
            warning: fallow_output::BaselineStalenessAdvisory::None,
            gate_trips: false,
            moved_entries: 0,
            unrecognised_format: false,
            saved_by: None,
            scope_reasons: fallow_output::BaselineScopeReasons::empty()
                .with(fallow_output::ScopeReason::Production),
        };
        let outcome = stale_baseline_outcome(Some(&staleness), true).expect("verdict published");
        assert_eq!(outcome.status, GateStatus::Skipped);
        assert!(!outcome.fails_run());
    }

    #[test]
    fn a_zero_threshold_arms_no_duplication_gate() {
        assert!(duplication_threshold_outcome(0.0, 100.0, true).is_none());
        let outcome = duplication_threshold_outcome(5.0, 100.0, true).expect("gate armed");
        assert_eq!(outcome.status, GateStatus::Fail);
        assert_eq!(outcome.observed, Some(100.0));
        assert_eq!(outcome.threshold, Some(5.0));
    }

    #[test]
    fn a_skipped_regression_comparison_is_not_a_pass() {
        let outcome = regression_outcome(
            Some(&crate::regression::RegressionOutcome::Skipped {
                reason: "changed-since",
            }),
            true,
        )
        .expect("comparison ran");
        assert_eq!(outcome.status, GateStatus::Skipped);
    }

    fn type_aware_meta(
        required: fallow_types::semantic::SemanticCompletenessRequirement,
        completeness: fallow_types::semantic::SemanticCompleteness,
    ) -> fallow_types::envelope::TypeAwareMeta {
        fallow_types::envelope::TypeAwareMeta {
            required_completeness: Some(required),
            identity: Some(fallow_types::semantic::SemanticAnalysisIdentity {
                completeness,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_type_aware_entry_follows_the_recorded_policy() {
        use fallow_types::semantic::{SemanticCompleteness, SemanticCompletenessRequirement};

        assert_eq!(type_aware_meta_outcome(None), None);
        let best_effort = type_aware_meta(
            SemanticCompletenessRequirement::BestEffort,
            SemanticCompleteness::Partial,
        );
        assert_eq!(type_aware_meta_outcome(Some(&best_effort)), None);

        let complete = type_aware_meta(
            SemanticCompletenessRequirement::Complete,
            SemanticCompleteness::Complete,
        );
        let partial = type_aware_meta(
            SemanticCompletenessRequirement::Complete,
            SemanticCompleteness::Partial,
        );
        for (meta, status) in [(&complete, GateStatus::Pass), (&partial, GateStatus::Fail)] {
            let outcome = type_aware_meta_outcome(Some(meta)).expect("the policy arms the gate");
            assert_eq!(outcome.status, status);
            assert!(outcome.enforced);
            assert_eq!(
                crate::exit_codes::gate_exit_code(GateName::TypeAwareRequire, outcome.status),
                u8::from(crate::report::ci::required_type_aware_incomplete(Some(
                    meta
                ))),
                "the entry and the exit path read one predicate"
            );
        }
    }

    fn degraded(root: &std::path::Path, file: &str, panicked: bool) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(file),
            WorkspaceDiagnosticKind::SourceParseDegraded {
                error_count: 2,
                panicked,
            },
        )
    }

    #[test]
    fn only_parse_degraded_files_count_and_they_come_sorted_and_relative() {
        let root = std::path::Path::new("/project");
        let diagnostics = vec![
            degraded(root, "src/z.ts", false),
            WorkspaceDiagnostic::new(
                root,
                root.join("node_modules"),
                WorkspaceDiagnosticKind::NodeModulesMissing,
            ),
            degraded(root, "src/a.tsx", true),
            degraded(root, "src/a.tsx", true),
        ];
        let files = parse_degraded_files(root, &diagnostics);
        assert_eq!(
            files,
            vec![
                GateFile {
                    path: "src/a.tsx".to_owned(),
                    error_count: 2,
                    panicked: true,
                },
                GateFile {
                    path: "src/z.ts".to_owned(),
                    error_count: 2,
                    panicked: false,
                },
            ]
        );
        assert_eq!(
            parse_error_reason(&files[0]),
            "2 parser errors, the parser stopped"
        );
    }

    #[test]
    fn an_unarmed_parse_error_gate_publishes_nothing() {
        let files = [GateFile {
            path: "src/a.ts".to_owned(),
            error_count: 1,
            panicked: false,
        }];
        assert_eq!(parse_error_outcome(false, true, &files), None);
        let armed = parse_error_outcome(true, true, &files).expect("armed");
        assert!(armed.fails_run());
        assert_eq!(armed.observed, Some(1.0));
        let clean = parse_error_outcome(true, true, &[]).expect("armed");
        assert_eq!(clean.status, GateStatus::Pass);
    }

    #[test]
    fn report_only_clamps_the_parse_error_entry() {
        let files = vec![GateFile {
            path: "src/a.ts".to_owned(),
            error_count: 1,
            panicked: false,
        }];
        let gates = health_gate_outcomes(&HealthGateInputs {
            report_only: true,
            min_score: None,
            min_severity: None,
            coverage_gaps: None,
            runtime_coverage: None,
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            has_findings: false,
            type_aware_meta: None,
            parse_error: parse_error_outcome(true, true, &files),
        })
        .expect("health always states its default rule");
        let entry = gates.get(GateName::ParseError).expect("armed");
        assert_eq!(entry.status, GateStatus::Fail);
        assert!(!entry.fails_run());
    }

    #[test]
    fn report_only_keeps_the_type_aware_entry_enforced() {
        use fallow_types::semantic::{SemanticCompleteness, SemanticCompletenessRequirement};

        let partial = type_aware_meta(
            SemanticCompletenessRequirement::Complete,
            SemanticCompleteness::Partial,
        );
        let gates = health_gate_outcomes(&HealthGateInputs {
            report_only: true,
            min_score: None,
            min_severity: None,
            coverage_gaps: None,
            runtime_coverage: None,
            baseline_staleness: None,
            fail_on_stale_baseline: false,
            has_findings: false,
            type_aware_meta: Some(&partial),
            parse_error: None,
        })
        .expect("health always states its default rule");
        let json = serde_json::to_value(&gates).expect("gate outcomes serialize");
        assert_eq!(
            json["type-aware-require"],
            serde_json::json!({ "status": "fail", "enforced": true })
        );
        assert_eq!(json["health-findings"]["enforced"], false);
    }
}
