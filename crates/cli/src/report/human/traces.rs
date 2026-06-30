use std::path::Path;

use colored::Colorize;
use fallow_types::trace::{CloneTrace, DependencyTrace, ExportTrace, FileTrace, TracedCloneGroup};

use super::{plural, relative_path};

pub(in crate::report) fn print_export_trace_human(trace: &ExportTrace) {
    print_lines(&build_export_trace_human_lines(trace));
}

pub(in crate::report) fn print_file_trace_human(trace: &FileTrace) {
    print_lines(&build_file_trace_human_lines(trace));
}

pub(in crate::report) fn print_dependency_trace_human(trace: &DependencyTrace) {
    print_lines(&build_dependency_trace_human_lines(trace));
}

pub(in crate::report) fn print_clone_trace_human(trace: &CloneTrace, root: &Path) {
    print_lines(&build_clone_trace_human_lines(trace, root));
}

fn print_lines(lines: &[String]) {
    for line in lines {
        eprintln!("{line}");
    }
}

fn build_export_trace_human_lines(trace: &ExportTrace) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    let status_icon = if trace.is_used {
        "USED".green().bold()
    } else {
        "UNUSED".red().bold()
    };
    lines.push(format!(
        "  {status_icon} {} in {}",
        trace.export_name.bold(),
        trace.file.display().to_string().dimmed()
    ));
    lines.push(String::new());

    let reachable = if trace.file_reachable {
        "reachable".green()
    } else {
        "unreachable".red()
    };
    let entry = if trace.is_entry_point {
        " (entry point)".cyan().to_string()
    } else {
        String::new()
    };
    lines.push(format!("  File: {reachable}{entry}"));
    lines.push(format!("  Reason: {}", trace.reason));

    if !trace.direct_references.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "  {} direct reference(s):",
            trace.direct_references.len()
        ));
        for r in &trace.direct_references {
            lines.push(format!(
                "    {} {} ({})",
                "->".dimmed(),
                r.from_file.display(),
                r.kind.dimmed()
            ));
        }
    }

    if !trace.re_export_chains.is_empty() {
        lines.push(String::new());
        lines.push("  Re-exported through:".to_string());
        for chain in &trace.re_export_chains {
            lines.push(format!(
                "    {} {} as '{}' ({} ref(s))",
                "->".dimmed(),
                chain.barrel_file.display(),
                chain.exported_as,
                chain.reference_count
            ));
        }
    }
    lines.push(String::new());
    lines
}

fn build_file_trace_human_lines(trace: &FileTrace) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    let reachable = if trace.is_reachable {
        "REACHABLE".green().bold()
    } else {
        "UNREACHABLE".red().bold()
    };
    let entry = if trace.is_entry_point {
        format!(" {}", "(entry point)".cyan())
    } else {
        String::new()
    };
    lines.push(format!(
        "  {reachable} {}{entry}",
        trace.file.display().to_string().bold()
    ));

    push_file_trace_exports(&mut lines, trace);
    push_file_trace_import_lists(&mut lines, trace);
    push_file_trace_re_exports(&mut lines, trace);
    lines.push(String::new());
    lines
}

/// Renders the file trace's exports section, including per-export referrers.
fn push_file_trace_exports(lines: &mut Vec<String>, trace: &FileTrace) {
    if trace.exports.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("  Exports ({}):", trace.exports.len()));
    for export in &trace.exports {
        let used_indicator = if export.reference_count > 0 {
            format!("{} ref(s)", export.reference_count)
                .green()
                .to_string()
        } else {
            "unused".red().to_string()
        };
        let type_tag = if export.is_type_only {
            " (type)".dimmed().to_string()
        } else {
            String::new()
        };
        lines.push(format!(
            "    {} {}{} [{}]",
            "export".dimmed(),
            export.name.bold(),
            type_tag,
            used_indicator
        ));
        for r in &export.referenced_by {
            lines.push(format!(
                "      {} {} ({})",
                "->".dimmed(),
                r.from_file.display(),
                r.kind.dimmed()
            ));
        }
    }
}

/// Renders the file trace's "Imports from" and "Imported by" path lists.
fn push_file_trace_import_lists(lines: &mut Vec<String>, trace: &FileTrace) {
    if !trace.imports_from.is_empty() {
        lines.push(String::new());
        lines.push(format!("  Imports from ({}):", trace.imports_from.len()));
        for path in &trace.imports_from {
            lines.push(format!("    {} {}", "<-".dimmed(), path.display()));
        }
    }

    if !trace.imported_by.is_empty() {
        lines.push(String::new());
        lines.push(format!("  Imported by ({}):", trace.imported_by.len()));
        for path in &trace.imported_by {
            lines.push(format!("    {} {}", "->".dimmed(), path.display()));
        }
    }
}

/// Renders the file trace's re-exports section.
fn push_file_trace_re_exports(lines: &mut Vec<String>, trace: &FileTrace) {
    if trace.re_exports.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(format!("  Re-exports ({}):", trace.re_exports.len()));
    for re in &trace.re_exports {
        lines.push(format!(
            "    {} '{}' as '{}' from {}",
            "re-export".dimmed(),
            re.imported_name,
            re.exported_name,
            re.source_file.display()
        ));
    }
}

fn build_dependency_trace_human_lines(trace: &DependencyTrace) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    let status = if trace.is_used {
        "USED".green().bold()
    } else {
        "UNUSED".red().bold()
    };
    lines.push(format!(
        "  {status} {} ({} import(s))",
        trace.package_name.bold(),
        trace.import_count
    ));

    if !trace.imported_by.is_empty() {
        lines.push(String::new());
        lines.push("  Imported by:".to_string());
        for path in &trace.imported_by {
            let is_type_only = trace.type_only_imported_by.contains(path);
            let tag = if is_type_only {
                " (type-only)".dimmed().to_string()
            } else {
                String::new()
            };
            lines.push(format!("    {} {}{}", "->".dimmed(), path.display(), tag));
        }
    }
    if trace.used_in_scripts {
        lines.push(String::new());
        lines.push(format!(
            "  {}",
            "Referenced from package.json scripts or CI configs.".dimmed()
        ));
    }
    lines.push(String::new());
    lines
}

fn build_clone_trace_human_lines(trace: &CloneTrace, root: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    if let Some(ref matched) = trace.matched_instance {
        let relative = relative_path(&matched.file, root);
        lines.push(format!(
            "  {} clone at {}:{}-{}",
            "FOUND".green().bold(),
            relative.display(),
            matched.start_line,
            matched.end_line,
        ));
    }
    lines.push(format!(
        "  {} clone group(s) containing this location",
        trace.clone_groups.len()
    ));
    for (i, group) in trace.clone_groups.iter().enumerate() {
        push_clone_group_lines(&mut lines, i, group, trace, root);
    }
    if let Some(ref matched) = trace.matched_instance {
        lines.push(String::new());
        lines.push(format!("  {}:", "Code fragment".dimmed()));
        for (i, line) in matched.fragment.lines().enumerate() {
            lines.push(format!(
                "    {} {}",
                format!("{:>4}", matched.start_line + i).dimmed(),
                line
            ));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "  {} {}",
        "Docs:".dimmed(),
        super::dupes::DOCS_DUPLICATION.dimmed()
    ));
    lines.push(String::new());
    lines
}

/// Renders one clone group: the header, each instance line (marking the queried
/// instance), and the suggested-refactor block.
fn push_clone_group_lines(
    lines: &mut Vec<String>,
    index: usize,
    group: &TracedCloneGroup,
    trace: &CloneTrace,
    root: &Path,
) {
    lines.push(String::new());
    lines.push(format!(
        "  {}  {} ({} lines, {} tokens, {} instance{})",
        format!("Clone group {}", index + 1).bold(),
        group.fingerprint.dimmed(),
        group.line_count,
        group.token_count,
        group.instances.len(),
        plural(group.instances.len())
    ));
    for instance in &group.instances {
        let relative = relative_path(&instance.file, root);
        let is_queried = trace.matched_instance.as_ref().is_some_and(|m| {
            m.file == instance.file
                && m.start_line == instance.start_line
                && m.end_line == instance.end_line
        });
        let marker = if is_queried {
            ">>".cyan()
        } else {
            "->".dimmed()
        };
        lines.push(format!(
            "    {} {}:{}-{}",
            marker,
            relative.display(),
            instance.start_line,
            instance.end_line
        ));
    }
    lines.push(format!("    {}", "Suggested refactor".bold()));
    lines.push(format!(
        "      Extract function, saves ~{} line{}",
        group.suggestion.estimated_savings,
        plural(group.suggestion.estimated_savings),
    ));
    if let Some(ref name) = group.suggested_name {
        lines.push(format!(
            "      Proposed name: {}  {}",
            name,
            "(best-effort, verify before applying)".dimmed(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_engine::{
        CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace, ReExportChain,
        TracedCloneGroup, TracedExport, TracedReExport,
    };
    use fallow_types::duplicates::{CloneInstance, RefactoringKind, RefactoringSuggestion};

    use super::*;

    fn plain(lines: &[String]) -> String {
        lines
            .iter()
            .map(|line| super::super::strip_ansi(line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn export_trace_renders_reachability_references_and_barrels() {
        let trace = ExportTrace {
            file: PathBuf::from("src/lib.ts"),
            export_name: "formatUser".to_string(),
            file_reachable: true,
            is_entry_point: true,
            is_used: true,
            direct_references: vec![ExportReference {
                from_file: PathBuf::from("src/app.ts"),
                kind: "value".to_string(),
            }],
            re_export_chains: vec![ReExportChain {
                barrel_file: PathBuf::from("src/index.ts"),
                exported_as: "formatUser".to_string(),
                reference_count: 2,
            }],
            reason: "referenced from reachable code".to_string(),
        };

        let rendered = plain(&build_export_trace_human_lines(&trace));

        assert!(rendered.contains("USED formatUser in src/lib.ts"));
        assert!(rendered.contains("File: reachable (entry point)"));
        assert!(rendered.contains("Reason: referenced from reachable code"));
        assert!(rendered.contains("1 direct reference(s):"));
        assert!(rendered.contains("-> src/app.ts (value)"));
        assert!(rendered.contains("Re-exported through:"));
        assert!(rendered.contains("-> src/index.ts as 'formatUser' (2 ref(s))"));
    }

    #[test]
    fn file_trace_renders_exports_imports_dependents_and_re_exports() {
        let trace = FileTrace {
            file: PathBuf::from("src/model.ts"),
            is_reachable: false,
            is_entry_point: false,
            exports: vec![TracedExport {
                name: "User".to_string(),
                is_type_only: true,
                reference_count: 0,
                referenced_by: Vec::new(),
            }],
            imports_from: vec![PathBuf::from("src/db.ts")],
            imported_by: vec![PathBuf::from("src/app.ts")],
            re_exports: vec![TracedReExport {
                source_file: PathBuf::from("src/types.ts"),
                imported_name: "Account".to_string(),
                exported_name: "Account".to_string(),
            }],
        };

        let rendered = plain(&build_file_trace_human_lines(&trace));

        assert!(rendered.contains("UNREACHABLE src/model.ts"));
        assert!(rendered.contains("Exports (1):"));
        assert!(rendered.contains("export User (type) [unused]"));
        assert!(rendered.contains("Imports from (1):"));
        assert!(rendered.contains("<- src/db.ts"));
        assert!(rendered.contains("Imported by (1):"));
        assert!(rendered.contains("-> src/app.ts"));
        assert!(rendered.contains("Re-exports (1):"));
        assert!(rendered.contains("re-export 'Account' as 'Account' from src/types.ts"));
    }

    #[test]
    fn dependency_trace_renders_type_only_imports_and_script_usage() {
        let trace = DependencyTrace {
            package_name: "zod".to_string(),
            imported_by: vec![PathBuf::from("src/schema.ts")],
            type_only_imported_by: vec![PathBuf::from("src/schema.ts")],
            used_in_scripts: true,
            is_used: true,
            import_count: 1,
        };

        let rendered = plain(&build_dependency_trace_human_lines(&trace));

        assert!(rendered.contains("USED zod (1 import(s))"));
        assert!(rendered.contains("Imported by:"));
        assert!(rendered.contains("-> src/schema.ts (type-only)"));
        assert!(rendered.contains("Referenced from package.json scripts or CI configs."));
    }

    #[test]
    fn clone_trace_renders_match_group_suggestion_fragment_and_docs() {
        let root = PathBuf::from("/repo");
        let matched = clone_instance("/repo/src/a.ts", 10, 12, "const x = 1;\nreturn x;");
        let trace = CloneTrace {
            file: PathBuf::from("/repo/src/a.ts"),
            line: 10,
            matched_instance: Some(matched.clone()),
            clone_groups: vec![TracedCloneGroup {
                fingerprint: "dup:abc123".to_string(),
                token_count: 42,
                line_count: 3,
                instances: vec![
                    matched,
                    clone_instance("/repo/src/b.ts", 20, 22, "const x = 1;\nreturn x;"),
                ],
                suggestion: RefactoringSuggestion {
                    kind: RefactoringKind::ExtractFunction,
                    description: "Extract shared helper".to_string(),
                    estimated_savings: 3,
                },
                suggested_name: Some("buildValue".to_string()),
            }],
        };

        let rendered = plain(&build_clone_trace_human_lines(&trace, &root));

        assert!(rendered.contains("FOUND clone at src/a.ts:10-12"));
        assert!(rendered.contains("1 clone group(s) containing this location"));
        assert!(rendered.contains("Clone group 1  dup:abc123"));
        assert!(rendered.contains(">> src/a.ts:10-12"));
        assert!(rendered.contains("-> src/b.ts:20-22"));
        assert!(rendered.contains("Suggested refactor"));
        assert!(rendered.contains("Extract function, saves ~3 lines"));
        assert!(rendered.contains("Proposed name: buildValue"));
        assert!(rendered.contains("Code fragment:"));
        assert!(rendered.contains("10 const x = 1;"));
        assert!(rendered.contains("Docs:"));
    }

    fn clone_instance(
        file: &str,
        start_line: usize,
        end_line: usize,
        fragment: &str,
    ) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 10,
            fragment: fragment.to_string(),
        }
    }
}
