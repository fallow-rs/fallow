//! Input loading for `fallow security survivors`: the candidate report and
//! the verifier verdict file.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_output::SecurityVerifierVerdict;
use fallow_types::results::SecurityFinding;

/// The candidates of one `fallow security --format json` report.
pub struct CandidateInput {
    pub candidates: BTreeMap<String, SecurityFinding>,
    /// Maps each `stable_finding_id` to the `finding_id` of its candidate.
    pub stable_ids: BTreeMap<String, String>,
    pub workspace_diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
}

/// Find the candidate that a verdict id names: a `finding_id` first, then a
/// `stable_finding_id`. Returns the `finding_id` of the candidate, so output
/// keys and duplicate checks do not depend on which id the verdict uses.
pub fn resolve_verdict_candidate<'a>(
    candidates: &'a BTreeMap<String, SecurityFinding>,
    stable_ids: &'a BTreeMap<String, String>,
    verdict_id: &str,
) -> Option<(&'a String, &'a SecurityFinding)> {
    candidates.get_key_value(verdict_id).or_else(|| {
        stable_ids
            .get(verdict_id)
            .and_then(|finding_id| candidates.get_key_value(finding_id))
    })
}

/// The error for a verdict id that names no candidate. The advice depends on
/// the kind of id that the verdict uses, and on whether the candidate file
/// has stable ids. A candidate file from a fallow version before
/// `stable_finding_id` has none.
pub fn unknown_verdict_message(verdict_id: &str, stable_ids: &BTreeMap<String, String>) -> String {
    let stable_prefix = format!("{}:", fallow_security::STABLE_SECURITY_ID_SCHEME);
    let advice = if verdict_id.starts_with(&stable_prefix) && stable_ids.is_empty() {
        "The candidate file has no stable_finding_id values. Regenerate it with \
         `fallow security --format json` from this fallow version."
    } else if verdict_id.starts_with(&stable_prefix) {
        "A stable_finding_id changes when the sink text, the rule or the file path changes. \
         An inline suppression comment also removes the candidate of its sink. \
         Review the candidate again and store a new verdict, or remove the verdict."
    } else {
        "A finding_id changes when an edit moves the sink. Key verdicts by \
         stable_finding_id to keep them across such edits."
    };
    format!("Verifier verdict references unknown finding_id `{verdict_id}`. {advice}")
}

pub fn load_candidate_map(path: &Path) -> Result<CandidateInput, String> {
    let mut value = load_json_file(path, "candidate")?;
    let workspace_diagnostics = value
        .get_mut("workspace_diagnostics")
        .map(serde_json::Value::take)
        .map(serde_json::from_value)
        .transpose()
        .map_err(|err| {
            format!(
                "Candidate file {} contains malformed workspace diagnostics: {err}",
                path.display()
            )
        })?
        .unwrap_or_default();
    let Some(serde_json::Value::Array(findings)) = value
        .get_mut("security_findings")
        .map(serde_json::Value::take)
    else {
        return Err(format!(
            "Candidate file {} must be raw `fallow security --format json` output with a security_findings array.",
            path.display()
        ));
    };
    let mut candidates = BTreeMap::new();
    let mut stable_ids = BTreeMap::new();
    for finding in findings {
        let finding: SecurityFinding = serde_json::from_value(finding).map_err(|err| {
            format!(
                "Candidate file {} contains a malformed security finding: {err}",
                path.display()
            )
        })?;
        if finding.finding_id.is_empty() {
            return Err(format!(
                "Candidate file {} contains a security finding with an empty finding_id.",
                path.display()
            ));
        }
        if let Some(stable_id) = &finding.stable_finding_id
            && stable_ids
                .insert(stable_id.clone(), finding.finding_id.clone())
                .is_some()
        {
            return Err(format!(
                "Candidate file {} contains duplicate stable_finding_id `{stable_id}`.",
                path.display()
            ));
        }
        match candidates.entry(finding.finding_id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(finding);
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                return Err(format!(
                    "Candidate file {} contains duplicate finding_id `{}`.",
                    path.display(),
                    entry.key()
                ));
            }
        }
    }
    Ok(CandidateInput {
        candidates,
        stable_ids,
        workspace_diagnostics,
    })
}

pub fn load_verdicts(path: &Path) -> Result<Vec<SecurityVerifierVerdict>, String> {
    let value = load_json_file(path, "verdict")?;
    let verdicts_value = if let Some(items) = value.get("verdicts") {
        if value
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            != Some("fallow-security-verdicts/v1")
        {
            return Err(format!(
                "Verifier verdict file {} must use schema_version `fallow-security-verdicts/v1`.",
                path.display()
            ));
        }
        if !items.is_array() {
            return Err(format!(
                "Verifier verdict file {} must contain a verdicts array.",
                path.display()
            ));
        }
        items.clone()
    } else {
        value
    };
    serde_json::from_value::<Vec<SecurityVerifierVerdict>>(verdicts_value).map_err(|err| {
        format!(
            "Failed to parse verifier verdict file {}: {err}",
            path.display()
        )
    })
}

fn load_json_file(path: &Path, label: &str) -> Result<serde_json::Value, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {label} file {}: {err}", path.display()))?;
    serde_json::from_str(&src)
        .map_err(|err| format!("Failed to parse {label} file {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::unknown_verdict_message;

    fn stable_ids(ids: &[&str]) -> BTreeMap<String, String> {
        ids.iter()
            .map(|id| ((*id).to_owned(), "0123456789abcdef".to_owned()))
            .collect()
    }

    #[test]
    fn unknown_stable_id_asks_for_a_new_review() {
        let message = unknown_verdict_message("sec1:0123456789abcdef", &stable_ids(&["sec1:aaaa"]));
        assert!(message.contains("`sec1:0123456789abcdef`"));
        assert!(message.contains("Review the candidate again"));
        assert!(!message.contains("Key verdicts by stable_finding_id"));
    }

    #[test]
    fn a_candidate_file_without_stable_ids_asks_for_a_new_candidate_file() {
        let message = unknown_verdict_message("sec1:0123456789abcdef", &stable_ids(&[]));
        assert!(message.contains("Regenerate it"), "{message}");
        assert!(!message.contains("Review the candidate again"));
    }
}
