use rustc_hash::FxHashMap;

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Url,
};

use fallow_core::results::AnalysisResults;

use super::{FIRST_LINE_RANGE, doc_link};

#[expect(
    clippy::cast_possible_truncation,
    reason = "identifier lengths are bounded by source size"
)]
pub fn push_export_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    let exports_iter = results.unused_exports.iter().map(|f| &f.export);
    let types_iter = results.unused_types.iter().map(|f| &f.export);
    for (exports, code, anchor, msg_prefix) in [
        (
            Box::new(exports_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedExport>>,
            "unused-export",
            "unused-exports",
            "Export" as &str,
        ),
        (
            Box::new(types_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedExport>>,
            "unused-type",
            "unused-types",
            "Type export",
        ),
    ] {
        for export in exports {
            if let Ok(uri) = Url::from_file_path(&export.path) {
                let line = export.line.saturating_sub(1);
                map.entry(uri).or_default().push(Diagnostic {
                    range: Range {
                        start: Position {
                            line,
                            character: export.col,
                        },
                        end: Position {
                            line,
                            character: export.col + export.export_name.len() as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::HINT),
                    source: Some("fallow".to_string()),
                    code: Some(NumberOrString::String(code.to_string())),
                    code_description: doc_link(anchor),
                    message: format!("{msg_prefix} '{}' is unused", export.export_name),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Default::default()
                });
            }
        }
    }

    for leak in &results.private_type_leaks {
        if let Ok(uri) = Url::from_file_path(&leak.leak.path) {
            let line = leak.leak.line.saturating_sub(1);
            map.entry(uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: leak.leak.col,
                    },
                    end: Position {
                        line,
                        character: leak.leak.col + leak.leak.type_name.len() as u32,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("private-type-leak".to_string())),
                code_description: doc_link("private-type-leaks"),
                message: format!(
                    "Export '{}' references private type '{}'",
                    leak.leak.export_name, leak.leak.type_name
                ),
                ..Default::default()
            });
        }
    }
}

pub fn push_file_diagnostics(map: &mut FxHashMap<Url, Vec<Diagnostic>>, results: &AnalysisResults) {
    for file in &results.unused_files {
        if let Ok(uri) = Url::from_file_path(&file.file.path) {
            map.entry(uri).or_default().push(Diagnostic {
                range: FIRST_LINE_RANGE,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-file".to_string())),
                code_description: doc_link("unused-files"),
                message: "File is not reachable from any entry point".to_string(),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                ..Default::default()
            });
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "specifier lengths are bounded by source size"
)]
pub fn push_import_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for import in &results.unresolved_imports {
        if let Ok(uri) = Url::from_file_path(&import.import.path) {
            let line = import.import.line.saturating_sub(1);
            map.entry(uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: import.import.specifier_col,
                    },
                    end: Position {
                        line,
                        // +2 accounts for the surrounding quotes on the string literal
                        character: import.import.specifier_col
                            + import.import.specifier.len() as u32
                            + 2,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unresolved-import".to_string())),
                code_description: doc_link("unresolved-imports"),
                message: format!("Cannot find module '{}'", import.import.specifier),
                ..Default::default()
            });
        }
    }
}

pub fn push_dep_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
    package_json_uri: Option<&Url>,
    root: &std::path::Path,
) {
    // Unused deps: dependencies, devDependencies, optionalDependencies
    type DepIter<'a> = Box<dyn Iterator<Item = &'a fallow_core::results::UnusedDependency> + 'a>;
    let groups: [(DepIter<'_>, &str, &str, &str); 3] = [
        (
            Box::new(results.unused_dependencies.iter().map(|f| &f.dep)),
            "unused-dependency",
            "unused-dependencies",
            "Unused dependency",
        ),
        (
            Box::new(results.unused_dev_dependencies.iter().map(|f| &f.dep)),
            "unused-dev-dependency",
            "unused-devdependencies",
            "Unused devDependency",
        ),
        (
            Box::new(results.unused_optional_dependencies.iter().map(|f| &f.dep)),
            "unused-optional-dependency",
            "unused-optionaldependencies",
            "Unused optionalDependency",
        ),
    ];
    for (deps, code, anchor, msg_prefix) in groups {
        for dep in deps {
            if let Ok(dep_uri) = Url::from_file_path(&dep.path) {
                let line = dep.line.saturating_sub(1);
                map.entry(dep_uri).or_default().push(Diagnostic {
                    range: Range {
                        start: Position { line, character: 0 },
                        end: Position {
                            line,
                            character: u32::MAX,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("fallow".to_string()),
                    code: Some(NumberOrString::String(code.to_string())),
                    code_description: doc_link(anchor),
                    message: format!("{msg_prefix}: {}", dep.package_name),
                    ..Default::default()
                });
            }
        }
    }

    // Unlisted deps still use root package.json
    if let Some(uri) = package_json_uri {
        for dep in &results.unlisted_dependencies {
            map.entry(uri.clone()).or_default().push(Diagnostic {
                range: FIRST_LINE_RANGE,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unlisted-dependency".to_string())),
                code_description: doc_link("unlisted-dependencies"),
                message: format!(
                    "Unlisted dependency: {} (used but not in package.json)",
                    dep.dep.package_name
                ),
                ..Default::default()
            });
        }
    }

    // Type-only dependencies: could be moved to devDependencies
    for dep in &results.type_only_dependencies {
        if let Ok(dep_uri) = Url::from_file_path(&dep.dep.path) {
            let line = dep.dep.line.saturating_sub(1);
            map.entry(dep_uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("type-only-dependency".to_string())),
                code_description: doc_link("type-only-dependencies"),
                message: format!(
                    "Type-only dependency: {} (only used via type imports, could be a devDependency)",
                    dep.dep.package_name
                ),
                ..Default::default()
            });
        }
    }

    // Test-only dependencies: could be moved to devDependencies
    for dep in &results.test_only_dependencies {
        if let Ok(dep_uri) = Url::from_file_path(&dep.dep.path) {
            let line = dep.dep.line.saturating_sub(1);
            map.entry(dep_uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("test-only-dependency".to_string())),
                code_description: doc_link("test-only-dependencies"),
                message: format!(
                    "Production dependency '{}' is only imported by test files; consider moving to devDependencies",
                    dep.dep.package_name
                ),
                ..Default::default()
            });
        }
    }

    // Unused pnpm catalog entries in pnpm-workspace.yaml.
    // entry.path is project-root-relative; Url::from_file_path requires an
    // absolute path, so join against the analyzer root before constructing.
    for entry in &results.unused_catalog_entries {
        if let Ok(entry_uri) = Url::from_file_path(root.join(&entry.path)) {
            let line = entry.line.saturating_sub(1);
            let message = if entry.catalog_name == "default" {
                format!(
                    "Unused catalog entry: '{}' is not referenced by any workspace package",
                    entry.entry_name
                )
            } else {
                format!(
                    "Unused catalog entry: '{}' in catalog '{}' is not referenced by any workspace package",
                    entry.entry_name, entry.catalog_name
                )
            };
            map.entry(entry_uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position {
                        line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-catalog-entry".to_string())),
                code_description: doc_link("unused-catalog-entries"),
                message,
                ..Default::default()
            });
        }
    }

    push_empty_catalog_group_diagnostics(map, results, root);

    push_unresolved_catalog_reference_diagnostics(map, results);
    push_dependency_override_diagnostics(map, results);
}

fn push_empty_catalog_group_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
    root: &std::path::Path,
) {
    for group in &results.empty_catalog_groups {
        let Ok(uri) = Url::from_file_path(root.join(&group.path)) else {
            continue;
        };
        let line = group.line.saturating_sub(1);
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("empty-catalog-group".to_string())),
            code_description: doc_link("empty-catalog-groups"),
            message: format!(
                "Empty catalog group: '{}' has no entries",
                group.catalog_name
            ),
            ..Default::default()
        });
    }
}

/// Emit one `ERROR`-severity diagnostic per unresolved-catalog-reference
/// finding. The finding's `path` is stored as an absolute filesystem path
/// (matching the existing convention for path-anchored findings), so
/// `Url::from_file_path` can be called directly.
fn push_unresolved_catalog_reference_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    use std::fmt::Write as _;
    for finding in &results.unresolved_catalog_references {
        let Ok(uri) = Url::from_file_path(&finding.path) else {
            continue;
        };
        let line = finding.line.saturating_sub(1);
        let catalog_phrase = if finding.catalog_name == "default" {
            "the default catalog".to_string()
        } else {
            format!("catalog '{}'", finding.catalog_name)
        };
        let mut message = format!(
            "Unresolved catalog reference: '{}' is not declared in {}",
            finding.entry_name, catalog_phrase,
        );
        if !finding.available_in_catalogs.is_empty() {
            let _ = write!(
                message,
                " (available in: {})",
                finding.available_in_catalogs.join(", ")
            );
        }
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "unresolved-catalog-reference".to_string(),
            )),
            code_description: doc_link("unresolved-catalog-references"),
            message,
            ..Default::default()
        });
    }
}

/// Emit diagnostics for unused and misconfigured pnpm dependency-override
/// findings. Both finding types carry an absolute `path` (matching the
/// `UnresolvedCatalogReference` convention so `--changed-since` and per-file
/// overrides.rules can compare directly). `Url::from_file_path` accepts the
/// path as-is. Severity matches the default rule severity: unused =
/// `WARNING`, misconfigured = `ERROR` (pnpm refuses to install).
fn push_dependency_override_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    use std::fmt::Write as _;
    for finding in &results.unused_dependency_overrides {
        let Ok(uri) = Url::from_file_path(&finding.path) else {
            continue;
        };
        let line = finding.line.saturating_sub(1);
        let mut message = format!(
            "Unused dependency override: `{}` forces `{}` to `{}` but it is not declared by any workspace package or resolved in pnpm-lock.yaml",
            finding.raw_key, finding.target_package, finding.version_range,
        );
        if let Some(hint) = &finding.hint {
            let _ = write!(message, " ({hint})");
        }
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "unused-dependency-override".to_string(),
            )),
            code_description: doc_link("unused-dependency-overrides"),
            message,
            ..Default::default()
        });
    }
    for finding in &results.misconfigured_dependency_overrides {
        let Ok(uri) = Url::from_file_path(&finding.path) else {
            continue;
        };
        let line = finding.line.saturating_sub(1);
        let message = format!(
            "Misconfigured dependency override: `{}` -> `{}` ({})",
            finding.raw_key,
            finding.raw_value,
            finding.reason.describe(),
        );
        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position { line, character: 0 },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "misconfigured-dependency-override".to_string(),
            )),
            code_description: doc_link("misconfigured-dependency-overrides"),
            message,
            ..Default::default()
        });
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "member name lengths are bounded by source size"
)]
pub fn push_member_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    let enum_iter = results.unused_enum_members.iter().map(|f| &f.member);
    let class_iter = results.unused_class_members.iter().map(|f| &f.member);
    for (members, code, anchor, kind_label) in [
        (
            Box::new(enum_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedMember>>,
            "unused-enum-member",
            "unused-enum-members",
            "Enum member" as &str,
        ),
        (
            Box::new(class_iter) as Box<dyn Iterator<Item = &fallow_core::results::UnusedMember>>,
            "unused-class-member",
            "unused-class-members",
            "Class member",
        ),
    ] {
        for member in members {
            if let Ok(uri) = Url::from_file_path(&member.path) {
                let line = member.line.saturating_sub(1);
                map.entry(uri).or_default().push(Diagnostic {
                    range: Range {
                        start: Position {
                            line,
                            character: member.col,
                        },
                        end: Position {
                            line,
                            character: member.col + member.member_name.len() as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::HINT),
                    source: Some("fallow".to_string()),
                    code: Some(NumberOrString::String(code.to_string())),
                    code_description: doc_link(anchor),
                    message: format!(
                        "{kind_label} '{}.{}' is unused",
                        member.parent_name, member.member_name
                    ),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Default::default()
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_core::duplicates::{DuplicationReport, DuplicationStats};
    use fallow_core::extract::MemberKind;
    use fallow_core::results::{
        AnalysisResults, DependencyLocation, EmptyCatalogGroup, ImportSite, TestOnlyDependency,
        TestOnlyDependencyFinding, TypeOnlyDependency, TypeOnlyDependencyFinding,
        UnlistedDependency, UnlistedDependencyFinding, UnresolvedCatalogReference,
        UnresolvedImport, UnresolvedImportFinding, UnusedCatalogEntry, UnusedClassMemberFinding,
        UnusedDependency, UnusedDependencyFinding, UnusedDevDependencyFinding,
        UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding, UnusedFile, UnusedFileFinding,
        UnusedMember, UnusedOptionalDependencyFinding, UnusedTypeFinding,
    };
    use tower_lsp::lsp_types::{DiagnosticSeverity, DiagnosticTag, NumberOrString, Url};

    use crate::diagnostics::{FIRST_LINE_RANGE, build_diagnostics};

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn empty_duplication() -> DuplicationReport {
        DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 0,
                files_with_clones: 0,
                total_lines: 0,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn unused_export_produces_hint_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 7,
                span_start: 40,
                is_re_export: false,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/utils.ts")).unwrap();
        let file_diags = diags.get(&uri).expect("should have diagnostics for file");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(d.message, "Export 'helper' is unused");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-export".to_string()))
        );
        assert_eq!(d.source, Some("fallow".to_string()));
        // Line is 1-based in results, 0-based in LSP
        assert_eq!(d.range.start.line, 4);
        assert_eq!(d.range.start.character, 7);
        // End character = col + export_name.len()
        assert_eq!(d.range.end.character, 7 + "helper".len() as u32);
        assert_eq!(d.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn unused_type_produces_hint_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/types.ts"),
                export_name: "MyType".to_string(),
                is_type_only: true,
                line: 10,
                col: 0,
                span_start: 100,
                is_re_export: false,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/types.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(d.message, "Type export 'MyType' is unused");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-type".to_string()))
        );
    }

    #[test]
    fn unused_file_produces_warning_at_zero_range() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/dead.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.range, FIRST_LINE_RANGE);
        assert_eq!(d.message, "File is not reachable from any entry point");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-file".to_string()))
        );
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn unresolved_import_produces_error_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        // import { foo } from './missing-module'
        //                     ^--- specifier_col = 20 (quote position)
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/app.ts"),
                specifier: "./missing-module".to_string(),
                line: 3,
                col: 0,
                specifier_col: 20,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/app.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.message, "Cannot find module './missing-module'");
        assert_eq!(d.range.start.line, 2); // 1-based -> 0-based
        // Range covers the specifier string literal including quotes
        assert_eq!(d.range.start.character, 20);
        assert_eq!(
            d.range.end.character,
            20 + "./missing-module".len() as u32 + 2
        );
    }

    #[test]
    fn unused_dependency_produces_warning_at_package_json() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.message, "Unused dependency: lodash");
        assert_eq!(d.range.start.line, 4); // 1-based line 5 → 0-based line 4
    }

    #[test]
    fn unused_dev_dependency_produces_warning() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "prettier".to_string(),
                location: DependencyLocation::DevDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.message, "Unused devDependency: prettier");
    }

    #[test]
    fn unlisted_dependency_uses_root_package_json() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_string(),
                    imported_from: vec![ImportSite {
                        path: root.join("src/cli.ts"),
                        line: 2,
                        col: 0,
                    }],
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert!(d.message.contains("chalk"));
        assert!(d.message.contains("Unlisted dependency"));
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn unused_enum_member_produces_hint() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Color".to_string(),
                member_name: "Blue".to_string(),
                kind: MemberKind::EnumMember,
                line: 4,
                col: 2,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/enums.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(d.message, "Enum member 'Color.Blue' is unused");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-enum-member".to_string()))
        );
        assert_eq!(d.range.start.line, 3);
        assert_eq!(d.range.start.character, 2);
        assert_eq!(d.range.end.character, 2 + "Blue".len() as u32);
    }

    #[test]
    fn unused_class_member_produces_hint() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "reset".to_string(),
                kind: MemberKind::ClassMethod,
                line: 20,
                col: 4,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/service.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(d.message, "Class member 'UserService.reset' is unused");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-class-member".to_string()))
        );
    }

    #[test]
    fn unused_optional_dependency_produces_warning() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: root.join("package.json"),
                    line: 12,
                    used_in_workspaces: Vec::new(),
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.message, "Unused optionalDependency: fsevents");
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "unused-optional-dependency".to_string()
            ))
        );
        assert_eq!(d.range.start.line, 11); // 1-based 12 -> 0-based 11
        assert_eq!(d.range.start.character, 0);
        assert_eq!(d.range.end.character, u32::MAX);
    }

    #[test]
    fn type_only_dependency_produces_information_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "@types/react".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("type-only-dependency".to_string()))
        );
        assert!(d.message.contains("@types/react"));
        assert!(d.message.contains("Type-only dependency"));
        assert!(d.message.contains("devDependency"));
        assert_eq!(d.range.start.line, 7); // 1-based 8 -> 0-based 7
        assert_eq!(d.range.start.character, 0);
        assert_eq!(d.range.end.character, u32::MAX);
    }

    #[test]
    fn test_only_dependency_produces_information_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "test-utils-lib".to_string(),
                    path: root.join("package.json"),
                    line: 5,
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("package.json")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("test-only-dependency".to_string()))
        );
        assert!(d.message.contains("test-utils-lib"));
        assert!(d.message.contains("test files"));
        assert!(d.message.contains("devDependencies"));
        assert_eq!(d.range.start.line, 4); // 1-based 5 -> 0-based 4
        assert_eq!(d.range.start.character, 0);
        assert_eq!(d.range.end.character, u32::MAX);
    }

    #[test]
    fn line_conversion_saturates_at_zero() {
        let root = test_root();
        // Line 0 in results (unusual) should become 0 in LSP, not underflow
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/edge.ts"),
                export_name: "x".to_string(),
                is_type_only: false,
                line: 0,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("src/edge.ts")).unwrap();
        let d = &diags[&uri][0];
        assert_eq!(d.range.start.line, 0);
    }

    #[test]
    fn unused_catalog_entry_produces_warning_diagnostic() {
        // Catalog entries store project-root-relative paths. The diagnostic
        // must build its URI by joining against the analyzer root, otherwise
        // Url::from_file_path silently fails on the relative path and no
        // squiggle ever lands on pnpm-workspace.yaml.
        let root = test_root();
        let mut results = AnalysisResults::default();
        results.unused_catalog_entries.push(UnusedCatalogEntry {
            entry_name: "is-even".to_string(),
            catalog_name: "default".to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 6,
            hardcoded_consumers: vec![],
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("catalog diagnostic should be keyed by the absolute YAML URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-catalog-entry".to_string()))
        );
        assert_eq!(d.source, Some("fallow".to_string()));
        assert!(d.message.contains("is-even"));
        // Line is 1-based in results, 0-based in LSP
        assert_eq!(d.range.start.line, 5);
    }

    #[test]
    fn unused_catalog_entry_message_mentions_named_catalog() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results.unused_catalog_entries.push(UnusedCatalogEntry {
            entry_name: "react-dom".to_string(),
            catalog_name: "react17".to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 12,
            hardcoded_consumers: vec![],
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
        let d = &diags[&uri][0];
        assert!(d.message.contains("react-dom"));
        assert!(
            d.message.contains("react17"),
            "named-catalog diagnostic must surface the catalog name, got: {}",
            d.message
        );
    }

    #[test]
    fn empty_catalog_group_produces_warning_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results.empty_catalog_groups.push(EmptyCatalogGroup {
            catalog_name: "legacy".to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 9,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("empty catalog diagnostic should be keyed by the absolute YAML URI");
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("empty-catalog-group".to_string()))
        );
        assert_eq!(d.source, Some("fallow".to_string()));
        assert!(d.message.contains("legacy"));
        assert_eq!(d.range.start.line, 8);
        assert_eq!(d.range.start.character, 0);
    }

    #[test]
    fn unresolved_catalog_reference_produces_error_diagnostic_with_absolute_uri() {
        // `UnresolvedCatalogReference.path` is stored as an absolute filesystem
        // path (matching the convention used by every other path-anchored
        // finding type), so the LSP can pass it directly into
        // `Url::from_file_path` without joining against any root.
        let root = test_root();
        let abs_path = root.join("packages/app/package.json");
        let mut results = AnalysisResults::default();
        results
            .unresolved_catalog_references
            .push(UnresolvedCatalogReference {
                entry_name: "old-react".to_string(),
                catalog_name: "react17".to_string(),
                path: abs_path.clone(),
                line: 14,
                available_in_catalogs: vec!["react18".to_string()],
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&abs_path).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("unresolved-catalog-reference diagnostic must be keyed by absolute URI");
        assert_eq!(file_diags.len(), 1);
        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "unresolved-catalog-reference".to_string()
            ))
        );
        assert!(d.message.contains("old-react"));
        assert!(d.message.contains("react17"));
        assert!(d.message.contains("available in: react18"));
        // Line 14 (1-based) -> LSP line 13 (0-based)
        assert_eq!(d.range.start.line, 13);
    }

    #[test]
    fn unresolved_catalog_reference_default_catalog_uses_default_phrasing() {
        let root = test_root();
        let abs_path = root.join("package.json");
        let mut results = AnalysisResults::default();
        results
            .unresolved_catalog_references
            .push(UnresolvedCatalogReference {
                entry_name: "foo".to_string(),
                catalog_name: "default".to_string(),
                path: abs_path.clone(),
                line: 5,
                available_in_catalogs: vec![],
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&abs_path).unwrap();
        let d = &diags[&uri][0];
        assert!(
            d.message.contains("the default catalog"),
            "bare `catalog:` should render as 'the default catalog', got: {}",
            d.message
        );
        assert!(
            !d.message.contains("available in"),
            "empty available_in_catalogs should not produce an 'available in' suffix",
        );
    }

    #[test]
    fn unused_dependency_override_produces_warning_diagnostic_with_absolute_uri() {
        // Override findings store project-root-relative paths (same convention
        // as UnusedCatalogEntry), so the diagnostic emitter must root.join
        // before calling Url::from_file_path. Asserting the key exists in the
        // map under the absolute URI proves the join happened.
        use fallow_core::results::{DependencyOverrideSource, UnusedDependencyOverride};

        let root = test_root();
        let mut results = AnalysisResults::default();
        let yaml_path = root.join("pnpm-workspace.yaml");
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverride {
                raw_key: "axios".to_string(),
                target_package: "axios".to_string(),
                parent_package: None,
                version_constraint: None,
                version_range: "^1.6.0".to_string(),
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
                path: yaml_path.clone(),
                line: 9,
                hint: Some("may be intentional transitive pin".to_string()),
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&yaml_path).unwrap();
        let file_diags = diags
            .get(&uri)
            .expect("unused-dependency-override diagnostic must key by absolute URI");
        assert_eq!(file_diags.len(), 1);
        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "unused-dependency-override".to_string()
            ))
        );
        assert!(d.message.contains("axios"));
        assert!(d.message.contains("^1.6.0"));
        assert!(
            d.message.contains("transitive pin"),
            "hint must surface in the diagnostic message, got: {}",
            d.message
        );
        assert_eq!(d.range.start.line, 8);
    }

    #[test]
    fn misconfigured_dependency_override_produces_error_diagnostic() {
        use fallow_core::results::{
            DependencyOverrideMisconfigReason, DependencyOverrideSource,
            MisconfiguredDependencyOverride,
        };

        let root = test_root();
        let json_path = root.join("package.json");
        let mut results = AnalysisResults::default();
        results
            .misconfigured_dependency_overrides
            .push(MisconfiguredDependencyOverride {
                raw_key: "@types/react@<<18".to_string(),
                target_package: None,
                raw_value: "18.0.0".to_string(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmPackageJson,
                path: json_path.clone(),
                line: 3,
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&json_path).unwrap();
        let d = &diags[&uri][0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "misconfigured-dependency-override".to_string()
            ))
        );
        assert!(d.message.contains("@types/react@<<18"));
        assert!(d.message.contains("override key cannot be parsed"));
        assert_eq!(d.range.start.line, 2);
    }
}
