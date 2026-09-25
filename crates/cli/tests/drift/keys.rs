//! Finding identity keys and one normalizer per envelope shape.
//!
//! Every surface reduces its output to a [`KeySet`], so the invariants compare
//! identity and never presentation (actions, columns, prose, timings).

use std::collections::BTreeSet;
use std::fmt;

use serde_json::Value;

/// Identity of one finding: (issue kind, root-relative path, symbol or package, line).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FindingKey {
    pub kind: String,
    pub path: String,
    pub symbol: String,
    pub line: u64,
}

impl fmt::Display for FindingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}:{}", self.kind, self.path, self.line)?;
        if !self.symbol.is_empty() {
            write!(f, " {}", self.symbol)?;
        }
        Ok(())
    }
}

pub type KeySet = BTreeSet<FindingKey>;

/// Top-level arrays of a dead-code envelope that do not hold findings.
const DEAD_CODE_NON_FINDING_ARRAYS: &[&str] = &["workspace_diagnostics", "next_steps"];

/// Fields tried in order for the symbol of a dead-code finding.
const SYMBOL_FIELDS: &[&str] = &[
    "export_name",
    "package_name",
    "member_name",
    "name",
    "specifier",
    "entry_name",
    "catalog_name",
];

/// The issue kind the harness uses for every clone group.
pub const DUPLICATION_KIND: &str = "code-duplication";
/// The issue kind the harness uses for every health finding.
pub const COMPLEXITY_KIND: &str = "complexity";

/// Keys of every finding in a dead-code envelope (CLI `dead-code`, the typed
/// programmatic result, MCP `analyze` and `check_changed`, and the `check`
/// section of a combined report).
pub fn dead_code_keys(envelope: &Value) -> KeySet {
    let mut keys = KeySet::new();
    let Some(map) = envelope.as_object() else {
        return keys;
    };
    for (kind, value) in map {
        if DEAD_CODE_NON_FINDING_ARRAYS.contains(&kind.as_str()) {
            continue;
        }
        let Some(items) = value.as_array() else {
            continue;
        };
        for item in items.iter().filter(|item| item.is_object()) {
            keys.insert(dead_code_key(kind, item));
        }
    }
    keys
}

fn dead_code_key(kind: &str, item: &Value) -> FindingKey {
    let path = item["path"]
        .as_str()
        .map_or_else(|| joined_strings(&item["files"]), str::to_string);
    let symbol = SYMBOL_FIELDS
        .iter()
        .find_map(|field| item[*field].as_str())
        .map(|symbol| match item["parent_name"].as_str() {
            Some(parent) => format!("{parent}.{symbol}"),
            None => symbol.to_string(),
        })
        .or_else(|| suppression_symbol(item))
        .unwrap_or_default();
    FindingKey {
        kind: kind.to_string(),
        path,
        symbol,
        line: item["line"].as_u64().unwrap_or(0),
    }
}

/// The symbol of a stale suppression: the directive and what it suppresses,
/// as in its audit key. Without it, two stale suppressions in one file would
/// share an identity.
fn suppression_symbol(item: &Value) -> Option<String> {
    let origin = item["origin"].as_object()?;
    let target = origin
        .get("issue_kind")
        .or_else(|| origin.get("export_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let scope = if origin.get("is_file_level").and_then(Value::as_bool) == Some(true) {
        "file"
    } else {
        "line"
    };
    let missing_reason = item["missing_reason"].as_bool() == Some(true);
    Some(format!(
        "{}:{scope}:{target}{}",
        origin
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        if missing_reason {
            ":missing-reason"
        } else {
            ""
        }
    ))
}

fn joined_strings(value: &Value) -> String {
    value.as_array().map_or_else(String::new, |items| {
        items
            .iter()
            .filter_map(|item| item.as_str().or_else(|| item["path"].as_str()))
            .collect::<Vec<_>>()
            .join(" -> ")
    })
}

/// Keys of every clone group in a dupes envelope. One key per group: the path
/// joins the instance files with ` -> ` (as for a circular dependency) and the
/// symbol lists each instance with its line range, so a surface that drops a
/// single instance changes the key.
pub fn dupes_keys(envelope: &Value) -> KeySet {
    let mut keys = KeySet::new();
    for group in envelope["clone_groups"].as_array().into_iter().flatten() {
        let mut instances: Vec<(String, u64, u64)> = group["instances"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|instance| {
                (
                    instance["file"].as_str().unwrap_or_default().to_string(),
                    instance["start_line"].as_u64().unwrap_or(0),
                    instance["end_line"].as_u64().unwrap_or(0),
                )
            })
            .collect();
        instances.sort();
        let Some(first) = instances.first() else {
            continue;
        };
        let mut files: Vec<&str> = instances.iter().map(|(file, _, _)| file.as_str()).collect();
        files.dedup();
        keys.insert(FindingKey {
            kind: DUPLICATION_KIND.to_string(),
            path: files.join(" -> "),
            symbol: instances
                .iter()
                .map(|(file, start, end)| format!("{file}:{start}-{end}"))
                .collect::<Vec<_>>()
                .join(" | "),
            line: first.1,
        });
    }
    keys
}

/// Keys of every function finding in a health envelope.
pub fn health_keys(envelope: &Value) -> KeySet {
    envelope["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| FindingKey {
            kind: COMPLEXITY_KIND.to_string(),
            path: finding["path"].as_str().unwrap_or_default().to_string(),
            symbol: finding["name"].as_str().unwrap_or_default().to_string(),
            line: finding["line"].as_u64().unwrap_or(0),
        })
        .collect()
}

/// Keys of the three sections of a combined (bare `fallow`) report.
pub fn combined_keys(envelope: &Value) -> SectionKeys {
    SectionKeys {
        dead_code: dead_code_keys(&envelope["check"]),
        dupes: dupes_keys(&envelope["dupes"]),
        health: health_keys(&envelope["health"]),
    }
}

/// Per-domain key sets of a report that carries several analyses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionKeys {
    pub dead_code: KeySet,
    pub dupes: KeySet,
    pub health: KeySet,
}

impl SectionKeys {
    /// The union of all domains.
    pub fn all(&self) -> KeySet {
        self.dead_code
            .iter()
            .chain(&self.dupes)
            .chain(&self.health)
            .cloned()
            .collect()
    }
}

/// Keys of an audit report, split by attribution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditKeys {
    pub introduced: KeySet,
    pub inherited: KeySet,
    pub verdict: String,
}

/// A function that reduces one envelope shape to keys.
type Normalizer = fn(&Value) -> KeySet;

/// Keys of an audit report (CLI `audit`, MCP `audit`, `fallow_api::run_audit`).
pub fn audit_keys(envelope: &Value) -> AuditKeys {
    let mut keys = AuditKeys {
        verdict: envelope["verdict"].as_str().unwrap_or_default().to_string(),
        ..AuditKeys::default()
    };
    let sections: [(&str, Normalizer); 3] = [
        ("dead_code", dead_code_keys),
        ("duplication", dupes_keys),
        ("complexity", health_keys),
    ];
    for (section, normalize) in sections {
        let Some(map) = envelope[section].as_object() else {
            continue;
        };
        for (field, value) in map {
            let Some(items) = value.as_array() else {
                continue;
            };
            for item in items {
                let mut single = serde_json::Map::new();
                single.insert(field.clone(), Value::Array(vec![item.clone()]));
                let target = if item["introduced"].as_bool() == Some(false) {
                    &mut keys.inherited
                } else {
                    &mut keys.introduced
                };
                target.extend(normalize(&Value::Object(single)));
            }
        }
    }
    keys
}

/// Keys of any machine envelope, dispatched on its `kind` member.
///
/// # Panics
///
/// Panics on an envelope kind the harness has no normalizer for, so a new
/// shape cannot pass as an empty key set.
pub fn envelope_keys(envelope: &Value) -> KeySet {
    match envelope["kind"].as_str() {
        Some("dead-code") => dead_code_keys(envelope),
        Some("dupes") => dupes_keys(envelope),
        Some("health") => health_keys(envelope),
        Some("combined") => combined_keys(envelope).all(),
        Some("audit") => {
            let keys = audit_keys(envelope);
            keys.introduced.union(&keys.inherited).cloned().collect()
        }
        other => panic!("no normalizer for envelope kind {other:?}: {envelope}"),
    }
}

/// Parse the text content of an MCP `tools/call` result into its envelope.
///
/// # Panics
///
/// Panics when the result is an error or carries no JSON text.
pub fn mcp_result_envelope(result: &Value) -> Value {
    assert_ne!(result["isError"], true, "MCP tool call failed: {result}");
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("MCP result has no text content: {result}"));
    serde_json::from_str(text)
        .unwrap_or_else(|err| panic!("MCP result text is not JSON ({err}): {text}"))
}

/// Render a key set, one key per line, for a failure message.
pub fn render(keys: &KeySet) -> String {
    if keys.is_empty() {
        return "    (none)".to_string();
    }
    keys.iter()
        .map(|key| format!("    {key}"))
        .collect::<Vec<_>>()
        .join("\n")
}
