use std::collections::BTreeMap;

use fallow_types::envelope::{Meta, MetaRule};
pub use fallow_types::issue_meta::{CODECLIMATE_RESULT_CODES, TsAliasMeta};
use fallow_types::issue_meta::{
    IssueResultMeta, issue_codeclimate_check_names, issue_result_meta_by_code,
    issue_sarif_rule_ids, issue_ts_alias, result_issue_metas,
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
    fn from_result_meta(meta: &IssueResultMeta) -> Option<Self> {
        Some(Self {
            code: meta.code,
            result_key: meta.result_key,
            counts_in_total: meta.counts_in_total,
            summary_label: meta.summary_label,
            summary_docs_anchor: meta.docs_anchor,
            meta_name: meta.meta_name,
            meta_description: issue_meta_description(meta.code)?,
            meta_docs_path: issue_meta_docs_path(meta.code)?,
            sarif_rule_ids: issue_sarif_rule_ids(meta.code),
            codeclimate_check_names: issue_codeclimate_check_names(meta.code),
            ts_alias: issue_ts_alias(meta.code),
        })
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
    result_issue_metas().filter_map(IssueOutputContract::from_result_meta)
}

/// Output-facing dead-code result contract by issue code.
#[must_use]
pub fn issue_output_contract_by_code(code: &str) -> Option<IssueOutputContract> {
    issue_result_meta_by_code(code).and_then(IssueOutputContract::from_result_meta)
}

#[allow(
    clippy::too_many_lines,
    reason = "dead-code meta prose is intentionally kept in one lookup table"
)]
fn issue_meta_description(code: &str) -> Option<&'static str> {
    Some(match code {
        "unused-file" => {
            "Source files that are not imported by any other module and are not entry points. Detection uses graph reachability from configured entry points."
        }
        "unused-export" => {
            "Named exports that are never imported by any other module in the project, including direct exports and re-exports through barrel files."
        }
        "unused-type" => {
            "Type-only exports that are never imported. These do not generate runtime code but add maintenance burden."
        }
        "private-type-leak" => {
            "Exported values or types whose public TypeScript signature references a same-file type declaration that is not exported."
        }
        "unused-dependency" => {
            "Packages listed in dependencies that are never imported or required by any source file."
        }
        "unused-dev-dependency" => {
            "Packages listed in devDependencies that are never imported by test files, config files, or scripts."
        }
        "unused-optional-dependency" => {
            "Packages listed in optionalDependencies that are never imported."
        }
        "unused-enum-member" => "Enum members that are never referenced in the codebase.",
        "unused-class-member" => {
            "Class methods and properties that are never referenced outside the class."
        }
        "unused-store-member" => {
            "Pinia store members declared but never accessed by any consumer project-wide."
        }
        "unresolved-import" => "Import specifiers that could not be resolved to a file on disk.",
        "unlisted-dependency" => "Packages imported in source code but not listed in package.json.",
        "duplicate-export" => "The same export name is defined in multiple modules.",
        "type-only-dependency" => {
            "Production dependencies that are only imported via type-only imports."
        }
        "test-only-dependency" => "Production dependencies that are only imported from test files.",
        "circular-dependency" => "A cycle in the module import graph.",
        "re-export-cycle" => {
            "A barrel file re-exports from another barrel that ultimately re-exports back."
        }
        "boundary-violation" => {
            "A module imports from a zone that its configured boundary rules do not allow."
        }
        "boundary-coverage" => {
            "A reachable source file is not assigned to any configured boundary zone while boundary coverage is required."
        }
        "boundary-call-violation" => {
            "A file classified into a boundary zone calls a callee matching one of the zone's forbidden call patterns."
        }
        "policy-violation" => {
            "A call site, import, or catalogue-derived effect matched a configured rule pack rule."
        }
        "invalid-client-export" => {
            "A file carrying the use client directive also exports a Next.js server-only or route-segment config name."
        }
        "mixed-client-server-barrel" => {
            "A barrel file forwards a name from a use client module alongside a name from a server-only module."
        }
        "misplaced-directive" => {
            "A use client or use server directive string appears after a non-directive statement and is ignored."
        }
        "unprovided-inject" => {
            "A Vue inject or Svelte getContext reads a dependency-injection key that no matching provider supplies."
        }
        "unrendered-component" => {
            "A Vue or Svelte single-file component is reachable through the graph but rendered nowhere in the project."
        }
        "unused-component-prop" => {
            "A declared Vue, Svelte, React, or Preact component prop is referenced nowhere inside its own component."
        }
        "unused-component-emit" => {
            "A Vue script setup defineEmits event is emitted nowhere in its own component."
        }
        "unused-component-input" => "An Angular input is read nowhere in its own component.",
        "unused-component-output" => "An Angular output is emitted nowhere in its own component.",
        "unused-svelte-event" => {
            "A Svelte component dispatches a custom event whose name is listened to nowhere in the analyzed project."
        }
        "unused-server-action" => {
            "A Next.js Server Action exported from a use server file is referenced by no code in the project."
        }
        "unused-load-data-key" => {
            "A SvelteKit load return-object key is read by no route or project-wide consumer."
        }
        "route-collision" => {
            "Two or more Next.js App Router route files resolve to the same URL within one app root."
        }
        "dynamic-segment-name-conflict" => {
            "Sibling Next.js dynamic route segments use different slug names at the same position."
        }
        "stale-suppression" => {
            "A fallow suppression comment or tag no longer matches any active issue."
        }
        "unused-catalog-entry" => {
            "A package manager catalog entry is not referenced by any workspace package.json."
        }
        "empty-catalog-group" => "A named package manager catalog group has no package entries.",
        "unresolved-catalog-reference" => {
            "A workspace package.json uses a catalog protocol reference that no catalog declares."
        }
        "unused-dependency-override" => {
            "A pnpm dependency override targets a package not declared by any workspace package and not present in the lockfile."
        }
        "misconfigured-dependency-override" => {
            "A pnpm dependency override key or value does not parse as a valid override spec."
        }
        "prop-drilling" => {
            "A React or Preact prop is forwarded unchanged through multiple pass-through components to a distant consumer."
        }
        "thin-wrapper" => {
            "A React or Preact component is structural indirection around a single spread-forwarded child render."
        }
        "duplicate-prop-shape" => {
            "Multiple React or Preact components declare an identical significant prop-name set."
        }
        _ => return None,
    })
}

fn issue_meta_docs_path(code: &str) -> Option<&'static str> {
    Some(match code {
        "unused-file" => "explanations/dead-code#unused-files",
        "unused-export" => "explanations/dead-code#unused-exports",
        "unused-type" => "explanations/dead-code#unused-types",
        "private-type-leak" => "explanations/dead-code#private-type-leaks",
        "unused-dependency" => "explanations/dead-code#unused-dependencies",
        "unused-dev-dependency" => "explanations/dead-code#unused-devdependencies",
        "unused-optional-dependency" => "explanations/dead-code#unused-optionaldependencies",
        "type-only-dependency" => "explanations/dead-code#type-only-dependencies",
        "test-only-dependency" => "explanations/dead-code#test-only-dependencies",
        "unused-enum-member" => "explanations/dead-code#unused-enum-members",
        "unused-class-member" => "explanations/dead-code#unused-class-members",
        "unused-store-member" => "explanations/dead-code#unused-store-members",
        "unresolved-import" => "explanations/dead-code#unresolved-imports",
        "unlisted-dependency" => "explanations/dead-code#unlisted-dependencies",
        "duplicate-export" => "explanations/dead-code#duplicate-exports",
        "circular-dependency" => "explanations/dead-code#circular-dependencies",
        "re-export-cycle" => "explanations/dead-code#re-export-cycles",
        "boundary-violation" | "boundary-coverage" | "boundary-call-violation" => {
            "explanations/dead-code#boundary-violations"
        }
        "policy-violation" => "explanations/dead-code#policy-violations",
        "stale-suppression" => "explanations/dead-code#stale-suppressions",
        "unused-catalog-entry" => "explanations/dead-code#unused-catalog-entries",
        "empty-catalog-group" => "explanations/dead-code#empty-catalog-groups",
        "unresolved-catalog-reference" => "explanations/dead-code#unresolved-catalog-references",
        "unused-dependency-override" => "explanations/dead-code#unused-dependency-overrides",
        "misconfigured-dependency-override" => {
            "explanations/dead-code#misconfigured-dependency-overrides"
        }
        "invalid-client-export" => "explanations/dead-code#invalid-client-exports",
        "mixed-client-server-barrel" => "explanations/dead-code#mixed-client-server-barrels",
        "misplaced-directive" => "explanations/dead-code#misplaced-directives",
        "unprovided-inject" => "explanations/dead-code#unprovided-injects",
        "unrendered-component" => "explanations/dead-code#unrendered-components",
        "unused-component-prop" => "explanations/dead-code#unused-component-props",
        "unused-component-emit" => "explanations/dead-code#unused-component-emits",
        "unused-component-input" => "explanations/dead-code#unused-component-inputs",
        "unused-component-output" => "explanations/dead-code#unused-component-outputs",
        "unused-svelte-event" => "explanations/dead-code#unused-svelte-events",
        "unused-server-action" => "explanations/dead-code#unused-server-actions",
        "unused-load-data-key" => "explanations/dead-code#unused-load-data-keys",
        "prop-drilling" => "explanations/dead-code#prop-drilling",
        "thin-wrapper" => "explanations/dead-code#thin-wrapper",
        "duplicate-prop-shape" => "explanations/dead-code#duplicate-prop-shape",
        "route-collision" => "explanations/dead-code#route-collisions",
        "dynamic-segment-name-conflict" => "explanations/dead-code#dynamic-segment-name-conflicts",
        _ => return None,
    })
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
