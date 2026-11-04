//! Human report sections for dependency override findings.

use super::*;

/// Render unused dependency overrides as a two-tier block: a headline row
/// shows `raw_key  source  path:line`, then an indented detail row shows the
/// forced version, target package, and optional CVE hint that the
/// conservative-static algorithm flags.
pub(super) struct UnusedDependencyOverridesInput<'a, 'b> {
    pub(super) lines: &'a mut Vec<String>,
    pub(super) findings: &'b [fallow_types::output_dead_code::UnusedDependencyOverrideFinding],
    pub(super) severity: fallow_config::Severity,
    pub(super) max_items: usize,
    pub(super) total_issues: usize,
    pub(super) root: &'b Path,
}

pub(super) fn push_unused_dependency_overrides_section(
    input: UnusedDependencyOverridesInput<'_, '_>,
) {
    let lines = input.lines;
    if input.findings.is_empty() {
        return;
    }
    let level = severity_to_level(input.severity);
    build_human_section_ex(
        HumanSectionInput {
            lines,
            items: input.findings,
            title: "Unused dependency overrides",
            level,
            max: input.max_items,
            total_issues: input.total_issues,
        },
        |finding| {
            let finding = &finding.entry;
            let path_display = input.root.join(&finding.path);
            let row = format!(
                "  {key}  {source}  {loc}",
                key = finding.raw_key.bold(),
                source = finding.source.as_label().dimmed(),
                loc = format!(
                    "{}:{}",
                    format_display_path(&path_display, input.root),
                    finding.line
                )
                .dimmed(),
            );
            let mut out = vec![row];
            let detail = format!(
                "forces {} to {}",
                finding.target_package, finding.version_range
            );
            out.push(format!("    {}", detail.dimmed()));
            if let Some(hint) = &finding.hint {
                out.push(format!("    {}", hint.as_str().dimmed()));
            }
            out
        },
    );
}

/// Render misconfigured dependency overrides as a two-tier block: a
/// headline row shows `raw_key  source  path:line`, then an indented detail
/// row shows the parsed reason. Package managers reject or ignore these shapes, so the
/// rule defaults to error.
pub(super) fn push_misconfigured_dependency_overrides_section(
    lines: &mut Vec<String>,
    findings: &[fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        HumanSectionInput {
            lines,
            items: findings,
            title: "Misconfigured dependency overrides",
            level,
            max: max_items,
            total_issues,
        },
        |finding| {
            let finding = &finding.entry;
            let path_display = root.join(&finding.path);
            let row = format!(
                "  {key}  {source}  {loc}",
                key = finding.raw_key.bold(),
                source = finding.source.as_label().dimmed(),
                loc = format!(
                    "{}:{}",
                    format_display_path(&path_display, root),
                    finding.line
                )
                .dimmed(),
            );
            vec![row, format!("    {}", finding.reason.describe().dimmed())]
        },
    );
}
