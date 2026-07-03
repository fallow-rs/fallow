use std::collections::BTreeMap;

use fallow_types::envelope::{Meta, MetaRule};
pub use fallow_types::issue_meta::{CODECLIMATE_RESULT_CODES, TsAliasMeta};
use fallow_types::issue_meta::{
    IssueResultMeta, issue_meta_by_code, issue_result_meta_by_code, result_issue_metas,
};

const DOCS_BASE: &str = "https://docs.fallow.tools";

/// Docs URL for the dead-code/check command.
pub const CHECK_DOCS: &str = "https://docs.fallow.tools/cli/dead-code";

/// `_meta` description for the per-finding `actions[]` array shared across
/// JSON output.
pub const ACTIONS_FIELD_DEFINITION: &str = "Per-finding fix and suppression suggestions. Each entry carries a `type` discriminant (kebab-case) plus a per-action `auto_fixable` bool. Consumers dispatch on `type` to choose the remediation and filter on `auto_fixable` of each individual entry.";

/// `_meta` description for the per-action `auto_fixable` bool.
pub const ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION: &str = "Evaluated PER FINDING, not per action type. The same `type` may carry `auto_fixable: true` on one finding and `auto_fixable: false` on another when per-instance guards in the `fallow fix` applier discriminate. Filter on this bool of each individual action, not on `type` alone. Current per-instance flips: (1) `remove-catalog-entry` is `true` only when the finding's `hardcoded_consumers` array is empty (else fallow fix skips the entry to avoid breaking `pnpm install`); (2) the primary dependency action flips between `remove-dependency` (`auto_fixable: true`) and `move-dependency` (`auto_fixable: false`) based on `used_in_workspaces`; (3) `add-to-config` for `ignoreExports` is `true` when fallow fix can safely apply the action, which means EITHER a fallow config file already exists OR no config exists and the working directory is NOT inside a monorepo subpackage (the applier then creates `.fallowrc.json` using `fallow init`'s framework-aware scaffolding and layers the new rules on top); `false` inside a monorepo subpackage with no workspace-root config because the applier refuses to fragment per-package configs; (4) `update-catalog-reference` is always `false` today (catalog-switching applier not yet wired). All `suppress-line` and `suppress-file` actions are uniformly `false`.";

/// Output-facing contract metadata for a serialized dead-code result row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueOutputContract {
    /// Canonical issue code that owns this result array.
    pub code: &'static str,
    /// Serialized `AnalysisResults` array key that carries this issue row.
    pub result_key: &'static str,
    /// Whether `result_key` contributes to `AnalysisResults::total_issues()`.
    pub counts_in_total: bool,
    /// Label used by CI summary tables.
    pub summary_label: &'static str,
    /// Documentation anchor used by CI summary tables.
    pub summary_docs_anchor: &'static str,
    /// Human-readable name emitted in dead-code `_meta.rules`.
    pub meta_name: &'static str,
    /// Explanation emitted in dead-code `_meta.rules`.
    pub meta_description: &'static str,
    /// Documentation path emitted in dead-code `_meta.rules`.
    pub meta_docs_path: &'static str,
    /// SARIF rule ids used by the CLI SARIF formatter for this result row.
    pub sarif_rule_ids: Vec<String>,
    /// CodeClimate check names used by the CodeClimate formatter.
    pub codeclimate_check_names: Vec<String>,
    /// Published TypeScript alias policy for backwards-compatible bare names.
    pub ts_alias: Option<TsAliasMeta>,
}

impl IssueOutputContract {
    #[must_use]
    fn from_result_meta(meta: &IssueResultMeta) -> Self {
        let issue = issue_meta_by_code(meta.code).unwrap_or_else(|| {
            panic!(
                "output contract must reference IssueKindMeta row: {}",
                meta.code
            )
        });
        Self {
            code: meta.code,
            result_key: meta.result_key,
            counts_in_total: meta.counts_in_total,
            summary_label: meta.summary_label,
            summary_docs_anchor: meta.docs_anchor,
            meta_name: meta.meta_name,
            meta_description: meta.meta_description,
            meta_docs_path: meta.meta_docs_path,
            sarif_rule_ids: issue.sarif_rule_ids(),
            codeclimate_check_names: issue.codeclimate_check_names(),
            ts_alias: issue.ts_alias(),
        }
    }
}

/// Build the `_meta` object for `fallow dead-code --format json --explain`.
#[must_use]
pub fn check_meta() -> Meta {
    let mut rules = BTreeMap::new();
    for contract in issue_output_contracts() {
        rules.insert(
            contract.code.to_string(),
            MetaRule {
                name: Some(contract.meta_name.to_string()),
                description: Some(contract.meta_description.to_string()),
                docs: Some(rule_docs_url(contract.meta_docs_path)),
            },
        );
    }
    rules.insert(
        "missing-suppression-reason".to_string(),
        MetaRule {
            name: Some("Missing Suppression Reason".to_string()),
            description: Some("A fallow-ignore-next-line or fallow-ignore-file suppression omits the explanatory reason required by the requireSuppressionReason rule. Add a short reason after the suppression token, or remove the suppression if the issue is no longer intentional.".to_string()),
            docs: Some(rule_docs_url("explanations/dead-code#stale-suppressions")),
        },
    );

    Meta {
        docs: Some(CHECK_DOCS.to_string()),
        field_definitions: BTreeMap::from([
            (
                "actions[]".to_string(),
                ACTIONS_FIELD_DEFINITION.to_string(),
            ),
            (
                "actions[].auto_fixable".to_string(),
                ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION.to_string(),
            ),
        ]),
        rules,
        ..Meta::default()
    }
}

#[must_use]
pub fn dead_code_docs_url(anchor: &str) -> String {
    format!("{DOCS_BASE}/explanations/dead-code#{anchor}")
}

#[must_use]
pub fn rule_docs_url(docs_path: &str) -> String {
    format!("{DOCS_BASE}/{docs_path}")
}

/// Output-facing dead-code result contracts in stable registry order.
pub fn issue_output_contracts() -> impl Iterator<Item = IssueOutputContract> {
    result_issue_metas().map(IssueOutputContract::from_result_meta)
}

/// Output-facing dead-code result contract by issue code.
#[must_use]
pub fn issue_output_contract_by_code(code: &str) -> Option<IssueOutputContract> {
    issue_result_meta_by_code(code).map(IssueOutputContract::from_result_meta)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_result_row_has_output_contract() {
        let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
        let output_codes: BTreeSet<&str> = issue_output_contracts()
            .map(|contract| contract.code)
            .collect();
        assert_eq!(result_codes, output_codes);
    }

    #[test]
    fn summary_contracts_are_present() {
        for contract in issue_output_contracts() {
            assert!(!contract.summary_label.is_empty());
            assert!(!contract.summary_docs_anchor.is_empty());
            assert!(!contract.meta_name.is_empty());
            assert!(!contract.meta_description.is_empty());
            assert!(!contract.meta_docs_path.is_empty());
        }
    }

    #[test]
    fn check_meta_uses_output_contracts() {
        let meta = check_meta();
        assert_eq!(meta.docs.as_deref(), Some(CHECK_DOCS));
        assert!(
            meta.field_definitions["actions[].auto_fixable"].contains("PER FINDING"),
            "auto_fixable definition should preserve per-finding guidance"
        );
        assert!(meta.rules.contains_key("unused-export"));
        assert!(meta.rules.contains_key("missing-suppression-reason"));
        assert_eq!(
            meta.rules["unused-dev-dependency"].docs.as_deref(),
            Some("https://docs.fallow.tools/explanations/dead-code#unused-devdependencies")
        );
    }

    #[test]
    fn ci_format_contracts_are_present() {
        for contract in issue_output_contracts() {
            assert!(
                contract
                    .sarif_rule_ids
                    .contains(&format!("fallow/{}", contract.code)),
                "result metadata code {} has wrong SARIF rule id",
                contract.code
            );
            for rule_id in contract.sarif_rule_ids {
                assert!(
                    rule_id.starts_with("fallow/"),
                    "result metadata code {} has unprefixed SARIF rule id {rule_id}",
                    contract.code
                );
            }
            for check_name in contract.codeclimate_check_names {
                assert!(
                    check_name.starts_with("fallow/"),
                    "result metadata code {} has unprefixed CodeClimate check name {check_name}",
                    contract.code
                );
            }
        }
    }

    #[test]
    fn codeclimate_result_exclusions_are_explicit() {
        let expected = BTreeSet::from(["duplicate-prop-shape", "prop-drilling", "thin-wrapper"]);
        let from_contracts: BTreeSet<&str> = issue_output_contracts()
            .filter(|contract| contract.codeclimate_check_names.is_empty())
            .map(|contract| contract.code)
            .collect();
        assert_eq!(expected, from_contracts);
    }

    #[test]
    fn codeclimate_result_codes_match_result_metadata() {
        let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
        let codeclimate_codes: BTreeSet<&str> = CODECLIMATE_RESULT_CODES.iter().copied().collect();
        assert!(codeclimate_codes.is_subset(&result_codes));
    }

    #[test]
    fn ts_alias_policy_is_explicit() {
        let aliases: BTreeSet<(&str, &str)> = issue_output_contracts()
            .filter_map(|contract| contract.ts_alias.map(|alias| (alias.name, alias.parent)))
            .collect();

        assert_eq!(
            BTreeSet::from([
                ("BoundaryViolation", "BoundaryViolationFinding"),
                ("CircularDependency", "CircularDependencyFinding"),
                (
                    "DevDependencyInProduction",
                    "DevDependencyInProductionFinding",
                ),
                ("DuplicateExport", "DuplicateExportFinding"),
                ("EmptyCatalogGroup", "EmptyCatalogGroupFinding"),
                (
                    "MisconfiguredDependencyOverride",
                    "MisconfiguredDependencyOverrideFinding",
                ),
                ("PrivateTypeLeak", "PrivateTypeLeakFinding"),
                ("ReExportCycle", "ReExportCycleFinding"),
                ("TestOnlyDependency", "TestOnlyDependencyFinding"),
                ("TypeOnlyDependency", "TypeOnlyDependencyFinding"),
                ("UnlistedDependency", "UnlistedDependencyFinding"),
                (
                    "UnresolvedCatalogReference",
                    "UnresolvedCatalogReferenceFinding",
                ),
                ("UnresolvedImport", "UnresolvedImportFinding"),
                ("UnusedCatalogEntry", "UnusedCatalogEntryFinding"),
                ("UnusedDependency", "UnusedDependencyFinding"),
                ("UnusedDependency", "UnusedDevDependencyFinding"),
                ("UnusedDependency", "UnusedOptionalDependencyFinding"),
                (
                    "UnusedDependencyOverride",
                    "UnusedDependencyOverrideFinding",
                ),
                ("UnusedExport", "UnusedExportFinding"),
                ("UnusedFile", "UnusedFileFinding"),
                ("UnusedMember", "UnusedClassMemberFinding"),
                ("UnusedMember", "UnusedEnumMemberFinding"),
                ("UnusedMember", "UnusedStoreMemberFinding"),
            ]),
            aliases
        );
    }
}
