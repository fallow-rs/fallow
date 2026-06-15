use crate::report::sink::{out, outln};
use std::fmt::Write;
use std::path::Path;

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::{
    AnalysisResults, UnresolvedCatalogReferenceFinding, UnusedCatalogEntryFinding,
    UnusedClassMemberFinding, UnusedDependencyOverrideFinding, UnusedEnumMemberFinding,
    UnusedExport, UnusedExportFinding, UnusedMember, UnusedStoreMemberFinding, UnusedTypeFinding,
};

use super::grouping::ResultGroup;
use super::{normalize_uri, plural, relative_path};

/// Escape backticks in user-controlled strings to prevent breaking markdown code spans.
fn escape_backticks(s: &str) -> String {
    s.replace('`', "\\`")
}

pub(super) fn print_markdown(results: &AnalysisResults, root: &Path) {
    outln!("{}", build_markdown(results, root));
}

/// Build markdown output for analysis results.
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

    markdown_section(&mut out, &results.unused_files, "Unused files", |file| {
        vec![format!("- `{}`", rel(&file.file.path))]
    });

    markdown_grouped_section(
        &mut out,
        &results.unused_exports,
        "Unused exports",
        root,
        |e| e.export.path.as_path(),
        |e: &UnusedExportFinding| format_export(&e.export),
    );

    markdown_grouped_section(
        &mut out,
        &results.unused_types,
        "Unused type exports",
        root,
        |e| e.export.path.as_path(),
        |e: &UnusedTypeFinding| format_export(&e.export),
    );

    markdown_grouped_section(
        &mut out,
        &results.private_type_leaks,
        "Private type leaks",
        root,
        |e| e.leak.path.as_path(),
        format_private_type_leak,
    );

    push_markdown_dependency_sections(&mut out, results, root);
    push_markdown_member_sections(&mut out, results, root);

    markdown_grouped_section(
        &mut out,
        &results.unresolved_imports,
        "Unresolved imports",
        root,
        |i| i.import.path.as_path(),
        |i| {
            format!(
                ":{} `{}`",
                i.import.line,
                escape_backticks(&i.import.specifier)
            )
        },
    );

    markdown_section(
        &mut out,
        &results.unlisted_dependencies,
        "Unlisted dependencies",
        |dep| vec![format!("- `{}`", escape_backticks(&dep.dep.package_name))],
    );

    markdown_section(
        &mut out,
        &results.duplicate_exports,
        "Duplicate exports",
        |dup| {
            let locations: Vec<String> = dup
                .export
                .locations
                .iter()
                .map(|loc| format!("`{}`", rel(&loc.path)))
                .collect();
            vec![format!(
                "- `{}` in {}",
                escape_backticks(&dup.export.export_name),
                locations.join(", ")
            )]
        },
    );

    push_markdown_dependency_detail_sections(&mut out, results, root);
    push_markdown_graph_sections(&mut out, results, &rel);
    push_markdown_catalog_sections(&mut out, results, &rel);

    out
}

fn push_markdown_dependency_sections(out: &mut String, results: &AnalysisResults, root: &Path) {
    markdown_section(
        out,
        &results.unused_dependencies,
        "Unused dependencies",
        |dep| {
            format_dependency(
                &dep.dep.package_name,
                &dep.dep.path,
                &dep.dep.used_in_workspaces,
                root,
            )
        },
    );
    markdown_section(
        out,
        &results.unused_dev_dependencies,
        "Unused devDependencies",
        |dep| {
            format_dependency(
                &dep.dep.package_name,
                &dep.dep.path,
                &dep.dep.used_in_workspaces,
                root,
            )
        },
    );
    markdown_section(
        out,
        &results.unused_optional_dependencies,
        "Unused optionalDependencies",
        |dep| {
            format_dependency(
                &dep.dep.package_name,
                &dep.dep.path,
                &dep.dep.used_in_workspaces,
                root,
            )
        },
    );
}

fn push_markdown_member_sections(out: &mut String, results: &AnalysisResults, root: &Path) {
    markdown_grouped_section(
        out,
        &results.unused_enum_members,
        "Unused enum members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedEnumMemberFinding| format_member(&m.member),
    );
    markdown_grouped_section(
        out,
        &results.unused_class_members,
        "Unused class members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedClassMemberFinding| format_member(&m.member),
    );
    markdown_grouped_section(
        out,
        &results.unused_store_members,
        "Unused store members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedStoreMemberFinding| format_member(&m.member),
    );
}

fn push_markdown_dependency_detail_sections(
    out: &mut String,
    results: &AnalysisResults,
    root: &Path,
) {
    markdown_section(
        out,
        &results.type_only_dependencies,
        "Type-only dependencies (consider moving to devDependencies)",
        |dep| format_dependency(&dep.dep.package_name, &dep.dep.path, &[], root),
    );
    markdown_section(
        out,
        &results.test_only_dependencies,
        "Test-only production dependencies (consider moving to devDependencies)",
        |dep| format_dependency(&dep.dep.package_name, &dep.dep.path, &[], root),
    );
}

fn push_markdown_graph_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
    markdown_section(
        out,
        &results.circular_dependencies,
        "Circular dependencies",
        |cycle| format_markdown_circular_dependency(cycle, rel),
    );
    markdown_section(
        out,
        &results.re_export_cycles,
        "Re-export cycles",
        |cycle| format_markdown_re_export_cycle(cycle, rel),
    );
    markdown_section(
        out,
        &results.boundary_violations,
        "Boundary violations",
        |v| format_markdown_boundary_violation(v, rel),
    );
    markdown_section(
        out,
        &results.boundary_coverage_violations,
        "Boundary coverage",
        |v| format_markdown_boundary_coverage(v, rel),
    );
    markdown_section(
        out,
        &results.boundary_call_violations,
        "Boundary calls",
        |v| format_markdown_boundary_call(v, rel),
    );
    markdown_section(out, &results.policy_violations, "Policy violations", |v| {
        format_markdown_policy_violation(v, rel)
    });
    markdown_section(
        out,
        &results.invalid_client_exports,
        "Invalid client exports",
        |e| format_markdown_invalid_client_export(e, rel),
    );
    markdown_section(
        out,
        &results.mixed_client_server_barrels,
        "Mixed client/server barrels",
        |b| format_markdown_mixed_client_server_barrel(b, rel),
    );
    markdown_section(
        out,
        &results.misplaced_directives,
        "Misplaced directives",
        |d| format_markdown_misplaced_directive(d, rel),
    );
    markdown_section(out, &results.route_collisions, "Route collisions", |c| {
        format_markdown_route_collision(c, rel)
    });
    markdown_section(
        out,
        &results.dynamic_segment_name_conflicts,
        "Dynamic segment conflicts",
        |c| format_markdown_dynamic_segment_name_conflict(c, rel),
    );
    markdown_section(
        out,
        &results.unprovided_injects,
        "Unprovided injects",
        |i| format_markdown_unprovided_inject(i, rel),
    );
    markdown_section(
        out,
        &results.unrendered_components,
        "Unrendered components",
        |c| format_markdown_unrendered_component(c, rel),
    );
    markdown_section(
        out,
        &results.unused_component_props,
        "Unused component props",
        |p| format_markdown_unused_component_prop(p, rel),
    );
    markdown_section(
        out,
        &results.unused_component_emits,
        "Unused component emits",
        |e| format_markdown_unused_component_emit(e, rel),
    );
    markdown_section(
        out,
        &results.unused_server_actions,
        "Unused server actions",
        |a| format_markdown_unused_server_action(a, rel),
    );
    markdown_section(
        out,
        &results.unused_load_data_keys,
        "Unused load data keys",
        |k| format_markdown_unused_load_data_key(k, rel),
    );
    markdown_section(
        out,
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
}

fn format_markdown_circular_dependency(
    cycle: &fallow_core::results::CircularDependencyFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let chain: Vec<String> = cycle.cycle.files.iter().map(|p| rel(p)).collect();
    let mut display_chain = chain.clone();
    if let Some(first) = chain.first() {
        display_chain.push(first.clone());
    }
    let cross_pkg_tag = if cycle.cycle.is_cross_package {
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
}

fn format_markdown_re_export_cycle(
    cycle: &fallow_core::results::ReExportCycleFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let chain: Vec<String> = cycle.cycle.files.iter().map(|p| rel(p)).collect();
    let kind_tag = match cycle.cycle.kind {
        fallow_core::results::ReExportCycleKind::SelfLoop => " *(self-loop)*",
        fallow_core::results::ReExportCycleKind::MultiNode => "",
    };
    vec![format!(
        "- {}{}",
        chain
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(" <-> "),
        kind_tag
    )]
}

fn format_markdown_boundary_violation(
    v: &fallow_core::results::BoundaryViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{}  \u{2192} `{}` ({} \u{2192} {})",
        rel(&v.violation.from_path),
        v.violation.line,
        rel(&v.violation.to_path),
        v.violation.from_zone,
        v.violation.to_zone,
    )]
}

fn format_markdown_boundary_coverage(
    v: &fallow_core::results::BoundaryCoverageViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} no matching boundary zone",
        rel(&v.violation.path),
        v.violation.line,
    )]
}

fn format_markdown_boundary_call(
    v: &fallow_core::results::BoundaryCallViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` forbidden in zone `{}` (pattern `{}`)",
        rel(&v.violation.path),
        v.violation.line,
        v.violation.callee,
        v.violation.zone,
        v.violation.pattern,
    )]
}

fn format_markdown_policy_violation(
    v: &fallow_core::results::PolicyViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` banned by `{}/{}`{}",
        rel(&v.violation.path),
        v.violation.line,
        v.violation.matched,
        v.violation.pack,
        v.violation.rule_id,
        v.violation
            .message
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default(),
    )]
}

fn format_markdown_invalid_client_export(
    e: &fallow_core::results::InvalidClientExportFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` (from `\"{}\"`)",
        rel(&e.export.path),
        e.export.line,
        e.export.export_name,
        e.export.directive,
    )]
}

fn format_markdown_mixed_client_server_barrel(
    b: &fallow_core::results::MixedClientServerBarrelFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} re-exports client `{}` and server-only `{}`",
        rel(&b.barrel.path),
        b.barrel.line,
        b.barrel.client_origin,
        b.barrel.server_origin,
    )]
}

fn format_markdown_misplaced_directive(
    d: &fallow_core::results::MisplacedDirectiveFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `\"{}\"` is not in the leading position and is ignored",
        rel(&d.directive_site.path),
        d.directive_site.line,
        d.directive_site.directive,
    )]
}

fn format_markdown_unprovided_inject(
    i: &fallow_core::results::UnprovidedInjectFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` has no matching provide(`{}`) in this project; at runtime it returns undefined",
        rel(&i.inject.path),
        i.inject.line,
        escape_backticks(&i.inject.key_name),
        escape_backticks(&i.inject.key_name),
    )]
}

fn format_markdown_unrendered_component(
    c: &fallow_core::results::UnrenderedComponentFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is reachable but rendered nowhere in this project (render it somewhere or remove it)",
        rel(&c.component.path),
        c.component.line,
        escape_backticks(&c.component.component_name),
    )]
}

fn format_markdown_unused_component_prop(
    p: &fallow_core::results::UnusedComponentPropFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is declared but referenced nowhere in this component (remove it or use it)",
        rel(&p.prop.path),
        p.prop.line,
        escape_backticks(&p.prop.prop_name),
    )]
}

fn format_markdown_unused_component_emit(
    e: &fallow_core::results::UnusedComponentEmitFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is declared but emitted nowhere in this component (remove it or emit it)",
        rel(&e.emit.path),
        e.emit.line,
        escape_backticks(&e.emit.emit_name),
    )]
}

fn format_markdown_unused_server_action(
    a: &fallow_core::results::UnusedServerActionFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is exported from a \"use server\" file but no code in this project references it",
        rel(&a.action.path),
        a.action.line,
        escape_backticks(&a.action.action_name),
    )]
}

fn format_markdown_unused_load_data_key(
    k: &fallow_core::results::UnusedLoadDataKeyFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is returned from load() but no consumer reads it",
        rel(&k.key.path),
        k.key.line,
        escape_backticks(&k.key.key_name),
    )]
}

fn format_markdown_route_collision(
    c: &fallow_core::results::RouteCollisionFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}` resolves to `{}` (shared with {} other route file(s))",
        rel(&c.collision.path),
        c.collision.url,
        c.collision.conflicting_paths.len(),
    )]
}

fn format_markdown_dynamic_segment_name_conflict(
    c: &fallow_core::results::DynamicSegmentNameConflictFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}` crashes at runtime: different slug names ({}) at the same dynamic path `{}`; \
         `next build` passes but the route fails on its first request (rename to one consistent slug)",
        rel(&c.conflict.path),
        c.conflict.conflicting_segments.join(" vs "),
        c.conflict.position,
    )]
}

fn push_markdown_catalog_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
    markdown_section(
        out,
        &results.unused_catalog_entries,
        "Unused catalog entries",
        |entry| format_unused_catalog_entry(entry, rel),
    );
    markdown_section(
        out,
        &results.empty_catalog_groups,
        "Empty catalog groups",
        |group| {
            vec![format!(
                "- `{}` `{}`:{}",
                escape_backticks(&group.group.catalog_name),
                rel(&group.group.path),
                group.group.line,
            )]
        },
    );
    markdown_section(
        out,
        &results.unresolved_catalog_references,
        "Unresolved catalog references",
        |finding| format_unresolved_catalog_reference(finding, rel),
    );
    markdown_section(
        out,
        &results.unused_dependency_overrides,
        "Unused dependency overrides",
        |finding| format_unused_dependency_override(finding, rel),
    );
    markdown_section(
        out,
        &results.misconfigured_dependency_overrides,
        "Misconfigured dependency overrides",
        |finding| {
            vec![format!(
                "- `{}` -> `{}` (`{}`) `{}`:{} ({})",
                escape_backticks(&finding.entry.raw_key),
                escape_backticks(&finding.entry.raw_value),
                finding.entry.source.as_label(),
                rel(&finding.entry.path),
                finding.entry.line,
                finding.entry.reason.describe(),
            )]
        },
    );
}

fn format_unused_catalog_entry(
    entry: &UnusedCatalogEntryFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let mut row = format!(
        "- `{}` (`{}`) `{}`:{}",
        escape_backticks(&entry.entry.entry_name),
        escape_backticks(&entry.entry.catalog_name),
        rel(&entry.entry.path),
        entry.entry.line,
    );
    if !entry.entry.hardcoded_consumers.is_empty() {
        let consumers = entry
            .entry
            .hardcoded_consumers
            .iter()
            .map(|p| format!("`{}`", rel(p)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(row, " (hardcoded in {consumers})");
    }
    vec![row]
}

fn format_unresolved_catalog_reference(
    finding: &UnresolvedCatalogReferenceFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let mut row = format!(
        "- `{}` (`{}`) `{}`:{}",
        escape_backticks(&finding.reference.entry_name),
        escape_backticks(&finding.reference.catalog_name),
        rel(&finding.reference.path),
        finding.reference.line,
    );
    if !finding.reference.available_in_catalogs.is_empty() {
        let alts = finding
            .reference
            .available_in_catalogs
            .iter()
            .map(|c| format!("`{}`", escape_backticks(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(row, " (available in: {alts})");
    }
    vec![row]
}

fn format_unused_dependency_override(
    finding: &UnusedDependencyOverrideFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let mut row = format!(
        "- `{}` -> `{}` (`{}`) `{}`:{}",
        escape_backticks(&finding.entry.raw_key),
        escape_backticks(&finding.entry.version_range),
        finding.entry.source.as_label(),
        rel(&finding.entry.path),
        finding.entry.line,
    );
    if let Some(hint) = &finding.entry.hint {
        let _ = write!(row, " (hint: {})", escape_backticks(hint));
    }
    vec![row]
}

/// Print grouped markdown output: each group gets an `## owner (N issues)` heading.
pub(super) fn print_grouped_markdown(groups: &[ResultGroup], root: &Path) {
    let total: usize = groups.iter().map(|g| g.results.total_issues()).sum();

    if total == 0 {
        outln!("## Fallow: no issues found");
        return;
    }

    outln!(
        "## Fallow: {total} issue{} found (grouped)\n",
        plural(total)
    );

    for group in groups {
        let count = group.results.total_issues();
        if count == 0 {
            continue;
        }
        outln!(
            "## {} ({count} issue{})\n",
            escape_backticks(&group.key),
            plural(count)
        );
        if let Some(ref owners) = group.owners
            && !owners.is_empty()
        {
            let joined = owners
                .iter()
                .map(|o| escape_backticks(o))
                .collect::<Vec<_>>()
                .join(" ");
            outln!("Owners: {joined}\n");
        }
        let body = build_markdown(&group.results, root);
        let sections = body
            .strip_prefix("## Fallow: no issues found\n")
            .or_else(|| body.find("\n\n").map(|pos| &body[pos + 2..]))
            .unwrap_or(&body);
        out!("{sections}");
    }
}

fn format_export(e: &UnusedExport) -> String {
    let re = if e.is_re_export { " (re-export)" } else { "" };
    format!(":{} `{}`{re}", e.line, escape_backticks(&e.export_name))
}

fn format_private_type_leak(
    entry: &fallow_types::output_dead_code::PrivateTypeLeakFinding,
) -> String {
    let e = &entry.leak;
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

pub(super) fn print_duplication_markdown(report: &DuplicationReport, root: &Path) {
    outln!("{}", build_duplication_markdown(report, root));
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

pub(super) fn print_health_markdown(report: &crate::health_types::HealthReport, root: &Path) {
    outln!("{}", build_health_markdown(report, root));
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
        && report.coverage_intelligence.is_none()
        && report.threshold_overrides.is_empty()
        && report.css_analytics.is_none()
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
    write_threshold_overrides_section(&mut out, report, root);
    write_runtime_coverage_section(&mut out, report, root);
    write_coverage_intelligence_section(&mut out, report, root);
    write_coverage_gaps_section(&mut out, report, root);
    write_file_scores_section(&mut out, report, root);
    write_hotspots_section(&mut out, report, root);
    write_targets_section(&mut out, report, root);
    write_css_analytics_section(&mut out, report);
    write_metric_legend(&mut out, report);

    out
}

/// Render the opt-in `## CSS Health` markdown section (present only with
/// `--css`): a summary of structural metrics, value sprawl, and candidate counts
/// plus a bounded list of the most actionable located candidates.
fn write_css_analytics_section(out: &mut String, report: &crate::health_types::HealthReport) {
    let Some(ref css) = report.css_analytics else {
        return;
    };
    let s = &css.summary;
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("## CSS Health\n\n");
    let important_pct = if s.total_declarations > 0 {
        f64::from(s.important_declarations) / f64::from(s.total_declarations) * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "- Stylesheets: {} | Rules: {} | !important: {important_pct:.1}% | Empty rules: {} | Max nesting: {}",
        s.files_analyzed, s.total_rules, s.empty_rules, s.max_nesting_depth,
    );
    let _ = writeln!(
        out,
        "- Value sprawl: {} colors | {} font sizes | {} z-index | {} shadows | {} radii | {} line-heights",
        s.unique_colors,
        s.unique_font_sizes,
        s.unique_z_indexes,
        s.unique_box_shadows,
        s.unique_border_radii,
        s.unique_line_heights,
    );
    let _ = writeln!(
        out,
        "- Candidates: {} unreferenced + {} undefined @keyframes | {} duplicate blocks | {} scoped-unused classes | {} Tailwind arbitrary values | {} unused @property | {} unused @layer",
        s.keyframes_unreferenced,
        s.keyframes_undefined,
        s.duplicate_declaration_blocks,
        s.scoped_unused_classes,
        s.tailwind_arbitrary_values,
        s.unused_property_registrations,
        s.unused_layers,
    );
    if !css.undefined_keyframes.is_empty() {
        let named: Vec<String> = css
            .undefined_keyframes
            .iter()
            .take(5)
            .map(|kf| format!("`{}` ({})", kf.name, kf.path))
            .collect();
        let _ = writeln!(
            out,
            "- Undefined @keyframes (candidates; likely typo or CSS-in-JS): {}",
            named.join(", "),
        );
    }
    if !css.tailwind_arbitrary_values.is_empty() {
        let named: Vec<String> = css
            .tailwind_arbitrary_values
            .iter()
            .take(5)
            .map(|a| format!("`{}` ({}x)", a.value, a.count))
            .collect();
        let _ = writeln!(out, "- Top Tailwind arbitrary values: {}", named.join(", "));
    }
    out.push('\n');
}

fn write_coverage_intelligence_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref intelligence) = report.coverage_intelligence else {
        return;
    };
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "## Coverage Intelligence\n\n- Verdict: {}\n- Findings: {}\n- Ambiguous matches skipped: {}\n",
        intelligence.verdict,
        intelligence.summary.findings,
        intelligence.summary.skipped_ambiguous_matches,
    );
    if intelligence.findings.is_empty() {
        if intelligence.summary.skipped_ambiguous_matches > 0 {
            let match_phrase = if intelligence.summary.skipped_ambiguous_matches == 1 {
                "evidence match was"
            } else {
                "evidence matches were"
            };
            let _ = writeln!(
                out,
                "No actionable findings were emitted because {} ambiguous {match_phrase} skipped.\n",
                intelligence.summary.skipped_ambiguous_matches,
            );
        }
        return;
    }
    out.push_str("| ID | Path | Identity | Verdict | Recommendation | Confidence | Signals |\n");
    out.push_str("|:---|:-----|:---------|:--------|:---------------|:-----------|:--------|\n");
    for finding in &intelligence.findings {
        let path = escape_backticks(&normalize_uri(
            &relative_path(&finding.path, root).display().to_string(),
        ));
        let identity = finding
            .identity
            .as_deref()
            .map_or_else(|| "-".to_owned(), escape_backticks);
        let signals = finding
            .signals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "| `{}` | `{}`:{} | `{}` | {} | {} | {} | {} |",
            escape_backticks(&finding.id),
            path,
            finding.line,
            identity,
            finding.verdict,
            finding.recommendation,
            finding.confidence,
            signals,
        );
    }
    out.push('\n');
}

fn write_runtime_coverage_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let Some(ref production) = report.runtime_coverage else {
        return;
    };
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
        let label = report.hotspot_summary.as_ref().map_or_else(
            || "Hotspots".to_string(),
            |summary| format!("Hotspots (since {})", summary.since),
        );
        let _ = writeln!(out, "| {label} | {v} |");
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
        let thresholds = finding.effective_thresholds.unwrap_or(
            crate::health_types::HealthEffectiveThresholds {
                max_cyclomatic: report.summary.max_cyclomatic_threshold,
                max_cognitive: report.summary.max_cognitive_threshold,
                max_crap: report.summary.max_crap_threshold,
            },
        );
        let cyc_marker = if finding.cyclomatic > thresholds.max_cyclomatic {
            " **!**"
        } else {
            ""
        };
        let cog_marker = if finding.cognitive > thresholds.max_cognitive {
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
                let marker = if crap >= thresholds.max_crap {
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

fn write_threshold_overrides_section(
    out: &mut String,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    if report.threshold_overrides.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("## Health Threshold Overrides\n\n");
    out.push_str("| Override | Status | Target | Metrics |\n");
    out.push_str("|---------:|:-------|:-------|:--------|\n");
    for entry in &report.threshold_overrides {
        let status = match entry.status {
            crate::health_types::ThresholdOverrideStatus::Active => "active",
            crate::health_types::ThresholdOverrideStatus::Stale => "stale",
            crate::health_types::ThresholdOverrideStatus::NoMatch => "no_match",
        };
        let target = entry.path.as_ref().map_or_else(
            || "<no matching file or function>".to_string(),
            |path| {
                let display = escape_backticks(&normalize_uri(
                    &relative_path(path, root).display().to_string(),
                ));
                entry.function.as_ref().map_or_else(
                    || display.clone(),
                    |name| format!("{display}:{}", escape_backticks(name)),
                )
            },
        );
        let metrics = entry.metrics.map_or_else(
            || "-".to_string(),
            |metrics| {
                let crap = metrics
                    .crap
                    .map_or(String::new(), |value| format!(", CRAP {value:.1}"));
                format!(
                    "cyclomatic {}, cognitive {}{}",
                    metrics.cyclomatic, metrics.cognitive, crap
                )
            },
        );
        let _ = writeln!(
            out,
            "| {} | {} | `{}` | {} |",
            entry.override_index, status, target, metrics
        );
    }
    out.push('\n');
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
                &relative_path(&item.file.path, root).display().to_string(),
            ));
            let _ = writeln!(
                out,
                "- `{file_str}` ({count} value export{})",
                if item.file.value_export_count == 1 {
                    ""
                } else {
                    "s"
                },
                count = item.file.value_export_count,
            );
        }
        out.push('\n');
    }

    if !gaps.exports.is_empty() {
        out.push_str("#### Exports\n");
        for item in &gaps.exports {
            let file_str = escape_backticks(&normalize_uri(
                &relative_path(&item.export.path, root).display().to_string(),
            ));
            let _ = writeln!(
                out,
                "- `{file_str}`:{} `{}`",
                item.export.line, item.export.export_name
            );
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
    if o.ownership_state == crate::health_types::OwnershipState::DeclaredInactive {
        notes.push("declared owner inactive");
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
        out.push_str("- **Order**: risk-aware triage order using the larger of low-MI concern and CRAP risk\n");
        out.push_str("- **Fan-in**: files that import this file (blast radius)\n");
        out.push_str("- **Fan-out**: files this file imports (coupling)\n");
        out.push_str("- **Dead Code**: % of value exports with zero references\n");
        out.push_str("- **Density**: cyclomatic complexity / lines of code\n");
        out.push_str(
            "- **Risk**: max CRAP score for the file; low <15, moderate 15-30, high >=30\n",
        );
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
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `src/dead.ts`"));
    }

    #[test]
    fn markdown_unused_export_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `src/utils.ts`"));
        assert!(md.contains(":10 `helperFn`"));
    }

    #[test]
    fn markdown_re_export_tagged() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "reExported".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("(re-export)"));
    }

    #[test]
    fn markdown_unused_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash`"));
    }

    #[test]
    fn markdown_circular_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                    length: 2,
                    line: 3,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("`src/a.ts`"));
        assert!(md.contains("`src/b.ts`"));
        assert!(md.contains("\u{2192}"));
    }

    #[test]
    fn markdown_strips_root_prefix() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/deep/nested/file.ts"),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("`src/deep/nested/file.ts`"));
        assert!(!md.contains("/project/"));
    }

    #[test]
    fn markdown_single_issue_no_plural() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.starts_with("## Fallow: 1 issue found\n"));
    }

    #[test]
    fn markdown_type_only_dep_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Type-only dependencies"));
        assert!(md.contains("- `zod`"));
    }

    #[test]
    fn markdown_escapes_backticks_in_export_names() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "foo`bar".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("foo\\`bar"));
        assert!(!md.contains("foo`bar`"));
    }

    #[test]
    fn markdown_escapes_backticks_in_package_names() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "pkg`name".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("pkg\\`name"));
    }

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
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
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
        assert!(md.contains("| - |"));
    }

    #[test]
    fn health_markdown_includes_coverage_intelligence_and_ambiguity_summary() {
        use crate::health_types::{
            CoverageIntelligenceAction, CoverageIntelligenceConfidence,
            CoverageIntelligenceEvidence, CoverageIntelligenceFinding,
            CoverageIntelligenceMatchConfidence, CoverageIntelligenceRecommendation,
            CoverageIntelligenceReport, CoverageIntelligenceSchemaVersion,
            CoverageIntelligenceSignal, CoverageIntelligenceSummary, CoverageIntelligenceVerdict,
            HealthReport, HealthSummary,
        };

        let root = PathBuf::from("/project");
        let mut report = HealthReport {
            summary: HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 50,
                ..Default::default()
            },
            coverage_intelligence: Some(CoverageIntelligenceReport {
                schema_version: CoverageIntelligenceSchemaVersion::V1,
                verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                summary: CoverageIntelligenceSummary {
                    findings: 1,
                    high_confidence_deletes: 1,
                    ..Default::default()
                },
                findings: vec![CoverageIntelligenceFinding {
                    id: "fallow:coverage-intel:abc123".to_owned(),
                    path: root.join("src/dead.ts"),
                    identity: Some("deadPath".to_owned()),
                    line: 9,
                    verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                    signals: vec![CoverageIntelligenceSignal::RuntimeCold],
                    recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                    confidence: CoverageIntelligenceConfidence::High,
                    related_ids: vec!["fallow:prod:deadbeef".to_owned()],
                    evidence: CoverageIntelligenceEvidence {
                        match_confidence: CoverageIntelligenceMatchConfidence::Direct,
                        ..Default::default()
                    },
                    actions: vec![CoverageIntelligenceAction {
                        kind: "delete-after-confirming-owner".to_owned(),
                        description: "Confirm ownership".to_owned(),
                        auto_fixable: false,
                    }],
                }],
            }),
            ..Default::default()
        };

        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Coverage Intelligence"));
        assert!(md.contains("fallow:coverage-intel:abc123"));
        assert!(md.contains("delete-after-confirming-owner"));
        assert!(md.contains("runtime_cold"));

        report.coverage_intelligence = Some(CoverageIntelligenceReport {
            schema_version: CoverageIntelligenceSchemaVersion::V1,
            verdict: CoverageIntelligenceVerdict::Clean,
            summary: CoverageIntelligenceSummary {
                skipped_ambiguous_matches: 2,
                ..Default::default()
            },
            findings: vec![],
        });
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("2 ambiguous evidence matches were skipped"));
        assert!(!md.contains("| ID | Path |"));
    }

    #[test]
    fn health_markdown_crap_column_shows_score_and_marker() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
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
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                functions_above_threshold: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("| 15 |"));
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
                }
                .into(),
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
                }
                .into(),
            ],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);

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
                files: vec![UntestedFileFinding::with_actions(
                    UntestedFile {
                        path: root.join("src/app.ts"),
                        value_export_count: 2,
                    },
                    &root,
                )],
                exports: vec![UntestedExportFinding::with_actions(
                    UntestedExport {
                        path: root.join("src/app.ts"),
                        export_name: "loader".into(),
                        line: 12,
                        col: 4,
                    },
                    &root,
                )],
            }),
            ..Default::default()
        };

        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Coverage Gaps"));
        assert!(md.contains("*1 untested files"));
        assert!(md.contains("`src/app.ts` (2 value exports)"));
        assert!(md.contains("`src/app.ts`:12 `loader`"));
    }

    #[test]
    fn markdown_dep_in_workspace_shows_package_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/core/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("(packages/core/package.json)"));
    }

    #[test]
    fn markdown_dep_at_root_no_extra_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash`"));
        assert!(!md.contains("(package.json)"));
    }

    #[test]
    fn markdown_root_dep_with_cross_workspace_context_uses_context_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("- `lodash-es` (imported in packages/consumer)"));
        assert!(!md.contains("(package.json; imported in packages/consumer)"));
    }

    #[test]
    fn markdown_exports_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "alpha".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "beta".to_string(),
                is_type_only: false,
                line: 10,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/other.ts"),
                export_name: "gamma".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let md = build_markdown(&results, &root);
        let utils_count = md.matches("- `src/utils.ts`").count();
        assert_eq!(utils_count, 1, "file header should appear once per file");
        assert!(md.contains(":5 `alpha`"));
        assert!(md.contains(":10 `beta`"));
    }

    #[test]
    fn markdown_multiple_issues_plural() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/b.ts"),
            }));
        let md = build_markdown(&results, &root);
        assert!(md.starts_with("## Fallow: 2 issues found\n"));
    }

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
        assert!(md.contains("## Vital Signs"));
        assert!(md.contains("| Metric | Value |"));
        assert!(md.contains("| Total LOC | 15200 |"));
        assert!(md.contains("| Avg Cyclomatic | 3.5 |"));
        assert!(md.contains("| P90 Cyclomatic | 12 |"));
        assert!(md.contains("| Dead Files | 5.0% |"));
        assert!(md.contains("| Dead Exports | 10.2% |"));
        assert!(md.contains("| Maintainability (avg) | 72.3 |"));
        assert!(md.contains("| Hotspots (since 6 months) | 3 |"));
        assert!(md.contains("| Circular Deps | 1 |"));
        assert!(md.contains("| Unused Deps | 2 |"));
    }

    #[test]
    fn health_markdown_hotspots_without_summary_omits_window() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            vital_signs: Some(crate::health_types::VitalSigns {
                avg_cyclomatic: 2.0,
                p90_cyclomatic: 5,
                hotspot_count: Some(0),
                total_loc: 1_000,
                ..Default::default()
            }),
            hotspot_summary: None,
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("| Hotspots | 0 |"));
        assert!(!md.contains("Hotspots (since"));
    }

    #[test]
    fn health_markdown_file_scores_table() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
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

    #[test]
    fn health_markdown_hotspots_table() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            hotspots: vec![
                crate::health_types::HotspotEntry {
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
                }
                .into(),
            ],
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

    #[test]
    fn health_markdown_metric_legend_with_scores() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
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

    #[test]
    fn health_markdown_truncated_findings_shown_count() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
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

    #[test]
    fn escape_backticks_handles_multiple() {
        assert_eq!(escape_backticks("a`b`c"), "a\\`b\\`c");
    }

    #[test]
    fn escape_backticks_no_backticks_unchanged() {
        assert_eq!(escape_backticks("hello"), "hello");
    }

    #[test]
    fn markdown_unresolved_import_grouped_by_file() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/app.ts"),
                specifier: "./missing".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unresolved imports (1)"));
        assert!(md.contains("- `src/app.ts`"));
        assert!(md.contains(":3 `./missing`"));
    }

    #[test]
    fn markdown_unused_optional_dep() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: root.join("package.json"),
                    line: 12,
                    used_in_workspaces: Vec::new(),
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unused optionalDependencies (1)"));
        assert!(md.contains("- `fsevents`"));
    }

    #[test]
    fn health_markdown_hotspots_no_excluded_message() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
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
                    coverage_source: None,
                    inherited_from: None,
                    component_rollup: None,
                    contributions: Vec::new(),
                    effective_thresholds: None,
                    threshold_source: None,
                }
                .into(),
            ],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 10,
                functions_above_threshold: 1,
                ..Default::default()
            },
            hotspots: vec![
                crate::health_types::HotspotEntry {
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
                }
                .into(),
            ],
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
