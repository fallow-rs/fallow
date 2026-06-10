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

#[expect(
    clippy::too_many_lines,
    reason = "one key-builder block per issue type keeps the audit-attribution key shape local and easy to audit; the count grows linearly with new issue types"
)]
pub(super) fn dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let mut keys = FxHashSet::default();
    for item in &results.unused_files {
        keys.insert(format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ));
    }
    for item in &results.unused_exports {
        keys.insert(format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in &results.unused_types {
        keys.insert(format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ));
    }
    for item in &results.private_type_leaks {
        keys.insert(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ));
    }
    for item in results
        .unused_dependencies
        .iter()
        .map(|f| &f.dep)
        .chain(results.unused_dev_dependencies.iter().map(|f| &f.dep))
        .chain(results.unused_optional_dependencies.iter().map(|f| &f.dep))
    {
        keys.insert(unused_dependency_key(item, root));
    }
    for item in &results.unused_enum_members {
        keys.insert(unused_member_key("unused-enum-member", &item.member, root));
    }
    for item in &results.unused_class_members {
        keys.insert(unused_member_key("unused-class-member", &item.member, root));
    }
    for item in &results.unresolved_imports {
        keys.insert(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ));
    }
    for item in results.unlisted_dependencies.iter().map(|f| &f.dep) {
        keys.insert(unlisted_dependency_key(item, root));
    }
    for item in &results.duplicate_exports {
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
    for item in &results.type_only_dependencies {
        keys.insert(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in &results.test_only_dependencies {
        keys.insert(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ));
    }
    for item in &results.circular_dependencies {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keys.insert(format!("circular-dependency:{}", files.join("|")));
    }
    for item in &results.re_export_cycles {
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
    for item in &results.boundary_violations {
        keys.insert(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ));
    }
    for item in &results.boundary_coverage_violations {
        keys.insert(format!(
            "boundary-coverage:{}",
            relative_key_path(&item.violation.path, root)
        ));
    }
    for item in &results.boundary_call_violations {
        keys.insert(format!(
            "boundary-call:{}:{}",
            relative_key_path(&item.violation.path, root),
            item.violation.callee
        ));
    }
    for item in &results.stale_suppressions {
        keys.insert(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ));
    }
    for item in &results.unresolved_catalog_references {
        keys.insert(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ));
    }
    for item in &results.unused_catalog_entries {
        keys.insert(unused_catalog_entry_key(&item.entry, root));
    }
    for item in &results.empty_catalog_groups {
        keys.insert(empty_catalog_group_key(&item.group, root));
    }
    for item in &results.unused_dependency_overrides {
        keys.insert(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    for item in &results.misconfigured_dependency_overrides {
        keys.insert(format!(
            "misconfigured-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ));
    }
    keys
}

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
    results.unused_files.retain(|item| {
        !base.contains(&format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ))
    });
    results.unused_exports.retain(|item| {
        !base.contains(&format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    results.unused_types.retain(|item| {
        !base.contains(&format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    let introduced = dead_code_keys(results, root)
        .into_iter()
        .filter(|key| !base.contains(key))
        .collect::<FxHashSet<_>>();
    let keep = |key: String| introduced.contains(&key);
    results.private_type_leaks.retain(|item| {
        keep(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ))
    });
    results
        .unused_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_dev_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_optional_dependencies
        .retain(|item| keep(unused_dependency_key(&item.dep, root)));
    results
        .unused_enum_members
        .retain(|item| keep(unused_member_key("unused-enum-member", &item.member, root)));
    results
        .unused_class_members
        .retain(|item| keep(unused_member_key("unused-class-member", &item.member, root)));
    results.unresolved_imports.retain(|item| {
        keep(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ))
    });
    results
        .unlisted_dependencies
        .retain(|item| keep(unlisted_dependency_key(&item.dep, root)));
    results.duplicate_exports.retain(|item| {
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
    results.type_only_dependencies.retain(|item| {
        keep(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    results.test_only_dependencies.retain(|item| {
        keep(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    results.circular_dependencies.retain(|item| {
        let mut files: Vec<String> = item
            .cycle
            .files
            .iter()
            .map(|path| relative_key_path(path, root))
            .collect();
        files.sort();
        keep(format!("circular-dependency:{}", files.join("|")))
    });
    results.re_export_cycles.retain(|item| {
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
    results.boundary_violations.retain(|item| {
        keep(format!(
            "boundary-violation:{}:{}:{}",
            relative_key_path(&item.violation.from_path, root),
            relative_key_path(&item.violation.to_path, root),
            item.violation.import_specifier
        ))
    });
    results.boundary_coverage_violations.retain(|item| {
        keep(format!(
            "boundary-coverage:{}",
            relative_key_path(&item.violation.path, root)
        ))
    });
    results.boundary_call_violations.retain(|item| {
        keep(format!(
            "boundary-call:{}:{}",
            relative_key_path(&item.violation.path, root),
            item.violation.callee
        ))
    });
    results.stale_suppressions.retain(|item| {
        keep(format!(
            "stale-suppression:{}:{}",
            relative_key_path(&item.path, root),
            item.description()
        ))
    });
    results.unresolved_catalog_references.retain(|item| {
        keep(format!(
            "unresolved-catalog-reference:{}:{}:{}:{}",
            relative_key_path(&item.reference.path, root),
            item.reference.line,
            item.reference.catalog_name,
            item.reference.entry_name
        ))
    });
    results
        .unused_catalog_entries
        .retain(|item| keep(unused_catalog_entry_key(&item.entry, root)));
    results
        .empty_catalog_groups
        .retain(|item| keep(empty_catalog_group_key(&item.group, root)));
    results.unused_dependency_overrides.retain(|item| {
        keep(format!(
            "unused-dependency-override:{}:{}:{}",
            relative_key_path(&item.entry.path, root),
            item.entry.line,
            item.entry.raw_key
        ))
    });
    results.misconfigured_dependency_overrides.retain(|item| {
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
