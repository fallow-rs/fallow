use std::path::Path;

use ls_types::{CodeLens, Command, Position, Range, Uri};
use serde::Serialize;

use fallow_api::{
    EditorAnalysisResults as AnalysisResults,
    EditorInlineComplexityExceeded as InlineComplexityExceeded,
    EditorInlineComplexityFinding as InlineComplexityFinding,
};

use crate::position::PositionMapper;

fn complexity_exceeded_label(exceeded: InlineComplexityExceeded) -> &'static str {
    match exceeded {
        InlineComplexityExceeded::Cyclomatic => "cyclomatic",
        InlineComplexityExceeded::Cognitive => "cognitive",
        InlineComplexityExceeded::CyclomaticAndCognitive => "cyclomatic, cognitive",
    }
}

/// Typed input for building Code Lens items from editor analysis state.
#[derive(Clone, Copy)]
pub struct CodeLensInput<'a> {
    pub results: &'a AnalysisResults,
    pub complexity: &'a [InlineComplexityFinding],
    pub file_path: &'a Path,
    pub document_uri: &'a Uri,
}

impl<'a> CodeLensInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        complexity: &'a [InlineComplexityFinding],
        file_path: &'a Path,
        document_uri: &'a Uri,
    ) -> Self {
        Self {
            results,
            complexity,
            file_path,
            document_uri,
        }
    }
}

/// Build Code Lens items for a file showing reference counts above each export declaration.
pub fn build_code_lenses(input: CodeLensInput<'_>) -> Vec<CodeLens> {
    let CodeLensInput {
        results,
        complexity,
        file_path,
        document_uri,
    } = input;
    let mut mapper = PositionMapper::default();
    let mut lenses = export_usage_code_lenses(results, file_path, document_uri, &mut mapper);
    lenses.extend(complexity_code_lenses(complexity, file_path, &mut mapper));
    lenses.extend(react_component_code_lenses(results, file_path, &mut mapper));

    lenses
}

#[cfg(test)]
fn build_code_lenses_for_test(
    results: &AnalysisResults,
    complexity: &[InlineComplexityFinding],
    file_path: &Path,
    document_uri: &Uri,
) -> Vec<CodeLens> {
    build_code_lenses(CodeLensInput::new(
        results,
        complexity,
        file_path,
        document_uri,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReferenceCommandPosition {
    line: u32,
    character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReferenceCommandRange {
    start: ReferenceCommandPosition,
    end: ReferenceCommandPosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReferenceLocationPayload {
    uri: String,
    range: ReferenceCommandRange,
}

/// Build the DESCRIPTIVE per-component React summary lenses for a file: one lens
/// above each component with its render-site / distinct-parent / prop / hook
/// breakdown. Ambient editor context, never a finding. Zero segments are
/// omitted cleanly (a component rendered nowhere with no props and no hooks
/// still gets a lens, but the segments it lacks are dropped).
fn react_component_code_lenses(
    results: &AnalysisResults,
    file_path: &Path,
    mapper: &mut PositionMapper,
) -> Vec<CodeLens> {
    results
        .react_component_intel
        .iter()
        .filter(|intel| intel.path == file_path)
        .map(|intel| react_component_code_lens(intel, mapper))
        .collect()
}

fn react_component_code_lens(
    intel: &fallow_api::editor_results::ReactComponentIntel,
    mapper: &mut PositionMapper,
) -> CodeLens {
    let position = Position {
        line: intel.anchor_line.saturating_sub(1),
        character: mapper.utf16_col(
            &intel.path,
            intel.anchor_line.saturating_sub(1),
            intel.anchor_col,
        ),
    };
    CodeLens {
        range: Range {
            start: position,
            end: position,
        },
        command: Some(Command {
            title: react_component_lens_title(intel),
            command: "fallow.noop".to_string(),
            arguments: None,
        }),
        data: None,
    }
}

/// Compose the component summary title from the non-zero segments:
/// `rendered 12x (8 parents) · 5 props · 9 hooks (4 state, 3 effect, ...)`.
/// A segment whose count is zero is omitted (no `· 0 props`); singular/plural is
/// honored (`1 prop`, `1 parent`). A component with no render sites, no props,
/// and no hooks falls back to a bare `component` label so the lens is never
/// empty.
fn react_component_lens_title(intel: &fallow_api::editor_results::ReactComponentIntel) -> String {
    let mut segments: Vec<String> = Vec::new();

    if intel.render_sites > 0 {
        let parents = pluralize(intel.distinct_parents, "parent");
        segments.push(format!("rendered {}x ({parents})", intel.render_sites));
    }
    if intel.prop_count > 0 {
        segments.push(pluralize(u32::from(intel.prop_count), "prop"));
    }

    if let Some(hooks) = react_hook_segment(&intel.hooks) {
        segments.push(hooks);
    }

    if segments.is_empty() {
        return "component".to_string();
    }
    segments.join(" · ")
}

/// Build the `N hooks (a state, b effect, ...)` segment, or `None` when the
/// component uses no hooks. Each kind sub-count is omitted when zero.
fn react_hook_segment(hooks: &fallow_api::editor_results::ReactHookSummary) -> Option<String> {
    let total = u32::from(hooks.state)
        + u32::from(hooks.effect)
        + u32::from(hooks.memo)
        + u32::from(hooks.callback)
        + u32::from(hooks.custom);
    if total == 0 {
        return None;
    }

    let mut breakdown: Vec<String> = Vec::new();
    for (count, label) in [
        (hooks.state, "state"),
        (hooks.effect, "effect"),
        (hooks.memo, "memo"),
        (hooks.callback, "callback"),
        (hooks.custom, "custom"),
    ] {
        if count > 0 {
            breakdown.push(format!("{count} {label}"));
        }
    }

    let head = pluralize(total, "hook");
    if breakdown.is_empty() {
        Some(head)
    } else {
        Some(format!("{head} ({})", breakdown.join(", ")))
    }
}

/// `count + " " + noun`, appending `s` when the count is not 1.
fn pluralize(count: u32, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn export_usage_code_lenses(
    results: &AnalysisResults,
    file_path: &Path,
    document_uri: &Uri,
    mapper: &mut PositionMapper,
) -> Vec<CodeLens> {
    results
        .export_usages
        .iter()
        .filter(|usage| usage.path == file_path)
        .map(|usage| export_usage_code_lens(usage, document_uri, mapper))
        .collect()
}

fn export_usage_code_lens(
    usage: &fallow_api::editor_results::ExportUsage,
    document_uri: &Uri,
    mapper: &mut PositionMapper,
) -> CodeLens {
    let line = usage.line.saturating_sub(1);
    let title = if usage.reference_count == 1 {
        "1 reference".to_string()
    } else {
        format!("{} references", usage.reference_count)
    };
    let export_position = Position {
        line,
        character: mapper.utf16_col(&usage.path, line, usage.col),
    };
    let ref_locations: Vec<ReferenceLocationPayload> = usage
        .reference_locations
        .iter()
        .filter_map(|loc| reference_location_payload(loc, mapper))
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
    loc: &fallow_api::editor_results::ReferenceLocation,
    mapper: &mut PositionMapper,
) -> Option<ReferenceLocationPayload> {
    let uri = Uri::from_file_path(&loc.path)?;
    let ref_line = loc.line.saturating_sub(1);
    let position = ReferenceCommandPosition {
        line: ref_line,
        character: mapper.utf16_col(&loc.path, ref_line, loc.col),
    };
    Some(ReferenceLocationPayload {
        uri: uri.as_str().to_string(),
        range: ReferenceCommandRange {
            start: position.clone(),
            end: position,
        },
    })
}

fn reference_command(
    document_uri: &Uri,
    export_position: Position,
    ref_locations: &[ReferenceLocationPayload],
) -> (String, Option<Vec<serde_json::Value>>) {
    if ref_locations.is_empty() {
        return ("fallow.noop".to_string(), None);
    }

    (
        "fallow.showReferences".to_string(),
        reference_command_arguments(document_uri, export_position, ref_locations),
    )
}

fn reference_command_arguments(
    document_uri: &Uri,
    export_position: Position,
    ref_locations: &[ReferenceLocationPayload],
) -> Option<Vec<serde_json::Value>> {
    let export_position = ReferenceCommandPosition {
        line: export_position.line,
        character: export_position.character,
    };
    Some(vec![
        serde_json::to_value(document_uri.as_str()).ok()?,
        serde_json::to_value(export_position).ok()?,
        serde_json::to_value(ref_locations).ok()?,
    ])
}

fn complexity_code_lenses(
    complexity: &[InlineComplexityFinding],
    file_path: &Path,
    mapper: &mut PositionMapper,
) -> Vec<CodeLens> {
    complexity
        .iter()
        .filter(|finding| finding.path == file_path)
        .map(|finding| complexity_code_lens(finding, mapper))
        .collect()
}

fn complexity_code_lens(
    finding: &InlineComplexityFinding,
    mapper: &mut PositionMapper,
) -> CodeLens {
    let line = finding.line.saturating_sub(1);
    let position = Position {
        line,
        character: mapper.utf16_col(&finding.path, line, finding.col),
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
                complexity_exceeded_label(finding.exceeded)
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

    use fallow_api::editor_results::{
        ExportUsage, ReactComponentIntel, ReactHookSummary, ReactPropIntel, ReferenceLocation,
    };

    fn react_intel(path: PathBuf) -> ReactComponentIntel {
        ReactComponentIntel {
            path,
            component_name: "Card".to_string(),
            anchor_line: 7,
            anchor_col: 13,
            render_sites: 12,
            distinct_parents: 8,
            prop_count: 5,
            hooks: ReactHookSummary {
                state: 4,
                effect: 3,
                memo: 1,
                callback: 1,
                custom: 0,
            },
            props: vec![ReactPropIntel {
                name: "title".to_string(),
                anchor_line: 7,
                anchor_col: 2,
                used_in_body: true,
                passed_from_sites: 3,
                drill: None,
            }],
        }
    }

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

        let lenses = build_code_lenses_for_test(&results, &[], &mod_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &mod_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &edge_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
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
        let lenses = build_code_lenses_for_test(&results, &[], &utils_path, &uri);

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
        let lenses = build_code_lenses_for_test(&results, &complexity, &utils_path, &uri);

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
    fn complexity_lens_uses_utf16_columns() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let path = root.join("src/non_ascii.ts");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        let source = "const emoji = \"🎉\"; function parseConfig() {}\n";
        std::fs::write(&path, source).expect("write fixture");
        let byte_col = source.find("parseConfig").expect("parseConfig") as u32;
        let utf16_col = source[..byte_col as usize].encode_utf16().count() as u32;
        let results = AnalysisResults::default();
        let complexity = vec![InlineComplexityFinding {
            path: path.clone(),
            name: "parseConfig".to_string(),
            line: 1,
            col: byte_col,
            cyclomatic: 31,
            cognitive: 26,
            exceeded: InlineComplexityExceeded::CyclomaticAndCognitive,
        }];

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses_for_test(&results, &complexity, &path, &uri);

        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].range.start.character, utf16_col);
        assert_eq!(lenses[0].range.end.character, utf16_col);
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
        let lenses = build_code_lenses_for_test(&results, &complexity, &utils_path, &uri);

        assert!(lenses.is_empty());
    }

    #[test]
    fn react_component_lens_full_summary() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let mut results = AnalysisResults::default();
        results
            .react_component_intel
            .push(react_intel(path.clone()));

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
        assert_eq!(lenses.len(), 1);

        let cmd = lenses[0].command.as_ref().unwrap();
        assert_eq!(
            cmd.title,
            "rendered 12x (8 parents) · 5 props · 9 hooks (4 state, 3 effect, 1 memo, 1 callback)"
        );
        // Anchored at the component definition (1-based line 7 -> 0-based 6).
        assert_eq!(lenses[0].range.start.line, 6);
        assert_eq!(lenses[0].range.start.character, 13);
        assert_eq!(cmd.command, "fallow.noop");
    }

    #[test]
    fn react_component_lens_omits_zero_segments_and_singularizes() {
        let root = test_root();
        let path = root.join("src/Solo.tsx");
        let mut results = AnalysisResults::default();
        let mut intel = react_intel(path.clone());
        intel.component_name = "Solo".to_string();
        intel.render_sites = 1;
        intel.distinct_parents = 1;
        intel.prop_count = 1;
        intel.hooks = ReactHookSummary {
            state: 1,
            ..ReactHookSummary::default()
        };
        results.react_component_intel.push(intel);

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
        let cmd = lenses[0].command.as_ref().unwrap();
        // Singular "1 parent" / "1 prop" / "1 hook", no zero memo/effect/etc.
        assert_eq!(
            cmd.title,
            "rendered 1x (1 parent) · 1 prop · 1 hook (1 state)"
        );
    }

    #[test]
    fn react_component_lens_omits_render_and_prop_when_zero() {
        let root = test_root();
        let path = root.join("src/Bare.tsx");
        let mut results = AnalysisResults::default();
        let mut intel = react_intel(path.clone());
        intel.component_name = "Bare".to_string();
        intel.render_sites = 0;
        intel.distinct_parents = 0;
        intel.prop_count = 0;
        intel.props = vec![];
        intel.hooks = ReactHookSummary::default();
        results.react_component_intel.push(intel);

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
        let cmd = lenses[0].command.as_ref().unwrap();
        // No segments -> the bare "component" fallback (never "rendered 0x").
        assert_eq!(cmd.title, "component");
    }

    #[test]
    fn react_component_lens_ignores_unrelated_file() {
        let root = test_root();
        let path = root.join("src/Card.tsx");
        let other = root.join("src/Other.tsx");
        let mut results = AnalysisResults::default();
        results.react_component_intel.push(react_intel(other));

        let uri = Uri::from_file_path(&path).unwrap();
        let lenses = build_code_lenses_for_test(&results, &[], &path, &uri);
        assert!(lenses.is_empty());
    }
}
