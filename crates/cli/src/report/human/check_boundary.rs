//! Human output section for boundary violations.

use super::*;

/// Build boundary violations section grouped by importing file.
pub(super) fn build_boundary_violations_section(
    lines: &mut Vec<String>,
    items: &[fallow_types::output_dead_code::BoundaryViolationFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Boundary violations";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for entry in &items[..shown] {
        let v = &entry.violation;
        let from = format_display_path(&v.from_path, root);
        let to = format_display_path(&v.to_path, root);
        let via = v.via_path.as_ref().map_or_else(String::new, |via| {
            format!(
                " {}",
                format!("via {}", format_display_path(via, root)).dimmed()
            )
        });
        lines.push(format!(
            "  {}:{} {} {} {} {}{}",
            from,
            v.line,
            "\u{2192}".dimmed(),
            to,
            format!("({}", v.from_zone).dimmed(),
            format!("\u{2192} {})", v.to_zone).dimmed(),
            via,
        ));
        if let Some(message) = v.internal_file_message(&|path| format_display_path(path, root)) {
            lines.push(format!("    {}", message.dimmed()));
        }
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}
