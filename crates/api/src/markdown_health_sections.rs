//! Markdown for the change coupling and shallow directories sections of
//! `fallow health`.

use std::fmt::Write;
use std::path::Path;

use fallow_output::{markdown_table_code_span, markdown_table_text, normalize_uri};

use super::{plural, relative_path};

/// Whether neither section has an entry to write.
pub(super) fn is_empty(report: &fallow_output::HealthReport) -> bool {
    report
        .change_coupling
        .as_ref()
        .is_none_or(|coupling| coupling.pairs.is_empty())
        && report.shallow_directories.is_none()
}

pub(super) fn write_change_coupling_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let Some(coupling) = report.change_coupling.as_ref() else {
        return;
    };
    if coupling.pairs.is_empty() {
        return;
    }
    let count = coupling.total_pairs;
    let _ = write!(
        out,
        "\n### Hidden Change Coupling ({count} pair{}, since {})\n\n",
        plural(count),
        coupling.since,
    );
    out.push_str("| File A | File B | Degree | Shared | Commits A | Commits B | Strong |\n");
    out.push_str("|:-------|:-------|:-------|:-------|:----------|:----------|:-------|\n");
    for pair in &coupling.pairs {
        let path_a = normalize_uri(&relative_path(&pair.path_a, root).display().to_string());
        let path_b = normalize_uri(&relative_path(&pair.path_b, root).display().to_string());
        let _ = writeln!(
            out,
            "| {} | {} | {:.0}% | {} | {} | {} | {} |",
            markdown_table_code_span(&path_a),
            markdown_table_code_span(&path_b),
            pair.degree * 100.0,
            pair.shared_revisions,
            pair.revisions_a,
            pair.revisions_b,
            if pair.strong { "**strong**" } else { "" },
        );
    }
    let hidden = count.saturating_sub(coupling.pairs.len());
    if hidden > 0 {
        let _ = writeln!(out, "\n*{hidden} more pair{} not shown*", plural(hidden));
    }
    out.push_str(
        "\n*Files that change together in the commit history with no import between them.*\n",
    );
}

/// The paths of the section are already relative to the root.
pub(super) fn write_shallow_directories_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
) {
    let Some(section) = report.shallow_directories.as_ref() else {
        return;
    };
    if section.directories.is_empty() {
        let _ = writeln!(
            out,
            "\n### Shallow Directories\n\nNo directory shows at least {} interface items and as many as it hides.",
            section.min_interface
        );
        return;
    }
    let count = section.total;
    let _ = write!(
        out,
        "\n### Shallow Directories ({count} director{})\n\n",
        if count == 1 { "y" } else { "ies" },
    );
    out.push_str("| Directory | Shown | Hidden | Files | Top items |\n");
    out.push_str("|:----------|:------|:-------|:------|:----------|\n");
    for dir in &section.directories {
        let path = normalize_uri(&dir.path.display().to_string());
        let mut items: Vec<String> = dir
            .items
            .iter()
            .map(|item| markdown_table_code_span(&item.name))
            .collect();
        if dir.more_items > 0 {
            items.push(markdown_table_text(&format!("+{} more", dir.more_items)));
        }
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            markdown_table_code_span(&path),
            dir.shown_count,
            dir.hidden_count,
            dir.shown_files,
            items.join(", "),
        );
    }
    let hidden = count.saturating_sub(section.directories.len());
    if hidden > 0 {
        let _ = writeln!(
            out,
            "\n*{hidden} more director{} not shown*",
            if hidden == 1 { "y" } else { "ies" }
        );
    }
    out.push_str("\n*Directories whose files outside use as many interface items as they hide.*\n");
}
