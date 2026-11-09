//! Normalized facts. Each surface has its own wire format, so the oracle
//! compares the facts in it: the findings with their paths, lines, columns
//! and ids, the clone groups and families in report order, and the trace
//! verdicts. Volatile values such as timings and run ids are not facts.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// The top-level arrays of a dead-code report that do not hold findings.
const NOT_FINDINGS: [&str; 6] = [
    "entry_points",
    "workspace_diagnostics",
    "next_steps",
    "gate_outcomes",
    "request_outcomes",
    "summary",
];

/// The fields that name the subject of a finding.
const SUBJECT_FIELDS: [&str; 6] = [
    "export_name",
    "package_name",
    "member_name",
    "parent_name",
    "specifier",
    "location",
];

/// One dead-code finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub category: String,
    pub path: String,
    pub subject: String,
    /// One-based line.
    pub line: Option<u64>,
    /// Zero-based column.
    pub col: Option<u64>,
    /// The `finding_id` field.
    pub id: Option<String>,
}

/// The findings of a dead-code report in the JSON of the CLI, MCP or the
/// Node bindings.
pub fn dead_code(report: &Value) -> BTreeSet<Finding> {
    let object = report.as_object().expect("a dead-code report is an object");
    let mut findings = BTreeSet::new();
    for (category, value) in object {
        if NOT_FINDINGS.contains(&category.as_str()) || category.starts_with('_') {
            continue;
        }
        let Some(items) = value.as_array() else {
            continue;
        };
        for item in items.iter().filter(|item| item.is_object()) {
            let subject = SUBJECT_FIELDS
                .iter()
                .filter_map(|field| item.get(*field).and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            findings.insert(Finding {
                category: category.clone(),
                path: string(item, "path"),
                subject,
                line: item.get("line").and_then(Value::as_u64),
                col: item.get("col").and_then(Value::as_u64),
                id: item
                    .get("finding_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
    }
    findings
}

/// The manifests in which the report flags `package` as unused.
pub fn flagged_manifests(findings: &BTreeSet<Finding>, package: &str) -> BTreeSet<String> {
    findings
        .iter()
        .filter(|finding| {
            finding.category.starts_with("unused_")
                && finding.category.ends_with("dependencies")
                && finding.subject.split(' ').next() == Some(package)
        })
        .map(|finding| finding.path.clone())
        .collect()
}

/// One editor diagnostic that carries a finding id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Diagnostic {
    pub finding_id: String,
    pub path: String,
    /// Zero-based line.
    pub line: u64,
    /// Zero-based UTF-16 column. The fixtures are ASCII, so it equals the
    /// byte column.
    pub character: u64,
}

/// The diagnostics that the report findings must give in an editor: one per
/// finding with an id, at the line and column of the finding, or at the
/// start of the file for a finding without a position.
pub fn expected_diagnostics(findings: &BTreeSet<Finding>) -> BTreeSet<Diagnostic> {
    findings
        .iter()
        .filter_map(|finding| {
            Some(Diagnostic {
                finding_id: finding.id.clone()?,
                path: finding.path.clone(),
                line: finding.line.map_or(0, |line| line.saturating_sub(1)),
                character: finding.col.unwrap_or(0),
            })
        })
        .collect()
}

/// One clone instance: file, start line, end line, start column, end column.
pub type Instance = (String, u64, u64, u64, u64);

/// One clone group in report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub instances: Vec<Instance>,
    pub line_count: u64,
    pub token_count: u64,
}

/// One clone family in report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    pub files: Vec<String>,
    pub groups: Vec<Group>,
    pub suggestions: Vec<String>,
}

/// The clone groups and families of a duplication report, in report order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplication {
    pub groups: Vec<Group>,
    pub families: Vec<Family>,
}

/// The facts of a duplication report: `dupes`, `find_dupes`, the
/// `duplication` section of `audit`, or the Node bindings.
pub fn duplication(report: &Value) -> Duplication {
    Duplication {
        groups: array(report, "clone_groups").iter().map(group).collect(),
        families: array(report, "clone_families")
            .iter()
            .map(|family| Family {
                files: array(family, "files")
                    .iter()
                    .map(|file| file.as_str().expect("family file").to_owned())
                    .collect(),
                groups: array(family, "groups").iter().map(group).collect(),
                suggestions: array(family, "suggestions")
                    .iter()
                    .map(|suggestion| string(suggestion, "description"))
                    .collect(),
            })
            .collect(),
    }
}

fn group(value: &Value) -> Group {
    Group {
        instances: array(value, "instances")
            .iter()
            .map(|instance| {
                (
                    string(instance, "file"),
                    number(instance, "start_line"),
                    number(instance, "end_line"),
                    number(instance, "start_col"),
                    number(instance, "end_col"),
                )
            })
            .collect(),
        line_count: number(value, "line_count"),
        token_count: number(value, "token_count"),
    }
}

/// A trace answer without the fields that name the wire format or the run.
pub fn trace(answer: &Value) -> BTreeMap<String, Value> {
    answer
        .as_object()
        .expect("a trace answer is an object")
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "kind" | "_meta" | "elapsed_ms"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}
