use std::path::Path;

use crate::report::human::colorize::Colorize;

use super::health::DOCS_HEALTH;
use super::{MAX_FLAT_ITEMS, format_path, push_docs_footer};
use crate::report::format_display_path;

pub(super) fn render_large_functions(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let listed = report.large_functions.len();
    let cut_by_top = report.large_functions_omitted.unwrap_or(0);
    let total = listed + cut_by_top;
    if total == 0 {
        return;
    }
    let shown = listed.min(MAX_FLAT_ITEMS);
    lines.push(format!(
        "{} {}",
        "\u{25cf}".red(),
        if shown < total {
            format!("Large functions ({shown} shown, {total} total)")
        } else {
            format!("Large functions ({total})")
        }
        .red()
        .bold()
    ));

    let mut last_file = String::new();
    for entry in report.large_functions.iter().take(MAX_FLAT_ITEMS) {
        let file_str = format_display_path(&entry.path, root);
        if file_str != last_file {
            lines.push(format!("  {}", format_path(&file_str)));
            last_file = file_str;
        }
        lines.push(format!(
            "    {} {}  {} lines",
            format!(":{}", entry.line).dimmed(),
            entry.name.bold(),
            format!("{:>3}", entry.line_count).red().bold(),
        ));
    }
    let unit_ceiling = report.summary.max_unit_size_threshold;
    let unit_word = if unit_ceiling == 1 { "line" } else { "lines" };
    push_docs_footer(
        lines,
        &format!("Functions exceeding {unit_ceiling} {unit_word} of code (very high risk)"),
        &format!("{DOCS_HEALTH}#unit-size"),
    );
    if cut_by_top > 0 {
        lines.push(format!("  {}", "run without --top to see all".dimmed()));
    } else if shown < total {
        lines.push(format!(
            "  {}",
            format!("use --top {total} to see all").dimmed()
        ));
    }
    lines.push(String::new());
}
