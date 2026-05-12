use std::fmt::Write;
use std::path::Path;

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{AnalysisResults, PrivateTypeLeak, UnusedExport, UnusedMember};

use super::grouping::ResultGroup;
use super::{normalize_uri, plural, relative_path};

/// Escape backticks in user-controlled strings to prevent breaking markdown code spans.
fn escape_backticks(s: &str) -> String {
    s.replace('`', "\\`")
}

pub(super) fn print_markdown(results: &AnalysisResults, root: &Path) {
    println!("{}", build_markdown(results, root));
}

/// Build markdown output for analysis results.
#[expect(
    clippy::too_many_lines,
    reason = "one section per issue type; splitting would fragment the output builder"
)]
pub fn build_markdown(results: &AnalysisResults, root: &Path) -> String {
    let rel = |p: &Path| {
        escape_backticks(&normalize_uri(
            &relative_path(p, root).display().to_string(),
        ))
    };

    let total = results.total_issues();
    let mut out = String::new();

    if total == 0 {
        out.push_str("## Fallow: no issues found\n");
        return out;
    }

    let _ = write!(out, "## Fallow: {total} issue{} found\n\n", plural(total));

    // ── Unused files ──
    markdown_section(&mut out, &results.unused_files, "Unused files", |file| {
        vec![format!("- `{}`", rel(&file.path))]
    });

    // ── Unused exports ──
    markdown_grouped_section(
        &mut out,
        &results.unused_exports,
        "Unused exports",
        root,
        |e| e.path.as_path(),
        format_export,
    );

    // ── Unused types ──
    markdown_grouped_section(
        &mut out,
        &results.unused_types,
        "Unused type exports",
        root,
        |e| e.path.as_path(),
        format_export,
    );

    markdown_grouped_section(
        &mut out,
        &results.private_type_leaks,
        "Private type leaks",
        root,
        |e| e.path.as_path(),
        format_private_type_leak,
    );

    // ── Unused dependencies ──
    markdown_section(
        &mut out,
        &results.unused_dependencies,
        "Unused dependencies",
        |dep| format_dependency(&dep.package_name, &dep.path, &dep.used_in_workspaces, root),
    );

    // ── Unused devDependencies ──
    markdown_section(
        &mut out,
        &results.unused_dev_dependencies,
        "Unused devDependencies",
        |dep| format_dependency(&dep.package_name, &dep.path, &dep.used_in_workspaces, root),
    );

    // ── Unused optionalDependencies ──
    markdown_section(
        &mut out,
        &results.unused_optional_dependencies,
        "Unused optionalDependencies",
        |dep| format_dependency(&dep.package_name, &dep.path, &dep.used_in_workspaces, root),
    );

    // ── Unused enum members ──
    markdown_grouped_section(
        &mut out,
        &results.unused_enum_members,
        "Unused enum members",
        root,
        |m| m.path.as_path(),
        format_member,
    );

    // ── Unused class members ──
    markdown_grouped_section(
        &mut out,
        &results.unused_class_members,
        "Unused class members",
        root,
        |m| m.path.as_path(),
        format_member,
    );

    // ── Unresolved imports ──
    markdown_grouped_section(
        &mut out,
        &results.unresolved_imports,
        "Unresolved imports",
        root,
        |i| i.path.as_path(),
        |i| format!(":{} `{}`", i.line, escape_backticks(&i.specifier)),
    );

    // ── Unlisted dependencies ──
    markdown_section(
        &mut out,
        &results.unlisted_dependencies,
        "Unlisted dependencies",
        |dep| vec![format!("- `{}`", escape_backticks(&dep.package_name))],
    );

    // ── Duplicate exports ──
    markdown_section(
        &mut out,
        &results.duplicate_exports,
        "Duplicate exports",
        |dup| {
            let locations: Vec<String> = dup
                .locations
                .iter()
                .map(|loc| format!("`{}`", rel(&loc.path)))
                .collect();
            vec![format!(
                "- `{}` in {}",
                escape_backticks(&dup.export_name),
                locations.join(", ")
            )]
        },
    );

    // ── Type-only dependencies ──
    markdown_section(
        &mut out,
        &results.type_only_dependencies,
        "Type-only dependencies (consider moving to devDependencies)",
        |dep| format_dependency(&dep.package_name, &dep.path, &[], root),
    );

    // ── Test-only dependencies ──
    markdown_section(
        &mut out,
        &results.test_only_dependencies,
        "Test-only production dependencies (consider moving to devDependencies)",
        |dep| format_dependency(&dep.package_name, &dep.path, &[], root),
    );

    // ── Circular dependencies ──
    markdown_section(
        &mut out,
        &results.circular_dependencies,
        "Circular dependencies",
        |cycle| {
            let chain: Vec<String> = cycle.files.iter().map(|p| rel(p)).collect();
            let mut display_chain = chain.clone();
            if let Some(first) = chain.first() {
                display_chain.push(first.clone());
            }
            let cross_pkg_tag = if cycle.is_cross_package {
                " *(cross-package)*"
            } else {
                ""
            };
            vec![format!(
                "- {}{}",
                display_chain
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(" \u{2192} "),
                cross_pkg_tag
            )]
        },
    );

    // ── Boundary violations ──
    markdown_section(
        &mut out,
        &results.boundary_violations,
        "Boundary violations",
        |v| {
            vec![format!(
                "- `{}`:{}  \u{2192} `{}` ({} \u{2192} {})",
                rel(&v.from_path),
                v.line,
                rel(&v.to_path),
                v.from_zone,
                v.to_zone,
            )]
        },
    );

    // ── Stale suppressions ──
    markdown_section(
        &mut out,
        &results.stale_suppressions,
        "Stale suppressions",
        |s| {
            vec![format!(
                "- `{}`:{} `{}` ({})",
                rel(&s.path),
                s.line,
                escape_backticks(&s.description()),
                escape_backticks(&s.explanation()),
            )]
        },
    );
    markdown_section(
        &mut out,
        &results.unused_catalog_entries,
        "Unused catalog entries",
        |entry| {
            let mut row = format!(
                "- `{}` (`{}`) `{}`:{}",
                escape_backticks(&entry.entry_name),
                escape_backticks(&entry.catalog_name),
                rel(&entry.path),
                entry.line,
            );
            if !entry.hardcoded_consumers.is_empty() {
                use std::fmt::Write as _;
                let consumers = entry
                    .hardcoded_consumers
                    .iter()
                    .map(|p| format!("`{}`", rel(p)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = write!(row, " (hardcoded in {consumers})");
            }
            vec![row]
        },
    );
    markdown_section(
        &mut out,
        &results.unresolved_catalog_references,
        "Unresolved catalog references",
        |finding| {
            let mut row = format!(
                "- `{}` (`{}`) `{}`:{}",
                escape_backticks(&finding.entry_name),
                escape_backticks(&finding.catalog_name),
                rel(&finding.path),
                finding.line,
            );
            if !finding.available_in_catalogs.is_empty() {
                use std::fmt::Write as _;
                let alts = finding
                    .available_in_catalogs
                    .iter()
                    .map(|c| format!("`{}`", escape_backticks(c)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = write!(row, " (available in: {alts})");
            }
            vec![row]
        },
    );
    markdown_section(
        &mut out,
        &results.unused_dependency_overrides,
        "Unused dependency overrides",
        |finding| {
            use std::fmt::Write as _;
            let mut row = format!(
                "- `{}` -> `{}` (`{}`) `{}`:{}",
                escape_backticks(&finding.raw_key),
                escape_backticks(&finding.version_range),
                finding.source.as_label(),
                rel(&finding.path),
                finding.line,
            );
            if let Some(hint) = &finding.hint {
                let _ = write!(row, " (hint: {})", escape_backticks(hint));
            }
            vec![row]
        },
    );
    markdown_section(
        &mut out,
        &results.misconfigured_dependency_overrides,
        "Misconfigured dependency overrides",
        |finding| {
            vec![format!(
                "- `{}` -> `{}` (`{}`) `{}`:{} ({})",
                escape_backticks(&finding.raw_key),
                escape_backticks(&finding.raw_value),
                finding.source.as_label(),
                rel(&finding.path),
                finding.line,
                finding.reason.describe(),
            )]
        },
    );

    out
}

/// Print grouped markdown output: each group gets an `## owner (N issues)` heading.
pub(super) fn print_grouped_markdown(groups: &[ResultGroup], root: &Path) {
    let total: usize = groups.iter().map(|g| g.results.total_issues()).sum();

    if total == 0 {
        println!("## Fallow: no issues found");
        return;
    }

    println!(
        "## Fallow: {total} issue{} found (grouped)\n",
        plural(total)
    );

    for group in groups {
        let count = group.results.total_issues();
        if count == 0 {
            continue;
        }
        println!(
            "## {} ({count} issue{})\n",
            escape_backticks(&group.key),
            plural(count)
        );
        // Section-mode: surface the section's default owners under the heading
        // so PR comment dashboards can see who approves without re-opening
        // CODEOWNERS. `owners` is `None` outside of `--group-by section` and
        // empty for the `(no section)` / `(unowned)` buckets.
        if let Some(ref owners) = group.owners
            && !owners.is_empty()
        {
            let joined = owners
                .iter()
                .map(|o| escape_backticks(o))
                .collect::<Vec<_>>()
                .join(" ");
            println!("Owners: {joined}\n");
        }
        // build_markdown already emits its own `## Fallow: N issues found` header;
        // we re-use the section-level rendering by extracting just the section body.
        let body = build_markdown(&group.results, root);
        // Skip the first `## Fallow: ...` line from build_markdown and print the rest.
        let sections = body
            .strip_prefix("## Fallow: no issues found\n")
            .or_else(|| body.find("\n\n").map(|pos| &body[pos + 2..]))
            .unwrap_or(&body);
        print!("{sections}");
    }
}

fn format_export(e: &UnusedExport) -> String {
    let re = if e.is_re_export { " (re-export)" } else { "" };
    format!(":{} `{}`{re}", e.line, escape_backticks(&e.export_name))
}

fn format_private_type_leak(e: &PrivateTypeLeak) -> String {
    format!(
        ":{} `{}` references private type `{}`",
        e.line,
        escape_backticks(&e.export_name),
        escape_backticks(&e.type_name)
    )
}

fn format_member(m: &UnusedMember) -> String {
    format!(
        ":{} `{}.{}`",
        m.line,
        escape_backticks(&m.parent_name),
        escape_backticks(&m.member_name)
    )
}

fn format_dependency(
    dep_name: &str,
    pkg_path: &Path,
    used_in_workspaces: &[std::path::PathBuf],
    root: &Path,
) -> Vec<String> {
    let name = escape_backticks(dep_name);
    let pkg_label = relative_path(pkg_path, root).display().to_string();
    let workspace_context = if used_in_workspaces.is_empty() {
        String::new()
    } else {
        let workspaces = used_in_workspaces
            .iter()
            .map(|path| escape_backticks(&relative_path(path, root).display().to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        format!("; imported in {workspaces}")
    };
    if pkg_label == "package.json" && workspace_context.is_empty() {
        vec![format!("- `{name}`")]
    } else {
        let label = if pkg_label == "package.json" {
            workspace_context.trim_start_matches("; ").to_string()
        } else {
            format!("{}{workspace_context}", escape_backticks(&pkg_label))
        };
        vec![format!("- `{name}` ({label})")]
    }
}

/// Emit a markdown section with a header and per-item lines. Skipped if empty.
fn markdown_section<T>(
    out: &mut String,
    items: &[T],
    title: &str,
    format_lines: impl Fn(&T) -> Vec<String>,
) {
    if items.is_empty() {
        return;
    }
    let _ = write!(out, "### {title} ({})\n\n", items.len());
    for item in items {
        for line in format_lines(item) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push('\n');
}

/// Emit a markdown section whose items are grouped by file path.
fn markdown_grouped_section<'a, T>(
    out: &mut String,
    items: &'a [T],
    title: &str,
    root: &Path,
    get_path: impl Fn(&'a T) -> &'a Path,
    format_detail: impl Fn(&T) -> String,
) {
    if items.is_empty() {
        return;
    }
    let _ = write!(out, "### {title} ({})\n\n", items.len());

    let mut indices: Vec<usize> = (0..items.len()).collect();
    indices.sort_by(|&a, &b| get_path(&items[a]).cmp(get_path(&items[b])));

    let rel = |p: &Path| normalize_uri(&relative_path(p, root).display().to_string());
    let mut last_file = String::new();
    for &i in &indices {
        let item = &items[i];
        let file_str = rel(get_path(item));
        if file_str != last_file {
            let _ = writeln!(out, "- `{file_str}`");
            last_file = file_str;
        }
        let _ = writeln!(out, "  - {}", format_detail(item));
    }
    out.push('\n');
}

// ── Duplication markdown output ──────────────────────────────────

pub(super) fn print_duplication_markdown(report: &DuplicationReport, root: &Path) {
    println!("{}", build_duplication_markdown(report, root));
}

/// Build markdown output for duplication results.
#[must_use]
pub fn build_duplication_markdown(report: &DuplicationReport, root: &Path) -> String {
    let rel = |p: &Path| normalize_uri(&relative_path(p, root).display().to_string());

    let mut out = String::new();

    if report.clone_groups.is_empty() {
        out.push_str("## Fallow: no code duplication found\n");
        return out;
    }

    let stats = &report.stats;
    let _ = write!(
        out,
        "## Fallow: {} clone group{} found ({:.1}% duplication)\n\n",
        stats.clone_groups,
        plural(stats.clone_groups),
        stats.duplication_percentage,
    );

    out.push_str("### Duplicates\n\n");
    for (i, group) in report.clone_groups.iter().enumerate() {
        let instance_count = group.instances.len();
        let _ = write!(
            out,
            "**Clone group {}** ({} lines, {instance_count} instance{})\n\n",
            i + 1,
            group.line_count,
            plural(instance_count)
        );
        for instance in &group.instances {
            let relative = rel(&instance.file);
            let _ = writeln!(
                out,
                "- `{relative}:{}-{}`",
                instance.start_line, instance.end_line
            );
        }
        out.push('\n');
    }

    // Clone families
    if !report.clone_families.is_empty() {
        out.push_str("### Clone Families\n\n");
        for (i, family) in report.clone_families.iter().enumerate() {
            let file_names: Vec<_> = family.files.iter().map(|f| rel(f)).collect();
            let _ = write!(
                out,
                "**Family {}** ({} group{}, {} lines across {})\n\n",
                i + 1,
                family.groups.len(),
                plural(family.groups.len()),
                family.total_duplicated_lines,
                file_names
                    .iter()
                    .map(|s| format!("`{s}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            for suggestion in &family.suggestions {
                let savings = if suggestion.estimated_savings > 0 {
                    format!(" (~{} lines saved)", suggestion.estimated_savings)
                } else {
                    String::new()
                };
                let _ = writeln!(out, "- {}{savings}", suggestion.description);
            }
            out.push('\n');
        }
    }

    // Summary line
    let _ = writeln!(
        out,
        "**Summary:** {} duplicated lines ({:.1}%) across {} file{}",
        stats.duplicated_lines,
        stats.duplication_percentage,
        stats.files_with_clones,
        plural(stats.files_with_clones),
    );

    out
}

// ── Health markdown output ──────────────────────────────────────────

pub(super) fn print_health_markdown(report: &crate::health_types::HealthReport, root: &Path) {
    println!("{}", build_health_markdown(report, root));
}

/// Build markdown output for health (complexity) results.
#[must_use]
pub fn build_health_markdown(report: &crate::health_types::HealthReport, root: &Path) -> String {
    let mut out = String::new();

    if let Some(ref hs) = report.health_score {
        let _ = writeln!(out, "## Health Score: {:.0} ({})\n", hs.score, hs.grade);
    }

    write_trend_section(&mut out, report);
    write_vital_signs_section(&mut out, report);

    if report.findings.is_empty()
        && report.file_scores.is_empty()
        && report.coverage_gaps.is_none()
        && report.hotspots.is_empty()
        && report.targets.is_empty()
        && report.runtime_coverage.is_none()
    {
        if report.vital_signs.is_none() {
            let _ = write!(
                out,
                "## Fallow: no functions exceed complexity thresholds\n\n\
                 **{}** functions analyzed (max cyclomatic: {}, max cognitive: {}, max CRAP: {:.1})\n",
                report.summary.functions_analyzed,
                report.summary.max_cyclomatic_threshold,
                report.summary.max_cognitive_threshold,
                report.summary.max_crap_threshold,
            );
        }
        return out;
    }

    write_findings_section(&mut out, report, root);
    write_runtime_coverage_section(&mut out, report, root);
    write_coverage_gaps_section(&mut out, report, root);
    write_file_scores_section(&mut out, report, root);
    write_hotspots_section(&mut out, report, root);
    write_targets_section(&mut out, report, root);
    write_metric_legend(&mut out, report);

    out
}

fn write_runtime_coverage_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref production) = report.runtime_coverage else {
        return;
    };
    // Prepend a blank line so the heading is not concatenated to the previous
    // section (GFM requires a blank line before headings to avoid the heading
    // being parsed as a paragraph continuation).
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "## Runtime Coverage\n\n- Verdict: {}\n- Functions tracked: {}\n- Hit: {}\n- Unhit: {}\n- Untracked: {}\n- Coverage: {:.1}%\n- Traces observed: {}\n- Period: {} day(s), {} deployment(s)\n",
        production.verdict,
        production.summary.functions_tracked,
        production.summary.functions_hit,
        production.summary.functions_unhit,
        production.summary.functions_untracked,
        production.summary.coverage_percent,
        production.summary.trace_count,
        production.summary.period_days,
        production.summary.deployments_seen,
    );
    if let Some(watermark) = production.watermark {
        let _ = writeln!(out, "- Watermark: {watermark}\n");
    }
    if let Some(ref quality) = production.summary.capture_quality
        && quality.lazy_parse_warning
    {
        let window = super::human::health::format_window(quality.window_seconds);
        let _ = writeln!(
            out,
            "- Capture quality: short window ({} from {} instance(s), {:.1}% of functions untracked); lazy-parsed scripts may not appear.\n",
            window, quality.instances_observed, quality.untracked_ratio_percent,
        );
    }
    let rel = |p: &Path| {
        escape_backticks(&normalize_uri(
            &relative_path(p, root).display().to_string(),
        ))
    };
    if !production.findings.is_empty() {
        out.push_str("| ID | Path | Function | Verdict | Invocations | Confidence |\n");
        out.push_str("|:---|:-----|:---------|:--------|------------:|:-----------|\n");
        for finding in &production.findings {
            let invocations = finding
                .invocations
                .map_or_else(|| "-".to_owned(), |hits| hits.to_string());
            let _ = writeln!(
                out,
                "| `{}` | `{}`:{} | `{}` | {} | {} | {} |",
                escape_backticks(&finding.id),
                rel(&finding.path),
                finding.line,
                escape_backticks(&finding.function),
                finding.verdict,
                invocations,
                finding.confidence,
            );
        }
        out.push('\n');
    }
    if !production.hot_paths.is_empty() {
        out.push_str("| ID | Hot path | Function | Invocations | Percentile |\n");
        out.push_str("|:---|:---------|:---------|------------:|-----------:|\n");
        for entry in &production.hot_paths {
            let _ = writeln!(
                out,
                "| `{}` | `{}`:{} | `{}` | {} | {} |",
                escape_backticks(&entry.id),
                rel(&entry.path),
                entry.line,
                escape_backticks(&entry.function),
                entry.invocations,
                entry.percentile,
            );
        }
        out.push('\n');
    }
}

/// Write the trend comparison table to the output.
fn write_trend_section(out: &mut String, report: &crate::health_types::HealthReport) {
    let Some(ref trend) = report.health_trend else {
        return;
    };
    let sha_str = trend
        .compared_to
        .git_sha
        .as_deref()
        .map_or(String::new(), |sha| format!(" ({sha})"));
    let _ = writeln!(
        out,
        "## Trend (vs {}{})\n",
        trend
            .compared_to
            .timestamp
            .get(..10)
            .unwrap_or(&trend.compared_to.timestamp),
        sha_str,
    );
    out.push_str("| Metric | Previous | Current | Delta | Direction |\n");
    out.push_str("|:-------|:---------|:--------|:------|:----------|\n");
    for m in &trend.metrics {
        let fmt_val = |v: f64| -> String {
            if m.unit == "%" {
                format!("{v:.1}%")
            } else if (v - v.round()).abs() < 0.05 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            }
        };
        let prev = fmt_val(m.previous);
        let cur = fmt_val(m.current);
        let delta = if m.unit == "%" {
            format!("{:+.1}%", m.delta)
        } else if (m.delta - m.delta.round()).abs() < 0.05 {
            format!("{:+.0}", m.delta)
        } else {
            format!("{:+.1}", m.delta)
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} {} |",
            m.label,
            prev,
            cur,
            delta,
            m.direction.arrow(),
            m.direction.label(),
        );
    }
    let md_sha = trend
        .compared_to
        .git_sha
        .as_deref()
        .map_or(String::new(), |sha| format!(" ({sha})"));
    let _ = writeln!(
        out,
        "\n*vs {}{} · {} {} available*\n",
        trend
            .compared_to
            .timestamp
            .get(..10)
            .unwrap_or(&trend.compared_to.timestamp),
        md_sha,
        trend.snapshots_loaded,
        if trend.snapshots_loaded == 1 {
            "snapshot"
        } else {
            "snapshots"
        },
    );
}

/// Write the vital signs summary table to the output.
fn write_vital_signs_section(out: &mut String, report: &crate::health_types::HealthReport) {
    let Some(ref vs) = report.vital_signs else {
        return;
    };
    out.push_str("## Vital Signs\n\n");
    out.push_str("| Metric | Value |\n");
    out.push_str("|:-------|------:|\n");
    if vs.total_loc > 0 {
        let _ = writeln!(out, "| Total LOC | {} |", vs.total_loc);
    }
    let _ = writeln!(out, "| Avg Cyclomatic | {:.1} |", vs.avg_cyclomatic);
    let _ = writeln!(out, "| P90 Cyclomatic | {} |", vs.p90_cyclomatic);
    if let Some(v) = vs.dead_file_pct {
        let _ = writeln!(out, "| Dead Files | {v:.1}% |");
    }
    if let Some(v) = vs.dead_export_pct {
        let _ = writeln!(out, "| Dead Exports | {v:.1}% |");
    }
    if let Some(v) = vs.maintainability_avg {
        let _ = writeln!(out, "| Maintainability (avg) | {v:.1} |");
    }
    if let Some(v) = vs.hotspot_count {
        let _ = writeln!(out, "| Hotspots | {v} |");
    }
    if let Some(v) = vs.circular_dep_count {
        let _ = writeln!(out, "| Circular Deps | {v} |");
    }
    if let Some(v) = vs.unused_dep_count {
        let _ = writeln!(out, "| Unused Deps | {v} |");
    }
    out.push('\n');
}

/// Write the complexity findings table to the output.
fn write_findings_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.findings.is_empty() {
        return;
    }

    let rel = |p: &Path| {
        escape_backticks(&normalize_uri(
            &relative_path(p, root).display().to_string(),
        ))
    };

    let count = report.summary.functions_above_threshold;
    let shown = report.findings.len();
    if shown < count {
        let _ = write!(
            out,
            "## Fallow: {count} high complexity function{} ({shown} shown)\n\n",
            plural(count),
        );
    } else {
        let _ = write!(
            out,
            "## Fallow: {count} high complexity function{}\n\n",
            plural(count),
        );
    }

    out.push_str("| File | Function | Severity | Cyclomatic | Cognitive | CRAP | Lines |\n");
    out.push_str("|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n");

    for finding in &report.findings {
        let file_str = rel(&finding.path);
        let cyc_marker = if finding.cyclomatic > report.summary.max_cyclomatic_threshold {
            " **!**"
        } else {
            ""
        };
        let cog_marker = if finding.cognitive > report.summary.max_cognitive_threshold {
            " **!**"
        } else {
            ""
        };
        let severity_label = match finding.severity {
            crate::health_types::FindingSeverity::Critical => "critical",
            crate::health_types::FindingSeverity::High => "high",
            crate::health_types::FindingSeverity::Moderate => "moderate",
        };
        let crap_cell = match finding.crap {
            Some(crap) => {
                let marker = if crap >= report.summary.max_crap_threshold {
                    " **!**"
                } else {
                    ""
                };
                format!("{crap:.1}{marker}")
            }
            None => "-".to_string(),
        };
        let _ = writeln!(
            out,
            "| `{file_str}:{line}` | `{name}` | {severity_label} | {cyc}{cyc_marker} | {cog}{cog_marker} | {crap_cell} | {lines} |",
            line = finding.line,
            name = escape_backticks(&finding.name),
            cyc = finding.cyclomatic,
            cog = finding.cognitive,
            lines = finding.line_count,
        );
    }

    let s = &report.summary;
    let _ = write!(
        out,
        "\n**{files}** files, **{funcs}** functions analyzed \
         (thresholds: cyclomatic > {cyc}, cognitive > {cog}, CRAP >= {crap:.1})\n",
        files = s.files_analyzed,
        funcs = s.functions_analyzed,
        cyc = s.max_cyclomatic_threshold,
        cog = s.max_cognitive_threshold,
        crap = s.max_crap_threshold,
    );
}

/// Write the file health scores table to the output.
fn write_file_scores_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.file_scores.is_empty() {
        return;
    }

    let rel = |p: &Path| {
        escape_backticks(&normalize_uri(
            &relative_path(p, root).display().to_string(),
        ))
    };

    out.push('\n');
    let _ = writeln!(
        out,
        "### File Health Scores ({} files)\n",
        report.file_scores.len(),
    );
    out.push_str("| File | Maintainability | Fan-in | Fan-out | Dead Code | Density | Risk |\n");
    out.push_str("|:-----|:---------------|:-------|:--------|:----------|:--------|:-----|\n");

    for score in &report.file_scores {
        let file_str = rel(&score.path);
        let _ = writeln!(
            out,
            "| `{file_str}` | {mi:.1} | {fi} | {fan_out} | {dead:.0}% | {density:.2} | {crap:.1} |",
            mi = score.maintainability_index,
            fi = score.fan_in,
            fan_out = score.fan_out,
            dead = score.dead_code_ratio * 100.0,
            density = score.complexity_density,
            crap = score.crap_max,
        );
    }

    if let Some(avg) = report.summary.average_maintainability {
        let _ = write!(out, "\n**Average maintainability index:** {avg:.1}/100\n");
    }
}

fn write_coverage_gaps_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref gaps) = report.coverage_gaps else {
        return;
    };

    out.push('\n');
    let _ = writeln!(out, "### Coverage Gaps\n");
    let _ = writeln!(
        out,
        "*{} untested files · {} untested exports · {:.1}% file coverage*\n",
        gaps.summary.untested_files, gaps.summary.untested_exports, gaps.summary.file_coverage_pct,
    );

    if gaps.files.is_empty() && gaps.exports.is_empty() {
        out.push_str("_No coverage gaps found in scope._\n");
        return;
    }

    if !gaps.files.is_empty() {
        out.push_str("#### Files\n");
        for item in &gaps.files {
            let file_str = escape_backticks(&normalize_uri(
                &relative_path(&item.path, root).display().to_string(),
            ));
            let _ = writeln!(
                out,
                "- `{file_str}` ({count} value export{})",
                if item.value_export_count == 1 {
                    ""
                } else {
                    "s"
                },
                count = item.value_export_count,
            );
        }
        out.push('\n');
    }

    if !gaps.exports.is_empty() {
        out.push_str("#### Exports\n");
        for item in &gaps.exports {
            let file_str = escape_backticks(&normalize_uri(
                &relative_path(&item.path, root).display().to_string(),
            ));
            let _ = writeln!(out, "- `{file_str}`:{} `{}`", item.line, item.export_name);
        }
    }
}

/// Write the hotspots table to the output.
/// Render the four ownership table cells (bus, top contributor, declared
/// owner, notes) for the markdown hotspots table. Cells fall back to an
/// en-dash (U+2013) when ownership data is missing for an entry.
fn ownership_md_cells(
    ownership: Option<&crate::health_types::OwnershipMetrics>,
) -> (String, String, String, String) {
    let Some(o) = ownership else {
        let dash = "\u{2013}".to_string();
        return (dash.clone(), dash.clone(), dash.clone(), dash);
    };
    let bus = o.bus_factor.to_string();
    let top = format!(
        "`{}` ({:.0}%)",
        o.top_contributor.identifier,
        o.top_contributor.share * 100.0,
    );
    let owner = o
        .declared_owner
        .as_deref()
        .map_or_else(|| "\u{2013}".to_string(), str::to_string);
    let mut notes: Vec<&str> = Vec::new();
    if o.unowned == Some(true) {
        notes.push("**unowned**");
    }
    if o.drift {
        notes.push("drift");
    }
    let notes_str = if notes.is_empty() {
        "\u{2013}".to_string()
    } else {
        notes.join(", ")
    };
    (bus, top, owner, notes_str)
}

fn write_hotspots_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.hotspots.is_empty() {
        return;
    }

    let rel = |p: &Path| {
        escape_backticks(&normalize_uri(
            &relative_path(p, root).display().to_string(),
        ))
    };

    out.push('\n');
    let header = report.hotspot_summary.as_ref().map_or_else(
        || format!("### Hotspots ({} files)\n", report.hotspots.len()),
        |summary| {
            format!(
                "### Hotspots ({} files, since {})\n",
                report.hotspots.len(),
                summary.since,
            )
        },
    );
    let _ = writeln!(out, "{header}");
    // Add ownership columns when at least one entry has ownership data.
    let any_ownership = report.hotspots.iter().any(|e| e.ownership.is_some());
    if any_ownership {
        out.push_str(
            "| File | Score | Commits | Churn | Density | Fan-in | Trend | Bus | Top | Owner | Notes |\n"
        );
        out.push_str(
            "|:-----|:------|:--------|:------|:--------|:-------|:------|:----|:----|:------|:------|\n"
        );
    } else {
        out.push_str("| File | Score | Commits | Churn | Density | Fan-in | Trend |\n");
        out.push_str("|:-----|:------|:--------|:------|:--------|:-------|:------|\n");
    }

    for entry in &report.hotspots {
        let file_str = rel(&entry.path);
        if any_ownership {
            let (bus, top, owner, notes) = ownership_md_cells(entry.ownership.as_ref());
            let _ = writeln!(
                out,
                "| `{file_str}` | {score:.1} | {commits} | {churn} | {density:.2} | {fi} | {trend} | {bus} | {top} | {owner} | {notes} |",
                score = entry.score,
                commits = entry.commits,
                churn = entry.lines_added + entry.lines_deleted,
                density = entry.complexity_density,
                fi = entry.fan_in,
                trend = entry.trend,
            );
        } else {
            let _ = writeln!(
                out,
                "| `{file_str}` | {score:.1} | {commits} | {churn} | {density:.2} | {fi} | {trend} |",
                score = entry.score,
                commits = entry.commits,
                churn = entry.lines_added + entry.lines_deleted,
                density = entry.complexity_density,
                fi = entry.fan_in,
                trend = entry.trend,
            );
        }
    }

    if let Some(ref summary) = report.hotspot_summary
        && summary.files_excluded > 0
    {
        let _ = write!(
            out,
            "\n*{} file{} excluded (< {} commits)*\n",
            summary.files_excluded,
            plural(summary.files_excluded),
            summary.min_commits,
        );
    }
}

/// Write the refactoring targets table to the output.
fn write_targets_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.targets.is_empty() {
        return;
    }
    let _ = write!(
        out,
        "\n### Refactoring Targets ({})\n\n",
        report.targets.len()
    );
    out.push_str("| Efficiency | Category | Effort / Confidence | File | Recommendation |\n");
    out.push_str("|:-----------|:---------|:--------------------|:-----|:---------------|\n");
    for target in &report.targets {
        let file_str = normalize_uri(&relative_path(&target.path, root).display().to_string());
        let category = target.category.label();
        let effort = target.effort.label();
        let confidence = target.confidence.label();
        let _ = writeln!(
            out,
            "| {:.1} | {category} | {effort} / {confidence} | `{file_str}` | {} |",
            target.efficiency, target.recommendation,
        );
    }
}

/// Write the metric legend collapsible section to the output.
fn write_metric_legend(out: &mut String, report: &crate::health_types::HealthReport) {
    let has_scores = !report.file_scores.is_empty();
    let has_coverage = report.coverage_gaps.is_some();
    let has_hotspots = !report.hotspots.is_empty();
    let has_targets = !report.targets.is_empty();
    if !has_scores && !has_coverage && !has_hotspots && !has_targets {
        return;
    }
    out.push_str("\n---\n\n<details><summary>Metric definitions</summary>\n\n");
    if has_scores {
        out.push_str("- **MI**: Maintainability Index (0\u{2013}100, higher is better)\n");
        out.push_str("- **Fan-in**: files that import this file (blast radius)\n");
        out.push_str("- **Fan-out**: files this file imports (coupling)\n");
        out.push_str("- **Dead Code**: % of value exports with zero references\n");
        out.push_str("- **Density**: cyclomatic complexity / lines of code\n");
    }
    if has_coverage {
        out.push_str(
            "- **File coverage**: runtime files also reachable from a discovered test root\n",
        );
        out.push_str("- **Untested export**: export with no reference chain from any test-reachable module\n");
    }
    if has_hotspots {
        out.push_str("- **Score**: churn \u{00d7} complexity (0\u{2013}100, higher = riskier)\n");
        out.push_str("- **Commits**: commits in the analysis window\n");
        out.push_str("- **Churn**: total lines added + deleted\n");
        out.push_str("- **Trend**: accelerating / stable / cooling\n");
    }
    if has_targets {
        out.push_str(
            "- **Efficiency**: priority / effort (higher = better quick-win value, default sort)\n",
        );
        out.push_str("- **Category**: recommendation type (churn+complexity, high impact, dead code, complexity, coupling, circular dep)\n");
        out.push_str("- **Effort**: estimated effort (low / medium / high) based on file size, function count, and fan-in\n");
        out.push_str("- **Confidence**: recommendation reliability (high = deterministic analysis, medium = heuristic, low = git-dependent)\n");
    }
    out.push_str(
        "\n[Full metric reference](https://docs.fallow.tools/explanations/metrics)\n\n</details>\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use fallow_core::duplicates::{
        CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
        RefactoringKind, RefactoringSuggestion,
    };
    use fallow_core::results::*;
    use std::path::PathBuf;

    #[test]
    fn markdown_empty_results_no_issues() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let md = build_markdown(&results, &root);
        assert_eq!(md, "## Fallow: no issues found\n");
    }

    #[test]
    fn markdown_contains_header_with_count() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let md = build_markdown(&results, &root);
        assert!(md.starts_with(&format!(
            "## Fallow: {} issues found\n",
            results.total_issues()
        )));
    }

    #[test]
    fn markdown_contains_all_sections() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let md = build_markdown(&results, &root);

        assert!(md.contains("### Unused files (1)"));
        assert!(md.contains("### Unused exports (1)"));
        assert!(md.contains("### Unused type exports (1)"));
        assert!(md.contains("### Unused dependencies (1)"));
        assert!(md.contains("### Unused devDependencies (1)"));
        assert!(md.contains("### Unused enum members (1)"));
        assert!(md.contains("### Unused class members (1)"));
        assert!(md.contains("### Unresolved imports (1)"));
        assert!(md.contains("### Unlisted dependencies (1)"));
        assert!(md.contains("### Duplicate exports (1)"));
        assert!(md.contains("### Type-only dependencies"));
        assert!(md.contains("### Test-only production dependencies"));
        assert!(md.contains("### Circular dependencies (1)"));
    }

    #[test]
    fn markdown_unused_file_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `src/dead.ts`"));
    }

    #[test]
    fn markdown_unused_export_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `src/utils.ts`"));
        assert!(md.contains(":10 `helperFn`"));
    }

    #[test]
    fn markdown_re_export_tagged() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExported".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("(re-export)"));
    }

    #[test]
    fn markdown_unused_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash`"));
    }

    #[test]
    fn markdown_circular_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
            length: 2,
            line: 3,
            col: 0,
            is_cross_package: false,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("`src/a.ts`"));
        assert!(md.contains("`src/b.ts`"));
        assert!(md.contains("\u{2192}"));
    }

    #[test]
    fn markdown_strips_root_prefix() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/deep/nested/file.ts"),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("`src/deep/nested/file.ts`"));
        assert!(!md.contains("/project/"));
    }

    #[test]
    fn markdown_single_issue_no_plural() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let md = build_markdown(&results, &root);
        assert!(md.starts_with("## Fallow: 1 issue found\n"));
    }

    #[test]
    fn markdown_type_only_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Type-only dependencies"));
        assert!(md.contains("- `zod`"));
    }

    #[test]
    fn markdown_escapes_backticks_in_export_names() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "foo`bar".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("foo\\`bar"));
        assert!(!md.contains("foo`bar`"));
    }

    #[test]
    fn markdown_escapes_backticks_in_package_names() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "pkg`name".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("pkg\\`name"));
    }

    // ── Duplication markdown ──

    #[test]
    fn duplication_markdown_empty() {
        let report = DuplicationReport::default();
        let root = PathBuf::from("/project");
        let md = build_duplication_markdown(&report, &root);
        assert_eq!(md, "## Fallow: no code duplication found\n");
    }

    #[test]
    fn duplication_markdown_contains_groups() {
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 1,
                        end_line: 10,
                        start_col: 0,
                        end_col: 0,
                        fragment: String::new(),
                    },
                    CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 5,
                        end_line: 14,
                        start_col: 0,
                        end_col: 0,
                        fragment: String::new(),
                    },
                ],
                token_count: 50,
                line_count: 10,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 10,
                files_with_clones: 2,
                total_lines: 500,
                duplicated_lines: 20,
                total_tokens: 2500,
                duplicated_tokens: 100,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 4.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let md = build_duplication_markdown(&report, &root);
        assert!(md.contains("**Clone group 1**"));
        assert!(md.contains("`src/a.ts:1-10`"));
        assert!(md.contains("`src/b.ts:5-14`"));
        assert!(md.contains("4.0% duplication"));
    }

    #[test]
    fn duplication_markdown_contains_families() {
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/a.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                }],
                token_count: 30,
                line_count: 5,
            }],
            clone_families: vec![CloneFamily {
                files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                groups: vec![],
                total_duplicated_lines: 20,
                total_duplicated_tokens: 100,
                suggestions: vec![RefactoringSuggestion {
                    kind: RefactoringKind::ExtractFunction,
                    description: "Extract shared utility function".to_string(),
                    estimated_savings: 15,
                }],
            }],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 2.0,
                ..Default::default()
            },
        };
        let md = build_duplication_markdown(&report, &root);
        assert!(md.contains("### Clone Families"));
        assert!(md.contains("**Family 1**"));
        assert!(md.contains("Extract shared utility function"));
        assert!(md.contains("~15 lines saved"));
    }

    // ── Health markdown ──

    #[test]
    fn health_markdown_empty_no_findings() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("no functions exceed complexity thresholds"));
        assert!(md.contains("**50** functions analyzed"));
    }

    #[test]
    fn health_markdown_table_format() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/utils.ts"),
                name: "parseExpression".to_string(),
                line: 42,
                col: 0,
                cyclomatic: 25,
                cognitive: 30,
                line_count: 80,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Fallow: 1 high complexity function\n"));
        assert!(md.contains("| File | Function |"));
        assert!(md.contains("`src/utils.ts:42`"));
        assert!(md.contains("`parseExpression`"));
        assert!(md.contains("25 **!**"));
        assert!(md.contains("30 **!**"));
        assert!(md.contains("| 80 |"));
        // CRAP column renders `-` when the finding didn't trigger on CRAP.
        assert!(md.contains("| - |"));
    }

    #[test]
    fn health_markdown_crap_column_shows_score_and_marker() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/risky.ts"),
                name: "branchy".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 67,
                cognitive: 10,
                line_count: 80,
                param_count: 1,
                exceeded: crate::health_types::ExceededThreshold::CyclomaticCrap,
                severity: crate::health_types::FindingSeverity::Critical,
                crap: Some(182.0),
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(
            md.contains("| CRAP |"),
            "markdown table should have CRAP column header: {md}"
        );
        assert!(
            md.contains("182.0 **!**"),
            "CRAP value should be rendered with a threshold marker: {md}"
        );
        assert!(
            md.contains("CRAP >="),
            "trailing summary line should reference the CRAP threshold: {md}"
        );
    }

    #[test]
    fn health_markdown_no_marker_when_below_threshold() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/utils.ts"),
                name: "helper".to_string(),
                line: 10,
                col: 0,
                cyclomatic: 15,
                cognitive: 20,
                line_count: 30,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Cognitive,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // Cyclomatic 15 is below threshold 20, no marker
        assert!(md.contains("| 15 |"));
        // Cognitive 20 exceeds threshold 15, has marker
        assert!(md.contains("20 **!**"));
    }

    #[test]
    fn health_markdown_with_targets() {
        use crate::health_types::*;

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            targets: vec![
                RefactoringTarget {
                    path: PathBuf::from("/project/src/complex.ts"),
                    priority: 82.5,
                    efficiency: 27.5,
                    recommendation: "Split high-impact file".into(),
                    category: RecommendationCategory::SplitHighImpact,
                    effort: crate::health_types::EffortEstimate::High,
                    confidence: crate::health_types::Confidence::Medium,
                    factors: vec![ContributingFactor {
                        metric: "fan_in",
                        value: 25.0,
                        threshold: 10.0,
                        detail: "25 files depend on this".into(),
                    }],
                    evidence: None,
                },
                RefactoringTarget {
                    path: PathBuf::from("/project/src/legacy.ts"),
                    priority: 45.0,
                    efficiency: 45.0,
                    recommendation: "Remove 5 unused exports".into(),
                    category: RecommendationCategory::RemoveDeadCode,
                    effort: crate::health_types::EffortEstimate::Low,
                    confidence: crate::health_types::Confidence::High,
                    factors: vec![],
                    evidence: None,
                },
            ],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);

        // Should have refactoring targets section
        assert!(
            md.contains("Refactoring Targets"),
            "should contain targets heading"
        );
        assert!(
            md.contains("src/complex.ts"),
            "should contain target file path"
        );
        assert!(md.contains("27.5"), "should contain efficiency score");
        assert!(
            md.contains("Split high-impact file"),
            "should contain recommendation"
        );
        assert!(md.contains("src/legacy.ts"), "should contain second target");
    }

    #[test]
    fn health_markdown_with_coverage_gaps() {
        use crate::health_types::*;

        let root = PathBuf::from("/project");
        let report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 2,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 1,
                },
                files: vec![UntestedFile {
                    path: root.join("src/app.ts"),
                    value_export_count: 2,
                }],
                exports: vec![UntestedExport {
                    path: root.join("src/app.ts"),
                    export_name: "loader".into(),
                    line: 12,
                    col: 4,
                }],
            }),
            ..Default::default()
        };

        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Coverage Gaps"));
        assert!(md.contains("*1 untested files"));
        assert!(md.contains("`src/app.ts` (2 value exports)"));
        assert!(md.contains("`src/app.ts`:12 `loader`"));
    }

    // ── Dependency in workspace package ──

    #[test]
    fn markdown_dep_in_workspace_shows_package_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/core/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        let md = build_markdown(&results, &root);
        // Non-root package.json should show the label
        assert!(md.contains("(packages/core/package.json)"));
    }

    #[test]
    fn markdown_dep_at_root_no_extra_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash`"));
        assert!(!md.contains("(package.json)"));
    }

    #[test]
    fn markdown_root_dep_with_cross_workspace_context_uses_context_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: vec![root.join("packages/consumer")],
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash-es` (imported in packages/consumer)"));
        assert!(!md.contains("(package.json; imported in packages/consumer)"));
    }

    // ── Multiple exports same file grouped ──

    #[test]
    fn markdown_exports_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "alpha".to_string(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "beta".to_string(),
            is_type_only: false,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: root.join("src/other.ts"),
            export_name: "gamma".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        let md = build_markdown(&results, &root);
        // File header should appear only once for utils.ts
        let utils_count = md.matches("- `src/utils.ts`").count();
        assert_eq!(utils_count, 1, "file header should appear once per file");
        // Both exports should be under it as sub-items
        assert!(md.contains(":5 `alpha`"));
        assert!(md.contains(":10 `beta`"));
    }

    // ── Multiple issues plural header ──

    #[test]
    fn markdown_multiple_issues_plural() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/a.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: root.join("src/b.ts"),
        });
        let md = build_markdown(&results, &root);
        assert!(md.starts_with("## Fallow: 2 issues found\n"));
    }

    // ── Duplication markdown with zero estimated savings ──

    #[test]
    fn duplication_markdown_zero_savings_no_suffix() {
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/a.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                }],
                token_count: 30,
                line_count: 5,
            }],
            clone_families: vec![CloneFamily {
                files: vec![root.join("src/a.ts")],
                groups: vec![],
                total_duplicated_lines: 5,
                total_duplicated_tokens: 30,
                suggestions: vec![RefactoringSuggestion {
                    kind: RefactoringKind::ExtractFunction,
                    description: "Extract function".to_string(),
                    estimated_savings: 0,
                }],
            }],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 1.0,
                ..Default::default()
            },
        };
        let md = build_duplication_markdown(&report, &root);
        assert!(md.contains("Extract function"));
        assert!(!md.contains("lines saved"));
    }

    // ── Health markdown vital signs ──

    #[test]
    fn health_markdown_vital_signs_table() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            vital_signs: Some(crate::health_types::VitalSigns {
                avg_cyclomatic: 3.5,
                p90_cyclomatic: 12,
                dead_file_pct: Some(5.0),
                dead_export_pct: Some(10.2),
                duplication_pct: None,
                maintainability_avg: Some(72.3),
                hotspot_count: Some(3),
                circular_dep_count: Some(1),
                unused_dep_count: Some(2),
                counts: None,
                unit_size_profile: None,
                unit_interfacing_profile: None,
                p95_fan_in: None,
                coupling_high_pct: None,
                total_loc: 15_200,
                ..Default::default()
            }),
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Vital Signs"));
        assert!(md.contains("| Metric | Value |"));
        assert!(md.contains("| Total LOC | 15200 |"));
        assert!(md.contains("| Avg Cyclomatic | 3.5 |"));
        assert!(md.contains("| P90 Cyclomatic | 12 |"));
        assert!(md.contains("| Dead Files | 5.0% |"));
        assert!(md.contains("| Dead Exports | 10.2% |"));
        assert!(md.contains("| Maintainability (avg) | 72.3 |"));
        assert!(md.contains("| Hotspots | 3 |"));
        assert!(md.contains("| Circular Deps | 1 |"));
        assert!(md.contains("| Unused Deps | 2 |"));
    }

    // ── Health markdown file scores ──

    #[test]
    fn health_markdown_file_scores_table() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/dummy.ts"),
                name: "fn".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                files_scored: Some(1),
                average_maintainability: Some(65.0),
                ..Default::default()
            },
            file_scores: vec![crate::health_types::FileHealthScore {
                path: root.join("src/utils.ts"),
                fan_in: 5,
                fan_out: 3,
                dead_code_ratio: 0.25,
                complexity_density: 0.8,
                maintainability_index: 72.5,
                total_cyclomatic: 40,
                total_cognitive: 30,
                function_count: 10,
                lines: 200,
                crap_max: 0.0,
                crap_above_threshold: 0,
            }],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### File Health Scores (1 files)"));
        assert!(md.contains("| File | Maintainability | Fan-in | Fan-out | Dead Code | Density |"));
        assert!(md.contains("| `src/utils.ts` | 72.5 | 5 | 3 | 25% | 0.80 |"));
        assert!(md.contains("**Average maintainability index:** 65.0/100"));
    }

    // ── Health markdown hotspots ──

    #[test]
    fn health_markdown_hotspots_table() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/dummy.ts"),
                name: "fn".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 50,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            hotspots: vec![crate::health_types::HotspotEntry {
                path: root.join("src/hot.ts"),
                score: 85.0,
                commits: 42,
                weighted_commits: 35.0,
                lines_added: 500,
                lines_deleted: 200,
                complexity_density: 1.2,
                fan_in: 10,
                trend: fallow_core::churn::ChurnTrend::Accelerating,
                ownership: None,
                is_test_path: false,
            }],
            hotspot_summary: Some(crate::health_types::HotspotSummary {
                since: "6 months".to_string(),
                min_commits: 3,
                files_analyzed: 50,
                files_excluded: 5,
                shallow_clone: false,
            }),
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Hotspots (1 files, since 6 months)"));
        assert!(md.contains("| `src/hot.ts` | 85.0 | 42 | 700 | 1.20 | 10 | accelerating |"));
        assert!(md.contains("*5 files excluded (< 3 commits)*"));
    }

    // ── Health markdown metric legend ──

    #[test]
    fn health_markdown_metric_legend_with_scores() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/x.ts"),
                name: "f".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 10,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 1,
                functions_above_threshold: 1,
                files_scored: Some(1),
                average_maintainability: Some(70.0),
                ..Default::default()
            },
            file_scores: vec![crate::health_types::FileHealthScore {
                path: root.join("src/x.ts"),
                fan_in: 1,
                fan_out: 1,
                dead_code_ratio: 0.0,
                complexity_density: 0.5,
                maintainability_index: 80.0,
                total_cyclomatic: 10,
                total_cognitive: 8,
                function_count: 2,
                lines: 50,
                crap_max: 0.0,
                crap_above_threshold: 0,
            }],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("<details><summary>Metric definitions</summary>"));
        assert!(md.contains("**MI**: Maintainability Index"));
        assert!(md.contains("**Fan-in**"));
        assert!(md.contains("Full metric reference"));
    }

    // ── Health markdown truncated findings ──

    #[test]
    fn health_markdown_truncated_findings_shown_count() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/x.ts"),
                name: "f".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 10,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                functions_above_threshold: 5, // 5 total but only 1 shown
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("5 high complexity functions (1 shown)"));
    }

    // ── escape_backticks ──

    #[test]
    fn escape_backticks_handles_multiple() {
        assert_eq!(escape_backticks("a`b`c"), "a\\`b\\`c");
    }

    #[test]
    fn escape_backticks_no_backticks_unchanged() {
        assert_eq!(escape_backticks("hello"), "hello");
    }

    // ── Unresolved import in markdown ──

    #[test]
    fn markdown_unresolved_import_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unresolved imports (1)"));
        assert!(md.contains("- `src/app.ts`"));
        assert!(md.contains(":3 `./missing`"));
    }

    // ── Markdown optional dep ──

    #[test]
    fn markdown_unused_optional_dep() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        });
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unused optionalDependencies (1)"));
        assert!(md.contains("- `fsevents`"));
    }

    // ── Health markdown no hotspot exclusion message when 0 excluded ──

    #[test]
    fn health_markdown_hotspots_no_excluded_message() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![crate::health_types::HealthFinding {
                path: root.join("src/x.ts"),
                name: "f".to_string(),
                line: 1,
                col: 0,
                cyclomatic: 25,
                cognitive: 20,
                line_count: 10,
                param_count: 0,
                exceeded: crate::health_types::ExceededThreshold::Both,
                severity: crate::health_types::FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            hotspots: vec![crate::health_types::HotspotEntry {
                path: root.join("src/hot.ts"),
                score: 50.0,
                commits: 10,
                weighted_commits: 8.0,
                lines_added: 100,
                lines_deleted: 50,
                complexity_density: 0.5,
                fan_in: 3,
                trend: fallow_core::churn::ChurnTrend::Stable,
                ownership: None,
                is_test_path: false,
            }],
            hotspot_summary: Some(crate::health_types::HotspotSummary {
                since: "6 months".to_string(),
                min_commits: 3,
                files_analyzed: 50,
                files_excluded: 0,
                shallow_clone: false,
            }),
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(!md.contains("files excluded"));
    }

    // ── Duplication markdown plural ──

    #[test]
    fn duplication_markdown_single_group_no_plural() {
        let root = PathBuf::from("/project");
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    file: root.join("src/a.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                }],
                token_count: 30,
                line_count: 5,
            }],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                clone_groups: 1,
                clone_instances: 1,
                duplication_percentage: 2.0,
                ..Default::default()
            },
        };
        let md = build_duplication_markdown(&report, &root);
        assert!(md.contains("1 clone group found"));
        assert!(!md.contains("1 clone groups found"));
    }
}
