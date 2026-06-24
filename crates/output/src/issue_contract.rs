use fallow_types::issue_meta::{IssueResultMeta, issue_result_meta_by_code, result_issue_metas};

/// TypeScript backwards-compat alias emitted for a dead-code result row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsAliasMeta {
    /// Bare alias name kept available from the published `fallow/types` subpath.
    pub name: &'static str,
    /// Generated `*Finding` wrapper type the alias resolves to.
    pub parent: &'static str,
}

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
    /// SARIF rule ids used by the CLI SARIF formatter for this result row.
    pub sarif_rule_ids: Vec<String>,
    /// CodeClimate check names used by the CodeClimate formatter.
    pub codeclimate_check_names: Vec<String>,
    /// Published TypeScript alias policy for backwards-compatible bare names.
    pub ts_alias: Option<TsAliasMeta>,
}

impl IssueOutputContract {
    #[must_use]
    fn from_result_meta(meta: &IssueResultMeta) -> Option<Self> {
        Some(Self {
            code: meta.code,
            result_key: meta.result_key,
            counts_in_total: meta.counts_in_total,
            summary_label: issue_summary_label(meta.code)?,
            summary_docs_anchor: issue_summary_docs_anchor(meta.code)?,
            sarif_rule_ids: sarif_rule_ids(meta.code),
            codeclimate_check_names: codeclimate_check_names(meta.code),
            ts_alias: ts_alias(meta.code),
        })
    }
}

/// Output-facing dead-code result contracts in stable registry order.
pub fn issue_output_contracts() -> impl Iterator<Item = IssueOutputContract> {
    result_issue_metas().filter_map(IssueOutputContract::from_result_meta)
}

/// Output-facing dead-code result contract by issue code.
#[must_use]
pub fn issue_output_contract_by_code(code: &str) -> Option<IssueOutputContract> {
    issue_result_meta_by_code(code).and_then(IssueOutputContract::from_result_meta)
}

fn sarif_rule_ids(code: &str) -> Vec<String> {
    let mut ids = vec![format!("fallow/{code}")];
    if code == "stale-suppression" {
        ids.push("fallow/missing-suppression-reason".to_string());
    }
    ids
}

fn codeclimate_check_names(code: &str) -> Vec<String> {
    if !CODECLIMATE_RESULT_CODES.contains(&code) {
        return Vec::new();
    }
    sarif_rule_ids(code)
}

fn ts_alias(code: &str) -> Option<TsAliasMeta> {
    let alias = match code {
        "unused-file" => TsAliasMeta {
            name: "UnusedFile",
            parent: "UnusedFileFinding",
        },
        "unused-export" => TsAliasMeta {
            name: "UnusedExport",
            parent: "UnusedExportFinding",
        },
        "private-type-leak" => TsAliasMeta {
            name: "PrivateTypeLeak",
            parent: "PrivateTypeLeakFinding",
        },
        "unused-dependency" => TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedDependencyFinding",
        },
        "unused-dev-dependency" => TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedDevDependencyFinding",
        },
        "unused-optional-dependency" => TsAliasMeta {
            name: "UnusedDependency",
            parent: "UnusedOptionalDependencyFinding",
        },
        "unused-enum-member" => TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedEnumMemberFinding",
        },
        "unused-class-member" => TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedClassMemberFinding",
        },
        "unused-store-member" => TsAliasMeta {
            name: "UnusedMember",
            parent: "UnusedStoreMemberFinding",
        },
        "unresolved-import" => TsAliasMeta {
            name: "UnresolvedImport",
            parent: "UnresolvedImportFinding",
        },
        "unlisted-dependency" => TsAliasMeta {
            name: "UnlistedDependency",
            parent: "UnlistedDependencyFinding",
        },
        "duplicate-export" => TsAliasMeta {
            name: "DuplicateExport",
            parent: "DuplicateExportFinding",
        },
        "type-only-dependency" => TsAliasMeta {
            name: "TypeOnlyDependency",
            parent: "TypeOnlyDependencyFinding",
        },
        "test-only-dependency" => TsAliasMeta {
            name: "TestOnlyDependency",
            parent: "TestOnlyDependencyFinding",
        },
        "circular-dependency" => TsAliasMeta {
            name: "CircularDependency",
            parent: "CircularDependencyFinding",
        },
        "re-export-cycle" => TsAliasMeta {
            name: "ReExportCycle",
            parent: "ReExportCycleFinding",
        },
        "boundary-violation" => TsAliasMeta {
            name: "BoundaryViolation",
            parent: "BoundaryViolationFinding",
        },
        "unused-catalog-entry" => TsAliasMeta {
            name: "UnusedCatalogEntry",
            parent: "UnusedCatalogEntryFinding",
        },
        "empty-catalog-group" => TsAliasMeta {
            name: "EmptyCatalogGroup",
            parent: "EmptyCatalogGroupFinding",
        },
        "unresolved-catalog-reference" => TsAliasMeta {
            name: "UnresolvedCatalogReference",
            parent: "UnresolvedCatalogReferenceFinding",
        },
        "unused-dependency-override" => TsAliasMeta {
            name: "UnusedDependencyOverride",
            parent: "UnusedDependencyOverrideFinding",
        },
        "misconfigured-dependency-override" => TsAliasMeta {
            name: "MisconfiguredDependencyOverride",
            parent: "MisconfiguredDependencyOverrideFinding",
        },
        _ => return None,
    };
    Some(alias)
}

fn issue_summary_label(code: &str) -> Option<&'static str> {
    Some(match code {
        "unused-file" => "Unused files",
        "unused-export" => "Unused exports",
        "unused-type" => "Unused types",
        "private-type-leak" => "Private type leaks",
        "unused-dependency" => "Unused dependencies",
        "unused-dev-dependency" => "Unused devDependencies",
        "unused-optional-dependency" => "Unused optionalDependencies",
        "unused-enum-member" => "Unused enum members",
        "unused-class-member" => "Unused class members",
        "unused-store-member" => "Unused store members",
        "unresolved-import" => "Unresolved imports",
        "unlisted-dependency" => "Unlisted dependencies",
        "duplicate-export" => "Duplicate exports",
        "type-only-dependency" => "Type-only dependencies",
        "test-only-dependency" => "Test-only dependencies",
        "circular-dependency" => "Circular dependencies",
        "re-export-cycle" => "Re-export cycles",
        "boundary-violation" => "Boundary violations",
        "boundary-coverage" => "Boundary coverage",
        "boundary-call-violation" => "Boundary calls",
        "policy-violation" => "Policy violations",
        "invalid-client-export" => "Invalid client exports",
        "mixed-client-server-barrel" => "Mixed client/server barrels",
        "misplaced-directive" => "Misplaced directives",
        "unprovided-inject" => "Unprovided injects",
        "unrendered-component" => "Unrendered components",
        "unused-component-prop" => "Unused component props",
        "unused-component-emit" => "Unused component emits",
        "unused-component-input" => "Unused component inputs",
        "unused-component-output" => "Unused component outputs",
        "unused-svelte-event" => "Unused Svelte events",
        "unused-server-action" => "Unused server actions",
        "unused-load-data-key" => "Unused load data keys",
        "route-collision" => "Route collisions",
        "dynamic-segment-name-conflict" => "Dynamic segment conflicts",
        "stale-suppression" => "Stale suppressions",
        "unused-catalog-entry" => "Unused catalog entries",
        "empty-catalog-group" => "Empty catalog groups",
        "unresolved-catalog-reference" => "Unresolved catalog references",
        "unused-dependency-override" => "Unused dependency overrides",
        "misconfigured-dependency-override" => "Misconfigured dependency overrides",
        "prop-drilling" => "Prop drilling",
        "thin-wrapper" => "Thin wrappers",
        "duplicate-prop-shape" => "Duplicate prop shapes",
        _ => return None,
    })
}

fn issue_summary_docs_anchor(code: &str) -> Option<&'static str> {
    Some(match code {
        "unused-file" => "unused-files",
        "unused-export" => "unused-exports",
        "unused-type" => "unused-types",
        "private-type-leak" => "private-type-leaks",
        "unused-dependency" | "unused-dev-dependency" | "unused-optional-dependency" => {
            "unused-dependencies"
        }
        "unused-enum-member" => "unused-enum-members",
        "unused-class-member" => "unused-class-members",
        "unused-store-member" => "unused-store-members",
        "unresolved-import" => "unresolved-imports",
        "unlisted-dependency" => "unlisted-dependencies",
        "duplicate-export" => "duplicate-exports",
        "type-only-dependency" => "type-only-dependencies",
        "test-only-dependency" => "test-only-dependencies",
        "circular-dependency" => "circular-dependencies",
        "re-export-cycle" => "re-export-cycles",
        "boundary-violation" | "boundary-coverage" | "boundary-call-violation" => {
            "boundary-violations"
        }
        "policy-violation" => "policy-violations",
        "invalid-client-export" => "invalid-client-exports",
        "mixed-client-server-barrel" => "mixed-client-server-barrels",
        "misplaced-directive" => "misplaced-directives",
        "unprovided-inject" => "unprovided-inject",
        "unrendered-component" => "unrendered-component",
        "unused-component-prop" => "unused-component-prop",
        "unused-component-emit" => "unused-component-emit",
        "unused-component-input" => "unused-component-input",
        "unused-component-output" => "unused-component-output",
        "unused-svelte-event" => "unused-svelte-event",
        "unused-server-action" => "unused-server-action",
        "unused-load-data-key" => "unused-load-data-key",
        "dynamic-segment-name-conflict" => "dynamic-segment-name-conflicts",
        "route-collision" => "route-collisions",
        "stale-suppression" => "stale-suppressions",
        "unused-catalog-entry" => "unused-catalog-entries",
        "empty-catalog-group" => "empty-catalog-groups",
        "unresolved-catalog-reference" => "unresolved-catalog-references",
        "unused-dependency-override" => "unused-dependency-overrides",
        "misconfigured-dependency-override" => "misconfigured-dependency-overrides",
        "prop-drilling" => "prop-drilling",
        "thin-wrapper" => "thin-wrapper",
        "duplicate-prop-shape" => "duplicate-prop-shape",
        _ => return None,
    })
}

/// Result issue codes emitted by the dead-code CodeClimate formatter.
pub const CODECLIMATE_RESULT_CODES: &[&str] = &[
    "unused-file",
    "unused-export",
    "unused-type",
    "private-type-leak",
    "unused-dependency",
    "unused-dev-dependency",
    "unused-optional-dependency",
    "unused-enum-member",
    "unused-class-member",
    "unused-store-member",
    "unresolved-import",
    "unlisted-dependency",
    "duplicate-export",
    "type-only-dependency",
    "test-only-dependency",
    "circular-dependency",
    "re-export-cycle",
    "boundary-violation",
    "boundary-coverage",
    "boundary-call-violation",
    "policy-violation",
    "invalid-client-export",
    "mixed-client-server-barrel",
    "misplaced-directive",
    "unprovided-inject",
    "unrendered-component",
    "unused-component-prop",
    "unused-component-emit",
    "unused-component-input",
    "unused-component-output",
    "unused-svelte-event",
    "unused-server-action",
    "unused-load-data-key",
    "route-collision",
    "dynamic-segment-name-conflict",
    "stale-suppression",
    "unused-catalog-entry",
    "empty-catalog-group",
    "unresolved-catalog-reference",
    "unused-dependency-override",
    "misconfigured-dependency-override",
];

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
        }
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
