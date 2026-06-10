use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use rustc_hash::FxHashSet;

fn relative_key_path(path: &Path, root: &Path) -> String {
    let simple_path = dunce::simplified(path);
    let simple_root = dunce::simplified(root);
    simple_path
        .strip_prefix(simple_root)
        .unwrap_or(simple_path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn dependency_location_key(location: &fallow_core::results::DependencyLocation) -> &'static str {
    match location {
        fallow_core::results::DependencyLocation::Dependencies => "unused-dependency",
        fallow_core::results::DependencyLocation::DevDependencies => "unused-dev-dependency",
        fallow_core::results::DependencyLocation::OptionalDependencies => {
            "unused-optional-dependency"
        }
    }
}

fn unused_dependency_key(item: &fallow_core::results::UnusedDependency, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        dependency_location_key(&item.location),
        relative_key_path(&item.path, root),
        item.package_name
    )
}

fn unlisted_dependency_key(item: &fallow_core::results::UnlistedDependency, root: &Path) -> String {
    let mut sites = item
        .imported_from
        .iter()
        .map(|site| {
            format!(
                "{}:{}:{}",
                relative_key_path(&site.path, root),
                site.line,
                site.col
            )
        })
        .collect::<Vec<_>>();
    sites.sort();
    sites.dedup();
    format!(
        "unlisted-dependency:{}:{}",
        item.package_name,
        sites.join("|")
    )
}

fn unused_member_key(
    rule_id: &str,
    item: &fallow_core::results::UnusedMember,
    root: &Path,
) -> String {
    format!(
        "{}:{}:{}:{}",
        rule_id,
        relative_key_path(&item.path, root),
        item.parent_name,
        item.member_name
    )
}

fn unused_catalog_entry_key(
    item: &fallow_core::results::UnusedCatalogEntry,
    root: &Path,
) -> String {
    format!(
        "unused-catalog-entry:{}:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name,
        item.entry_name
    )
}

fn empty_catalog_group_key(item: &fallow_core::results::EmptyCatalogGroup, root: &Path) -> String {
    format!(
        "empty-catalog-group:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name
    )
}

/// Build the set of audit attribution keys for all dead-code findings in
/// `results`.
///
/// Each key is a stable string that uniquely identifies one finding across
/// runs (e.g. `unused-file:src/dead.ts`, `unused-export:src/a.ts:Foo`).
/// `retain_introduced_dead_code` and `annotate_dead_code_json` use the same
/// key format to diff the current run against a base snapshot.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an audit key (add a loop)
/// or has no key representation today (bind with underscore and document why).
///
/// Sibling exhaustive sites: `fallow_core::changed_files::filter_results_by_changed_files`,
/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection severity branches in
/// `crates/cli/src/check/rules.rs` (`apply_rules`, `has_error_severity_issues`).
/// TypeScript mirror: `editors/vscode/scripts/codegen-types.mjs` (`BARE_DEAD_CODE_ALIASES`).
#[expect(
    clippy::too_many_lines,
    reason = "one key-builder block per issue type keeps the audit-attribution key shape local and easy to audit; the count grows linearly with new issue types"
)]
pub(super) fn dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let fallow_core::results::AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        type_only_dependencies,
        test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        stale_suppressions,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        // Non-finding fields: counts and metadata, not attributable to a key.
        suppression_count: _suppression_count,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no dead-code key representation today.
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
    } = results;

    let mut keys = FxHashSet::default();
    for item in unused_files {
        keys.insert(format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ));
    }
    for item in unused_exports {
        keys.insert(format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in unused_types {
        keys.insert(format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in private_type_leaks {
        keys.insert(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ));
    }
    for item in unused_dependencies
        .iter()
        .map(|f| &f.dep)
        .chain(unused_dev_dependencies.iter().map(|f| &f.dep))
        .chain(unused_optional_dependencies.iter().map(|f| &f.dep))
    {
        keys.insert(unused_dependency_key(item, root));
    }
    for item in unused_enum_members {
        keys.insert(unused_member_key("unused-enum-member", &item.member, root));
    }
    for item in unused_class_members {
        keys.insert(unused_member_key("unused-class-member", &item.member, root));
    }
    for item in unresolved_imports {
        keys.insert(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ));
    }
    for item in unlisted_dependencies.iter().map(|f| &f.dep) {
        keys.insert(unlisted_dependency_key(item, root));
    }
    for item in duplicate_exports {
        let mut locations: Vec<String> = item
            .export
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keys.insert(format!(
            "duplicate-export:{}:{}",
            item.export.export_name,
            locations.join("|")
        ));
    }
    for item in type_only_dependencies {
        keys.insert(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in test_only_dependencies {
        keys.insert(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in circular_dependencies {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("circular-dependency:{}", files.join("|")));
    }
    for item in re_export_cycles {
        let kind = match item.cycle.kind {
            fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
            fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
        };
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("re-export-cycle:{kind}:{}", files.join("|")));
    }
    for item in boundary_violations {
        keys.insert(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ));
    }
    for item in boundary_coverage_violations {
        keys.insert(format!(
            "boundary-coverage:{}",
            relative_key_path(&item.violation.path, root)
        ));
    }
    for item in boundary_call_violations {
        keys.insert(format!(
            "boundary-call:{}:{}",
            relative_key_path(&item.violation.path, root),
            item.violation.callee
        ));
    }
    for item in stale_suppressions {
        keys.insert(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ));
    }
    for item in unresolved_catalog_references {
        keys.insert(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ));
    }
    for item in unused_catalog_entries {
        keys.insert(unused_catalog_entry_key(&item.entry, root));
    }
    for item in empty_catalog_groups {
        keys.insert(empty_catalog_group_key(&item.group, root));
    }
    for item in unused_dependency_overrides {
        keys.insert(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    for item in misconfigured_dependency_overrides {
        keys.insert(format!(
            "misconfigured-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    keys
}

/// Retain only findings whose audit key was NOT present in `base` (i.e. was
/// introduced on the current branch).
///
/// When `base` is `None` (no baseline), all findings are kept.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an introduced-retain (add a
/// retain block) or has no key representation today (bind with underscore and
/// document why).
///
/// Sibling exhaustive sites: `fallow_core::changed_files::filter_results_by_changed_files`,
/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection severity branches in
/// `crates/cli/src/check/rules.rs` (`apply_rules`, `has_error_severity_issues`).
/// TypeScript mirror: `editors/vscode/scripts/codegen-types.mjs` (`BARE_DEAD_CODE_ALIASES`).
#[expect(
    clippy::too_many_lines,
    reason = "one retain block per issue type keeps the gate-filter local and grep-friendly; the count grows linearly with new issue types and parallels dead_code_keys"
)]
pub(super) fn retain_introduced_dead_code(
    results: &mut fallow_core::results::AnalysisResults,
    root: &Path,
    base: Option<&FxHashSet<String>>,
) {
    let Some(base) = base else {
        return;
    };

    // Compute the introduced set before taking any mutable borrows. Note the
    // order differs from the pre-destructure code, which narrowed
    // unused_files/exports/types first and computed keys from the narrowed
    // results. Computing from the un-narrowed results is equivalent: those
    // retains keep exactly the items whose key is NOT in `base`, and the
    // `!base.contains(key)` filter below removes the same base-member keys
    // from the full key set, so `introduced` is identical either way.
    let introduced = dead_code_keys(results, root)
        .into_iter()
        .filter(|key| !base.contains(key))
        .collect::<FxHashSet<_>>();
    let keep = |key: String| introduced.contains(&key);

    let fallow_core::results::AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        type_only_dependencies,
        test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        stale_suppressions,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        // Non-finding fields: counts and metadata, not subject to base-keyed
        // filtering.
        suppression_count: _suppression_count,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no key representation and are not filtered
        // here.
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
    } = results;

    // The three "fast path" retains use a direct base-lookup rather than the
    // introduced set; both predicates are equivalent for these collections
    // (see the `introduced` comment above), so this preserves the original
    // behavior.
    unused_files.retain(|item| {
        !base.contains(&format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ))
    });
    unused_exports.retain(|item| {
        !base.contains(&format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    unused_types.retain(|item| {
        !base.contains(&format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    private_type_leaks.retain(|item| {
        keep(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ))
    });
    unused_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_dev_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_optional_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_enum_members
        .retain(|item| keep(unused_member_key("unused-enum-member", &item.member, root)));
    unused_class_members
        .retain(|item| keep(unused_member_key("unused-class-member", &item.member, root)));
    unresolved_imports.retain(|item| {
        keep(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ))
    });
    unlisted_dependencies.retain(|item| keep(unlisted_dependency_key(&item.dep, root)));
    duplicate_exports.retain(|item| {
        let mut locations: Vec<String> = item
            .export
            .locations
            .iter()
            .map(|loc| relative_key_path(&loc.path, root))
            .collect();
        locations.sort();
        locations.dedup();
        keep(format!(
            "duplicate-export:{}:{}",
            item.export.export_name,
            locations.join("|")
        ))
    });
    type_only_dependencies.retain(|item| {
        keep(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    test_only_dependencies.retain(|item| {
        keep(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    circular_dependencies.retain(|item| {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("circular-dependency:{}", files.join("|")))
    });
    re_export_cycles.retain(|item| {
        let kind = match item.cycle.kind {
            fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
            fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
        };
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("re-export-cycle:{kind}:{}", files.join("|")))
    });
    boundary_violations.retain(|item| {
        keep(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ))
    });
    boundary_coverage_violations.retain(|item| {
        keep(format!(
            "boundary-coverage:{}",
            relative_key_path(&item.violation.path, root)
        ))
    });
    boundary_call_violations.retain(|item| {
        keep(format!(
            "boundary-call:{}:{}",
            relative_key_path(&item.violation.path, root),
            item.violation.callee
        ))
    });
    stale_suppressions.retain(|item| {
        keep(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ))
    });
    unresolved_catalog_references.retain(|item| {
        keep(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ))
    });
    unused_catalog_entries.retain(|item| keep(unused_catalog_entry_key(&item.entry, root)));
    empty_catalog_groups.retain(|item| keep(empty_catalog_group_key(&item.group, root)));
    unused_dependency_overrides.retain(|item| {
        keep(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ))
    });
    misconfigured_dependency_overrides.retain(|item| {
        keep(format!(
            "misconfigured-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ))
    });
}

fn issue_was_introduced(key: &str, base: &FxHashSet<String>) -> bool {
    !base.contains(key)
}

fn annotate_issue_array<I>(json: &mut serde_json::Value, key: &str, introduced: I)
where
    I: IntoIterator<Item = bool>,
{
    let Some(items) = json.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    for (item, introduced) in items.iter_mut().zip(introduced) {
        if let serde_json::Value::Object(map) = item {
            map.insert("introduced".to_string(), serde_json::json!(introduced));
        }
    }
}

pub(super) fn annotate_dead_code_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_files",
        results.unused_files.iter().map(|item| {
            issue_was_introduced(
                &format!("unused-file:{}", relative_key_path(&item.file.path, root)),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_exports",
        results.unused_exports.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-export:{}:{}",
                    relative_key_path(&item.export.path, root),
                    item.export.export_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_types",
        results.unused_types.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-type:{}:{}",
                    relative_key_path(&item.export.path, root),
                    item.export.export_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "private_type_leaks",
        results.private_type_leaks.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "private-type-leak:{}:{}:{}",
                    relative_key_path(&item.leak.path, root),
                    item.leak.export_name,
                    item.leak.type_name
                ),
                base,
            )
        }),
    );
    annotate_dependency_json(json, results, root, base);
    annotate_member_json(json, results, root, base);
    annotate_issue_array(
        json,
        "unresolved_imports",
        results.unresolved_imports.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-import:{}:{}",
                    relative_key_path(&item.import.path, root),
                    item.import.specifier
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unlisted_dependencies",
        results
            .unlisted_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unlisted_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "duplicate_exports",
        results.duplicate_exports.iter().map(|item| {
            let mut locations: Vec<String> = item
                .export
                .locations
                .iter()
                .map(|loc| relative_key_path(&loc.path, root))
                .collect();
            locations.sort();
            locations.dedup();
            issue_was_introduced(
                &format!(
                    "duplicate-export:{}:{}",
                    item.export.export_name,
                    locations.join("|")
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "type_only_dependencies",
        results.type_only_dependencies.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "type-only-dependency:{}:{}",
                    relative_key_path(&item.dep.path, root),
                    item.dep.package_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "test_only_dependencies",
        results.test_only_dependencies.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "test-only-dependency:{}:{}",
                    relative_key_path(&item.dep.path, root),
                    item.dep.package_name
                ),
                base,
            )
        }),
    );
    annotate_graph_json(json, results, root, base);
    annotate_catalog_json(json, results, root, base);
}

fn annotate_dependency_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_dependencies",
        results
            .unused_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dev_dependencies",
        results
            .unused_dev_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_optional_dependencies",
        results
            .unused_optional_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
}

fn annotate_member_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_enum_members",
        results.unused_enum_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-enum-member", &item.member, root),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_class_members",
        results.unused_class_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-class-member", &item.member, root),
                base,
            )
        }),
    );
}

fn annotate_graph_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "circular_dependencies",
        results.circular_dependencies.iter().map(|item| {
            let mut files: Vec<String> = item
                .cycle
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("circular-dependency:{}", files.join("|")), base)
        }),
    );
    annotate_issue_array(
        json,
        "re_export_cycles",
        results.re_export_cycles.iter().map(|item| {
            let kind = match item.cycle.kind {
                fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
                fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
            };
            let mut files: Vec<String> = item
                .cycle
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("re-export-cycle:{kind}:{}", files.join("|")), base)
        }),
    );
    annotate_issue_array(
        json,
        "boundary_violations",
        results.boundary_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-violation:{}:{}:{}",
                    relative_key_path(&item.violation.from_path, root),
                    relative_key_path(&item.violation.to_path, root),
                    item.violation.import_specifier
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "boundary_coverage_violations",
        results.boundary_coverage_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-coverage:{}",
                    relative_key_path(&item.violation.path, root)
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "boundary_call_violations",
        results.boundary_call_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-call:{}:{}",
                    relative_key_path(&item.violation.path, root),
                    item.violation.callee
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "stale_suppressions",
        results.stale_suppressions.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "stale-suppression:{}:{}",
                    relative_key_path(&item.path, root),
                    item.description()
                ),
                base,
            )
        }),
    );
}

fn annotate_catalog_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unresolved_catalog_references",
        results.unresolved_catalog_references.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-catalog-reference:{}:{}:{}:{}",
                    relative_key_path(&item.reference.path, root),
                    item.reference.line,
                    item.reference.catalog_name,
                    item.reference.entry_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_catalog_entries",
        results
            .unused_catalog_entries
            .iter()
            .map(|item| issue_was_introduced(&unused_catalog_entry_key(&item.entry, root), base)),
    );
    annotate_issue_array(
        json,
        "empty_catalog_groups",
        results
            .empty_catalog_groups
            .iter()
            .map(|item| issue_was_introduced(&empty_catalog_group_key(&item.group, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dependency_overrides",
        results.unused_dependency_overrides.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-dependency-override:{}:{}:{}",
                    relative_key_path(&item.entry.path, root),
                    item.entry.line,
                    item.entry.raw_key
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "misconfigured_dependency_overrides",
        results
            .misconfigured_dependency_overrides
            .iter()
            .map(|item| {
                issue_was_introduced(
                    &format!(
                        "misconfigured-dependency-override:{}:{}:{}",
                        relative_key_path(&item.entry.path, root),
                        item.entry.line,
                        item.entry.raw_key
                    ),
                    base,
                )
            }),
    );
}

pub(super) fn annotate_health_json(
    json: &mut serde_json::Value,
    report: &crate::health_types::HealthReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("findings")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, finding) in items.iter_mut().zip(&report.findings) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(
                    &health_finding_key(finding, root),
                    base
                )),
            );
        }
    }
}

pub(super) fn annotate_dupes_json(
    json: &mut serde_json::Value,
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("clone_groups")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, group) in items.iter_mut().zip(&report.clone_groups) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(&dupe_group_key(group, root), base)),
            );
        }
    }
}

pub(super) fn health_keys(
    report: &crate::health_types::HealthReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .findings
        .iter()
        .map(|finding| health_finding_key(finding, root))
        .collect()
}

pub(super) fn health_finding_key(
    finding: &crate::health_types::ComplexityViolation,
    root: &Path,
) -> String {
    format!(
        "complexity:{}:{}:{:?}",
        relative_key_path(&finding.path, root),
        finding.name,
        finding.exceeded
    )
}

pub(super) fn dupes_keys(
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .clone_groups
        .iter()
        .map(|group| dupe_group_key(group, root))
        .collect()
}

pub(super) fn dupe_group_key(group: &fallow_core::duplicates::CloneGroup, root: &Path) -> String {
    let mut files: Vec<String> = group
        .instances
        .iter()
        .map(|instance| relative_key_path(&instance.file, root))
        .collect();
    files.sort();
    files.dedup();
    let mut hasher = DefaultHasher::new();
    for instance in &group.instances {
        instance.fragment.hash(&mut hasher);
    }
    format!(
        "dupe:{}:{}:{}:{:x}",
        files.join("|"),
        group.token_count,
        group.line_count,
        hasher.finish()
    )
}
