use rustc_hash::FxHashMap;

use ls_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Uri,
};

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_types::output_dead_code::{MutationEvidence, ReachabilityCaveat, caveat_suffix};

use super::{FIRST_LINE_RANGE, doc_link_for_code};
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
        .map(|f| (&f.export, f.reachability_caveats()));
    let types_iter = results
        .unused_types
        .iter()
        .map(|f| (&f.export, f.reachability_caveats()));
    for (exports, code, msg_prefix) in [
        (
            Box::new(exports_iter)
                as Box<
                    dyn Iterator<
                        Item = (
                            &fallow_api::editor_results::UnusedExport,
                            &[ReachabilityCaveat],
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
                        ),
                    >,
                >,
            "unused-type",
            "Type export",
        ),
    ] {
        for (export, caveats) in exports {
            push_unused_export_diagnostic(map, export, caveats, code, msg_prefix, mapper);
        }
    }

    push_private_type_leak_diagnostics(map, results, mapper);
}

/// Push one HINT diagnostic for an unused export or type export.
fn push_unused_export_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    export: &fallow_api::editor_results::UnusedExport,
    caveats: &[ReachabilityCaveat],
    code: &str,
    msg_prefix: &str,
    mapper: &mut PositionMapper,
) {
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
    type DepIter<'a> =
        Box<dyn Iterator<Item = &'a fallow_api::editor_results::UnusedDependency> + 'a>;
    let groups: [(DepIter<'_>, &str, &str); 3] = [
        (
            Box::new(results.unused_dependencies.iter().map(|f| &f.dep)),
            "unused-dependency",
            "Unused dependency",
        ),
        (
            Box::new(results.unused_dev_dependencies.iter().map(|f| &f.dep)),
            "unused-dev-dependency",
            "Unused devDependency",
        ),
        (
            Box::new(results.unused_optional_dependencies.iter().map(|f| &f.dep)),
            "unused-optional-dependency",
            "Unused optionalDependency",
        ),
    ];
    for (deps, code, msg_prefix) in groups {
        for dep in deps {
            push_unused_dependency_diagnostic(map, dep, code, msg_prefix);
        }
    }

    push_unlisted_dependency_diagnostics(map, results, package_json_uri);

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
            ..Default::default()
        });
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
    for entry in &results.unused_catalog_entries {
        let entry = &entry.entry;
        if let Some(entry_uri) = Uri::from_file_path(root.join(&entry.path)) {
            let line = entry.line.saturating_sub(1);
            map.entry(entry_uri).or_default().push(Diagnostic {
                range: full_line_range(line),
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("unused-catalog-entry".to_string())),
                code_description: doc_link_for_code("unused-catalog-entry"),
                message: unused_catalog_entry_message(entry),
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
    for group in &results.empty_catalog_groups {
        let group = &group.group;
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
        .map(|f| (&f.member, f.reachability_caveats()));
    let class_iter = results
        .unused_class_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats()));
    let store_iter = results
        .unused_store_members
        .iter()
        .map(|f| (&f.member, f.reachability_caveats()));
    type MemberRows<'a> = Box<
        dyn Iterator<
                Item = (
                    &'a fallow_api::editor_results::UnusedMember,
                    &'a [ReachabilityCaveat],
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
    ] {
        for (member, caveats) in members {
            push_unused_member_diagnostic(map, member, caveats, code, kind_label, mapper);
        }
    }

    push_unrendered_component_diagnostics(map, results, mapper);
    push_unused_component_prop_diagnostics(map, results, mapper);
    push_unused_component_emit_diagnostics(map, results, mapper);
    push_unused_component_input_diagnostics(map, results, mapper);
    push_unused_component_output_diagnostics(map, results, mapper);
    push_unused_svelte_event_diagnostics(map, results, mapper);
    push_unused_server_action_diagnostics(map, results, mapper);
    push_unused_load_data_key_diagnostics(map, results, mapper);
}

/// Push one HINT diagnostic for an unused enum / class / store member.
fn push_unused_member_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    member: &fallow_api::editor_results::UnusedMember,
    caveats: &[ReachabilityCaveat],
    code: &str,
    kind_label: &str,
    mapper: &mut PositionMapper,
) {
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
    push_anchor_diagnostic(map, mapper, &anchor, code, message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unrendered-component", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-component-prop", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-component-emit", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-component-input", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-component-output", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-svelte-event", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-load-data-key", message);
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
        push_anchor_diagnostic(map, mapper, &anchor, "unused-server-action", message);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_api::editor_duplicates::{DuplicationReport, DuplicationStats};
    use fallow_api::editor_extract::MemberKind;
    use fallow_api::editor_results::{
        AnalysisResults, DependencyLocation, EmptyCatalogGroup, EmptyCatalogGroupFinding,
        ImportSite, TestOnlyDependency, TestOnlyDependencyFinding, TypeOnlyDependency,
        TypeOnlyDependencyFinding, UnlistedDependency, UnlistedDependencyFinding,
        UnresolvedCatalogReference, UnresolvedCatalogReferenceFinding, UnresolvedImport,
        UnresolvedImportFinding, UnusedCatalogEntry, UnusedCatalogEntryFinding,
        UnusedClassMemberFinding, UnusedDependency, UnusedDependencyFinding,
        UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding,
        UnusedFile, UnusedFileFinding, UnusedMember, UnusedOptionalDependencyFinding,
        UnusedStoreMemberFinding, UnusedTypeFinding,
    };
    use ls_types::{DiagnosticSeverity, DiagnosticTag, NumberOrString, Uri};

    use crate::diagnostics::{FIRST_LINE_RANGE, build_diagnostics_for_test};

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
                clone_families: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
                clone_groups_ignored: 0,
                near_candidates_skipped: 0,
            },
        }
    }

    /// One finding per named-anchor kind, each in its own file, at 1-based
    /// line 3 and column 4.
    fn named_anchor_results(root: &std::path::Path) -> AnalysisResults {
        use fallow_api::editor_results as r;
        let mut results = AnalysisResults::default();
        results
            .unrendered_components
            .push(r::UnrenderedComponentFinding::with_actions(
                r::UnrenderedComponent {
                    path: root.join("card-element.ts"),
                    component_name: "my-card".to_string(),
                    framework: "lit".to_string(),
                    reachable_via: None,
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_component_props
            .push(r::UnusedComponentPropFinding::with_actions(
                r::UnusedComponentProp {
                    path: root.join("Prop.vue"),
                    component_name: "Prop".to_string(),
                    prop_name: "size".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_component_emits
            .push(r::UnusedComponentEmitFinding::with_actions(
                r::UnusedComponentEmit {
                    path: root.join("Emit.vue"),
                    component_name: "Emit".to_string(),
                    emit_name: "change".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_component_inputs
            .push(r::UnusedComponentInputFinding::with_actions(
                r::UnusedComponentInput {
                    path: root.join("input.component.ts"),
                    component_name: "InputComponent".to_string(),
                    input_name: "label".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_component_outputs
            .push(r::UnusedComponentOutputFinding::with_actions(
                r::UnusedComponentOutput {
                    path: root.join("output.component.ts"),
                    component_name: "OutputComponent".to_string(),
                    output_name: "closed".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_svelte_events
            .push(r::UnusedSvelteEventFinding::with_actions(
                r::UnusedSvelteEvent {
                    path: root.join("Child.svelte"),
                    component_name: "Child".to_string(),
                    event_name: "dead".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_server_actions
            .push(r::UnusedServerActionFinding::with_actions(
                r::UnusedServerAction {
                    path: root.join("app/actions.ts"),
                    action_name: "createUser".to_string(),
                    line: 3,
                    col: 4,
                },
            ));
        results
            .unused_load_data_keys
            .push(r::UnusedLoadDataKeyFinding::with_actions(
                r::UnusedLoadDataKey {
                    path: root.join("src/routes/+page.server.ts"),
                    key_name: "posts".to_string(),
                    line: 3,
                    col: 4,
                    route_dir: None,
                },
            ));
        results
    }

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test string lengths are trivially small"
    )]
    fn named_anchor_diagnostics_keep_exact_message_code_and_range() {
        let root = test_root();
        let results = named_anchor_results(&root);
        let diags = build_diagnostics_for_test(&results, &empty_duplication(), &root);
        let cases = [
            (
                "card-element.ts",
                "my-card",
                "unrendered-component",
                "Component 'my-card' is reachable but rendered nowhere in this project",
            ),
            (
                "Prop.vue",
                "size",
                "unused-component-prop",
                "Prop 'size' is declared but referenced nowhere in this component",
            ),
            (
                "Emit.vue",
                "change",
                "unused-component-emit",
                "Emit 'change' is declared but emitted nowhere in this component",
            ),
            (
                "input.component.ts",
                "label",
                "unused-component-input",
                "Input 'label' is declared but read nowhere in this component",
            ),
            (
                "output.component.ts",
                "closed",
                "unused-component-output",
                "Output 'closed' is declared but emitted nowhere in this component",
            ),
            (
                "Child.svelte",
                "dead",
                "unused-svelte-event",
                "Event 'dead' is dispatched but listened to nowhere in this project",
            ),
            (
                "app/actions.ts",
                "createUser",
                "unused-server-action",
                "Server action 'createUser' is exported from a \"use server\" file but no code in this project references it",
            ),
            (
                "src/routes/+page.server.ts",
                "posts",
                "unused-load-data-key",
                "load() return key 'posts' is read by no consumer (sibling +page.svelte data.<key> or project-wide page.data.<key>)",
            ),
        ];
        for (file, name, code, message) in cases {
            let uri = Uri::from_file_path(root.join(file)).unwrap();
            let file_diags = diags.get(&uri).unwrap_or_else(|| panic!("{file}"));
            assert_eq!(file_diags.len(), 1, "{file}");
            let d = &file_diags[0];
            assert_eq!(d.message, message, "{file}");
            assert_eq!(d.code, Some(NumberOrString::String(code.to_string())));
            assert_eq!(d.code_description, super::doc_link_for_code(code), "{file}");
            assert_eq!(d.severity, Some(DiagnosticSeverity::HINT), "{file}");
            assert_eq!(d.source.as_deref(), Some("fallow"), "{file}");
            assert_eq!(d.tags, Some(vec![DiagnosticTag::UNNECESSARY]), "{file}");
            assert_eq!(d.range.start.line, 2, "{file}");
            assert_eq!(d.range.end.line, 2, "{file}");
            assert_eq!(d.range.start.character, 4, "{file}");
            assert_eq!(d.range.end.character, 4 + name.len() as u32, "{file}");
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
                deprecated: false,
                deprecated_reason: None,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/utils.ts")).unwrap();
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
        assert_eq!(d.range.start.line, 4);
        assert_eq!(d.range.start.character, 7);
        assert_eq!(d.range.end.character, 7 + "helper".len() as u32);
        assert_eq!(d.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    /// Withholding the quick fix without saying why turns a stated limit into
    /// a missing feature, so the diagnostic has to carry the caveat. The
    /// wording is the shared `caveat_suffix`, which is what the human report
    /// and the SARIF result message already append.
    #[test]
    fn a_caveated_finding_says_so_in_its_diagnostic() {
        use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};

        let root = test_root();
        let mut results = AnalysisResults::default();

        let mut export = UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helper".to_string(),
            is_type_only: false,
            line: 5,
            col: 7,
            span_start: 40,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        });
        export.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        results.unused_exports.push(export);

        let mut file = UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/orphan.ts"),
        });
        file.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteFileAnalysis]);
        results.unused_files.push(file);

        let mut member = UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/colors.ts"),
            parent_name: "Color".to_string(),
            member_name: "Blue".to_string(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 2,
        });
        member.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        results.unused_enum_members.push(member);

        let mut class_member = UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/widget.ts"),
            parent_name: "Widget".to_string(),
            member_name: "render".to_string(),
            kind: MemberKind::ClassMethod,
            line: 4,
            col: 2,
        });
        class_member.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        results.unused_class_members.push(class_member);

        let mut store_member = UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: root.join("src/cart.ts"),
            parent_name: "useCart".to_string(),
            member_name: "subtotal".to_string(),
            kind: MemberKind::StoreMember,
            line: 6,
            col: 2,
        });
        store_member.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
        results.unused_store_members.push(store_member);

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let message = |relative: &str| {
            let uri = Uri::from_file_path(root.join(relative)).unwrap();
            diags[&uri][0].message.clone()
        };

        assert_eq!(
            message("src/utils.ts"),
            "Export 'helper' is unused (caveat: incomplete import graph)"
        );
        assert_eq!(
            message("src/orphan.ts"),
            "File is not reachable from any entry point (caveat: incomplete file analysis)"
        );
        assert_eq!(
            message("src/colors.ts"),
            "Enum member 'Color.Blue' is unused (caveat: incomplete import graph)"
        );
        assert_eq!(
            message("src/widget.ts"),
            "Class member 'Widget.render' is unused (caveat: incomplete import graph)"
        );
        assert_eq!(
            message("src/cart.ts"),
            "Store member 'useCart.subtotal' is unused (caveat: incomplete import graph)"
        );
    }

    /// A clean run must render exactly as it did: the caveat is additive, and
    /// an editor consumer diffing messages should see no change.
    #[test]
    fn an_uncaveated_finding_keeps_its_exact_message() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/orphan.ts"),
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);
        let uri = Uri::from_file_path(root.join("src/orphan.ts")).unwrap();

        assert_eq!(
            diags[&uri][0].message,
            "File is not reachable from any entry point"
        );
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
                deprecated: false,
                deprecated_reason: None,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/types.ts")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/dead.ts")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/app.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.message, "Cannot find module './missing-module'");
        assert_eq!(d.range.start.line, 2); // 1-based -> 0-based
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/enums.ts")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/service.ts")).unwrap();
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
    fn unused_store_member_produces_hint() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: root.join("src/store.ts"),
                parent_name: "useStore".to_string(),
                member_name: "reset".to_string(),
                kind: MemberKind::StoreMember,
                line: 20,
                col: 4,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/store.ts")).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(d.message, "Store member 'useStore.reset' is unused");
        assert_eq!(
            d.code,
            Some(NumberOrString::String("unused-store-member".to_string()))
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("package.json")).unwrap();
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
                deprecated: false,
                deprecated_reason: None,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("src/edge.ts")).unwrap();
        let d = &diags[&uri][0];
        assert_eq!(d.range.start.line, 0);
    }

    #[test]
    fn unused_catalog_entry_produces_warning_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "is-even".to_string(),
                    catalog_name: "default".to_string(),
                    path: PathBuf::from("pnpm-workspace.yaml"),
                    line: 6,
                    hardcoded_consumers: vec![],
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
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
        assert_eq!(d.range.start.line, 5);
    }

    #[test]
    fn unused_catalog_entry_message_mentions_named_catalog() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "react-dom".to_string(),
                    catalog_name: "react17".to_string(),
                    path: PathBuf::from("pnpm-workspace.yaml"),
                    line: 12,
                    hardcoded_consumers: vec![],
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
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
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "legacy".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 9,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(root.join("pnpm-workspace.yaml")).unwrap();
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
        let root = test_root();
        let abs_path = root.join("packages/app/package.json");
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "old-react".to_string(),
                catalog_name: "react17".to_string(),
                path: abs_path.clone(),
                line: 14,
                available_in_catalogs: vec!["react18".to_string()],
            }),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&abs_path).unwrap();
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
        assert_eq!(d.range.start.line, 13);
    }

    #[test]
    fn unresolved_catalog_reference_default_catalog_uses_default_phrasing() {
        let root = test_root();
        let abs_path = root.join("package.json");
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "foo".to_string(),
                catalog_name: "default".to_string(),
                path: abs_path.clone(),
                line: 5,
                available_in_catalogs: vec![],
            }),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&abs_path).unwrap();
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
        use fallow_api::editor_results::{
            DependencyOverrideSource, UnusedDependencyOverride, UnusedDependencyOverrideFinding,
        };

        let root = test_root();
        let mut results = AnalysisResults::default();
        let yaml_path = root.join("pnpm-workspace.yaml");
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "axios".to_string(),
                    target_package: "axios".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^1.6.0".to_string(),
                    source: DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: yaml_path.clone(),
                    line: 9,
                    hint: Some("may be intentional transitive pin".to_string()),
                },
            ));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&yaml_path).unwrap();
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
        use fallow_api::editor_results::{
            DependencyOverrideMisconfigReason, DependencyOverrideSource,
            MisconfiguredDependencyOverride, MisconfiguredDependencyOverrideFinding,
        };

        let root = test_root();
        let json_path = root.join("package.json");
        let mut results = AnalysisResults::default();
        results.misconfigured_dependency_overrides.push(
            MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
                raw_key: "@types/react@<<18".to_string(),
                target_package: None,
                raw_value: "18.0.0".to_string(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmPackageJson,
                path: json_path.clone(),
                line: 3,
            }),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&json_path).unwrap();
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
