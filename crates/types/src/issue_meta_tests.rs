use std::collections::BTreeSet;

use crate::results::TOTAL_ISSUE_RESULT_KEYS;

use super::*;

#[test]
fn fixable_codes_are_canonical_issue_codes() {
    for code in FIXABLE_ISSUE_CODES {
        assert!(
            issue_meta_by_code(code).is_some(),
            "fixable code {code} is not a canonical ISSUE_KIND_META code"
        );
    }
    assert!(issue_is_fixable("unused-export"));
    assert!(!issue_is_fixable("unused-file"));
}

#[test]
fn known_names_round_trip_through_metadata() {
    for name in KNOWN_ISSUE_KIND_NAMES.iter() {
        let meta = issue_meta_for_token(name)
            .unwrap_or_else(|| panic!("known issue name {name} missing metadata row"));
        assert!(
            meta.kind.is_some(),
            "known issue name {name} maps to non-IssueKind metadata"
        );
    }
}

#[test]
fn issue_kind_variants_have_metadata() {
    for &kind in IssueKind::ALL {
        assert!(
            issue_meta_by_kind(kind).is_some(),
            "IssueKind {kind:?} has no metadata row"
        );
    }
}

#[test]
fn dead_code_filter_flags_match_metadata() {
    let from_constants: BTreeSet<&str> = DEAD_CODE_FILTER_FLAGS.iter().copied().collect();
    let from_meta: BTreeSet<&str> = ISSUE_KIND_META
        .iter()
        .filter_map(|meta| meta.filter_flag)
        .collect();
    assert_eq!(from_constants, from_meta);
}

#[test]
fn mcp_issue_type_flags_match_metadata() {
    let from_constants: BTreeSet<(&str, &str)> = MCP_ISSUE_TYPE_FLAGS.iter().copied().collect();
    let from_meta: BTreeSet<(&str, &str)> = ISSUE_KIND_META
        .iter()
        .filter_map(|meta| meta.mcp_pair())
        .collect();
    assert_eq!(from_constants, from_meta);
}

#[test]
fn lsp_exposes_only_actual_diagnostic_codes() {
    let codes: BTreeSet<&str> = diagnostic_issue_metas().map(|meta| meta.code).collect();
    assert!(codes.contains("boundary-violation"));
    assert!(!codes.contains("boundary-coverage"));
    assert!(!codes.contains("boundary-call-violation"));
}

#[test]
fn issue_codes_are_unique() {
    let mut seen = BTreeSet::new();
    for meta in ISSUE_KIND_META {
        assert!(seen.insert(meta.code), "duplicate issue code {}", meta.code);
    }
}

#[test]
#[cfg_attr(
    miri,
    ignore = "too slow under Miri: loops over every issue kind in safe code"
)]
fn contract_tokens_resolve_through_metadata() {
    for meta in ISSUE_KIND_META {
        assert_eq!(
            issue_meta_for_contract_token(meta.code).map(|resolved| resolved.code),
            Some(meta.code),
            "canonical issue code {} should resolve through contract token lookup",
            meta.code
        );
        for token in meta.aliases {
            assert!(
                issue_meta_matches_contract_token(meta, token),
                "alias {token} should match {}",
                meta.code
            );
        }
        for token in [
            meta.config_key,
            meta.mcp_issue_type,
            meta.suppress_token,
            meta.filter_flag,
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                issue_meta_for_contract_token(token).is_some(),
                "contract token {token} should resolve through metadata"
            );
            assert!(
                issue_meta_matches_contract_token(meta, token),
                "contract token {token} should match {}",
                meta.code
            );
        }
    }
}

#[test]
fn sarif_rule_descriptions_cover_result_metadata() {
    for meta in result_issue_metas() {
        for rule_id in issue_sarif_rule_ids(meta.code) {
            assert!(
                issue_sarif_rule_description(&rule_id).is_some(),
                "SARIF rule {rule_id} for {} needs a central description",
                meta.code
            );
        }
    }
}

#[test]
fn result_meta_codes_have_issue_metadata() {
    for meta in ISSUE_RESULT_META {
        assert!(
            issue_meta_by_code(meta.code).is_some(),
            "result metadata code {} has no issue metadata row",
            meta.code
        );
    }
}

#[test]
fn result_meta_codes_have_docs_anchors() {
    for meta in ISSUE_RESULT_META {
        let issue = issue_meta_by_code(meta.code)
            .unwrap_or_else(|| panic!("result metadata code {} has no issue row", meta.code));
        assert_eq!(
            issue.docs_anchor(),
            Some(meta.docs_anchor),
            "result metadata code {} has mismatched docs anchor",
            meta.code
        );
    }
}

#[test]
fn result_meta_codes_have_summary_labels() {
    for meta in ISSUE_RESULT_META {
        assert!(
            !meta.summary_label.is_empty(),
            "result metadata code {} has no summary label",
            meta.code
        );
    }
}

#[test]
fn result_meta_codes_have_meta_names() {
    for meta in ISSUE_RESULT_META {
        assert!(
            !meta.meta_name.is_empty(),
            "result metadata code {} has no meta name",
            meta.code
        );
    }
}

#[test]
fn result_meta_codes_have_meta_docs_paths() {
    for meta in ISSUE_RESULT_META {
        assert!(
            meta.meta_docs_path.starts_with("explanations/dead-code#"),
            "result metadata code {} has invalid meta docs path",
            meta.code
        );
    }
}

#[test]
fn result_meta_codes_have_meta_descriptions() {
    for meta in ISSUE_RESULT_META {
        assert!(
            !meta.meta_description.is_empty(),
            "result metadata code {} has no meta description",
            meta.code
        );
    }
}

#[test]
fn ci_format_ids_are_prefixed_and_known() {
    let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
    let codeclimate_codes: BTreeSet<&str> = CODECLIMATE_RESULT_CODES.iter().copied().collect();
    assert!(codeclimate_codes.is_subset(&result_codes));

    for meta in result_issue_metas() {
        let issue = issue_meta_by_code(meta.code)
            .unwrap_or_else(|| panic!("result metadata code {} has no issue row", meta.code));
        assert!(issue.has_result_contract());
        let sarif_ids = issue.sarif_rule_ids();
        assert!(sarif_ids.contains(&format!("fallow/{}", meta.code)));
        for rule_id in sarif_ids {
            assert!(
                rule_id.starts_with("fallow/"),
                "result metadata code {} has unprefixed SARIF rule id {rule_id}",
                meta.code
            );
        }
        for check_name in issue.codeclimate_check_names() {
            assert!(
                check_name.starts_with("fallow/"),
                "result metadata code {} has unprefixed CodeClimate check name {check_name}",
                meta.code
            );
        }
    }
}

#[test]
fn ts_alias_policy_is_explicit() {
    let aliases: BTreeSet<(&str, &str)> = ISSUE_TS_ALIAS_META
        .iter()
        .map(|meta| (meta.alias.name, meta.alias.parent))
        .collect();

    assert_eq!(
        BTreeSet::from([
            ("BoundaryViolation", "BoundaryViolationFinding"),
            ("CircularDependency", "CircularDependencyFinding"),
            ("DeprecatedExportInUse", "DeprecatedExportInUseFinding"),
            (
                "DevDependencyInProduction",
                "DevDependencyInProductionFinding",
            ),
            ("DuplicateExport", "DuplicateExportFinding"),
            ("DuplicateExportAlias", "DuplicateExportAliasFinding"),
            ("EmptyCatalogGroup", "EmptyCatalogGroupFinding"),
            (
                "MisconfiguredDependencyOverride",
                "MisconfiguredDependencyOverrideFinding",
            ),
            ("PrivateTypeLeak", "PrivateTypeLeakFinding"),
            ("PackageCycle", "PackageCycleFinding"),
            ("ReExportCycle", "ReExportCycleFinding"),
            ("TestOnlyDependency", "TestOnlyDependencyFinding"),
            ("TypeOnlyDependency", "TypeOnlyDependencyFinding"),
            (
                "UnresolvedCatalogReference",
                "UnresolvedCatalogReferenceFinding",
            ),
            ("UnresolvedImport", "UnresolvedImportFinding"),
            ("UnlistedDependency", "UnlistedDependencyFinding"),
            ("UnlistedBinary", "UnlistedBinaryFinding"),
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

#[test]
fn ts_alias_registry_rows_match_result_metadata() {
    let result_codes: BTreeSet<&str> = result_issue_metas().map(|meta| meta.code).collect();
    let mut seen_codes = BTreeSet::new();
    for meta in ISSUE_TS_ALIAS_META {
        assert!(
            seen_codes.insert(meta.code),
            "duplicate TypeScript alias row for {}",
            meta.code
        );
        assert!(
            result_codes.contains(meta.code),
            "TypeScript alias row {} has no result metadata",
            meta.code
        );
        assert!(
            meta.alias.parent.ends_with("Finding"),
            "TypeScript alias row {} must point at a generated Finding wrapper",
            meta.code
        );
        assert_eq!(
            Some(meta.alias),
            issue_meta_by_code(meta.code).and_then(|issue| issue.ts_alias()),
            "IssueKindMeta helper must resolve TypeScript alias row {} through the registry",
            meta.code
        );
    }
}

#[test]
fn result_keys_are_unique() {
    let mut seen = BTreeSet::new();
    for meta in ISSUE_RESULT_META {
        assert!(
            seen.insert(meta.result_key),
            "duplicate result key {}",
            meta.result_key
        );
    }
}

#[test]
fn counted_result_keys_match_total_issue_fields() {
    let from_total: BTreeSet<&str> = TOTAL_ISSUE_RESULT_KEYS.iter().copied().collect();
    let from_meta: BTreeSet<&str> = counted_result_issue_metas()
        .map(|meta| meta.result_key)
        .collect();
    assert_eq!(from_total, from_meta);
}
