//! Human output for the shallow directories section of `fallow health`.

use colored::Colorize;

use super::health::DOCS_HEALTH;
use super::{plural, push_docs_footer};

/// Render the shallow directories, when the run computed the section and it
/// found one. The paths of the section are already relative to the root.
pub(super) fn render_shallow_directories(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
) {
    let Some(section) = report.shallow_directories.as_ref() else {
        return;
    };
    if section.directories.is_empty() {
        lines.push(format!(
            "{} {}",
            "\u{2713}".green(),
            format!(
                "No shallow directories (at least {} interface items)",
                section.min_interface
            )
            .dimmed()
        ));
        lines.push(String::new());
        return;
    }
    let count = section.total;
    lines.push(format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!(
            "Shallow directories ({count} director{})",
            if count == 1 { "y" } else { "ies" }
        )
        .yellow()
        .bold()
    ));
    lines.push(String::new());
    for dir in &section.directories {
        lines.push(format!(
            "  {}  {}",
            dir.path.display().to_string().replace('\\', "/").bold(),
            format!(
                "shows {} interface item{} from {} file{}, hides {}",
                dir.shown_count,
                plural(dir.shown_count as usize),
                dir.shown_files,
                plural(dir.shown_files as usize),
                dir.hidden_count,
            )
            .dimmed(),
        ));
        lines.push(format!("    {}", item_list(dir)));
    }
    if section.directories.len() < count {
        let hidden = count - section.directories.len();
        lines.push(format!(
            "  {}",
            format!(
                "{hidden} more director{} not shown",
                if hidden == 1 { "y" } else { "ies" }
            )
            .dimmed()
        ));
    }
    lines.push(String::new());
    push_docs_footer(
        lines,
        "Directories that show as many interface items as they hide",
        &format!("{DOCS_HEALTH}#shallow-directories"),
    );
    lines.push(String::new());
}

/// `name (N files), name (N files), +M more`.
fn item_list(dir: &fallow_output::ShallowDirectoryFinding) -> String {
    let mut parts: Vec<String> = dir
        .items
        .iter()
        .map(|item| {
            format!(
                "{} {}",
                item.name,
                format!(
                    "({} file{})",
                    item.outside_files,
                    plural(item.outside_files as usize)
                )
                .dimmed()
            )
        })
        .collect();
    if dir.more_items > 0 {
        parts.push(format!("+{} more", dir.more_items).dimmed().to_string());
    }
    parts.join(", ")
}
