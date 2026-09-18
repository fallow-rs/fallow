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
//! its own blast radius. The line is informational on every surface, so the
//! integration that knows which gates the repository armed keeps ownership of
//! the failing exit.

use serde_json::Value;

/// One gate's verdict, read off a saved or live envelope.
pub struct GateLine {
    pub name: String,
    pub status: String,
    pub enforced: bool,
    pub observed: Option<f64>,
    pub threshold: Option<f64>,
    pub threshold_label: Option<String>,
}

impl GateLine {
    /// The trailing `(85 against 90)` clause, when the gate compared something.
    fn measured_clause(&self) -> String {
        match (
            self.observed,
            self.threshold,
            self.threshold_label.as_deref(),
        ) {
            (Some(observed), Some(threshold), _) => {
                format!(" ({} against {})", trim_num(observed), trim_num(threshold))
            }
            (Some(observed), None, Some(label)) => {
                format!(" ({} at or above {label})", trim_num(observed))
            }
            (Some(observed), None, None) => format!(" ({})", trim_num(observed)),
            _ => String::new(),
        }
    }

    fn described(&self) -> String {
        format!("{}{}", self.name, self.measured_clause())
    }

    /// The `observed` display text for a decision-surface row: the numbers the
    /// gate compared when it compared any, and the status word otherwise.
    ///
    /// A row with an empty `observed` renders as a bare label in the check run
    /// and as `Fallow / <id>: ` in a split commit status, so a gate that
    /// measured nothing says what it concluded instead.
    fn observed_text(&self) -> String {
        match (
            self.observed,
            self.threshold,
            self.threshold_label.as_deref(),
        ) {
            (Some(observed), Some(threshold), _) => {
                format!("{} against {}", trim_num(observed), trim_num(threshold))
            }
            (Some(observed), None, Some(label)) => {
                format!("{} at or above {label}", trim_num(observed))
            }
            (Some(observed), None, None) => trim_num(observed),
            _ => self.status.clone(),
        }
    }

    /// The `threshold` display text, `None` when the gate has no threshold to
    /// show.
    fn threshold_text(&self) -> Option<String> {
        self.threshold
            .map(trim_num)
            .or_else(|| self.threshold_label.clone())
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
                threshold_label: entry
                    .get("threshold_label")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// The four outcomes, kept apart.
///
/// `skipped` is not a pass and `warn` is not a pass: the status is four-valued
/// precisely so a gate that stood down is distinguishable from one that was
/// evaluated and held. Collapsing them was how a change-scoped pull request
/// ended up with "Gates passed: stale-baseline." directly under a job-summary
/// advisory saying the same baseline had gone stale.
struct Partitioned<'a> {
    failed: Vec<&'a GateLine>,
    warned: Vec<&'a GateLine>,
    skipped: Vec<&'a GateLine>,
    passed: Vec<&'a GateLine>,
}

fn partition(gates: &[GateLine]) -> Partitioned<'_> {
    let mut out = Partitioned {
        failed: Vec::new(),
        warned: Vec::new(),
        skipped: Vec::new(),
        passed: Vec::new(),
    };
    for gate in gates {
        match gate.status.as_str() {
            "fail" => out.failed.push(gate),
            "skipped" => out.skipped.push(gate),
            "pass" => out.passed.push(gate),
            // "warn", and any status this build does not recognise: the set is
            // open, and an unknown value must not be silently counted as a pass.
            _ => out.warned.push(gate),
        }
    }
    out
}

fn join(gates: &[&GateLine]) -> String {
    gates
        .iter()
        .map(|gate| gate.described())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A one-line verdict for the job summary, the pull-request comment and the
/// merge-request note, or `None` when the run armed no gate.
///
/// Reads as an inventory ("Gate outcomes: failed X; passed Y.") rather than as
/// a verdict of its own. The pull-request comment already carries a
/// check-run-derived heading, and a line starting "Gates failed" sat under a
/// "Quality gate passed" heading as a flat contradiction; naming the outcomes
/// instead lets both be true at once, because they answer different questions.
///
/// Informational on purpose. Whether a tripped gate should fail the build is
/// the consumer's decision, not this renderer's: `enforced` describes the
/// CLI's own exit code, and the GitHub Action deliberately does not pass
/// `--fail-on-issues` to the CLI, so a gate this line calls enforced may sit on
/// a job the repository has configured to pass. The line says what happened and
/// the integration decides what to do about it.
pub fn summary_line(envelope: &Value) -> Option<String> {
    let gates = read_gate_outcomes(envelope);
    if gates.is_empty() {
        return None;
    }
    let parts = partition(&gates);
    let mut clauses: Vec<String> = Vec::new();
    if !parts.failed.is_empty() {
        let enforced = parts.failed.iter().filter(|gate| gate.enforced).count();
        let suffix = if enforced == 0 {
            " (none of which fails this run)"
        } else {
            ""
        };
        clauses.push(format!("failed {}{suffix}", join(&parts.failed)));
    }
    if !parts.warned.is_empty() {
        clauses.push(format!("warned {}", join(&parts.warned)));
    }
    if !parts.skipped.is_empty() {
        clauses.push(format!("stood down {}", join(&parts.skipped)));
    }
    if !parts.passed.is_empty() {
        clauses.push(format!("passed {}", join(&parts.passed)));
    }
    Some(format!("Gate outcomes: {}.", clauses.join("; ")))
}

/// [`summary_line`] for a live run, which holds the gates typed rather than as
/// a parsed envelope.
///
/// Routed through the same function on purpose: `fallow report --from` must
/// render byte-identically to the direct `--format` run, which is a contract
/// with its own parity suite, so the live and saved paths cannot each format
/// the verdict their own way.
pub fn summary_line_for_gates(gates: Option<&fallow_output::GateOutcomes>) -> Option<String> {
    let gates = gates?;
    let envelope = serde_json::json!({ "gate_outcomes": gates });
    summary_line(&envelope)
}

/// The same verdict as a GitHub workflow-command annotation, or `None` when the
/// run armed no gate.
///
/// Always `::notice::`, never `::error::`. The render cannot know whether the
/// consumer armed the gate through its own inputs, and `audit-verdict` is
/// always enforced by the CLI while `command: audit` with `fail-on-issues:
/// false` is a passing reporting job, so an error-level line here paints a red
/// annotation on a green run. The integration owns the `::error::` and the
/// failing exit; this line owns the fact.
pub fn annotation_line(envelope: &Value) -> Option<String> {
    let line = summary_line(envelope)?;
    Some(format!("::notice::Fallow: {line}"))
}

/// One decision-surface row per gate the run armed, so every gate the envelope
/// reports reaches the check run as a named gate rather than as a failed step.
///
/// Generalized rather than special-cased: `--fail-on-stale-baseline` was the
/// gate a reviewer could not see (#2675), and building a row per entry covers
/// the twelve others at the same cost, including any gate a later release adds.
///
/// The surface `conclusion` is deliberately NOT derived from these rows. It is
/// a documented non-blocker and moving it is a separate decision with its own
/// blast radius, so a caller extends its `gates` array after computing its own
/// conclusion.
pub fn gate_rows(envelope: &Value) -> Vec<fallow_output::PrDecisionGate> {
    read_gate_outcomes(envelope)
        .iter()
        .map(|gate| fallow_output::PrDecisionGate {
            id: gate.name.clone(),
            label: gate_label(&gate.name),
            status: row_status(gate),
            observed: gate.observed_text(),
            threshold: gate.threshold_text(),
            // The existing command row says "new code". A gate verdict is not
            // scoped to the change: the baseline rule compares what the whole
            // run matched, and a health floor reads the whole score.
            scope: "this run".to_owned(),
        })
        .collect()
}

/// [`gate_rows`] for a live run, which holds the gates typed rather than as a
/// parsed envelope.
///
/// Routed through the same function for the same reason
/// [`summary_line_for_gates`] is: the saved render and the direct render are
/// one contract with its own parity suite.
pub fn gate_rows_for_gates(
    gates: Option<&fallow_output::GateOutcomes>,
) -> Vec<fallow_output::PrDecisionGate> {
    let Some(gates) = gates else {
        return Vec::new();
    };
    gate_rows(&serde_json::json!({ "gate_outcomes": gates }))
}

/// How a gate's four-valued status maps onto the check-run conclusions the
/// decision surface publishes.
///
/// An unenforced failure is `neutral`, not `failure`: `enforced` is the CLI's
/// statement about its own exit code, and a verdict published without the flag
/// that arms it must not paint a red gate on a run the repository configured to
/// pass. `warn` and any status this build does not recognise are `neutral` too,
/// never `success`, which is the rule [`partition`] already follows.
fn row_status(gate: &GateLine) -> fallow_output::PrDecisionConclusion {
    use fallow_output::PrDecisionConclusion as Conclusion;
    match gate.status.as_str() {
        "fail" if gate.enforced => Conclusion::Failure,
        "pass" => Conclusion::Success,
        "skipped" => Conclusion::Skipped,
        _ => Conclusion::Neutral,
    }
}

/// The display label for a gate name.
///
/// The name set is OPEN, so an unrecognised name degrades to its kebab spelling
/// read as words rather than being dropped or panicking: a gate from a newer
/// build still reaches the check run with a readable label.
fn gate_label(name: &str) -> String {
    known_gate_label(name).map_or_else(|| sentence_case(name), str::to_owned)
}

/// The label for a gate this build emits, `None` for a name it does not know.
///
/// Separate from [`gate_label`] so the coverage test can tell a deliberate
/// label from the open-set fallback, which for a one-word name renders the same
/// string.
fn known_gate_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "error-severity-findings" => "Error-severity findings",
        "regression" => "Regression",
        "stale-baseline" => "Stale baseline",
        "duplication-threshold" => "Duplication threshold",
        "health-min-score" => "Health minimum score",
        "health-min-severity" => "Health minimum severity",
        "health-findings" => "Health findings",
        "health-coverage-gaps" => "Coverage gaps",
        "health-runtime-coverage" => "Runtime coverage",
        "security" => "Security",
        "security-advisory" => "Security advisory",
        "audit-verdict" => "Audit verdict",
        "type-aware-require" => "Type-aware completeness",
        _ => return None,
    })
}

/// `some-future-gate` -> `Some future gate`.
fn sentence_case(name: &str) -> String {
    let spaced = name.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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
    fn a_failing_gate_names_what_it_compared() {
        let value = envelope(&serde_json::json!({
            "health-min-score": {
                "status": "fail", "enforced": true, "observed": 85.0, "threshold": 90.0
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: failed health-min-score (85 against 90)."
        );
    }

    /// The render never escalates. `enforced` is the CLI's statement about its
    /// own exit code, and the integrations deliberately do not pass
    /// `--fail-on-issues` to the CLI, so an error-level annotation here would
    /// paint a red line on a job the repository configured to pass.
    #[test]
    fn the_annotation_is_always_a_notice() {
        for enforced in [true, false] {
            let value = envelope(&serde_json::json!({
                "audit-verdict": { "status": "fail", "enforced": enforced }
            }));
            let line = annotation_line(&value).expect("a gate ran");
            assert!(
                line.starts_with("::notice::"),
                "the render states the fact and leaves the escalation to the consumer: {line}"
            );
        }
    }

    #[test]
    fn an_unenforced_failure_says_it_does_not_fail_the_run() {
        let value = envelope(&serde_json::json!({
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: failed stale-baseline (none of which fails this run)."
        );
    }

    /// A change-scoped pull request stands the baseline gate down, and #2674
    /// built a whole mechanism to keep "could not be judged" apart from
    /// "passed". Collapsing `skipped` into the passed list reversed that inside
    /// one job summary.
    #[test]
    fn a_stood_down_gate_is_not_a_pass() {
        let value = envelope(&serde_json::json!({
            "stale-baseline": { "status": "skipped", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: stood down stale-baseline."
        );
    }

    #[test]
    fn a_warn_tier_is_not_a_pass() {
        let value = envelope(&serde_json::json!({
            "audit-verdict": { "status": "warn", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: warned audit-verdict."
        );
    }

    #[test]
    fn passing_gates_are_named_on_their_own() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "pass", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: passed regression."
        );
    }

    #[test]
    fn all_four_outcomes_stay_apart_in_one_line() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "fail", "enforced": true },
            "audit-verdict": { "status": "warn", "enforced": true },
            "stale-baseline": { "status": "skipped", "enforced": false },
            "duplication-threshold": { "status": "pass", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("gates ran"),
            "Gate outcomes: failed regression; warned audit-verdict; \
             stood down stale-baseline; passed duplication-threshold."
        );
    }

    #[test]
    fn a_named_floor_is_rendered_instead_of_a_number() {
        let value = envelope(&serde_json::json!({
            "health-min-severity": {
                "status": "fail", "enforced": true, "observed": 3.0,
                "threshold_label": "critical"
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: failed health-min-severity (3 at or above critical)."
        );
    }

    #[test]
    fn an_unrecognised_gate_name_still_reports() {
        let value = envelope(&serde_json::json!({
            "some-future-gate": { "status": "fail", "enforced": true }
        }));
        assert_eq!(
            annotation_line(&value).expect("a gate ran"),
            "::notice::Fallow: Gate outcomes: failed some-future-gate."
        );
    }

    /// The status set is open, so a value this build does not know must not be
    /// silently counted as a pass.
    #[test]
    fn an_unrecognised_status_is_not_a_pass() {
        let value = envelope(&serde_json::json!({
            "some-future-gate": { "status": "deferred", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gate outcomes: warned some-future-gate."
        );
    }

    /// The "(none of which fails this run)" note is reserved for the case where
    /// it is true of every failed gate. With a mix, the note would be false of
    /// the enforced one, and naming a count here would only invite a reader to
    /// guess which is which; `enforced` on each entry is where that lives.
    #[test]
    fn a_mix_of_enforced_and_unenforced_failures_adds_no_note() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "fail", "enforced": true },
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("gates ran"),
            "Gate outcomes: failed regression, stale-baseline."
        );
    }

    #[test]
    fn an_envelope_without_the_object_builds_no_rows() {
        assert!(gate_rows(&serde_json::json!({ "kind": "dead-code" })).is_empty());
        assert!(gate_rows_for_gates(None).is_empty());
    }

    /// The row #2675 asked for: an armed and tripped baseline gate reaching the
    /// check run as a named gate rather than as a failed step.
    #[test]
    fn an_armed_stale_baseline_gate_becomes_a_failing_row() {
        let value = envelope(&serde_json::json!({
            "stale-baseline": { "status": "fail", "enforced": true }
        }));

        let rows = gate_rows(&value);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "stale-baseline");
        assert_eq!(rows[0].label, "Stale baseline");
        assert_eq!(rows[0].status, fallow_output::PrDecisionConclusion::Failure);
        assert_eq!(rows[0].observed, "fail");
        assert_eq!(rows[0].threshold, None);
        assert_eq!(rows[0].scope, "this run");
    }

    /// `enforced` is the CLI's statement about its own exit code. A verdict
    /// published without the flag that arms it must not paint a red gate on a
    /// run the repository configured to pass.
    #[test]
    fn the_four_statuses_map_onto_the_check_run_conclusions() {
        use fallow_output::PrDecisionConclusion as Conclusion;
        for (status, enforced, expected) in [
            ("fail", true, Conclusion::Failure),
            ("fail", false, Conclusion::Neutral),
            ("warn", true, Conclusion::Neutral),
            ("skipped", false, Conclusion::Skipped),
            ("pass", true, Conclusion::Success),
            ("deferred", true, Conclusion::Neutral),
        ] {
            let value = envelope(&serde_json::json!({
                "stale-baseline": { "status": status, "enforced": enforced }
            }));
            let rows = gate_rows(&value);
            assert_eq!(rows[0].status, expected, "{status} enforced={enforced}");
            assert_eq!(
                rows[0].observed, status,
                "{status} must say what it concluded"
            );
        }
    }

    #[test]
    fn a_measured_gate_carries_both_numbers() {
        let value = envelope(&serde_json::json!({
            "health-min-score": {
                "status": "fail", "enforced": true, "observed": 85.0, "threshold": 90.0
            }
        }));

        let rows = gate_rows(&value);

        assert_eq!(rows[0].observed, "85 against 90");
        assert_eq!(rows[0].threshold.as_deref(), Some("90"));
    }

    #[test]
    fn a_named_floor_is_carried_as_the_threshold() {
        let value = envelope(&serde_json::json!({
            "health-min-severity": {
                "status": "fail", "enforced": true, "observed": 3.0,
                "threshold_label": "critical"
            }
        }));

        let rows = gate_rows(&value);

        assert_eq!(rows[0].observed, "3 at or above critical");
        assert_eq!(rows[0].threshold.as_deref(), Some("critical"));
    }

    /// The name set is OPEN, so a gate from a newer build must still reach the
    /// check run with a readable label rather than being dropped.
    #[test]
    fn an_unrecognised_gate_name_still_builds_a_readable_row() {
        let value = envelope(&serde_json::json!({
            "some-future-gate": { "status": "fail", "enforced": true }
        }));

        let rows = gate_rows(&value);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "some-future-gate");
        assert_eq!(rows[0].label, "Some future gate");
    }

    /// Tested against the emitter rather than against a hand-kept list: a gate
    /// added without a label would otherwise reach the check run reading as a
    /// kebab identifier.
    #[test]
    fn every_gate_this_build_emits_has_its_own_label() {
        for name in fallow_output::GateName::ALL {
            let key = name.as_str();
            assert!(
                known_gate_label(key).is_some(),
                "{key} falls through to the open-set fallback"
            );
        }
    }

    /// The live path holds the gates typed and the saved path reads them off
    /// the envelope. The rows must be identical, or the check run a pipeline
    /// publishes from `report --from` differs from a direct render's.
    #[test]
    fn the_live_and_saved_rows_agree() {
        let mut gates = fallow_output::GateOutcomes::new();
        gates.insert(
            fallow_output::GateName::StaleBaseline,
            fallow_output::GateOutcome::new(fallow_output::GateStatus::Fail, true),
        );
        gates.insert(
            fallow_output::GateName::HealthMinScore,
            fallow_output::GateOutcome::measured(fallow_output::GateStatus::Fail, true, 85.0, 90.0),
        );
        let envelope = serde_json::json!({ "gate_outcomes": gates });

        assert_eq!(gate_rows(&envelope), gate_rows_for_gates(Some(&gates)));
    }
}
