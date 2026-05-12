use rustc_hash::FxHashMap;

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range, Url,
};

use fallow_core::results::AnalysisResults;

use super::{FIRST_LINE_RANGE, doc_link};

pub fn push_circular_dep_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for cycle in &results.circular_dependencies {
        if let Some(first_file) = cycle.files.first()
            && let Ok(uri) = Url::from_file_path(first_file)
        {
            let chain: Vec<String> = cycle
                .files
                .iter()
                .map(|f| {
                    f.file_name().map_or_else(
                        || f.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let message = format!("Circular dependency: {}", chain.join(" \u{2192} "));
            let line = cycle.line.saturating_sub(1);

            // Related info: link to each file in the cycle chain
            let related_info: Vec<DiagnosticRelatedInformation> = cycle
                .files
                .iter()
                .skip(1) // skip the first file (it's the diagnostic location)
                .enumerate()
                .filter_map(|(i, f)| {
                    let file_uri = Url::from_file_path(f).ok()?;
                    let name = f.file_name().map_or_else(
                        || f.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    Some(DiagnosticRelatedInformation {
                        location: Location {
                            uri: file_uri,
                            range: FIRST_LINE_RANGE,
                        },
                        message: format!("Step {} in cycle: {name}", i + 2),
                    })
                })
                .collect();

            map.entry(uri).or_default().push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: cycle.col,
                    },
                    end: Position {
                        line,
                        character: u32::MAX,
                    },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("fallow".to_string()),
                code: Some(NumberOrString::String("circular-dependency".to_string())),
                code_description: doc_link("circular-dependencies"),
                message,
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

pub fn push_boundary_violation_diagnostics(
    map: &mut FxHashMap<Url, Vec<Diagnostic>>,
    results: &AnalysisResults,
) {
    for v in &results.boundary_violations {
        let Ok(uri) = Url::from_file_path(&v.from_path) else {
            continue;
        };
        let line = v.line.saturating_sub(1);
        let to_name = v.to_path.file_name().map_or_else(
            || v.to_path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let message = format!(
            "Boundary violation: import of {} (zone '{}') is not allowed from zone '{}'",
            to_name, v.to_zone, v.from_zone,
        );

        // Related info: link to the target file
        let related_info = Url::from_file_path(&v.to_path).ok().map(|target_uri| {
            vec![DiagnosticRelatedInformation {
                location: Location {
                    uri: target_uri,
                    range: FIRST_LINE_RANGE,
                },
                message: format!("Target file in zone '{}'", v.to_zone),
            }]
        });

        map.entry(uri).or_default().push(Diagnostic {
            range: Range {
                start: Position {
                    line,
                    character: v.col,
                },
                end: Position {
                    line,
                    character: u32::MAX,
                },
            },
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String("boundary-violation".to_string())),
            code_description: doc_link("boundary-violations"),
            message,
            related_information: related_info,
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_core::duplicates::{DuplicationReport, DuplicationStats};
    use fallow_core::results::{AnalysisResults, BoundaryViolation, CircularDependency};
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
    fn circular_dependency_produces_warning_with_chain_message() {
        let root = test_root();
        let file_a = root.join("src/a.ts");
        let file_b = root.join("src/b.ts");
        let file_c = root.join("src/c.ts");

        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![file_a.clone(), file_b.clone(), file_c.clone()],
            length: 3,
            line: 2,
            col: 20,
            is_cross_package: false,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        // Diagnostic should be on the first file in the cycle
        let uri_a = Url::from_file_path(&file_a).unwrap();
        let file_diags = &diags[&uri_a];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("circular-dependency".to_string()))
        );
        assert!(d.message.contains("Circular dependency"));
        assert!(d.message.contains("a.ts"));
        assert!(d.message.contains("b.ts"));
        assert!(d.message.contains("c.ts"));
        assert!(d.message.contains("\u{2192}")); // arrow separator

        // Line should be 0-based
        assert_eq!(d.range.start.line, 1); // 1-based 2 -> 0-based 1
        assert_eq!(d.range.start.character, 20);
        assert_eq!(d.range.end.character, u32::MAX);

        // Related information should point to other files in the cycle
        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 2); // file_b and file_c (skips first file)
        assert_eq!(related[0].message, "Step 2 in cycle: b.ts");
        assert_eq!(related[1].message, "Step 3 in cycle: c.ts");

        let uri_b = Url::from_file_path(&file_b).unwrap();
        let uri_c = Url::from_file_path(&file_c).unwrap();
        assert_eq!(related[0].location.uri, uri_b);
        assert_eq!(related[1].location.uri, uri_c);
    }

    #[test]
    fn circular_dependency_with_single_file_has_no_related_info() {
        let root = test_root();
        let file_a = root.join("src/self.ts");

        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![file_a.clone()],
            length: 1,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&file_a).unwrap();
        let d = &diags[&uri][0];
        // With a single file, skip(1) yields nothing, so related_information is None
        assert!(d.related_information.is_none());
    }

    #[test]
    fn circular_dependency_with_empty_files_produces_no_diagnostic() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![],
            length: 0,
            line: 0,
            col: 0,
            is_cross_package: false,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);
        assert!(diags.is_empty());
    }

    #[test]
    fn boundary_violation_produces_warning_with_zone_message() {
        let root = test_root();
        let from_file = root.join("src/feature/api.ts");
        let to_file = root.join("src/core/secret.ts");

        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: from_file.clone(),
            to_path: to_file,
            from_zone: "feature".to_string(),
            to_zone: "core".to_string(),
            import_specifier: "../core/secret".to_string(),
            line: 3,
            col: 10,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&from_file).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 1);

        let d = &file_diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("boundary-violation".to_string()))
        );
        assert!(d.message.contains("Boundary violation"));
        assert!(d.message.contains("secret.ts"));
        assert!(d.message.contains("core"));
        assert!(d.message.contains("feature"));

        // Line should be 0-based
        assert_eq!(d.range.start.line, 2); // 1-based 3 -> 0-based 2
        assert_eq!(d.range.start.character, 10);
        assert_eq!(d.range.end.character, u32::MAX);
    }

    #[test]
    fn boundary_violation_has_warning_severity() {
        let root = test_root();
        let from_file = root.join("src/ui/button.ts");
        let to_file = root.join("src/infra/db.ts");

        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: from_file.clone(),
            to_path: to_file,
            from_zone: "ui".to_string(),
            to_zone: "infra".to_string(),
            import_specifier: "../infra/db".to_string(),
            line: 1,
            col: 0,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&from_file).unwrap();
        let d = &diags[&uri][0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.source, Some("fallow".to_string()));
    }

    #[test]
    fn boundary_violation_has_related_info_linking_to_target() {
        let root = test_root();
        let from_file = root.join("src/app/page.ts");
        let to_file = root.join("src/domain/entity.ts");

        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: from_file.clone(),
            to_path: to_file.clone(),
            from_zone: "app".to_string(),
            to_zone: "domain".to_string(),
            import_specifier: "../domain/entity".to_string(),
            line: 5,
            col: 0,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&from_file).unwrap();
        let d = &diags[&uri][0];

        let related = d.related_information.as_ref().unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, "Target file in zone 'domain'");

        let target_uri = Url::from_file_path(&to_file).unwrap();
        assert_eq!(related[0].location.uri, target_uri);
    }

    #[test]
    fn multiple_boundary_violations_in_same_file_aggregate() {
        let root = test_root();
        let from_file = root.join("src/feature/handler.ts");
        let to_file_a = root.join("src/core/auth.ts");
        let to_file_b = root.join("src/infra/cache.ts");

        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: from_file.clone(),
            to_path: to_file_a,
            from_zone: "feature".to_string(),
            to_zone: "core".to_string(),
            import_specifier: "../core/auth".to_string(),
            line: 1,
            col: 0,
        });
        results.boundary_violations.push(BoundaryViolation {
            from_path: from_file.clone(),
            to_path: to_file_b,
            from_zone: "feature".to_string(),
            to_zone: "infra".to_string(),
            import_specifier: "../infra/cache".to_string(),
            line: 2,
            col: 0,
        });

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);

        let uri = Url::from_file_path(&from_file).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 2);

        assert!(file_diags[0].message.contains("auth.ts"));
        assert!(file_diags[1].message.contains("cache.ts"));
    }

    #[test]
    fn empty_boundary_violations_produces_no_diagnostics() {
        let root = test_root();
        let results = AnalysisResults::default();

        let duplication = empty_duplication();
        let diags = build_diagnostics(&results, &duplication, &root);
        assert!(diags.is_empty());
    }
}
