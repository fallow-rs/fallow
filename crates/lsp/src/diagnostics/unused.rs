use rustc_hash::FxHashMap;

use ls_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Uri,
};

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_types::output_dead_code::{MutationEvidence, ReachabilityCaveat, caveat_suffix};

use super::{FIRST_LINE_RANGE, doc_link_for_code, finding_data};
use crate::position::{NamedAnchor, PositionMapper};

/// Append the run's caveat parenthetical to a diagnostic message.
///
/// The editor is the surface where the caveat matters most: the user is one
/// keystroke from the quick fix, and the workspace diagnostic that explains
/// why the evidence is thin lives in a JSON envelope they never see. The
/// suffix goes in the MESSAGE rather than in `relatedInformation` because a
/// related-information entry needs a second location to point at, and the one
/// location worth pointing at (the file the run could not read) is not part of
/// the editor results at all. The wording is the shared `caveat_suffix`, so the
/// editor, the human report, and the SARIF result message all read alike.
fn with_caveats(message: String, caveats: &[ReachabilityCaveat]) -> String {
    match caveat_suffix(caveats) {
        Some(suffix) => format!("{message}{suffix}"),
        None => message,
    }
}

pub fn push_export_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    let exports_iter = results
        .unused_exports
        .iter()
        .map(|f| (&f.export, f.reachability_caveats(), f.finding_id.as_deref()));
    let types_iter = results
        .unused_types
        .iter()
        .map(|f| (&f.export, f.reachability_caveats(), f.finding_id.as_deref()));
    for (exports, code, msg_prefix) in [
        (
            Box::new(exports_iter)
                as Box<
                    dyn Iterator<
                        Item = (
                            &fallow_api::editor_results::UnusedExport,
                            &[ReachabilityCaveat],
                            Option<&str>,
                        ),
                    >,
                >,
            "unused-export",
            "Export" as &str,
        ),
        (
            Box::new(types_iter)
                as Box<
                    dyn Iterator<
                        Item = (
                            &fallow_api::editor_results::UnusedExport,
                            &[ReachabilityCaveat],
                            Option<&str>,
                        ),
                    >,
                >,
            "unused-type",
            "Type export",
        ),
    ] {
        for (export, caveats, finding_id) in exports {
            let row = ExportRow {
                export,
                caveats,
                finding_id,
            };
            push_unused_export_diagnostic(map, &row, code, msg_prefix, mapper);
        }
    }

    push_private_type_leak_diagnostics(map, results, mapper);
}

/// One unused export or type export with the parts its diagnostic needs.
struct ExportRow<'a> {
    export: &'a fallow_api::editor_results::UnusedExport,
    caveats: &'a [ReachabilityCaveat],
    finding_id: Option<&'a str>,
}

/// Push one HINT diagnostic for an unused export or type export.
fn push_unused_export_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    row: &ExportRow<'_>,
    code: &str,
    msg_prefix: &str,
    mapper: &mut PositionMapper,
) {
    let ExportRow {
        export,
        caveats,
        finding_id,
    } = *row;
    let Some(uri) = Uri::from_file_path(&export.path) else {
        return;
    };
    let line = export.line.saturating_sub(1);
    let range = identifier_range(mapper, &export.path, line, export.col, &export.export_name);
    map.entry(uri).or_default().push(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::HINT),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: doc_link_for_code(code),
        message: with_caveats(
            format!("{msg_prefix} '{}' is unused", export.export_name),
            caveats,
        ),
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        data: finding_data(finding_id),
        ..Default::default()
    });
}

/// Push WARNING diagnostics for exports that reference a private type.
fn push_private_type_leak_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for leak in &results.private_type_leaks {
        if let Some(uri) = Uri::from_file_path(&leak.leak.path) {
            let line = leak.leak.line.saturating_sub(1);
            let range = identifier_range(
                mapper,
                &leak.leak.path,
                line,
                leak.leak.col,
                &leak.leak.type_name,
            );
            map.entry(uri).or_default().push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("private-type-leak".to_string())),
                code_description: doc_link_for_code("private-type-leak"),
                message: format!(
                    "Export '{}' references private type '{}'",
                    leak.leak.export_name, leak.leak.type_name
                ),
                data: finding_data(leak.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

pub fn push_file_diagnostics(map: &mut FxHashMap<Uri, Vec<Diagnostic>>, results: &AnalysisResults) {
    for file in &results.unused_files {
        if let Some(uri) = Uri::from_file_path(&file.file.path) {
            map.entry(uri).or_default().push(Diagnostic {
                range: FIRST_LINE_RANGE,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-file".to_string())),
                code_description: doc_link_for_code("unused-file"),
                message: with_caveats(
                    "File is not reachable from any entry point".to_string(),
                    file.reachability_caveats(),
                ),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                data: finding_data(file.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

pub fn push_import_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for import in &results.unresolved_imports {
        if let Some(uri) = Uri::from_file_path(&import.import.path) {
            let line = import.import.line.saturating_sub(1);
            let start = mapper.utf16_col(&import.import.path, line, import.import.specifier_col);
            let width =
                u32::try_from(import.import.specifier.encode_utf16().count()).unwrap_or(u32::MAX);
            map.entry(uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: start,
                    },
                    end: Position {
                        line,
                        character: start.saturating_add(width).saturating_add(2),
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unresolved-import".to_string())),
                code_description: doc_link_for_code("unresolved-import"),
                message: format!("Cannot find module '{}'", import.import.specifier),
                data: finding_data(import.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

pub fn push_dep_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    package_json_uri: Option<&Uri>,
    root: &std::path::Path,
) {
    type DepIter<'a> = Box<
        dyn Iterator<
                Item = (
                    &'a fallow_api::editor_results::UnusedDependency,
                    Option<&'a str>,
                ),
            > + 'a,
    >;
    let groups: [(DepIter<'_>, &str, &str); 3] = [
        (
            Box::new(
                results
                    .unused_dependencies
                    .iter()
                    .map(|f| (&f.dep, f.finding_id.as_deref())),
            ),
            "unused-dependency",
            "Unused dependency",
        ),
        (
            Box::new(
                results
                    .unused_dev_dependencies
                    .iter()
                    .map(|f| (&f.dep, f.finding_id.as_deref())),
            ),
            "unused-dev-dependency",
            "Unused devDependency",
        ),
        (
            Box::new(
                results
                    .unused_optional_dependencies
                    .iter()
                    .map(|f| (&f.dep, f.finding_id.as_deref())),
            ),
            "unused-optional-dependency",
            "Unused optionalDependency",
        ),
    ];
    for (deps, code, msg_prefix) in groups {
        for (dep, finding_id) in deps {
            push_unused_dependency_diagnostic(map, dep, finding_id, code, msg_prefix);
        }
    }

    push_unlisted_dependency_diagnostics(map, results, package_json_uri);
    push_unlisted_binary_diagnostics(map, results);

    push_type_only_dependency_diagnostics(map, results);
    push_test_only_dependency_diagnostics(map, results);
    push_dev_dependency_in_production_diagnostics(map, results);
    push_unused_catalog_entry_diagnostics(map, results, root);

    push_empty_catalog_group_diagnostics(map, results, root);

    push_unresolved_catalog_reference_diagnostics(map, results);
    push_dependency_override_diagnostics(map, results);
}

/// Push one full-line WARNING diagnostic for an unused dependency group entry.
fn push_unused_dependency_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    dep: &fallow_api::editor_results::UnusedDependency,
    finding_id: Option<&str>,
    code: &str,
    msg_prefix: &str,
) {
    let Some(dep_uri) = Uri::from_file_path(&dep.path) else {
        return;
    };
    let line = dep.line.saturating_sub(1);
    map.entry(dep_uri).or_default().push(Diagnostic {
        range: full_line_range(line),
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: doc_link_for_code(code),
        message: format!("{msg_prefix}: {}", dep.package_name),
        data: finding_data(finding_id),
        ..Default::default()
    });
}

/// Push WARNING diagnostics for unlisted dependencies, anchored at the root
/// `package.json` (when one is known).
fn push_unlisted_dependency_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    package_json_uri: Option<&Uri>,
) {
    let Some(uri) = package_json_uri else {
        return;
    };
    for dep in &results.unlisted_dependencies {
        map.entry(uri.clone()).or_default().push(Diagnostic {
            range: FIRST_LINE_RANGE,
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("unlisted-dependency".to_string())),
            code_description: doc_link_for_code("unlisted-dependency"),
            message: format!(
                "Unlisted dependency: {} (used but not in package.json)",
                dep.dep.package_name
            ),
            data: finding_data(dep.finding_id.as_deref()),
            ..Default::default()
        });
    }
}

/// Push one full-line WARNING diagnostic for each site that runs an unlisted
/// binary: the script line in `package.json`, or the process call in source.
fn push_unlisted_binary_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.unlisted_binaries {
        for site in &finding.binary.used_in {
            let Some(uri) = Uri::from_file_path(&site.path) else {
                continue;
            };
            map.entry(uri).or_default().push(Diagnostic {
                range: full_line_range(site.line.saturating_sub(1)),
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unlisted-binary".to_string())),
                code_description: doc_link_for_code("unlisted-binary"),
                message: format!(
                    "Unlisted binary: {} (no installed or listed package provides it)",
                    finding.binary.binary_name
                ),
                data: finding_data(finding.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

fn push_type_only_dependency_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for dep in &results.type_only_dependencies {
        if let Some(dep_uri) = Uri::from_file_path(&dep.dep.path) {
            let line = dep.dep.line.saturating_sub(1);
            map.entry(dep_uri).or_default().push(Diagnostic {
                range: full_line_range(line),
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("type-only-dependency".to_string())),
                code_description: doc_link_for_code("type-only-dependency"),
                message: format!(
                    "Type-only dependency: {} (only used via type imports, could be a devDependency)",
                    dep.dep.package_name
                ),
                data: finding_data(dep.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

fn push_test_only_dependency_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for dep in &results.test_only_dependencies {
        if let Some(dep_uri) = Uri::from_file_path(&dep.dep.path) {
            let line = dep.dep.line.saturating_sub(1);
            map.entry(dep_uri).or_default().push(Diagnostic {
                range: full_line_range(line),
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("test-only-dependency".to_string())),
                code_description: doc_link_for_code("test-only-dependency"),
                message: format!(
                    "Production dependency '{}' is only imported by test files; consider moving to devDependencies",
                    dep.dep.package_name
                ),
                data: finding_data(dep.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

fn push_dev_dependency_in_production_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for dep in &results.dev_dependencies_in_production {
        if let Some(dep_uri) = Uri::from_file_path(&dep.dep.path) {
            let line = dep.dep.line.saturating_sub(1);
            map.entry(dep_uri).or_default().push(Diagnostic {
                range: full_line_range(line),
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String(
                    "dev-dependency-in-production".to_string(),
                )),
                code_description: doc_link_for_code("dev-dependency-in-production"),
                message: format!(
                    "devDependency '{}' is imported by production code at runtime; consider moving to dependencies",
                    dep.dep.package_name
                ),
                data: finding_data(dep.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

fn push_unused_catalog_entry_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    root: &std::path::Path,
) {
    for finding in &results.unused_catalog_entries {
        let entry = &finding.entry;
        if let Some(entry_uri) = Uri::from_file_path(root.join(&entry.path)) {
            let line = entry.line.saturating_sub(1);
            map.entry(entry_uri).or_default().push(Diagnostic {
                range: full_line_range(line),
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-catalog-entry".to_string())),
                code_description: doc_link_for_code("unused-catalog-entry"),
                message: unused_catalog_entry_message(entry),
                data: finding_data(finding.finding_id.as_deref()),
                ..Default::default()
            });
        }
    }
}

fn unused_catalog_entry_message(entry: &fallow_api::editor_results::UnusedCatalogEntry) -> String {
    if entry.catalog_name == "default" {
        format!(
            "Unused catalog entry: '{}' is not referenced by any workspace package",
            entry.entry_name
        )
    } else {
        format!(
            "Unused catalog entry: '{}' in catalog '{}' is not referenced by any workspace package",
            entry.entry_name, entry.catalog_name
        )
    }
}

fn full_line_range(line: u32) -> Range {
    Range {
        start: Position { line, character: 0 },
        end: Position {
            line,
            character: u32::MAX,
        },
    }
}

fn push_empty_catalog_group_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    root: &std::path::Path,
) {
    for finding in &results.empty_catalog_groups {
        let group = &finding.group;
        let Some(uri) = Uri::from_file_path(root.join(&group.path)) else {
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
            code_description: doc_link_for_code("empty-catalog-group"),
            message: format!(
                "Empty catalog group: '{}' has no entries",
                group.catalog_name
            ),
            data: finding_data(finding.finding_id.as_deref()),
            ..Default::default()
        });
    }
}

/// Emit one `ERROR`-severity diagnostic per unresolved-catalog-reference
/// finding. The finding's `path` is stored as an absolute filesystem path
/// (matching the existing convention for path-anchored findings), so
/// `Uri::from_file_path` can be called directly.
fn push_unresolved_catalog_reference_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    use std::fmt::Write as _;
    for finding in &results.unresolved_catalog_references {
        let finding_id = finding.finding_id.as_deref();
        let finding = &finding.reference;
        let Some(uri) = Uri::from_file_path(&finding.path) else {
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
            code_description: doc_link_for_code("unresolved-catalog-reference"),
            message,
            data: finding_data(finding_id),
            ..Default::default()
        });
    }
}

/// Emit diagnostics for unused and misconfigured package-manager override
/// findings. Both finding types carry an absolute `path` (matching the
/// `UnresolvedCatalogReference` convention so `--changed-since` and per-file
/// overrides.rules can compare directly). `Uri::from_file_path` accepts the
/// path as-is. Severity matches the default rule severity: unused =
/// `WARNING`, misconfigured = `ERROR` (the package manager rejects or ignores it).
fn push_dependency_override_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    push_unused_dependency_override_diagnostics(map, results);
    push_misconfigured_dependency_override_diagnostics(map, results);
}

/// Push WARNING diagnostics for unused dependency overrides.
fn push_unused_dependency_override_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    use std::fmt::Write as _;
    for finding in &results.unused_dependency_overrides {
        let finding_id = finding.finding_id.as_deref();
        let finding = &finding.entry;
        let Some(uri) = Uri::from_file_path(&finding.path) else {
            continue;
        };
        let line = finding.line.saturating_sub(1);
        let mut message = format!(
            "Unused dependency override: `{}` forces `{}` to `{}` but it is not declared by any workspace package or resolved in the lockfile",
            finding.raw_key, finding.target_package, finding.version_range,
        );
        if let Some(hint) = &finding.hint {
            let _ = write!(message, " ({hint})");
        }
        map.entry(uri).or_default().push(Diagnostic {
            range: full_line_range(line),
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "unused-dependency-override".to_string(),
            )),
            code_description: doc_link_for_code("unused-dependency-override"),
            message,
            data: finding_data(finding_id),
            ..Default::default()
        });
    }
}

/// Push ERROR diagnostics for misconfigured dependency overrides.
fn push_misconfigured_dependency_override_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for finding in &results.misconfigured_dependency_overrides {
        let finding_id = finding.finding_id.as_deref();
        let finding = &finding.entry;
        let Some(uri) = Uri::from_file_path(&finding.path) else {
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
            range: full_line_range(line),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(
                "misconfigured-dependency-override".to_string(),
            )),
            code_description: doc_link_for_code("misconfigured-dependency-override"),
            message,
            data: finding_data(finding_id),
            ..Default::default()
        });
    }
}

pub fn push_member_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    // All three member kinds carry caveats off the same reachability-free
    // access walk, so all three render the same qualifier. A store member has
    // no quick fix to withhold, which makes the caveat disclosure only: the
    // reader deciding by hand still has to be told what the run did not see.
    let enum_iter = results
        .unused_enum_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats(), f.finding_id.as_deref()));
    let class_iter = results
        .unused_class_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats(), f.finding_id.as_deref()));
    let store_iter = results
        .unused_store_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats(), f.finding_id.as_deref()));
    let namespace_iter = results
        .unused_namespace_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats(), f.finding_id.as_deref()));
    type MemberRows<'a> = Box<
        dyn Iterator<
                Item = (
                    &'a fallow_api::editor_results::UnusedMember,
                    &'a [ReachabilityCaveat],
                    Option<&'a str>,
                ),
            > + 'a,
    >;
    for (members, code, kind_label) in [
        (
            Box::new(enum_iter) as MemberRows<'_>,
            "unused-enum-member",
            "Enum member" as &str,
        ),
        (
            Box::new(class_iter) as MemberRows<'_>,
            "unused-class-member",
            "Class member",
        ),
        (
            Box::new(store_iter) as MemberRows<'_>,
            "unused-store-member",
            "Store member",
        ),
        (
            Box::new(namespace_iter) as MemberRows<'_>,
            "unused-namespace-member",
            "Namespace member",
        ),
    ] {
        for (member, caveats, finding_id) in members {
            let row = MemberRow {
                member,
                caveats,
                finding_id,
            };
            push_unused_member_diagnostic(map, &row, code, kind_label, mapper);
        }
    }

    push_unrendered_component_diagnostics(map, results, mapper);
    push_unused_component_prop_diagnostics(map, results, mapper);
    push_unused_component_emit_diagnostics(map, results, mapper);
    push_unused_component_input_diagnostics(map, results, mapper);
    push_unused_component_output_diagnostics(map, results, mapper);
    push_unused_svelte_event_diagnostics(map, results, mapper);
    push_effect_diagnostics(map, results, mapper);
    push_unused_server_action_diagnostics(map, results, mapper);
    push_unused_load_data_key_diagnostics(map, results, mapper);
}

/// One unused enum, class or store member with the parts its diagnostic needs.
struct MemberRow<'a> {
    member: &'a fallow_api::editor_results::UnusedMember,
    caveats: &'a [ReachabilityCaveat],
    finding_id: Option<&'a str>,
}

/// Push one HINT diagnostic for an unused enum / class / store member.
fn push_unused_member_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    row: &MemberRow<'_>,
    code: &str,
    kind_label: &str,
    mapper: &mut PositionMapper,
) {
    let MemberRow {
        member,
        caveats,
        finding_id,
    } = *row;
    let anchor = NamedAnchor {
        path: &member.path,
        line: member.line,
        col: member.col,
        name: &member.member_name,
    };
    let message = with_caveats(
        format!(
            "{kind_label} '{}.{}' is unused",
            member.parent_name, member.member_name
        ),
        caveats,
    );
    push_anchor_diagnostic(map, mapper, &anchor, code, message, finding_id);
}

fn identifier_range(
    mapper: &mut PositionMapper,
    path: &std::path::Path,
    line: u32,
    col: u32,
    text: &str,
) -> Range {
    let (start, end) = mapper.utf16_col_span(path, line, col, text);
    Range {
        start: Position {
            line,
            character: start,
        },
        end: Position {
            line,
            character: end,
        },
    }
}

/// Push one HINT diagnostic on the identifier of a named anchor.
fn push_anchor_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    mapper: &mut PositionMapper,
    anchor: &NamedAnchor<'_>,
    code: &str,
    message: String,
    finding_id: Option<&str>,
) {
    let Some(uri) = Uri::from_file_path(anchor.path) else {
        return;
    };
    let line = anchor.line.saturating_sub(1);
    let range = identifier_range(mapper, anchor.path, line, anchor.col, anchor.name);
    map.entry(uri).or_default().push(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::HINT),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: doc_link_for_code(code),
        message,
        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
        data: finding_data(finding_id),
        ..Default::default()
    });
}

fn push_unrendered_component_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unrendered_components {
        let c = &finding.component;
        let anchor = NamedAnchor {
            path: &c.path,
            line: c.line,
            col: c.col,
            name: &c.component_name,
        };
        let message = format!(
            "Component '{}' is reachable but rendered nowhere in this project",
            c.component_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unrendered-component",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

fn push_unused_component_prop_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_component_props {
        let p = &finding.prop;
        let anchor = NamedAnchor {
            path: &p.path,
            line: p.line,
            col: p.col,
            name: &p.prop_name,
        };
        let message = format!(
            "Prop '{}' is declared but referenced nowhere in this component",
            p.prop_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-component-prop",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

fn push_unused_component_emit_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_component_emits {
        let e = &finding.emit;
        let anchor = NamedAnchor {
            path: &e.path,
            line: e.line,
            col: e.col,
            name: &e.emit_name,
        };
        let message = format!(
            "Emit '{}' is declared but emitted nowhere in this component",
            e.emit_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-component-emit",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

fn push_unused_component_input_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_component_inputs {
        let i = &finding.input;
        let anchor = NamedAnchor {
            path: &i.path,
            line: i.line,
            col: i.col,
            name: &i.input_name,
        };
        let message = format!(
            "Input '{}' is declared but read nowhere in this component",
            i.input_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-component-input",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

fn push_unused_component_output_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_component_outputs {
        let o = &finding.output;
        let anchor = NamedAnchor {
            path: &o.path,
            line: o.line,
            col: o.col,
            name: &o.output_name,
        };
        let message = format!(
            "Output '{}' is declared but emitted nowhere in this component",
            o.output_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-component-output",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

/// Push HINT diagnostics for the Effect rules: layers composed only into
/// unused layers, tagged errors that no code produces, and `catchTag`
/// handlers for a tag that no code produces.
fn push_effect_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_effect_layers {
        let layer = &finding.layer;
        let anchor = NamedAnchor {
            path: &layer.path,
            line: layer.line,
            col: layer.col,
            name: layer.name.rsplit('.').next().unwrap_or(&layer.name),
        };
        let message = format!(
            "Layer '{}' is composed only into layers that nothing uses: {}",
            layer.name,
            layer.dead_consumers.join(", ")
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-effect-layer",
            message,
            finding.finding_id.as_deref(),
        );
    }
    for finding in &results.unused_tagged_errors {
        let error = &finding.error;
        let anchor = NamedAnchor {
            path: &error.path,
            line: error.line,
            col: error.col,
            name: &error.name,
        };
        let message = format!(
            "Tagged error '{}' is never constructed or failed with in this project",
            error.name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-tagged-error",
            message,
            finding.finding_id.as_deref(),
        );
    }
    for finding in &results.unproduced_catch_tags {
        let catch_tag = &finding.catch_tag;
        let anchor = NamedAnchor {
            path: &catch_tag.path,
            line: catch_tag.line,
            col: catch_tag.col,
            name: &catch_tag.tag,
        };
        let message = format!(
            "catchTag handler for '{}' never runs: no code in this project produces the tag",
            catch_tag.tag
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unproduced-catch-tag",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

fn push_unused_svelte_event_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_svelte_events {
        let e = &finding.event;
        let anchor = NamedAnchor {
            path: &e.path,
            line: e.line,
            col: e.col,
            name: &e.event_name,
        };
        let message = format!(
            "Event '{}' is dispatched but listened to nowhere in this project",
            e.event_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-svelte-event",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

/// Push HINT diagnostics for unused SvelteKit `load()` return-object keys
/// (returned by `+page.{ts,server.ts,js,server.js}` but read by no consumer).
fn push_unused_load_data_key_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_load_data_keys {
        let k = &finding.key;
        let anchor = NamedAnchor {
            path: &k.path,
            line: k.line,
            col: k.col,
            name: &k.key_name,
        };
        let message = format!(
            "load() return key '{}' is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>)",
            k.key_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-load-data-key",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

/// Push HINT diagnostics for unused Next.js server actions (exports of a
/// `"use server"` file referenced by no code in the project).
fn push_unused_server_action_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.unused_server_actions {
        let a = &finding.action;
        let anchor = NamedAnchor {
            path: &a.path,
            line: a.line,
            col: a.col,
            name: &a.action_name,
        };
        let message = format!(
            "Server action '{}' is exported from a \"use server\" file but no code in this project references it",
            a.action_name
        );
        push_anchor_diagnostic(
            map,
            mapper,
            &anchor,
            "unused-server-action",
            message,
            finding.finding_id.as_deref(),
        );
    }
}

#[cfg(test)]
#[path = "unused_tests.rs"]
mod tests;
