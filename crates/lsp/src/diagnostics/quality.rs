use rustc_hash::FxHashMap;

use ls_types::{
    CodeDescription, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag,
    Location, NumberOrString, Position, Range, Uri,
};

use fallow_api::editor_results::{DuplicateExport, DuplicateLocation};
use fallow_api::{
    EditorAnalysisResults as AnalysisResults, EditorDuplicationReport as DuplicationReport,
};

use super::doc_link_for_code;
use crate::position::PositionMapper;

pub fn push_duplicate_export_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for dup in &results.duplicate_exports {
        let dup = &dup.export;
        for loc in &dup.locations {
            push_duplicate_export_location_diagnostic(map, dup, loc, mapper);
        }
    }
}

fn push_duplicate_export_location_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    dup: &DuplicateExport,
    loc: &DuplicateLocation,
    mapper: &mut PositionMapper,
) {
    let Some(uri) = Uri::from_file_path(&loc.path) else {
        return;
    };
    let related_info = duplicate_export_related_info(dup, loc, mapper);
    let line = loc.line.saturating_sub(1);
    let (start, end) = mapper.utf16_col_span(&loc.path, line, loc.col, &dup.export_name);
    map.entry(uri).or_default().push(Diagnostic {
        range: Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        },
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String("duplicate-export".to_string())),
        code_description: doc_link_for_code("duplicate-export"),
        message: format!("Duplicate export '{}'", dup.export_name),
        related_information: (!related_info.is_empty()).then_some(related_info),
        ..Default::default()
    });
}

fn duplicate_export_related_info(
    dup: &DuplicateExport,
    loc: &DuplicateLocation,
    mapper: &mut PositionMapper,
) -> Vec<DiagnosticRelatedInformation> {
    let mut related = Vec::new();
    for l in dup.locations.iter().filter(|l| l.path != loc.path) {
        let Some(other_uri) = Uri::from_file_path(&l.path) else {
            continue;
        };
        let line = l.line.saturating_sub(1);
        let (start, end) = mapper.utf16_col_span(&l.path, line, l.col, &dup.export_name);
        related.push(DiagnosticRelatedInformation {
            location: Location {
                uri: other_uri,
                range: Range {
                    start: Position {
                        line,
                        character: start,
                    },
                    end: Position {
                        line,
                        character: end,
                    },
                },
            },
            message: "Also exported here".to_string(),
        });
    }
    related
}

pub fn push_duplication_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    duplication: &DuplicationReport,
    mapper: &mut PositionMapper,
) {
    for group in &duplication.clone_groups {
        for instance in &group.instances {
            push_duplication_instance_diagnostic(map, group, instance, mapper);
        }
    }
}

/// Push one INFORMATION diagnostic for a single clone instance, with the
/// group's other instances linked as related info.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
fn push_duplication_instance_diagnostic(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    group: &fallow_api::editor_duplicates::CloneGroup,
    instance: &fallow_api::editor_duplicates::CloneInstance,
    mapper: &mut PositionMapper,
) {
    let Some(inst_uri) = Uri::from_file_path(&instance.file) else {
        return;
    };

    let start_line = (instance.start_line as u32).saturating_sub(1);
    let end_line = (instance.end_line as u32).saturating_sub(1);
    let start_col = mapper.utf16_col(&instance.file, start_line, instance.start_col as u32);

    let related_info = duplication_related_info(group, instance, mapper);

    map.entry(inst_uri).or_default().push(Diagnostic {
        range: Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: u32::MAX,
            },
        },
        severity: Some(DiagnosticSeverity::INFORMATION),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String("code-duplication".to_string())),
        code_description: "https://docs.fallow.tools/explanations/duplication"
            .parse::<Uri>()
            .ok()
            .map(|href| CodeDescription { href }),
        message: format!(
            "Duplicated code block ({} lines, {} instances)",
            group.line_count,
            group.instances.len()
        ),
        related_information: if related_info.is_empty() {
            None
        } else {
            Some(related_info)
        },
        ..Default::default()
    });
}

/// Build the "Also duplicated here" related-info entries for every clone
/// instance in `group` other than `instance` itself.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
fn duplication_related_info(
    group: &fallow_api::editor_duplicates::CloneGroup,
    instance: &fallow_api::editor_duplicates::CloneInstance,
    mapper: &mut PositionMapper,
) -> Vec<DiagnosticRelatedInformation> {
    let mut related = Vec::new();
    for other in group
        .instances
        .iter()
        .filter(|other| !(other.file == instance.file && other.start_line == instance.start_line))
    {
        let Some(other_uri) = Uri::from_file_path(&other.file) else {
            continue;
        };
        let start_line = (other.start_line as u32).saturating_sub(1);
        let start_col = mapper.utf16_col(&other.file, start_line, other.start_col as u32);
        related.push(DiagnosticRelatedInformation {
            location: Location {
                uri: other_uri,
                range: Range {
                    start: Position {
                        line: start_line,
                        character: start_col,
                    },
                    end: Position {
                        line: (other.end_line as u32).saturating_sub(1),
                        character: u32::MAX,
                    },
                },
            },
            message: "Also duplicated here".to_string(),
        });
    }
    related
}

pub fn push_stale_suppression_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for s in &results.stale_suppressions {
        let Some(uri) = Uri::from_file_path(&s.path) else {
            continue;
        };
        let line = s.line.saturating_sub(1);
        let col = mapper.utf16_col(&s.path, line, s.col);
        let message = format!(
            "Stale suppression: {} ({})",
            s.description(),
            s.explanation()
        );

        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::HINT),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("stale-suppression".to_string())),
            code_description: doc_link_for_code("stale-suppression"),
            message,
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_api::editor_duplicates::{
        CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
    };
    use fallow_api::editor_results::{
        AnalysisResults, DuplicateExport, DuplicateExportFinding, DuplicateLocation, UnusedExport,
        UnusedExportFinding, UnusedTypeFinding,
    };
    use ls_types::{DiagnosticSeverity, NumberOrString, Uri};

    use crate::diagnostics::build_diagnostics_for_test;

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
    fn duplicate_export_produces_warning_with_related_files() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let helpers_path = root.join("src/helpers.ts");

        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "formatDate".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: utils_path.clone(),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: helpers_path.clone(),
                        line: 30,
                        col: 0,
                    },
                ],
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri_utils = Uri::from_file_path(&utils_path).unwrap();
        let uri_helpers = Uri::from_file_path(&helpers_path).unwrap();

        let utils_diags = &diags[&uri_utils];
        assert_eq!(utils_diags.len(), 1);
        let d = &utils_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert!(d.message.contains("formatDate"));
        assert_eq!(d.range.start.line, 14);
        assert_eq!(d.range.start.character, 0);
        assert_eq!(d.range.end.character, "formatDate".len() as u32);
        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].location.uri, uri_helpers);
        assert_eq!(related[0].message, "Also exported here");

        let helpers_diags = &diags[&uri_helpers];
        assert_eq!(helpers_diags.len(), 1);
        let dh = &helpers_diags[0];
        let related_h = dh.related_information.as_ref().unwrap();
        assert_eq!(related_h[0].location.uri, uri_utils);
    }

    #[test]
    fn duplication_diagnostic_has_related_information() {
        let root = test_root();
        let results = AnalysisResults::default();
        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 10,
                        end_line: 15,
                        start_col: 0,
                        end_col: 20,
                        fragment: "duplicated code".to_string(),
                    },
                    CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 20,
                        end_line: 25,
                        start_col: 4,
                        end_col: 24,
                        fragment: "duplicated code".to_string(),
                    },
                ],
                token_count: 50,
                line_count: 6,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 2,
                files_with_clones: 2,
                total_lines: 100,
                duplicated_lines: 12,
                total_tokens: 500,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 12.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri_a = Uri::from_file_path(root.join("src/a.ts")).unwrap();
        let diags_a = &diags[&uri_a];
        assert_eq!(diags_a.len(), 1);

        let d = &diags_a[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("code-duplication".to_string()))
        );
        assert!(d.message.contains("6 lines"));
        assert!(d.message.contains("2 instances"));

        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "Also duplicated here");
        let related_uri = Uri::from_file_path(root.join("src/b.ts")).unwrap();
        assert_eq!(related[0].location.uri, related_uri);
        assert_eq!(related[0].location.range.start.line, 19);
        assert_eq!(related[0].location.range.start.character, 4);

        let uri_b = Uri::from_file_path(root.join("src/b.ts")).unwrap();
        let diags_b = &diags[&uri_b];
        assert_eq!(diags_b.len(), 1);
        let related_b = diags_b[0].related_information.as_ref().unwrap();
        assert_eq!(related_b.len(), 1);
        assert_eq!(related_b[0].location.uri, uri_a);
    }

    #[test]
    fn duplication_with_single_instance_has_no_related_info() {
        let root = test_root();
        let results = AnalysisResults::default();
        let duplication = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/only.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 10,
                    fragment: "code".to_string(),
                }],
                token_count: 20,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 1,
                files_with_clones: 1,
                total_lines: 20,
                duplicated_lines: 5,
                total_tokens: 100,
                duplicated_tokens: 20,
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 25.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let diags = build_diagnostics_for_test(&results, &duplication, &root);
        let uri = Uri::from_file_path(root.join("src/only.ts")).unwrap();
        let d = &diags[&uri][0];

        assert!(d.related_information.is_none());
    }

    #[test]
    fn duplicate_export_with_single_location_has_no_related_info() {
        let root = test_root();
        let path = root.join("src/solo.ts");

        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".to_string(),
                locations: vec![DuplicateLocation {
                    path: path.clone(),
                    line: 5,
                    col: 0,
                }],
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&path).unwrap();
        let d = &diags[&uri][0];
        assert!(d.related_information.is_none());
    }

    #[test]
    fn all_diagnostic_codes_have_doc_links() {
        let root = test_root();
        let path = root.join("src/file.ts");
        let mut results = AnalysisResults::default();

        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "e".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "T".to_string(),
                is_type_only: true,
                line: 2,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_files
            .push(fallow_api::editor_results::UnusedFileFinding::with_actions(
                fallow_api::editor_results::UnusedFile { path: path.clone() },
            ));
        results.unused_enum_members.push(
            fallow_api::editor_results::UnusedEnumMemberFinding::with_actions(
                fallow_api::editor_results::UnusedMember {
                    path: path.clone(),
                    parent_name: "E".to_string(),
                    member_name: "A".to_string(),
                    kind: fallow_api::editor_extract::MemberKind::EnumMember,
                    line: 3,
                    col: 0,
                },
            ),
        );

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&path).unwrap();
        let file_diags = &diags[&uri];

        for d in file_diags {
            assert!(
                d.code_description.is_some(),
                "Diagnostic code {:?} should have a code_description doc link",
                d.code
            );
            let href = &d.code_description.as_ref().unwrap().href;
            assert!(
                href.as_str().starts_with("https://docs.fallow.tools/"),
                "Doc link should point to fallow docs: {href:?}"
            );
        }
    }
}
