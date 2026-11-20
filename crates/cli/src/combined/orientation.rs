use colored::Colorize;

use crate::check::CheckResult;
use crate::health::HealthResult;
use crate::report;

/// Print orientation header: vital signs summary + start-here nudge.
///
/// Renders a compact one-or-two-line block at the top of combined mode output
/// so users immediately see the project's vital signs and top refactoring target.
pub(super) fn print_orientation_header(
    health: &HealthResult,
    check: Option<&CheckResult>,
    root: &std::path::Path,
) {
    OrientationHeader {
        health,
        check,
        root,
    }
    .print();
}

struct OrientationHeader<'a> {
    health: &'a HealthResult,
    check: Option<&'a CheckResult>,
    root: &'a std::path::Path,
}

impl OrientationHeader<'_> {
    fn print(&self) {
        let rendered_score = self.print_score();
        self.print_vital_signs(rendered_score);
        self.print_scope();
        if let Some(result) = self.check {
            print_entry_point_summary(&result.results);
        }
        self.print_target_hint();
    }

    fn print_score(&self) -> bool {
        let mut score_lines: Vec<String> = Vec::new();
        report::render_health_score(&mut score_lines, &self.health.report);
        report::render_health_trend(&mut score_lines, &self.health.report);
        let rendered_score = !score_lines.is_empty();
        for line in &score_lines {
            eprintln!("{line}");
        }
        rendered_score
    }

    fn print_vital_signs(&self, rendered_score: bool) {
        let Some(ref vs) = self.health.report.vital_signs else {
            return;
        };
        if self.health.report.health_trend.is_some() {
            return;
        }

        let parts = Self::vital_sign_parts(vs);
        if !parts.is_empty() {
            if !rendered_score {
                eprintln!();
            }
            eprintln!(
                "{} {} {}",
                "\u{25a0}".dimmed(),
                "Metrics:".dimmed(),
                parts.join(" \u{00b7} ").dimmed()
            );
        }
    }

    fn vital_sign_parts(vs: &fallow_output::VitalSigns) -> Vec<String> {
        let mut parts = Vec::new();
        Self::push_dead_code_parts(vs, &mut parts);
        Self::push_maintainability_part(vs, &mut parts);
        Self::push_risk_parts(vs, &mut parts);
        parts
    }

    fn push_dead_code_parts(vs: &fallow_output::VitalSigns, parts: &mut Vec<String>) {
        if let Some(dfp) = vs.dead_file_pct {
            if let Some(ref c) = vs.counts {
                parts.push(format!(
                    "dead files {dfp:.1}% ({} of {})",
                    c.dead_files, c.total_files
                ));
            } else {
                parts.push(format!("dead files {dfp:.1}%"));
            }
        }
        if let Some(dep) = vs.dead_export_pct {
            if let Some(ref c) = vs.counts {
                parts.push(format!(
                    "dead exports {dep:.1}% ({} of {})",
                    c.dead_exports, c.total_exports
                ));
            } else {
                parts.push(format!("dead exports {dep:.1}%"));
            }
        }
    }

    fn push_maintainability_part(vs: &fallow_output::VitalSigns, parts: &mut Vec<String>) {
        if let Some(mi) = vs.maintainability_avg {
            let label = if mi >= 85.0 {
                "good"
            } else if mi >= 65.0 {
                "moderate"
            } else {
                "low"
            };
            parts.push(format!("MI {mi:.1} ({label})"));
        }
    }

    fn push_risk_parts(vs: &fallow_output::VitalSigns, parts: &mut Vec<String>) {
        if let Some(hc) = vs.hotspot_count
            && hc > 0
        {
            parts.push(format!(
                "{hc} churn hotspot{}",
                if hc == 1 { "" } else { "s" }
            ));
        }
        if let Some(cd) = vs.circular_dep_count
            && cd > 0
        {
            parts.push(format!(
                "{cd} circular {}",
                if cd == 1 {
                    "dependency"
                } else {
                    "dependencies"
                }
            ));
        }
    }

    fn print_scope(&self) {
        let files = self.health.report.summary.files_analyzed;
        if files == 0 {
            return;
        }

        let config = self.check.map_or(&self.health.config, |c| &c.config);
        let plugin_count = config.external_plugins.len();
        use std::fmt::Write as _;
        let mut scope = format!("  {files} files analyzed");
        if plugin_count > 0 {
            let names: Vec<&str> = config
                .external_plugins
                .iter()
                .take(5)
                .map(|p| p.name.as_str())
                .collect();
            let _ = write!(
                scope,
                ", {plugin_count} plugin{}",
                if plugin_count == 1 { "" } else { "s" }
            );
            let _ = write!(scope, " ({})", names.join(", "));
            if plugin_count > 5 {
                let _ = write!(scope, " +{}", plugin_count - 5);
            }
        }
        eprintln!("{}", scope.dimmed());
    }

    fn print_target_hint(&self) {
        if self.health.report.targets.is_empty() {
            return;
        }

        let target_count = self.health.report.targets.len();
        let total_issues = self.check.map_or(0, |c| c.results.total_issues());
        let start = if total_issues > SCOPE_HINT_ISSUE_THRESHOLD {
            None
        } else {
            self.health
                .report
                .targets
                .iter()
                .find(|t| !is_non_production_path(&t.path, self.root))
                .map(|top| {
                    (
                        report::format_display_path(&top.path, self.root),
                        top.category.label(),
                    )
                })
        };
        let line = target_hint_line(
            target_count,
            total_issues,
            start.as_ref().map(|(file, label)| (file.as_str(), *label)),
        );
        eprintln!("{}", line.dimmed());
    }
}

/// Above this issue count the target hint suggests a workspace scope instead
/// of one file to start with.
const SCOPE_HINT_ISSUE_THRESHOLD: usize = 500;

/// Text of the refactoring-target hint. `start` is the display path and the
/// category label of the first production target, when one exists.
fn target_hint_line(
    target_count: usize,
    total_issues: usize,
    start: Option<(&str, &str)>,
) -> String {
    let plural = if target_count == 1 { "" } else { "s" };
    if total_issues > SCOPE_HINT_ISSUE_THRESHOLD {
        return format!(
            "  {target_count} refactoring target{plural}: try `fallow dead-code --workspace <name>` to scope"
        );
    }
    match start {
        Some((file_name, label)) => {
            format!("  {target_count} refactoring target{plural}: start with {file_name} ({label})")
        }
        None => format!("  {target_count} refactoring target{plural}"),
    }
}

/// Warning that the combined report prints when no entry point was found.
const NO_ENTRY_POINTS_WARNING: &str = "  \u{26a0} No entry points detected. Exports may appear unused. Run: fallow list --entry-points";

/// Directory names of sample, benchmark and snapshot code. These paths are
/// not tests, but they are also no good start for a refactor.
const NON_PRODUCTION_DIR_NAMES: &[&str] = &[
    "examples",
    "example",
    "snapshots",
    "benchmark",
    "benchmarks",
    "bench",
    "playground",
    "playgrounds",
];

/// File-name markers of story and benchmark files.
const NON_PRODUCTION_FILE_MARKERS: &[&str] = &[".bench.", ".story.", ".stories."];

/// A file stem of at most this length, one lower-case letter followed by
/// digits (`a1`, `b12`), marks a generated or scratch file.
const MAX_SCRATCH_STEM_LEN: usize = 3;

/// Whether a path is test, sample, benchmark, story or scratch code that
/// should not be recommended as a refactoring starting point.
///
/// The path is classified relative to `root`, so a directory above the
/// project root never excludes a file.
pub(super) fn is_non_production_path(path: &std::path::Path, root: &std::path::Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if fallow_engine::test_paths::is_test_path(root, relative) {
        return true;
    }
    if relative.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        NON_PRODUCTION_DIR_NAMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&s))
    }) {
        return true;
    }
    let Some(name) = relative.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower_name = name.to_ascii_lowercase();
    if NON_PRODUCTION_FILE_MARKERS
        .iter()
        .any(|marker| lower_name.contains(marker))
    {
        return true;
    }
    let stem = relative.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.len() <= MAX_SCRATCH_STEM_LEN
        && stem.starts_with(|c: char| c.is_ascii_lowercase())
        && stem[1..].bytes().all(|b| b.is_ascii_digit())
}

/// Print entry-point detection summary to stderr.
///
/// Shows a dimmed informational line so users can verify that fallow found the
/// right entry points. When zero entry points are detected, emits a warning
/// with a remediation command.
pub fn print_entry_point_summary(results: &fallow_types::results::AnalysisResults) {
    let Some(ref summary) = results.entry_point_summary else {
        return;
    };
    if summary.total == 0 {
        eprintln!("{}", NO_ENTRY_POINTS_WARNING.yellow());
        return;
    }
    use std::fmt::Write as _;
    let mut line = format!(
        "  {} entry point{} detected",
        summary.total,
        if summary.total == 1 { "" } else { "s" }
    );
    if !summary.by_source.is_empty() {
        let parts: Vec<String> = summary
            .by_source
            .iter()
            .map(|(source, count)| format!("{count} {source}"))
            .collect();
        let _ = write!(line, " ({})", parts.join(", "));
    }
    eprintln!("{}", line.dimmed());
}

#[cfg(test)]
mod tests {
    use super::{NO_ENTRY_POINTS_WARNING, target_hint_line};

    #[test]
    fn orientation_messages_use_no_em_dash() {
        let messages = [
            target_hint_line(3, 900, None),
            target_hint_line(3, 10, Some(("src/big.ts", "high complexity"))),
            target_hint_line(1, 10, None),
            NO_ENTRY_POINTS_WARNING.to_string(),
        ];
        for message in &messages {
            assert!(
                !message.contains('\u{2014}'),
                "orientation message contains U+2014: {message}"
            );
        }
    }

    #[test]
    fn target_hint_line_names_scope_or_start_file() {
        assert_eq!(
            target_hint_line(3, 900, Some(("src/a.ts", "complexity"))),
            "  3 refactoring targets: try `fallow dead-code --workspace <name>` to scope"
        );
        assert_eq!(
            target_hint_line(1, 10, Some(("src/a.ts", "complexity"))),
            "  1 refactoring target: start with src/a.ts (complexity)"
        );
        assert_eq!(target_hint_line(2, 10, None), "  2 refactoring targets");
    }
}
