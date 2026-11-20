//! The facts of a run verdict, read off an untyped envelope member.
//!
//! The CLI report renderers and the MCP server state the same verdict in
//! different sentences. Some wording differences are deliberate, so each
//! surface keeps its own sentences. The facts under the sentences are the same
//! on each surface: which gates an envelope reports, whether a gate is
//! enforced, how a gate number reads as text, and which requests applied over
//! an empty scope. This module owns those facts, so a fix of one fact reaches
//! every surface.
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
    #[must_use]
    pub fn observed_text(&self) -> Option<String> {
        let observed = self.observed?;
        if self.name == GateName::MutationMinScore.as_str() {
            return Some(gate_number_text(
                observed.mul_add(10.0, 1e-9).floor() / 10.0,
            ));
        }
        Some(gate_number_text(observed))
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
    if value.fract() == 0.0 {
        return format!("{value:.0}");
    }
    let rounded = format!("{value:.2}");
    let trimmed = rounded.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        return "0".to_owned();
    }
    trimmed.to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

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
