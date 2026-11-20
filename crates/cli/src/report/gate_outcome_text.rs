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

use fallow_output::GateVerdict as GateLine;
use serde_json::Value;

/// How many files a gate line names before it says "and N more", so a run with
/// many broken files keeps a one-line summary.
const MAX_NAMED_FILES: usize = 5;

/// The CLI text of one gate verdict. The facts and the number rule come from
/// [`fallow_output::GateVerdict`], so the MCP result shows the same numbers.
trait GateText {
    fn measured_clause(&self) -> String;
    fn described(&self) -> String;
    fn files_clause(&self) -> String;
    fn row_observed(&self) -> String;
    fn row_threshold(&self) -> Option<String>;
}

impl GateText for GateLine {
    /// The trailing `(85 against 90)` clause, when the gate compared something.
    fn measured_clause(&self) -> String {
        match self.observed_text() {
            Some(_) => format!(" ({})", self.row_observed()),
            None => String::new(),
        }
    }

    fn described(&self) -> String {
        if self.files.is_empty() {
            format!("{}{}", self.name, self.measured_clause())
        } else {
            format!("{} ({})", self.name, self.files_clause())
        }
    }

    /// `src/a.ts: 1 parser error, the parser recovered; and 2 more`.
    fn files_clause(&self) -> String {
        let mut named: Vec<String> = self
            .files
            .iter()
            .take(MAX_NAMED_FILES)
            .map(|file| match &file.parse {
                Some(parse) => {
                    format!("{}: {}", file.path, crate::gates::parse_error_reason(parse))
                }
                None => file.path.clone(),
            })
            .collect();
        let rest = self.files.len().saturating_sub(MAX_NAMED_FILES);
        if rest > 0 {
            named.push(format!("and {rest} more"));
        }
        named.join("; ")
    }

    /// The `observed` display text for a decision-surface row: the numbers the
    /// gate compared when it compared any, and the status word otherwise.
    ///
    /// A row with an empty `observed` renders as a bare label in the check run
    /// and as `Fallow / <id>: ` in a split commit status, so a gate that
    /// measured nothing says what it concluded instead.
    fn row_observed(&self) -> String {
        let Some(observed) = self.observed_text() else {
            return self.status.clone();
        };
        match (
            self.threshold_number_text(),
            self.threshold_label.as_deref(),
        ) {
            (Some(threshold), _) => format!("{observed} against {threshold}"),
            (None, Some(label)) => format!("{observed} at or above {label}"),
            (None, None) => observed,
        }
    }

    /// The `threshold` display text, `None` when the gate has no threshold to
    /// show.
    fn row_threshold(&self) -> Option<String> {
        self.threshold_number_text()
            .or_else(|| self.threshold_label.clone())
    }
}

/// Read `gate_outcomes` off an envelope root, in wire order.
///
/// Returns an empty vector when the run armed no gate, which is every run
/// produced before this object existed and every run that asked for nothing.
fn read_gate_outcomes(envelope: &Value) -> Vec<GateLine> {
    fallow_output::read_gate_verdicts(envelope.get("gate_outcomes"))
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
/// Reads as an inventory ("Gates: failed X; passed Y.") rather than as
/// a verdict of its own. The pull-request comment already carries a
/// check-run-derived heading, and a line starting "Gates failed" sat under a
/// "Quality gate passed" heading as a flat contradiction; naming the outcomes
/// instead lets both be true at once, because they answer different questions.
///
/// Informational on purpose. Whether a tripped gate should fail the build is
/// the consumer's decision, not this renderer's: `enforced` describes the
/// CLI's own exit code, and the GitHub Action deliberately does not pass
/// `--fail-on-issues` to the CLI, so the action can fail a job on a gate that
/// the CLI did not enforce. The line therefore says only what each gate
/// concluded, never whether it fails the run; the job status says that.
pub fn summary_line(envelope: &Value) -> Option<String> {
    let gates = read_gate_outcomes(envelope);
    if gates.is_empty() {
        return None;
    }
    let parts = partition(&gates);
    let mut clauses: Vec<String> = Vec::new();
    if !parts.failed.is_empty() {
        clauses.push(format!("failed {}", join(&parts.failed)));
    }
    if !parts.warned.is_empty() {
        clauses.push(format!("warned {}", join(&parts.warned)));
    }
    if !parts.skipped.is_empty() {
        clauses.push(format!("stood down {}", join(&parts.skipped)));
    }
    if !parts.passed.is_empty() {
        // A passed gate names no measured value: "0 at or above error" says
        // nothing that "passed" does not.
        let names: Vec<&str> = parts.passed.iter().map(|gate| gate.name.as_str()).collect();
        clauses.push(format!("passed {}", names.join(", ")));
    }
    Some(format!("Gates: {}.", clauses.join("; ")))
}

/// Whether the sticky comment of the same envelope says "Quality gate failed":
/// an enforced gate failed, or a findings gate failed because a finding has the
/// `error` severity. An unenforced gate of another kind, for example a stale
/// baseline, leaves the verdict at "Review needed", as in the comment.
pub fn verdict_gate_failed(envelope: &Value) -> bool {
    const FINDINGS_GATES: [&str; 2] = ["error-severity-findings", "health-findings"];
    envelope
        .get("gate_outcomes")
        .and_then(Value::as_object)
        .is_some_and(|gates| {
            gates.iter().any(|(name, gate)| {
                gate.get("status").and_then(Value::as_str) == Some("fail")
                    && (gate.get("enforced").and_then(Value::as_bool) == Some(true)
                        || FINDINGS_GATES.contains(&name.as_str()))
            })
        })
}

/// Gates that print their own stderr line in every mode, also under
/// `--quiet`. The exit-reason line does not repeat them.
const GATES_WITH_OWN_LINE: [&str; 3] = ["parse-error", "stale-baseline", "baseline-growth"];

/// The stderr line that says which enforced gates made the run exit with
/// `code`, or `None` when the run passed or when every failed gate already
/// printed its own line.
///
/// Exit 1 has one meaning on the analysis commands: an enforced gate failed.
/// A load warning or a workspace diagnostic never sets it. A machine format and
/// `--quiet` show no report, so without this line a CI log shows exit 1 and no
/// cause.
///
/// `fail_on_issues` is true under `--fail-on-issues` and `--ci`. Those flags
/// promote a `warn` rule to `error`, so the line then names the flag instead of
/// the advice to set a rule to `warn`.
///
/// `own_lines` names the gates that printed their own failure line in this
/// run, for example the regression outcome when the run is not quiet. The line
/// does not repeat them.
pub fn exit_reason_line(
    gates: Option<&fallow_output::GateOutcomes>,
    code: u8,
    fail_on_issues: bool,
    own_lines: &[fallow_output::GateName],
) -> Option<String> {
    if code == 0 {
        return None;
    }
    let envelope = serde_json::json!({ "gate_outcomes": gates? });
    let failed: Vec<GateLine> = read_gate_outcomes(&envelope)
        .into_iter()
        .filter(|gate| {
            let name = gate.name.as_str();
            gate.status == "fail"
                && gate.enforced
                && !GATES_WITH_OWN_LINE.contains(&name)
                && !own_lines.iter().any(|own| own.as_str() == name)
        })
        .collect();
    if failed.is_empty() {
        return None;
    }
    let refs: Vec<&GateLine> = failed.iter().collect();
    let noun = if refs.len() == 1 { "gate" } else { "gates" };
    let mut line = format!("Exit code {code}: {noun} {} failed.", join(&refs));
    if fail_on_issues {
        if failed.iter().any(|gate| exit_hint(&gate.name).is_some()) {
            line.push(' ');
            line.push_str(FAIL_ON_ISSUES_HINT);
        }
        return Some(line);
    }
    for hint in failed.iter().filter_map(|gate| exit_hint(&gate.name)) {
        line.push(' ');
        line.push_str(hint);
    }
    Some(line)
}

/// The stderr line of a bare machine run that exits 0 although a findings gate
/// failed, or `None` when no such gate failed.
///
/// Without `--fail-on-issues`, bare `fallow` in a machine format publishes the
/// findings gates with `enforced: false` and exits 0, while `fallow dead-code`
/// exits 1 on the same findings. Without this line, the exit code looks like a
/// pass (#3826). `format` is the `--format` spelling, so the line names the
/// run the way the user typed it.
pub fn unenforced_gates_line(
    gates: Option<&fallow_output::GateOutcomes>,
    format: &str,
) -> Option<String> {
    let envelope = serde_json::json!({ "gate_outcomes": gates? });
    let failed: Vec<GateLine> = read_gate_outcomes(&envelope)
        .into_iter()
        .filter(|gate| {
            gate.status == "fail"
                && !gate.enforced
                && crate::gates::FAIL_ON_ISSUES_GATES
                    .iter()
                    .any(|name| name.as_str() == gate.name)
        })
        .collect();
    if failed.is_empty() {
        return None;
    }
    let refs: Vec<&GateLine> = failed.iter().collect();
    let (noun, pronoun) = if refs.len() == 1 {
        ("gate", "it")
    } else {
        ("gates", "them")
    };
    Some(format!(
        "Exit code 0: {noun} {} failed, but bare fallow does not enforce {pronoun} in \
         --format {format}. To exit 1 on {pronoun}, pass --fail-on-issues.",
        join(&refs)
    ))
}

/// The hint for a findings gate under `--fail-on-issues` or `--ci`. A rule
/// set to `warn` does not help there, because the flag promotes it to `error`.
const FAIL_ON_ISSUES_HINT: &str = "Under --fail-on-issues (also set by --ci), a rule set to \
     \"warn\" also fails the run. To report findings without a failure, remove the flag and set \
     the rules to \"warn\".";

/// How to keep the findings of a default exit rule without a failed run.
fn exit_hint(name: &str) -> Option<&'static str> {
    match name {
        "health-findings" => Some(
            "To report complexity findings without a failure, set the rules \
             complexity-cyclomatic, complexity-cognitive, complexity-crap and \
             complexity-file-lines to \"warn\", or run `fallow health --report-only`.",
        ),
        "error-severity-findings" => {
            Some("To report a finding type without a failure, set its rule to \"warn\".")
        }
        _ => None,
    }
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
            observed: gate.row_observed(),
            threshold: gate.row_threshold(),
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
        "baseline-growth" => "Baseline growth",
        "duplication-threshold" => "Duplication threshold",
        "duplication-findings" => "Duplication findings",
        "health-min-score" => "Health minimum score",
        "health-min-severity" => "Health minimum severity",
        "health-findings" => "Health findings",
        "health-coverage-gaps" => "Coverage gaps",
        "health-runtime-coverage" => "Runtime coverage",
        "security" => "Security",
        "security-advisory" => "Security advisory",
        "audit-verdict" => "Audit verdict",
        "type-aware-require" => "Type-aware completeness",
        "type-aware-fallback" => "Type-aware sidecar not found",
        "parse-error" => "Parse errors",
        "mutation-min-score" => "Mutation score",
        _ => return None,
    })
}

/// `some-future-gate` -> `Some future gate`.
fn sentence_case(name: &str) -> String {
    let spaced = name.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
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
            "Gates: failed health-min-score (85 against 90)."
        );
    }

    #[test]
    fn a_failing_gate_just_over_its_threshold_never_reads_as_equal_to_it() {
        let value = envelope(&serde_json::json!({
            "duplication-threshold": {
                "status": "fail", "enforced": true, "observed": 5.004, "threshold": 5.0
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates: failed duplication-threshold (5.004 against 5)."
        );
    }

    /// The render never escalates. `enforced` is the CLI's statement about its
    /// own exit code, and the integrations deliberately do not pass
    /// `--fail-on-issues` to the CLI, so an error-level annotation here would
    /// paint a red line on a job the repository configured to pass.
    #[test]
    fn a_file_judging_gate_names_each_file_and_caps_the_list() {
        let files: Vec<Value> = (0..7)
            .map(|index| {
                serde_json::json!({
                    "path": format!("src/f{index}.ts"),
                    "error_count": 1,
                    "panicked": false
                })
            })
            .collect();
        let value = envelope(&serde_json::json!({
            "parse-error": {
                "status": "fail", "enforced": true, "observed": 7.0, "files": files
            }
        }));
        let line = summary_line(&value).expect("armed gate renders");
        assert_eq!(
            line,
            "Gates: failed parse-error (src/f0.ts: 1 parser error, the parser recovered; \
             src/f1.ts: 1 parser error, the parser recovered; \
             src/f2.ts: 1 parser error, the parser recovered; \
             src/f3.ts: 1 parser error, the parser recovered; \
             src/f4.ts: 1 parser error, the parser recovered; and 2 more)."
        );
    }

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

    /// The line never says whether a gate fails the run: the GitHub Action can
    /// fail a job on a gate that the CLI did not enforce, so only the job
    /// status can state that.
    #[test]
    fn an_unenforced_failure_reads_like_an_enforced_one() {
        let unenforced = envelope(&serde_json::json!({
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        let enforced = envelope(&serde_json::json!({
            "stale-baseline": { "status": "fail", "enforced": true }
        }));
        assert_eq!(
            summary_line(&unenforced).expect("a gate ran"),
            "Gates: failed stale-baseline."
        );
        assert_eq!(summary_line(&unenforced), summary_line(&enforced));
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
            "Gates: stood down stale-baseline."
        );
    }

    #[test]
    fn a_warn_tier_is_not_a_pass() {
        let value = envelope(&serde_json::json!({
            "audit-verdict": { "status": "warn", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates: warned audit-verdict."
        );
    }

    #[test]
    fn passing_gates_are_named_on_their_own() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "pass", "enforced": true }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates: passed regression."
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
            "Gates: failed regression; warned audit-verdict; \
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
            "Gates: failed health-min-severity (3 at or above critical)."
        );
    }

    #[test]
    fn an_unrecognised_gate_name_still_reports() {
        let value = envelope(&serde_json::json!({
            "some-future-gate": { "status": "fail", "enforced": true }
        }));
        assert_eq!(
            annotation_line(&value).expect("a gate ran"),
            "::notice::Fallow: Gates: failed some-future-gate."
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
            "Gates: warned some-future-gate."
        );
    }

    #[test]
    fn a_mix_of_enforced_and_unenforced_failures_adds_no_note() {
        let value = envelope(&serde_json::json!({
            "regression": { "status": "fail", "enforced": true },
            "stale-baseline": { "status": "fail", "enforced": false }
        }));
        assert_eq!(
            summary_line(&value).expect("gates ran"),
            "Gates: failed regression, stale-baseline."
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

    #[test]
    fn the_mutation_score_gate_row_reads_mutation_score() {
        let value = envelope(&serde_json::json!({
            "mutation-min-score": {
                "status": "fail", "enforced": true, "observed": 62.5, "threshold": 80.0
            }
        }));

        let rows = gate_rows(&value);

        assert_eq!(rows[0].id, "mutation-min-score");
        assert_eq!(rows[0].label, "Mutation score");
        assert_eq!(rows[0].observed, "62.5 against 80");
        assert_eq!(rows[0].threshold.as_deref(), Some("80"));
    }

    #[test]
    fn the_mutation_score_shows_one_decimal_rounded_down() {
        // 2 of 3 mutants killed, and a score just below the minimum.
        let value = envelope(&serde_json::json!({
            "mutation-min-score": {
                "status": "fail", "enforced": true,
                "observed": 200.0 / 3.0, "threshold": 80.0
            },
            "health-findings": {
                "status": "fail", "enforced": true, "observed": 2.5, "threshold": 1.0
            }
        }));
        let near = envelope(&serde_json::json!({
            "mutation-min-score": {
                "status": "fail", "enforced": true, "observed": 79.96, "threshold": 80.0
            }
        }));

        let rows = gate_rows(&value);
        let observed = |id: &str| {
            rows.iter()
                .find(|row| row.id == id)
                .map(|row| row.observed.clone())
        };

        assert_eq!(
            observed("mutation-min-score").as_deref(),
            Some("66.6 against 80")
        );
        assert_eq!(
            observed("health-findings").as_deref(),
            Some("2.5 against 1")
        );
        assert_eq!(gate_rows(&near)[0].observed, "79.9 against 80");
    }

    /// A measured percentage arrives at full `f64` precision. The line shows
    /// two decimals, the same text that the MCP result shows.
    #[test]
    fn a_measured_percentage_shows_two_decimals() {
        let value = envelope(&serde_json::json!({
            "duplication-threshold": {
                "status": "fail", "enforced": true,
                "observed": 97.826_086_956_521_72, "threshold": 0.1
            }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates: failed duplication-threshold (97.83 against 0.1)."
        );
        assert_eq!(gate_rows(&value)[0].observed, "97.83 against 0.1");
    }

    /// `enforced` is a required member, but an entry from another producer can
    /// lack it. The entry still reports, as not enforced, the same as in the MCP
    /// result.
    #[test]
    fn a_gate_entry_without_enforced_still_reports_as_not_enforced() {
        let value = envelope(&serde_json::json!({
            "health-min-score": { "status": "fail", "observed": 85.0, "threshold": 90.0 }
        }));
        assert_eq!(
            summary_line(&value).expect("a gate ran"),
            "Gates: failed health-min-score (85 against 90)."
        );
        let rows = gate_rows(&value);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, fallow_output::PrDecisionConclusion::Neutral);
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

    fn typed_gates(
        entries: &[(fallow_output::GateName, fallow_output::GateOutcome)],
    ) -> fallow_output::GateOutcomes {
        let mut gates = fallow_output::GateOutcomes::new();
        for (name, outcome) in entries {
            gates.insert(*name, outcome.clone());
        }
        gates
    }

    #[test]
    fn the_exit_reason_names_every_enforced_failure_with_its_numbers() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let gates = typed_gates(&[
            (
                GateName::HealthMinScore,
                GateOutcome::measured(GateStatus::Fail, true, 65.0, 70.0),
            ),
            (
                GateName::HealthFindings,
                crate::gates::health_findings_outcome(3, true),
            ),
            (
                GateName::HealthCoverageGaps,
                GateOutcome::new(GateStatus::Pass, true),
            ),
        ]);
        let line = exit_reason_line(Some(&gates), 1, false, &[]).expect("two gates failed");
        assert!(
            line.starts_with(
                "Exit code 1: gates health-min-score (65 against 70), \
                 health-findings (3 at or above error) failed."
            ),
            "{line}"
        );
        assert!(line.contains("--report-only"), "{line}");
        assert!(!line.contains("health-coverage-gaps"), "{line}");
    }

    #[test]
    fn the_exit_reason_skips_gates_that_print_their_own_line() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let own_lines = typed_gates(&[
            (
                GateName::ParseError,
                GateOutcome::new(GateStatus::Fail, true),
            ),
            (
                GateName::StaleBaseline,
                GateOutcome::new(GateStatus::Fail, true),
            ),
            (
                GateName::BaselineGrowth,
                GateOutcome::new(GateStatus::Fail, true),
            ),
        ]);
        assert!(exit_reason_line(Some(&own_lines), 1, false, &[]).is_none());

        let mixed = typed_gates(&[
            (
                GateName::ParseError,
                GateOutcome::new(GateStatus::Fail, true),
            ),
            (
                GateName::ErrorSeverityFindings,
                GateOutcome::new(GateStatus::Fail, true),
            ),
        ]);
        let line = exit_reason_line(Some(&mixed), 1, false, &[]).expect("one gate has no own line");
        assert!(
            line.starts_with("Exit code 1: gate error-severity-findings failed."),
            "{line}"
        );
        assert!(!line.contains("parse-error"), "{line}");
    }

    #[test]
    fn a_passing_run_or_an_unenforced_failure_has_no_exit_reason() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let unenforced = typed_gates(&[(
            GateName::HealthFindings,
            GateOutcome::new(GateStatus::Fail, false),
        )]);
        assert!(exit_reason_line(Some(&unenforced), 0, false, &[]).is_none());
        assert!(exit_reason_line(Some(&unenforced), 1, false, &[]).is_none());
        assert!(exit_reason_line(None, 1, false, &[]).is_none());
    }

    #[test]
    fn under_fail_on_issues_the_exit_reason_names_the_flag_not_the_warn_advice() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let gates = typed_gates(&[
            (
                GateName::ErrorSeverityFindings,
                GateOutcome::new(GateStatus::Fail, true),
            ),
            (
                GateName::HealthFindings,
                crate::gates::health_findings_outcome(2, true),
            ),
        ]);
        let line = exit_reason_line(Some(&gates), 1, true, &[]).expect("two gates failed");
        assert!(line.contains("--fail-on-issues"), "{line}");
        assert!(!line.contains("set its rule"), "{line}");
        assert!(!line.contains("--report-only"), "{line}");
        assert_eq!(line.matches("--fail-on-issues").count(), 1, "{line}");
        assert!(line.contains("remove the flag"), "{line}");
    }

    #[test]
    fn the_exit_reason_skips_gates_that_printed_their_own_line_in_this_run() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let gates = typed_gates(&[
            (
                GateName::Regression,
                GateOutcome::measured(GateStatus::Fail, true, 4.0, 0.0),
            ),
            (
                GateName::ErrorSeverityFindings,
                crate::gates::error_severity_outcome(2, true),
            ),
        ]);
        let line = exit_reason_line(Some(&gates), 1, false, &[GateName::Regression])
            .expect("one gate has no own line");
        assert!(
            line.starts_with(
                "Exit code 1: gate error-severity-findings (2 at or above error) failed."
            ),
            "{line}"
        );
        assert!(!line.contains("regression"), "{line}");
        let only_regression = typed_gates(&[(
            GateName::Regression,
            GateOutcome::measured(GateStatus::Fail, true, 4.0, 0.0),
        )]);
        assert!(
            exit_reason_line(Some(&only_regression), 1, false, &[GateName::Regression]).is_none()
        );
    }

    #[test]
    fn the_unenforced_line_names_only_failing_findings_gates_that_are_not_enforced() {
        use fallow_output::{GateName, GateOutcome, GateStatus};
        let gates = typed_gates(&[
            (
                GateName::ErrorSeverityFindings,
                crate::gates::error_severity_outcome(2, false),
            ),
            (
                GateName::HealthFindings,
                crate::gates::health_findings_outcome(0, false),
            ),
            (
                GateName::StaleBaseline,
                GateOutcome::measured(GateStatus::Fail, false, 1.0, 0.0),
            ),
        ]);
        assert_eq!(
            unenforced_gates_line(Some(&gates), "sarif").as_deref(),
            Some(
                "Exit code 0: gate error-severity-findings (2 at or above error) failed, but \
                 bare fallow does not enforce it in --format sarif. To exit 1 on it, pass \
                 --fail-on-issues."
            )
        );

        let both = typed_gates(&[
            (
                GateName::ErrorSeverityFindings,
                crate::gates::error_severity_outcome(2, false),
            ),
            (
                GateName::HealthFindings,
                crate::gates::health_findings_outcome(3, false),
            ),
        ]);
        let line = unenforced_gates_line(Some(&both), "json").expect("two gates failed");
        assert!(line.contains("gates error-severity-findings"), "{line}");
        assert!(line.contains("enforce them"), "{line}");

        let enforced = typed_gates(&[(
            GateName::ErrorSeverityFindings,
            crate::gates::error_severity_outcome(2, true),
        )]);
        assert!(unenforced_gates_line(Some(&enforced), "json").is_none());
        assert!(unenforced_gates_line(None, "json").is_none());
    }
}
