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
        if total_issues > 500 {
            eprintln!(
                "{}",
                format!(
                    "  {target_count} refactoring target{} \u{2014} try `fallow dead-code --workspace <name>` to scope",
                    if target_count == 1 { "" } else { "s" },
                )
                .dimmed()
            );
        } else if let Some(top) = self
            .health
            .report
            .targets
            .iter()
            .find(|t| !is_test_path(&t.path))
        {
            let file_name = report::format_display_path(&top.path, self.root);
            eprintln!(
                "{}",
                format!(
                    "  {target_count} refactoring target{} \u{2014} start with {file_name} ({})",
                    if target_count == 1 { "" } else { "s" },
                    top.category.label()
                )
                .dimmed()
            );
        } else {
            eprintln!(
                "{}",
                format!(
                    "  {target_count} refactoring target{}",
                    if target_count == 1 { "" } else { "s" },
                )
                .dimmed()
            );
        }
    }
}

/// Check if a path is a test, fixture, or generated file that shouldn't be
/// recommended as a refactoring starting point.
pub(super) fn is_test_path(path: &std::path::Path) -> bool {
    if path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            "test"
                | "tests"
                | "__tests__"
                | "__test__"
                | "spec"
                | "specs"
                | "__mocks__"
                | "__fixtures__"
                | "fixtures"
                | "examples"
                | "example"
                | "__snapshots__"
                | "snapshots"
                | "benchmark"
                | "benchmarks"
                | "bench"
                | "e2e"
                | "playground"
                | "playgrounds"
        )
    }) {
        return true;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.contains(".test.")
            || name.contains(".spec.")
            || name.contains(".fixture.")
            || name.contains(".e2e.")
            || name.contains(".bench.")
            || name.contains(".story.")
            || name.contains(".stories.")
        {
            return true;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.len() <= 3
            && stem.starts_with(|c: char| c.is_ascii_lowercase())
            && stem[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    false
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
        eprintln!(
            "{}",
            "  \u{26a0} No entry points detected \u{2014} exports may appear unused. Run: fallow list --entry-points"
                .yellow()
        );
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
