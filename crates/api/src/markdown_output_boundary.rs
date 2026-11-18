//! Markdown lines for boundary violations.

use super::*;

pub(super) fn format_markdown_boundary_violation(
    v: &fallow_types::output_dead_code::BoundaryViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let via = v
        .violation
        .via_path
        .as_ref()
        .map_or_else(String::new, |via| {
            format!(" via {}", markdown_code_span(&rel(via)))
        });
    let mut lines = vec![format!(
        "- {}:{}  \u{2192} {} ({} \u{2192} {}){}",
        markdown_code_span(&rel(&v.violation.from_path)),
        v.violation.line,
        markdown_code_span(&rel(&v.violation.to_path)),
        v.violation.from_zone,
        v.violation.to_zone,
        via,
    )];
    if let Some(message) = v
        .violation
        .internal_file_message(&|path| markdown_code_span(&rel(path)))
    {
        lines.push(format!("  - {message}"));
    }
    lines
}
