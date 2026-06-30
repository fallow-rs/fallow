use crate::report::sink::outln;
use std::path::Path;

use colored::Colorize;
use fallow_config::OutputFormat;

use super::{plural, relative_path};

pub(in crate::report) fn print_cross_reference_findings(
    cross_ref: &fallow_engine::CrossReferenceResult,
    root: &Path,
    quiet: bool,
    output: OutputFormat,
) {
    if cross_ref.combined_findings.is_empty() {
        return;
    }

    if !matches!(output, OutputFormat::Human) {
        return;
    }

    if quiet {
        return;
    }

    for line in build_cross_reference_lines(cross_ref, root) {
        outln!("{line}");
    }

    let total = cross_ref.total();
    let files = cross_ref.clones_in_unused_files;
    let exports = cross_ref.clones_with_unused_exports;
    eprintln!(
        "  {} combined finding{}: {} in unused file{}, {} overlapping unused export{}",
        total,
        plural(total),
        files,
        plural(files),
        exports,
        plural(exports),
    );
}

/// Build human-readable output lines for cross-reference findings.
pub(in crate::report) fn build_cross_reference_lines(
    cross_ref: &fallow_engine::CrossReferenceResult,
    root: &Path,
) -> Vec<String> {
    use fallow_engine::DeadCodeKind;

    let mut lines = Vec::new();

    if cross_ref.combined_findings.is_empty() {
        return lines;
    }

    lines.push(String::new());
    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        "Duplicated + Unused (safe to delete)".yellow().bold()
    ));
    lines.push(String::new());

    for finding in &cross_ref.combined_findings {
        let relative = relative_path(&finding.clone_instance.file, root);
        let location = format!(
            "{}:{}-{}",
            relative.display(),
            finding.clone_instance.start_line,
            finding.clone_instance.end_line
        );

        let reason = match &finding.dead_code_kind {
            DeadCodeKind::UnusedFile => "entire file is unused".to_string(),
            DeadCodeKind::UnusedExport { export_name } => {
                format!("export '{export_name}' is unused")
            }
            DeadCodeKind::UnusedType { type_name } => {
                format!("type '{type_name}' is unused")
            }
        };

        lines.push(format!(
            "  {} {}",
            location.bold(),
            format!("({reason})").dimmed()
        ));
    }

    lines.push(String::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;
    use fallow_engine::{CombinedFinding, CrossReferenceResult, DeadCodeKind};
    use fallow_types::duplicates::CloneInstance;
    use std::path::PathBuf;

    #[test]
    fn cross_reference_empty_findings_produces_header_and_blanks() {
        let root = PathBuf::from("/project");
        let cross_ref = CrossReferenceResult {
            combined_findings: vec![CombinedFinding {
                clone_instance: CloneInstance {
                    file: root.join("src/dead.ts"),
                    start_line: 1,
                    end_line: 10,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                },
                dead_code_kind: DeadCodeKind::UnusedFile,
                group_index: 0,
            }],
            clones_in_unused_files: 1,
            clones_with_unused_exports: 0,
        };
        let lines = build_cross_reference_lines(&cross_ref, &root);
        let text = plain(&lines);
        assert!(text.contains("Duplicated + Unused (safe to delete)"));
        assert!(text.contains("src/dead.ts:1-10"));
        assert!(text.contains("(entire file is unused)"));
    }

    #[test]
    fn cross_reference_unused_export_reason() {
        let root = PathBuf::from("/project");
        let cross_ref = CrossReferenceResult {
            combined_findings: vec![CombinedFinding {
                clone_instance: CloneInstance {
                    file: root.join("src/utils.ts"),
                    start_line: 5,
                    end_line: 15,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                },
                dead_code_kind: DeadCodeKind::UnusedExport {
                    export_name: "processData".to_string(),
                },
                group_index: 0,
            }],
            clones_in_unused_files: 0,
            clones_with_unused_exports: 1,
        };
        let lines = build_cross_reference_lines(&cross_ref, &root);
        let text = plain(&lines);
        assert!(text.contains("export 'processData' is unused"));
    }

    #[test]
    fn cross_reference_unused_type_reason() {
        let root = PathBuf::from("/project");
        let cross_ref = CrossReferenceResult {
            combined_findings: vec![CombinedFinding {
                clone_instance: CloneInstance {
                    file: root.join("src/types.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                },
                dead_code_kind: DeadCodeKind::UnusedType {
                    type_name: "OldConfig".to_string(),
                },
                group_index: 0,
            }],
            clones_in_unused_files: 0,
            clones_with_unused_exports: 1,
        };
        let lines = build_cross_reference_lines(&cross_ref, &root);
        let text = plain(&lines);
        assert!(text.contains("type 'OldConfig' is unused"));
    }
}
