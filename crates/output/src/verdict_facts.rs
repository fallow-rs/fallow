//! The facts of a run verdict, read off an untyped envelope member.
//!
//! The CLI report renderers and the MCP server state the same verdict in
//! different sentences. Some wording differences are deliberate, so each
//! surface keeps its own sentences. The facts under the sentences are the same
//! on each surface: which gates an envelope reports, whether a gate is
//! enforced, how a gate number reads as text, which requests applied over an
//! empty scope, and what a loaded baseline matched. This module owns those
//! facts, so a fix of one fact reaches every surface.
//!
//! Each reader takes the raw `serde_json::Value` of one member, because
//! `fallow report --from` and the MCP subprocess route hold the envelope
//! untyped. The readers tolerate names and statuses that this build does not
//! recognise, the same open-set contract that the member types document.

use serde_json::Value;

use crate::{GateFile, GateFileReason, GateName, RequestName};

/// One entry of `gate_outcomes`, read off a saved or live envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct GateVerdict {
    /// The gate name as the envelope spells it. The set is open.
    pub name: String,
    /// The status as the envelope spells it. The set is open.
    pub status: String,
    /// Whether a `fail` of this gate made the run exit non-zero. An entry
    /// without the member reads as not enforced: the schema requires the
    /// member, so its absence gives no evidence that the gate changed the exit
    /// code, and the entry still reports its status.
    pub enforced: bool,
    /// The measured value that the gate compared.
    pub observed: Option<f64>,
    /// The limit that the gate compared `observed` against.
    pub threshold: Option<f64>,
    /// How the limit was spelled, for a gate whose number has no unit.
    pub threshold_label: Option<String>,
    /// The files that a file-judging gate named, in wire order.
    pub files: Vec<GateVerdictFile>,
}

/// One item of the `files` of a gate entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateVerdictFile {
    /// The file path, relative to the project root.
    pub path: String,
    /// The parser facts of the file, or `None` when the item does not carry
    /// them, for example an item from a newer file-judging gate.
    pub parse: Option<GateFile>,
}

impl GateVerdict {
    /// The observed number as display text, or `None` when the gate measured
    /// nothing.
    ///
    /// The mutation score shows one decimal, rounded down, so a score just
    /// below the minimum never shows as equal to it. Every other number uses
    /// [`gate_number_text`].
    ///
    /// An observed number that differs from the threshold never shows as equal
    /// to it. When the rounded text of the two numbers is the same, for
    /// example `5.004` against `5`, the text shows more decimals until the two
    /// differ.
    #[must_use]
    pub fn observed_text(&self) -> Option<String> {
        let observed = self.observed?;
        let text = if self.name == GateName::MutationMinScore.as_str() {
            gate_number_text(observed.mul_add(10.0, 1e-9).floor() / 10.0)
        } else {
            gate_number_text(observed)
        };
        let Some(threshold) = self.threshold else {
            return Some(text);
        };
        // Exact equality, not a tolerance: only the same number may show the
        // same text as the threshold.
        let same_number = observed.partial_cmp(&threshold) == Some(std::cmp::Ordering::Equal);
        if same_number || text != gate_number_text(threshold) {
            return Some(text);
        }
        Some(distinct_number_text(observed, &text))
    }

    /// The threshold number as display text, or `None` when the gate has no
    /// threshold number.
    #[must_use]
    pub fn threshold_number_text(&self) -> Option<String> {
        self.threshold.map(gate_number_text)
    }
}

/// A gate number as display text: a whole number without a fraction, and any
/// other number rounded to two decimals without trailing zeros.
///
/// A measured percentage arrives at full `f64` precision, for example
/// `97.82608695652172`. Two decimals carry the verdict, and the envelope keeps
/// every digit for a consumer that computes on it.
#[must_use]
pub fn gate_number_text(value: f64) -> String {
    rounded_number_text(value, 2)
}

/// `value` rounded to `decimals`, without trailing zeros, and with a negative
/// zero shown as `0`.
fn rounded_number_text(value: f64, decimals: usize) -> String {
    if value.fract() == 0.0 {
        // Adding zero turns a negative zero into a positive zero.
        return format!("{:.0}", value + 0.0);
    }
    let rounded = format!("{value:.decimals$}");
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        return "0".to_owned();
    }
    trimmed.to_owned()
}

/// The most decimals that [`distinct_number_text`] tries before it falls back
/// to the shortest text that reads back as the same `f64`.
const MAX_DISTINCT_DECIMALS: usize = 15;

/// `value` with the fewest decimals (three or more) whose text differs from
/// `equal_text`, without trailing zeros.
fn distinct_number_text(value: f64, equal_text: &str) -> String {
    for decimals in 3..=MAX_DISTINCT_DECIMALS {
        let text = rounded_number_text(value, decimals);
        if text != equal_text {
            return text;
        }
    }
    format!("{value}")
}

/// Read the entries of a `gate_outcomes` member, in wire order.
///
/// Returns an empty vector when the member is absent or is not an object. An
/// entry without a string `status` is skipped, because it states no verdict.
#[must_use]
pub fn read_gate_verdicts(gate_outcomes: Option<&Value>) -> Vec<GateVerdict> {
    let Some(map) = gate_outcomes.and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            Some(GateVerdict {
                name: name.clone(),
                status: entry.get("status")?.as_str()?.to_owned(),
                enforced: entry
                    .get("enforced")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                observed: entry.get("observed").and_then(Value::as_f64),
                threshold: entry.get("threshold").and_then(Value::as_f64),
                threshold_label: entry
                    .get("threshold_label")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                files: read_gate_files(entry),
            })
        })
        .collect()
}

/// Read the `files` of one gate entry. An item without a `path` is skipped.
fn read_gate_files(entry: &Value) -> Vec<GateVerdictFile> {
    let Some(items) = entry.get("files").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let path = item.get("path")?.as_str()?.to_owned();
            let error_count = item
                .get("error_count")
                .and_then(Value::as_u64)
                .and_then(|count| u32::try_from(count).ok());
            let panicked = item.get("panicked").and_then(Value::as_bool);
            let reason = (item.get("reason").and_then(Value::as_str) == Some("nesting-too-deep"))
                .then_some(GateFileReason::NestingTooDeep);
            let parse = match (error_count, panicked) {
                (Some(error_count), Some(panicked)) => Some(GateFile {
                    path: path.clone(),
                    error_count,
                    panicked,
                    reason,
                }),
                _ => None,
            };
            Some(GateVerdictFile { path, parse })
        })
        .collect()
}

/// One entry of `request_outcomes`, read off a saved or live envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestVerdict {
    /// The request name as the envelope spells it. The set is open.
    pub name: String,
    /// The status as the envelope spells it. The set is open.
    pub status: String,
    /// What the request governs, as the envelope spells it.
    pub affects: Option<String>,
    /// Why the request did not apply.
    pub reason: Option<String>,
    /// The size of the scope that an applied request measured.
    pub scope_size: Option<u64>,
}

impl RequestVerdict {
    /// Whether the request applied. A status that this build does not
    /// recognise is never read as applied.
    #[must_use]
    pub fn applied(&self) -> bool {
        self.status == "applied"
    }

    /// Whether the entry describes the scope of the report rather than a file
    /// written beside it. A missing or unrecognised class is not read as
    /// `scope`.
    #[must_use]
    pub fn affects_scope(&self) -> bool {
        self.affects.as_deref() == Some("scope")
    }

    /// Whether an unapplied entry means that a requested file was not written.
    #[must_use]
    pub fn withholds_artifact(&self) -> bool {
        self.affects.as_deref() == Some("artifact")
    }

    /// Whether the request narrowed the run to a scope that it measured as
    /// empty. An entry without `scope_size` says nothing here.
    #[must_use]
    pub fn applied_over_empty_scope(&self) -> bool {
        self.applied() && self.affects_scope() && self.scope_size == Some(0)
    }

    /// Whether the request scopes the run to a change (`changed-since` or
    /// `diff-filter`). Only a change scope has a diff or a base ref to check
    /// when the scope is empty.
    #[must_use]
    pub fn is_change_scope(&self) -> bool {
        CHANGE_SCOPES
            .iter()
            .any(|scope| scope.as_str() == self.name)
    }
}

/// The requests that scope the run to a change.
const CHANGE_SCOPES: [RequestName; 2] = [RequestName::ChangedSince, RequestName::DiffFilter];

/// The requests that applied over an empty scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyScope {
    /// The request names, in wire order.
    pub names: Vec<String>,
    /// Whether every request is a change scope, so the advice to check the
    /// diff or the base ref applies.
    pub change_scope_only: bool,
}

impl EmptyScope {
    /// The empty-scope fact of a set of requests, or `None` when no request
    /// applied over an empty scope.
    #[must_use]
    pub fn of(requests: &[RequestVerdict]) -> Option<Self> {
        let empty: Vec<&RequestVerdict> = requests
            .iter()
            .filter(|request| request.applied_over_empty_scope())
            .collect();
        if empty.is_empty() {
            return None;
        }
        Some(Self {
            change_scope_only: empty.iter().all(|request| request.is_change_scope()),
            names: empty.iter().map(|request| request.name.clone()).collect(),
        })
    }
}

/// Read the entries of a `request_outcomes` member, in wire order.
///
/// Returns an empty vector when the member is absent or is not an object. An
/// entry without a string `status` is skipped.
#[must_use]
pub fn read_request_verdicts(request_outcomes: Option<&Value>) -> Vec<RequestVerdict> {
    let Some(map) = request_outcomes.and_then(Value::as_object) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, entry)| {
            let text = |field: &str| entry.get(field).and_then(Value::as_str).map(str::to_owned);
            Some(RequestVerdict {
                name: name.clone(),
                status: entry.get("status")?.as_str()?.to_owned(),
                affects: text("affects"),
                reason: text("reason"),
                scope_size: entry.get("scope_size").and_then(Value::as_u64),
            })
        })
        .collect()
}

/// One `baseline_staleness` object, read off a saved or live envelope.
///
/// A multi-section envelope carries one object per analysis. The caller finds
/// each object with `fallow_types::envelope_sites::baseline_staleness_objects`
/// and reads it with [`read_baseline_verdict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineVerdict {
    /// The advisory as the envelope spells it, or `none` when the member is
    /// absent. The set is open.
    pub warning: String,
    /// Whether the stale-baseline gate rule holds for this baseline.
    pub gate_trips: bool,
    /// The number of entries that the baseline was saved with.
    pub baseline_entries: u64,
    /// The number of saved entries that matched nothing on this run.
    pub stale_entries: u64,
    /// Whether the run analyzed only part of the project.
    pub change_scoped: bool,
    /// Whether the file has no entries that this command recognises.
    pub unrecognised_format: bool,
    /// The command that saved a foreign baseline, when `saved_by` is a plain
    /// kebab-case token. Any other value reads as absent, so a sentence never
    /// quotes unexpected text.
    pub saved_by: Option<String>,
    /// Why the run analyzed only part of the project, in wire order.
    pub scope_reasons: Vec<String>,
}

/// Read one `baseline_staleness` object. An absent or mistyped member reads
/// as its empty value: `none`, `false`, `0` or an empty list.
#[must_use]
pub fn read_baseline_verdict(staleness: &Value) -> BaselineVerdict {
    let flag = |field: &str| {
        staleness
            .get(field)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    let count = |field: &str| staleness.get(field).and_then(Value::as_u64).unwrap_or(0);
    BaselineVerdict {
        warning: staleness
            .get("warning")
            .and_then(Value::as_str)
            .unwrap_or("none")
            .to_owned(),
        gate_trips: flag("gate_trips"),
        baseline_entries: count("baseline_entries"),
        stale_entries: count("stale_entries"),
        change_scoped: flag("change_scoped"),
        unrecognised_format: flag("unrecognised_format"),
        saved_by: staleness
            .get("saved_by")
            .and_then(Value::as_str)
            .filter(|token| is_command_token(token))
            .map(str::to_owned),
        scope_reasons: staleness
            .get("scope_reasons")
            .and_then(Value::as_array)
            .map(|reasons| {
                reasons
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Whether `token` is a plain kebab-case command name.
fn is_command_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_baseline_verdict_reads_each_fact_and_validates_saved_by() {
        let verdict = read_baseline_verdict(&serde_json::json!({
            "warning": "partial",
            "gate_trips": true,
            "baseline_entries": 8,
            "stale_entries": 5,
            "change_scoped": true,
            "unrecognised_format": true,
            "saved_by": "dead-code",
            "scope_reasons": ["changed-since", 3, "workspace"],
        }));
        assert_eq!(
            verdict,
            BaselineVerdict {
                warning: "partial".to_owned(),
                gate_trips: true,
                baseline_entries: 8,
                stale_entries: 5,
                change_scoped: true,
                unrecognised_format: true,
                saved_by: Some("dead-code".to_owned()),
                scope_reasons: vec!["changed-since".to_owned(), "workspace".to_owned()],
            }
        );
        for odd in ["Not A Token", "", "health`", "a_b"] {
            let verdict = read_baseline_verdict(&serde_json::json!({ "saved_by": odd }));
            assert_eq!(verdict.saved_by, None, "{odd}");
        }
        let empty = read_baseline_verdict(&serde_json::json!({}));
        assert_eq!(empty.warning, "none");
        assert!(!empty.gate_trips && !empty.unrecognised_format && !empty.change_scoped);
        assert_eq!((empty.baseline_entries, empty.stale_entries), (0, 0));
        assert!(empty.scope_reasons.is_empty());
    }

    #[test]
    fn a_gate_number_shows_at_most_two_decimals() {
        let cases = [
            (3.0, "3"),
            (0.0, "0"),
            (2.5, "2.5"),
            (0.1, "0.1"),
            (97.826_086_956_521_72, "97.83"),
            (79.98, "79.98"),
            (0.001, "0"),
            (-0.001, "0"),
        ];
        for (value, text) in cases {
            assert_eq!(gate_number_text(value), text, "{value}");
        }
    }

    fn gate(name: &str, observed: f64) -> GateVerdict {
        let member = serde_json::json!({
            name: { "status": "fail", "enforced": true, "observed": observed, "threshold": 80.0 }
        });
        read_gate_verdicts(Some(&member)).remove(0)
    }

    #[test]
    fn the_mutation_score_rounds_down_to_one_decimal() {
        assert_eq!(
            gate("mutation-min-score", 79.98).observed_text().as_deref(),
            Some("79.9")
        );
        assert_eq!(
            gate("mutation-min-score", 200.0 / 3.0)
                .observed_text()
                .as_deref(),
            Some("66.6")
        );
        assert_eq!(
            gate("health-min-score", 79.98).observed_text().as_deref(),
            Some("79.98")
        );
        assert_eq!(
            gate("health-min-score", 79.98)
                .threshold_number_text()
                .as_deref(),
            Some("80")
        );
    }

    #[test]
    fn an_observed_number_never_shows_as_equal_to_a_different_threshold() {
        let observed_against = |name: &str, observed: f64, threshold: f64| {
            let member = serde_json::json!({
                name: { "status": "fail", "enforced": true, "observed": observed, "threshold": threshold }
            });
            read_gate_verdicts(Some(&member)).remove(0).observed_text()
        };
        let cases = [
            ("duplication-threshold", 5.004, 5.0, "5.004"),
            (
                "duplication-threshold",
                5.000_000_000_1,
                5.0,
                "5.0000000001",
            ),
            ("duplication-threshold", 4.9996, 5.0, "4.9996"),
            ("duplication-threshold", 5.0, 5.0, "5"),
            ("duplication-threshold", -0.0, 0.0, "0"),
            ("duplication-threshold", -0.0001, 0.0, "-0.0001"),
            ("duplication-threshold", 5.2, 5.0, "5.2"),
            ("health-min-score", 79.996, 80.0, "79.996"),
            (
                "mutation-min-score",
                79.999_999_999_9,
                80.0,
                "79.9999999999",
            ),
            ("mutation-min-score", 79.98, 80.0, "79.9"),
        ];
        for (name, observed, threshold, text) in cases {
            assert_eq!(
                observed_against(name, observed, threshold).as_deref(),
                Some(text),
                "{name} {observed} against {threshold}"
            );
        }
    }

    #[test]
    fn a_gate_entry_without_enforced_reads_as_not_enforced() {
        let member = serde_json::json!({
            "health-min-score": { "status": "fail" },
            "no-status": { "enforced": true },
        });
        let gates = read_gate_verdicts(Some(&member));
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].name, "health-min-score");
        assert!(!gates[0].enforced);
        assert!(read_gate_verdicts(None).is_empty());
    }

    #[test]
    fn a_gate_file_without_parser_facts_keeps_its_path() {
        let member = serde_json::json!({
            "parse-error": {
                "status": "fail", "enforced": true,
                "files": [
                    { "path": "a.ts", "error_count": 1, "panicked": true,
                      "reason": "nesting-too-deep" },
                    { "path": "b.ts" },
                    { "error_count": 1 },
                ]
            }
        });
        let files = &read_gate_verdicts(Some(&member))[0].files;
        assert_eq!(files.len(), 2);
        assert_eq!(
            files[0].parse.as_ref().and_then(|parse| parse.reason),
            Some(GateFileReason::NestingTooDeep)
        );
        assert_eq!(files[1].path, "b.ts");
        assert!(files[1].parse.is_none());
    }

    fn requests(member: &Value) -> Vec<RequestVerdict> {
        read_request_verdicts(Some(member))
    }

    #[test]
    fn a_request_entry_without_a_string_status_is_skipped() {
        let read = requests(&serde_json::json!({
            "changed-since": { "affects": "scope" },
            "diff-filter": { "status": 3 },
            "group-filter": { "status": "ignored", "reason": "unsupported" },
        }));
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].name, "group-filter");
        assert!(read_request_verdicts(None).is_empty());
    }

    #[test]
    fn the_empty_scope_names_each_request_and_tells_a_change_scope_apart() {
        let change = requests(&serde_json::json!({
            "changed-since": { "status": "applied", "affects": "scope", "scope_size": 0 },
            "diff-filter": { "status": "applied", "affects": "scope", "scope_size": 0 },
            "sarif-file": { "status": "applied", "affects": "artifact", "scope_size": 0 },
        }));
        assert_eq!(
            EmptyScope::of(&change),
            Some(EmptyScope {
                names: vec!["changed-since".to_owned(), "diff-filter".to_owned()],
                change_scope_only: true,
            })
        );

        let group = requests(&serde_json::json!({
            "changed-since": { "status": "applied", "affects": "scope", "scope_size": 0 },
            "group-filter": { "status": "applied", "affects": "scope", "scope_size": 0 },
        }));
        assert_eq!(
            EmptyScope::of(&group).map(|empty| empty.change_scope_only),
            Some(false)
        );

        let not_empty = requests(&serde_json::json!({
            "changed-since": { "status": "applied", "affects": "scope", "scope_size": 2 },
            "diff-filter": { "status": "not-applied", "affects": "scope", "scope_size": 0 },
        }));
        assert_eq!(EmptyScope::of(&not_empty), None);
    }

    #[test]
    fn the_wire_name_of_each_request_matches_its_serialized_name() {
        for name in [
            RequestName::ChangedSince,
            RequestName::DiffFilter,
            RequestName::PackageBaselines,
            RequestName::SarifFile,
            RequestName::GroupFilter,
        ] {
            assert_eq!(
                serde_json::to_value(name).expect("a name serializes"),
                Value::from(name.as_str())
            );
        }
    }
}
