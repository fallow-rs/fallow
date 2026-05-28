use std::path::Path;

use colored::Colorize;
use fallow_core::trace::{CloneTrace, DependencyTrace, ExportTrace, FileTrace};

use super::{plural, relative_path};

pub(in crate::report) fn print_export_trace_human(trace: &ExportTrace) {
    eprintln!();
    let status_icon = if trace.is_used {
        "USED".green().bold()
    } else {
        "UNUSED".red().bold()
    };
    eprintln!(
        "  {} {} in {}",
        status_icon,
        trace.export_name.bold(),
        trace.file.display().to_string().dimmed()
    );
    eprintln!();

    // File status
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
    eprintln!("  File: {reachable}{entry}");
    eprintln!("  Reason: {}", trace.reason);

    if !trace.direct_references.is_empty() {
        eprintln!();
        eprintln!("  {} direct reference(s):", trace.direct_references.len());
        for r in &trace.direct_references {
            eprintln!(
                "    {} {} ({})",
                "->".dimmed(),
                r.from_file.display(),
                r.kind.dimmed()
            );
        }
    }

    if !trace.re_export_chains.is_empty() {
        eprintln!();
        eprintln!("  Re-exported through:");
        for chain in &trace.re_export_chains {
            eprintln!(
                "    {} {} as '{}' ({} ref(s))",
                "->".dimmed(),
                chain.barrel_file.display(),
                chain.exported_as,
                chain.reference_count
            );
        }
    }
    eprintln!();
}

pub(in crate::report) fn print_file_trace_human(trace: &FileTrace) {
    eprintln!();
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
    eprintln!(
        "  {} {}{}",
        reachable,
        trace.file.display().to_string().bold(),
        entry
    );

    if !trace.exports.is_empty() {
        eprintln!();
        eprintln!("  Exports ({}):", trace.exports.len());
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
            eprintln!(
                "    {} {}{} [{}]",
                "export".dimmed(),
                export.name.bold(),
                type_tag,
                used_indicator
            );
            for r in &export.referenced_by {
                eprintln!(
                    "      {} {} ({})",
                    "->".dimmed(),
                    r.from_file.display(),
                    r.kind.dimmed()
                );
            }
        }
    }

    if !trace.imports_from.is_empty() {
        eprintln!();
        eprintln!("  Imports from ({}):", trace.imports_from.len());
        for path in &trace.imports_from {
            eprintln!("    {} {}", "<-".dimmed(), path.display());
        }
    }

    if !trace.imported_by.is_empty() {
        eprintln!();
        eprintln!("  Imported by ({}):", trace.imported_by.len());
        for path in &trace.imported_by {
            eprintln!("    {} {}", "->".dimmed(), path.display());
        }
    }

    if !trace.re_exports.is_empty() {
        eprintln!();
        eprintln!("  Re-exports ({}):", trace.re_exports.len());
        for re in &trace.re_exports {
            eprintln!(
                "    {} '{}' as '{}' from {}",
                "re-export".dimmed(),
                re.imported_name,
                re.exported_name,
                re.source_file.display()
            );
        }
    }
    eprintln!();
}

pub(in crate::report) fn print_dependency_trace_human(trace: &DependencyTrace) {
    eprintln!();
    let status = if trace.is_used {
        "USED".green().bold()
    } else {
        "UNUSED".red().bold()
    };
    eprintln!(
        "  {} {} ({} import(s))",
        status,
        trace.package_name.bold(),
        trace.import_count
    );

    if !trace.imported_by.is_empty() {
        eprintln!();
        eprintln!("  Imported by:");
        for path in &trace.imported_by {
            let is_type_only = trace.type_only_imported_by.contains(path);
            let tag = if is_type_only {
                " (type-only)".dimmed().to_string()
            } else {
                String::new()
            };
            eprintln!("    {} {}{}", "->".dimmed(), path.display(), tag);
        }
    }
    if trace.used_in_scripts {
        eprintln!();
        eprintln!(
            "  {}",
            "Referenced from package.json scripts or CI configs.".dimmed()
        );
    }
    eprintln!();
}

pub(in crate::report) fn print_clone_trace_human(trace: &CloneTrace, root: &Path) {
    eprintln!();
    if let Some(ref matched) = trace.matched_instance {
        let relative = relative_path(&matched.file, root);
        eprintln!(
            "  {} clone at {}:{}-{}",
            "FOUND".green().bold(),
            relative.display(),
            matched.start_line,
            matched.end_line,
        );
    }
    eprintln!(
        "  {} clone group(s) containing this location",
        trace.clone_groups.len()
    );
    for (i, group) in trace.clone_groups.iter().enumerate() {
        eprintln!();
        eprintln!(
            "  {}  {} ({} lines, {} tokens, {} instance{})",
            format!("Clone group {}", i + 1).bold(),
            group.fingerprint.dimmed(),
            group.line_count,
            group.token_count,
            group.instances.len(),
            plural(group.instances.len())
        );
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
            eprintln!(
                "    {} {}:{}-{}",
                marker,
                relative.display(),
                instance.start_line,
                instance.end_line
            );
        }
        eprintln!("    {}", "Suggested refactor".bold());
        eprintln!(
            "      Extract function, saves ~{} line{}",
            group.suggestion.estimated_savings,
            plural(group.suggestion.estimated_savings),
        );
        if let Some(ref name) = group.suggested_name {
            eprintln!(
                "      Proposed name: {}  {}",
                name,
                "(best-effort, verify before applying)".dimmed(),
            );
        }
    }
    if let Some(ref matched) = trace.matched_instance {
        eprintln!();
        eprintln!("  {}:", "Code fragment".dimmed());
        for (i, line) in matched.fragment.lines().enumerate() {
            eprintln!(
                "    {} {}",
                format!("{:>4}", matched.start_line + i).dimmed(),
                line
            );
        }
    }
    eprintln!();
    eprintln!(
        "  {} {}",
        "Docs:".dimmed(),
        super::dupes::DOCS_DUPLICATION.dimmed()
    );
    eprintln!();
}
