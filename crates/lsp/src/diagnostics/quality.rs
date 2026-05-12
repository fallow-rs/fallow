use rustc_hash::FxHashMap;

use tower_lsp::lsp_types::{
    CodeDescription, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag,
    Location, NumberOrString, Position, Range, Url,
};

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;

use super::doc_link;

#[expect(
    clippy::cast_possible_truncation,
    reason = "export name lengths are bounded by source size"
)]
pub fn push_duplicate_export_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for dup in &results.duplicate_exports {
        // Build related information linking all duplicate locations together
        for loc in &dup.locations {
            if let Ok(uri) = Url::from_file_path(&loc.path) {
                let related_info: Vec<DiagnosticRelatedInformation> = dup
                    .locations
                    .iter()
                    .filter(|l| l.path != loc.path)
                    .filter_map(|l| {
                        let other_uri = Url::from_file_path(&l.path).ok()?;
                        Some(DiagnosticRelatedInformation {
                            location: Location {
                                uri: other_uri,
                                range: Range {
                                    start: Position {
                                        line: l.line.saturating_sub(1),
                                        character: l.col,
                                    },
                                    end: Position {
                                        line: l.line.saturating_sub(1),
                                        character: l.col + dup.export_name.len() as u32,
                                    },
                                },
                            },
                            message: "Also exported here".to_string(),
                        })
                    })
                    .collect();
                let line = loc.line.saturating_sub(1);
                map.entry(uri).or_default().push(Diagnostic {
                    range: Range {
                        start: Position {
                            line,
                            character: loc.col,
                        },
                        end: Position {
                            line,
                            character: loc.col + dup.export_name.len() as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("fallow".to_string()),
                    code: Some(NumberOrString::String("duplicate-export".to_string())),
                    code_description: doc_link("duplicate-exports"),
                    message: format!("Duplicate export '{}'", dup.export_name),
                    related_information: if related_info.is_empty() {
                        None
                    } else {
                        Some(related_info)
                    },
                    ..Default::default()
                });
            }
        }
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "line/col numbers are bounded by source size"
)]
pub fn push_duplication_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    duplication: &DuplicationReport,
) {
    for group in &duplication.clone_groups {
        for instance in &group.instances {
            let Ok(inst_uri) = Url::from_file_path(&instance.file) else {
                continue;
            };

            let start_line = (instance.start_line as u32).saturating_sub(1);
            let end_line = (instance.end_line as u32).saturating_sub(1);

            // Build related information pointing to other instances in the group
            let related_info: Vec<DiagnosticRelatedInformation> = group
                .instances
                .iter()
                .filter(|other| {
                    !(other.file == instance.file && other.start_line == instance.start_line)
                })
                .filter_map(|other| {
                    let other_uri = Url::from_file_path(&other.file).ok()?;
                    Some(DiagnosticRelatedInformation {
                        location: Location {
                            uri: other_uri,
                            range: Range {
                                start: Position {
                                    line: (other.start_line as u32).saturating_sub(1),
                                    character: other.start_col as u32,
                                },
                                end: Position {
                                    line: (other.end_line as u32).saturating_sub(1),
                                    character: u32::MAX,
                                },
                            },
                        },
                        message: "Also duplicated here".to_string(),
                    })
                })
                .collect();

            map.entry(inst_uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line: start_line,
                        character: instance.start_col as u32,
                    },
                    end: Position {
                        line: end_line,
                        // Extend to end of last line to ensure full block is underlined
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::INFORMATION),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("code-duplication".to_string())),
                code_description: Url::parse("https://docs.fallow.tools/explanations/duplication")
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
    }
}

pub fn push_stale_suppression_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for s in &results.stale_suppressions {
        let Ok(uri) = Url::from_file_path(&s.path) else {
            continue;
        };
        let line = s.line.saturating_sub(1);
        let message = format!(
            "Stale suppression: {} ({})",
            s.description(),
            s.explanation()
        );

        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: s.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::HINT),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("stale-suppression".to_string())),
            code_description: doc_link("stale-suppressions"),
            message,
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
    use fallow_core::results::{AnalysisResults, DuplicateExport, DuplicateLocation, UnusedExport};
    use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString, Url};

    use crate::diagnostics::build_diagnostics;

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
        results.duplicate_exports.push(DuplicateExport {
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
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        // Both files should have a diagnostic
        let uri_utils = Url::from_file_path(&utils_path).unwrap();
        let uri_helpers = Url::from_file_path(&helpers_path).unwrap();

        let utils_diags = &diags[&uri_utils];
        assert_eq!(utils_diags.len(), 1);
        let d = &utils_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert!(d.message.contains("formatDate"));
        // line 15 (1-based) → 14 (0-based)
        assert_eq!(d.range.start.line, 14);
        assert_eq!(d.range.start.character, 0);
        // Range spans the export name
        assert_eq!(d.range.end.character, "formatDate".len() as u32);
        // Related info points to the other file
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

        let diags = build_diagnostics(&results, &duplication, &root);

        // File a.ts should have a diagnostic with related info pointing to b.ts
        let uri_a = Url::from_file_path(root.join("src/a.ts")).unwrap();
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

        // Check related info
        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "Also duplicated here");
        let related_uri = Url::from_file_path(root.join("src/b.ts")).unwrap();
        assert_eq!(related[0].location.uri, related_uri);
        // b.ts start_line = 20 (1-based) → 19 (0-based)
        assert_eq!(related[0].location.range.start.line, 19);
        assert_eq!(related[0].location.range.start.character, 4);

        // File b.ts should have related info pointing to a.ts
        let uri_b = Url::from_file_path(root.join("src/b.ts")).unwrap();
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

        let diags = build_diagnostics(&results, &duplication, &root);
        let uri = Url::from_file_path(root.join("src/only.ts")).unwrap();
        let d = &diags[&uri][0];

        // Single instance => no "other" instances => no related info
        assert!(d.related_information.is_none());
    }

    #[test]
    fn duplicate_export_with_single_location_has_no_related_info() {
        let root = test_root();
        let path = root.join("src/solo.ts");

        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "helper".to_string(),
            locations: vec![DuplicateLocation {
                path: path.clone(),
                line: 5,
                col: 0,
            }],
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&path).unwrap();
        let d = &diags[&uri][0];
        // No other locations to relate to
        assert!(d.related_information.is_none());
    }

    #[test]
    fn all_diagnostic_codes_have_doc_links() {
        let root = test_root();
        let path = root.join("src/file.ts");
        let mut results = AnalysisResults::default();

        // Add one of each issue type to verify all produce code_description
        results.unused_exports.push(UnusedExport {
            path: path.clone(),
            export_name: "e".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_types.push(UnusedExport {
            path: path.clone(),
            export_name: "T".to_string(),
            is_type_only: true,
            line: 2,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results
            .unused_files
            .push(fallow_core::results::UnusedFile { path: path.clone() });
        results
            .unused_enum_members
            .push(fallow_core::results::UnusedMember {
                path: path.clone(),
                parent_name: "E".to_string(),
                member_name: "A".to_string(),
                kind: fallow_core::extract::MemberKind::EnumMember,
                line: 3,
                col: 0,
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&path).unwrap();
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
                "Doc link should point to fallow docs: {href}"
            );
        }
    }
}
