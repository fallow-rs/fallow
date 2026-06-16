use std::path::{Path, PathBuf};

use ls_types::{CodeLens, Command, Position, Range, Uri};

use fallow_core::results::AnalysisResults;

/// LSP-local inline complexity signal rendered as a code lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineComplexityFinding {
    pub path: PathBuf,
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub cyclomatic: u16,
    pub cognitive: u16,
    pub exceeded: InlineComplexityExceeded,
}

/// Which health complexity threshold(s) a function exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineComplexityExceeded {
    Cyclomatic,
    Cognitive,
    CyclomaticAndCognitive,
}

impl InlineComplexityExceeded {
    fn label(self) -> &'static str {
        match self {
            Self::Cyclomatic => "cyclomatic",
            Self::Cognitive => "cognitive",
            Self::CyclomaticAndCognitive => "cyclomatic, cognitive",
        }
    }
}

/// Build Code Lens items for a file showing reference counts above each export declaration.
pub fn build_code_lenses(
    results: &AnalysisResults,
    complexity: &[InlineComplexityFinding],
    file_path: &Path,
    document_uri: &Uri,
) -> Vec<CodeLens> {
    let mut lenses = export_usage_code_lenses(results, file_path, document_uri);
    lenses.extend(complexity_code_lenses(complexity, file_path));

    lenses
}

fn export_usage_code_lenses(
    results: &AnalysisResults,
    file_path: &Path,
    document_uri: &Uri,
) -> Vec<CodeLens> {
    results
        .export_usages
        .iter()
        .filter(|usage| usage.path == file_path)
        .map(|usage| export_usage_code_lens(usage, document_uri))
        .collect()
}

fn export_usage_code_lens(
    usage: &fallow_core::results::ExportUsage,
    document_uri: &Uri,
) -> CodeLens {
    let line = usage.line.saturating_sub(1);
    let title = if usage.reference_count == 1 {
        "1 reference".to_string()
    } else {
        format!("{} references", usage.reference_count)
    };
    let export_position = Position {
        line,
        character: usage.col,
    };
    let ref_locations: Vec<serde_json::Value> = usage
        .reference_locations
        .iter()
        .filter_map(reference_location_payload)
        .collect();
    let (command_name, arguments) =
        reference_command(document_uri, export_position, &ref_locations);

    CodeLens {
        range: Range {
            start: export_position,
            end: export_position,
        },
        command: Some(Command {
            title,
            command: command_name,
            arguments,
        }),
        data: None,
    }
}

fn reference_location_payload(
    loc: &fallow_core::results::ReferenceLocation,
) -> Option<serde_json::Value> {
    let uri = Uri::from_file_path(&loc.path)?;
    let ref_line = loc.line.saturating_sub(1);
    Some(serde_json::json!({
        "uri": uri.as_str(),
        "range": {
            "start": { "line": ref_line, "character": loc.col },
            "end": { "line": ref_line, "character": loc.col }
        }
    }))
}

fn reference_command(
    document_uri: &Uri,
    export_position: Position,
    ref_locations: &[serde_json::Value],
) -> (String, Option<Vec<serde_json::Value>>) {
    if ref_locations.is_empty() {
        return ("fallow.noop".to_string(), None);
    }

    (
        "fallow.showReferences".to_string(),
        Some(vec![
            serde_json::json!(document_uri.as_str()),
            serde_json::json!({
                "line": export_position.line,
                "character": export_position.character,
            }),
            serde_json::json!(ref_locations),
        ]),
    )
}

fn complexity_code_lenses(
    complexity: &[InlineComplexityFinding],
    file_path: &Path,
) -> Vec<CodeLens> {
    complexity
        .iter()
        .filter(|finding| finding.path == file_path)
        .map(complexity_code_lens)
        .collect()
}

fn complexity_code_lens(finding: &InlineComplexityFinding) -> CodeLens {
    let position = Position {
        line: finding.line.saturating_sub(1),
        character: finding.col,
    };
    CodeLens {
        range: Range {
            start: position,
            end: position,
        },
        command: Some(Command {
            title: format!(
                "{} complexity: {} cyc, {} cog ({})",
                finding.name,
                finding.cyclomatic,
                finding.cognitive,
                finding.exceeded.label()
            ),
            command: "fallow.noop".to_string(),
            arguments: None,
        }),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_core::results::{ExportUsage, ReferenceLocation};

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    #[test]
    fn no_lenses_for_empty_results() {
        let root = test_root();
        let mod_path = root.join("src/mod.ts");
        let results = AnalysisResults::default();
        let uri = Uri::from_file_path(&mod_path).unwrap();

        let lenses = build_code_lenses(&results, &[], &mod_path, &uri);
        assert!(lenses.is_empty());
    }

    #[test]
    fn no_lenses_for_unrelated_file() {
        let root = test_root();
        let mod_path = root.join("src/mod.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: root.join("src/other.ts"),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&mod_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &mod_path, &uri);
        assert!(lenses.is_empty());
    }

    #[test]
    fn single_reference_uses_singular_title() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "helper".to_string(),
            line: 10,
            col: 7,
            reference_count: 1,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.title, "1 reference");
    }

    #[test]
    fn multiple_references_uses_plural_title() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "helper".to_string(),
            line: 10,
            col: 7,
            reference_count: 5,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.title, "5 references");
    }

    #[test]
    fn zero_references_uses_plural_title() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "unused".to_string(),
            line: 1,
            col: 0,
            reference_count: 0,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.title, "0 references");
    }

    #[test]
    fn lens_position_matches_export_span() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "myExport".to_string(),
            line: 15, // 1-based
            col: 4,
            reference_count: 2,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        assert_eq!(lenses[0].range.start.line, 14);
        assert_eq!(lenses[0].range.start.character, 4);
        assert_eq!(lenses[0].range.end.line, 14);
        assert_eq!(lenses[0].range.end.character, 4);
    }

    #[test]
    fn noop_command_when_no_reference_locations() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "x".to_string(),
            line: 1,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.command, "fallow.noop");
        assert!(cmd.arguments.is_none());
    }

    #[test]
    fn show_references_command_with_reference_locations() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "helper".to_string(),
            line: 5,
            col: 7,
            reference_count: 2,
            reference_locations: vec![
                ReferenceLocation {
                    path: root.join("src/app.ts"),
                    line: 3,
                    col: 10,
                },
                ReferenceLocation {
                    path: root.join("src/main.ts"),
                    line: 8,
                    col: 0,
                },
            ],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.command, "fallow.showReferences");

        let args = cmd.arguments.as_ref().unwrap();
        assert_eq!(args.len(), 3);

        assert_eq!(args[0], serde_json::json!(uri.as_str()));
        assert_eq!(args[1]["line"], 4); // 1-based 5 → 0-based 4
        assert_eq!(args[1]["character"], 7);
        let ref_locs = args[2].as_array().unwrap();
        assert_eq!(ref_locs.len(), 2);
        let app_uri = Uri::from_file_path(root.join("src/app.ts")).unwrap();
        assert_eq!(ref_locs[0]["uri"], app_uri.as_str());
        assert_eq!(ref_locs[0]["range"]["start"]["line"], 2);
        assert_eq!(ref_locs[0]["range"]["start"]["character"], 10);
    }

    #[test]
    fn multiple_exports_produce_multiple_lenses() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        let path = root.join("src/utils.ts");
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 1,
            reference_locations: vec![],
        });
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "bar".to_string(),
            line: 10,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "baz".to_string(),
            line: 20,
            col: 0,
            reference_count: 0,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses(&results, &[], &path, &uri);
        assert_eq!(lenses.len(), 3);

        let titles: Vec<&str> = lenses
            .iter()
            .map(|l| l.command.as_ref().unwrap().title.as_str())
            .collect();
        assert_eq!(titles, vec!["1 reference", "3 references", "0 references"]);

        let lines: Vec<u32> = lenses.iter().map(|l| l.range.start.line).collect();
        assert_eq!(lines, vec![0, 9, 19]);
    }

    #[test]
    fn line_zero_saturates_correctly() {
        let root = test_root();
        let edge_path = root.join("src/edge.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: edge_path.clone(),
            export_name: "x".to_string(),
            line: 0,
            col: 0,
            reference_count: 1,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&edge_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &edge_path, &uri);
        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].range.start.line, 0);
    }

    #[test]
    fn reference_locations_with_mixed_valid_invalid_paths() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "helper".to_string(),
            line: 5,
            col: 7,
            reference_count: 2,
            reference_locations: vec![
                ReferenceLocation {
                    path: root.join("src/app.ts"),
                    line: 3,
                    col: 10,
                },
                ReferenceLocation {
                    path: std::path::PathBuf::new(),
                    line: 1,
                    col: 0,
                },
            ],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(cmd.command, "fallow.showReferences");

        let args = cmd.arguments.as_ref().unwrap();
        let ref_locs = args[2].as_array().unwrap();
        assert_eq!(ref_locs.len(), 1);
    }

    #[test]
    fn lens_range_is_zero_width_point() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "fn".to_string(),
            line: 10,
            col: 5,
            reference_count: 1,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses(&results, &[], &path, &uri);
        assert_eq!(lenses.len(), 1);

        assert_eq!(lenses[0].range.start, lenses[0].range.end);
    }

    #[test]
    fn lens_data_is_none() {
        let root = test_root();
        let path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: path.clone(),
            export_name: "fn".to_string(),
            line: 1,
            col: 0,
            reference_count: 1,
            reference_locations: vec![],
        });

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses(&results, &[], &path, &uri);
        assert!(
            lenses[0].data.is_none(),
            "Code lens data should be None since resolve_provider is false"
        );
    }

    #[test]
    fn reference_location_line_is_converted_to_zero_based() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let mut results = AnalysisResults::default();
        results.export_usages.push(ExportUsage {
            path: utils_path.clone(),
            export_name: "x".to_string(),
            line: 1,
            col: 0,
            reference_count: 1,
            reference_locations: vec![ReferenceLocation {
                path: root.join("src/consumer.ts"),
                line: 42, // 1-based
                col: 5,
            }],
        });

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &[], &utils_path, &uri);

        let cmd = lenses[0].command.as_ref().unwrap();
        let args = cmd.arguments.as_ref().unwrap();
        let ref_locs = args[2].as_array().unwrap();

        assert_eq!(ref_locs[0]["range"]["start"]["line"], 41);
        assert_eq!(ref_locs[0]["range"]["start"]["character"], 5);
    }

    #[test]
    fn complexity_lens_is_anchored_to_function_start() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let results = AnalysisResults::default();
        let complexity = vec![InlineComplexityFinding {
            path: utils_path.clone(),
            name: "parseConfig".to_string(),
            line: 12,
            col: 2,
            cyclomatic: 31,
            cognitive: 26,
            exceeded: InlineComplexityExceeded::CyclomaticAndCognitive,
        }];

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &complexity, &utils_path, &uri);

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].range.start.line, 11);
        assert_eq!(lenses[0].range.start.character, 2);
        let command = lenses[0].command.as_ref().expect("complexity lens command");
        assert_eq!(command.command, "fallow.noop");
        assert_eq!(
            command.title,
            "parseConfig complexity: 31 cyc, 26 cog (cyclomatic, cognitive)"
        );
    }

    #[test]
    fn complexity_lens_ignores_unrelated_file() {
        let root = test_root();
        let utils_path = root.join("src/utils.ts");
        let other_path = root.join("src/other.ts");
        let results = AnalysisResults::default();
        let complexity = vec![InlineComplexityFinding {
            path: other_path,
            name: "parseConfig".to_string(),
            line: 12,
            col: 2,
            cyclomatic: 31,
            cognitive: 26,
            exceeded: InlineComplexityExceeded::CyclomaticAndCognitive,
        }];

        let uri = Uri::from_file_path(&utils_path).unwrap();
        let lenses = build_code_lenses(&results, &complexity, &utils_path, &uri);

        assert!(lenses.is_empty());
    }
}
