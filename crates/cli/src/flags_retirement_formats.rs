//! Compact, SARIF, CodeClimate and markdown output of the retirement report
//! of `fallow flags --retirement`.
//!
//! Each format lists the candidates only: a row without a reason is not a
//! finding. The per-site `feature_flags[]` output of each format does not
//! change; these entries come after it.

use std::fmt::Write as _;

use fallow_output::{codeclimate_fingerprint_hash, markdown_table_code_span};
use fallow_types::flag_retirement::{
    FlagRetirementReport, FlagSiteRole, RetirementFlag, RetirementFlagKind, RetirementReason,
};

/// SARIF rule id of a retirement candidate.
pub const SARIF_RULE_ID: &str = "fallow/flag-retirement-candidate";

/// CodeClimate check name of a retirement reason.
pub const CODECLIMATE_CHECK_NAME: &str = "fallow/flag-retirement";

/// Compact tag of a retirement reason.
const COMPACT_TAG: &str = "flag-retire";

/// Documentation of the retirement report.
const HELP_URI: &str = "https://docs.fallow.tools/cli/flags#retirement-report";

fn candidates(report: &FlagRetirementReport) -> impl Iterator<Item = &RetirementFlag> {
    report.flags.iter().filter(|row| !row.reasons.is_empty())
}

/// The location of the first evidence of `reason`.
fn reason_location(row: &RetirementFlag, reason: RetirementReason) -> Option<(&str, u32)> {
    row.evidence
        .iter()
        .find(|evidence| evidence.reason == reason)
        .map(|evidence| (evidence.path.as_str(), evidence.line))
}

/// The location of a candidate: the first read site, else the first site,
/// else the first evidence (a `vendor-only` row has no site).
fn row_location(row: &RetirementFlag) -> Option<(&str, u32, u32)> {
    row.sites
        .iter()
        .find(|site| site.role == FlagSiteRole::Read)
        .or_else(|| row.sites.first())
        .map(|site| (site.path.as_str(), site.line, site.col))
        .or_else(|| {
            row.evidence
                .first()
                .map(|evidence| (evidence.path.as_str(), evidence.line, 0))
        })
}

fn reason_codes(row: &RetirementFlag) -> Vec<&'static str> {
    row.reasons.iter().map(|reason| reason.code()).collect()
}

const fn kind_code(kind: RetirementFlagKind) -> &'static str {
    match kind {
        RetirementFlagKind::EnvironmentVariable => "environment_variable",
        RetirementFlagKind::SdkCall => "sdk_call",
        RetirementFlagKind::ConfigObject => "config_object",
        RetirementFlagKind::Constant => "constant",
        RetirementFlagKind::VendorExport => "vendor_export",
    }
}

/// One `flag-retire:<reason>:<path>:<line>:<name>` line per reason of each
/// candidate.
#[must_use]
pub fn compact_lines(report: &FlagRetirementReport) -> Vec<String> {
    let mut lines = Vec::new();
    for row in candidates(report) {
        for reason in &row.reasons {
            let (path, line) = reason_location(row, *reason).unwrap_or(("", 0));
            lines.push(format!(
                "{COMPACT_TAG}:{}:{path}:{line}:{}",
                reason.code(),
                row.flag_name
            ));
        }
    }
    lines
}

/// The SARIF rule of a retirement candidate.
#[must_use]
pub fn sarif_rule() -> serde_json::Value {
    serde_json::json!({
        "id": SARIF_RULE_ID,
        "shortDescription": { "text": "Feature flag is a retirement candidate" },
        "fullDescription": {
            "text": "The evidence shows that the flag can be retired. A person decides; Fallow does not remove code."
        },
        "helpUri": HELP_URI,
        "defaultConfiguration": { "level": "note" },
    })
}

/// One SARIF result per candidate.
#[must_use]
pub fn sarif_results(report: &FlagRetirementReport) -> Vec<serde_json::Value> {
    candidates(report)
        .filter_map(|row| {
            let (path, line, col) = row_location(row)?;
            let reasons = reason_codes(row);
            let mut message = format!(
                "Feature flag '{}' is a retirement candidate: {}",
                row.flag_name,
                reasons.join(", ")
            );
            if let Some(days) = row.age_days {
                let _ = write!(message, " (age {days} days)");
            }
            Some(serde_json::json!({
                "ruleId": SARIF_RULE_ID,
                "level": "note",
                "message": { "text": message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": crate::report::normalize_uri(path) },
                        "region": { "startLine": line.max(1), "startColumn": col + 1 },
                    }
                }],
                "properties": {
                    "flag_name": row.flag_name,
                    "kind": kind_code(row.kind),
                    "reasons": reasons,
                    "age_days": row.age_days,
                },
            }))
        })
        .collect()
}

/// One CodeClimate issue per reason of each candidate. The fingerprint
/// hashes the flag identity and the reason, so it does not move when a line
/// moves.
#[must_use]
pub fn codeclimate_issues(report: &FlagRetirementReport) -> Vec<serde_json::Value> {
    let mut issues = Vec::new();
    for row in candidates(report) {
        for reason in &row.reasons {
            let Some((path, line)) = reason_location(row, *reason) else {
                continue;
            };
            let detail = row
                .evidence
                .iter()
                .find(|evidence| evidence.reason == *reason)
                .map_or("", |evidence| evidence.detail.as_str());
            let fingerprint = codeclimate_fingerprint_hash(&[
                "flag-retirement",
                kind_code(row.kind),
                row.sdk_name.as_deref().unwrap_or(""),
                row.workspace.as_deref().unwrap_or(""),
                &row.flag_name,
                reason.code(),
            ]);
            issues.push(serde_json::json!({
                "type": "issue",
                "check_name": CODECLIMATE_CHECK_NAME,
                "description": format!(
                    "Feature flag '{}' is a retirement candidate ({}): {detail}",
                    row.flag_name,
                    reason.code()
                ),
                "categories": ["Clarity"],
                "severity": "info",
                "fingerprint": fingerprint,
                "location": {
                    "path": crate::report::normalize_uri(path),
                    "lines": { "begin": line.max(1) },
                }
            }));
        }
    }
    issues
}

/// The "Retirement candidates" markdown section: one table row per
/// candidate with the flag, age, read sites and reasons.
#[must_use]
pub fn markdown_section(report: &FlagRetirementReport) -> String {
    let rows: Vec<&RetirementFlag> = candidates(report).collect();
    let mut out = format!(
        "### Retirement candidates ({} of {} flags)\n\n",
        rows.len(),
        report.summary.listed_flags()
    );
    if rows.is_empty() {
        out.push_str("No flag has a retirement reason.\n");
        return out;
    }
    out.push_str("| Flag | Age | Sites | Reasons |\n|------|-----|-------|---------|\n");
    for row in rows {
        let age = row
            .age_days
            .map_or_else(|| "-".to_string(), |days| format!("{days} days"));
        let _ = writeln!(
            out,
            "| {} | {age} | {} | {} |",
            markdown_table_code_span(&row.flag_name),
            row.read_sites,
            reason_codes(row).join(", ")
        );
    }
    out.push_str("\nFallow does not remove flags. A person decides what to retire.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::flag_retirement::{
        FlagAgeMode, RetirementEvidence, RetirementSite, RetirementSummary,
    };

    fn row(name: &str, reasons: &[RetirementReason], age: Option<u64>) -> RetirementFlag {
        RetirementFlag {
            flag_name: name.to_string(),
            kind: RetirementFlagKind::SdkCall,
            sdk_name: Some("LaunchDarkly".to_string()),
            workspace: None,
            sites: vec![
                RetirementSite {
                    path: "src/def.ts".to_string(),
                    line: 2,
                    col: 0,
                    role: FlagSiteRole::Definition,
                    in_test: false,
                },
                RetirementSite {
                    path: "src/app.ts".to_string(),
                    line: 7,
                    col: 4,
                    role: FlagSiteRole::Read,
                    in_test: false,
                },
            ],
            read_sites: 1,
            test_only: false,
            first_seen: None,
            oldest_surviving_site: None,
            last_touched: None,
            age_days: age,
            reasons: reasons.to_vec(),
            evidence: reasons
                .iter()
                .map(|reason| RetirementEvidence {
                    reason: *reason,
                    path: "src/app.ts".to_string(),
                    line: 7,
                    detail: format!("detail of {}", reason.code()),
                })
                .collect(),
            actions: Vec::new(),
            vendor: None,
        }
    }

    fn report() -> FlagRetirementReport {
        FlagRetirementReport {
            generated_at_clock: None,
            age_mode: FlagAgeMode::Blame,
            vendor_state: None,
            summary: RetirementSummary {
                distinct_flags: 3,
                ..RetirementSummary::default()
            },
            regression: None,
            max_flag_age: None,
            flags: vec![
                row(
                    "beta",
                    &[
                        RetirementReason::SingleReadSite,
                        RetirementReason::FullyRolledOut,
                    ],
                    Some(120),
                ),
                row("live", &[], Some(3)),
                row("old", &[RetirementReason::ArchivedInVendor], None),
            ],
        }
    }

    #[test]
    fn compact_prints_one_line_per_reason_of_each_candidate() {
        assert_eq!(
            compact_lines(&report()),
            vec![
                "flag-retire:single-read-site:src/app.ts:7:beta",
                "flag-retire:fully-rolled-out:src/app.ts:7:beta",
                "flag-retire:archived-in-vendor:src/app.ts:7:old",
            ]
        );
    }

    #[test]
    fn sarif_gives_one_note_per_candidate_at_the_first_read_site() {
        let results = sarif_results(&report());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["ruleId"], SARIF_RULE_ID);
        assert_eq!(results[0]["level"], "note");
        let region = &results[0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 7);
        assert_eq!(region["startColumn"], 5);
        assert_eq!(
            results[0]["message"]["text"],
            "Feature flag 'beta' is a retirement candidate: single-read-site, fully-rolled-out (age 120 days)"
        );
        assert_eq!(sarif_rule()["id"], SARIF_RULE_ID);
    }

    #[test]
    fn codeclimate_fingerprints_do_not_follow_the_line() {
        let issues = codeclimate_issues(&report());
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0]["check_name"], CODECLIMATE_CHECK_NAME);
        assert_eq!(issues[0]["severity"], "info");
        let mut moved = report();
        moved.flags[0].evidence[0].line = 40;
        let moved_issues = codeclimate_issues(&moved);
        assert_eq!(issues[0]["fingerprint"], moved_issues[0]["fingerprint"]);
        assert_ne!(issues[0]["fingerprint"], issues[1]["fingerprint"]);
    }

    #[test]
    fn markdown_lists_the_candidates_in_one_table() {
        let section = markdown_section(&report());
        assert!(section.starts_with("### Retirement candidates (2 of 3 flags)"));
        assert!(section.contains("| `beta` | 120 days | 1 | single-read-site, fully-rolled-out |"));
        assert!(section.contains("| `old` | - | 1 | archived-in-vendor |"));
        assert!(!section.contains("`live`"));
    }

    #[test]
    fn markdown_keeps_a_flag_name_with_a_pipe_or_backtick_in_one_cell() {
        let mut odd = report();
        odd.flags[0].flag_name = "a|b`c".to_string();
        let section = markdown_section(&odd);
        assert!(section.contains("| ``a\\|b`c`` | 120 days |"), "{section}");
    }
}
