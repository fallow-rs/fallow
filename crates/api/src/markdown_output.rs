use std::borrow::Cow;
use std::fmt::Write;
use std::path::Path;

use fallow_types::duplicates::DuplicationReport;
use fallow_types::output_dead_code::*;
use fallow_types::results::{AnalysisResults, UnusedExport, UnusedMember};

use fallow_output::normalize_uri;

use crate::ResultGroup;

fn relative_path<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn format_window(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds} s");
    }
    let minutes = seconds / 60;
    if minutes < 120 {
        return format!("{minutes} min");
    }
    let hours = minutes / 60;
    if hours < 48 {
        format!("{hours} h")
    } else {
        format!("{} d", hours / 24)
    }
}

/// Escape backticks in user-controlled strings to prevent breaking markdown code spans.
fn escape_backticks(s: &str) -> String {
    s.replace('`', "\\`")
}

fn escape_table_code_span(s: &str) -> String {
    escape_backticks(s).replace('|', "\\|")
}

fn display_complexity_entry_name(name: &str) -> Cow<'_, str> {
    match name {
        "<template>" => Cow::Borrowed("<template> (template complexity)"),
        "<component>" => Cow::Borrowed("<component> (component rollup)"),
        _ => Cow::Borrowed(name),
    }
}

/// Build markdown output for analysis results.
pub fn build_markdown(results: &AnalysisResults, root: &Path) -> String {
    let total = results.total_issues();
    let mut out = String::new();

    if total == 0 {
        out.push_str("## Fallow: no issues found\n");
        return out;
    }

    let _ = write!(out, "## Fallow: {total} issue{} found\n\n", plural(total));

    push_markdown_primary_sections(&mut out, results, root);
    push_markdown_import_sections(&mut out, results, root);
    push_markdown_dependency_detail_sections(&mut out, results, root);
    push_markdown_graph_sections(&mut out, results, &|path| {
        markdown_relative_path(path, root)
    });
    push_markdown_catalog_sections(&mut out, results, &|path| {
        markdown_relative_path(path, root)
    });

    out
}

fn markdown_relative_path(path: &Path, root: &Path) -> String {
    escape_backticks(&normalize_uri(
        &relative_path(path, root).display().to_string(),
    ))
}

fn push_markdown_primary_sections(out: &mut String, results: &AnalysisResults, root: &Path) {
    markdown_section(out, &results.unused_files, "Unused files", |file| {
        vec![format!(
            "- `{}`",
            markdown_relative_path(&file.file.path, root)
        )]
    });

    markdown_grouped_section(
        out,
        &results.unused_exports,
        "Unused exports",
        root,
        |e| e.export.path.as_path(),
        |e: &UnusedExportFinding| format_export(&e.export),
    );

    markdown_grouped_section(
        out,
        &results.unused_types,
        "Unused type exports",
        root,
        |e| e.export.path.as_path(),
        |e: &UnusedTypeFinding| format_export(&e.export),
    );

    markdown_grouped_section(
        out,
        &results.private_type_leaks,
        "Private type leaks",
        root,
        |e| e.leak.path.as_path(),
        format_private_type_leak,
    );

    push_markdown_dependency_sections(out, results, root);
    push_markdown_member_sections(out, results, root);
}

fn push_markdown_import_sections(out: &mut String, results: &AnalysisResults, root: &Path) {
    markdown_grouped_section(
        out,
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
        out,
        &results.unlisted_dependencies,
        "Unlisted dependencies",
        |dep| vec![format!("- `{}`", escape_backticks(&dep.dep.package_name))],
    );

    markdown_section(
        out,
        &results.duplicate_exports,
        "Duplicate exports",
        |dup| {
            let locations: Vec<String> = dup
                .export
                .locations
                .iter()
                .map(|loc| format!("`{}`", markdown_relative_path(&loc.path, root)))
                .collect();
            vec![format!(
                "- `{}` in {}",
                escape_backticks(&dup.export.export_name),
                locations.join(", ")
            )]
        },
    );
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
    push_markdown_structure_sections(out, results, rel);
    push_markdown_framework_sections(out, results, rel);
    push_markdown_component_sections(out, results, rel);
    push_markdown_suppression_sections(out, results, rel);
}

fn push_markdown_structure_sections(
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
}

fn push_markdown_framework_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
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
}

fn push_markdown_component_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
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
        &results.unused_component_inputs,
        "Unused component inputs",
        |i| format_markdown_unused_component_input(i, rel),
    );
    markdown_section(
        out,
        &results.unused_component_outputs,
        "Unused component outputs",
        |o| format_markdown_unused_component_output(o, rel),
    );
    markdown_section(
        out,
        &results.unused_svelte_events,
        "Unused Svelte events",
        |e| format_markdown_unused_svelte_event(e, rel),
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
}

fn push_markdown_suppression_sections(
    out: &mut String,
    results: &AnalysisResults,
    rel: &dyn Fn(&Path) -> String,
) {
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
    cycle: &fallow_types::output_dead_code::CircularDependencyFinding,
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
    cycle: &fallow_types::output_dead_code::ReExportCycleFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    let chain: Vec<String> = cycle.cycle.files.iter().map(|p| rel(p)).collect();
    let kind_tag = match cycle.cycle.kind {
        fallow_types::results::ReExportCycleKind::SelfLoop => " *(self-loop)*",
        fallow_types::results::ReExportCycleKind::MultiNode => "",
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
    v: &fallow_types::output_dead_code::BoundaryViolationFinding,
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
    v: &fallow_types::output_dead_code::BoundaryCoverageViolationFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} no matching boundary zone",
        rel(&v.violation.path),
        v.violation.line,
    )]
}

fn format_markdown_boundary_call(
    v: &fallow_types::output_dead_code::BoundaryCallViolationFinding,
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
    v: &fallow_types::output_dead_code::PolicyViolationFinding,
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
    e: &fallow_types::output_dead_code::InvalidClientExportFinding,
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
    b: &fallow_types::output_dead_code::MixedClientServerBarrelFinding,
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
    d: &fallow_types::output_dead_code::MisplacedDirectiveFinding,
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
    i: &fallow_types::output_dead_code::UnprovidedInjectFinding,
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
    c: &fallow_types::output_dead_code::UnrenderedComponentFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    // Lit: `component_name` is the registered TAG, so render it as a custom
    // element `<x-foo>` (mirrors the human formatter's `framework == "lit"`
    // branch so the two human-facing surfaces stay consistent).
    if c.component.framework == "lit" {
        return vec![format!(
            "- `{}`:{} `<{}>` is a registered custom element but rendered in no template (render it or remove it)",
            rel(&c.component.path),
            c.component.line,
            escape_backticks(&c.component.component_name),
        )];
    }
    vec![format!(
        "- `{}`:{} `{}` is reachable but rendered nowhere in this project (render it somewhere or remove it)",
        rel(&c.component.path),
        c.component.line,
        escape_backticks(&c.component.component_name),
    )]
}

fn format_markdown_unused_component_prop(
    p: &fallow_types::output_dead_code::UnusedComponentPropFinding,
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
    e: &fallow_types::output_dead_code::UnusedComponentEmitFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is declared but emitted nowhere in this component (remove it or emit it)",
        rel(&e.emit.path),
        e.emit.line,
        escape_backticks(&e.emit.emit_name),
    )]
}

fn format_markdown_unused_svelte_event(
    e: &fallow_types::output_dead_code::UnusedSvelteEventFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is dispatched but listened to nowhere in the project (remove it or listen for it)",
        rel(&e.event.path),
        e.event.line,
        escape_backticks(&e.event.event_name),
    )]
}

fn format_markdown_unused_component_input(
    i: &fallow_types::output_dead_code::UnusedComponentInputFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is declared but referenced nowhere in this component (remove it or use it)",
        rel(&i.input.path),
        i.input.line,
        escape_backticks(&i.input.input_name),
    )]
}

fn format_markdown_unused_component_output(
    o: &fallow_types::output_dead_code::UnusedComponentOutputFinding,
    rel: &dyn Fn(&Path) -> String,
) -> Vec<String> {
    vec![format!(
        "- `{}`:{} `{}` is declared but emitted nowhere in this component (remove it or emit it)",
        rel(&o.output.path),
        o.output.line,
        escape_backticks(&o.output.output_name),
    )]
}

fn format_markdown_unused_server_action(
    a: &fallow_types::output_dead_code::UnusedServerActionFinding,
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
    k: &fallow_types::output_dead_code::UnusedLoadDataKeyFinding,
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
    c: &fallow_types::output_dead_code::RouteCollisionFinding,
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
    c: &fallow_types::output_dead_code::DynamicSegmentNameConflictFinding,
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

/// Build grouped markdown output: each group gets a heading and issue sections.
#[must_use]
pub fn build_grouped_markdown(groups: &[ResultGroup], root: &Path) -> String {
    let total: usize = groups.iter().map(|g| g.results.total_issues()).sum();
    let mut out = String::new();

    if total == 0 {
        out.push_str("## Fallow: no issues found\n");
        return out;
    }

    let _ = writeln!(
        out,
        "## Fallow: {total} issue{} found (grouped)\n",
        plural(total)
    );

    for group in groups {
        let count = group.results.total_issues();
        if count == 0 {
            continue;
        }
        let _ = writeln!(
            out,
            "## {} ({count} issue{})\n",
            escape_backticks(&group.key),
            plural(count)
        );
        if let Some(ref owners) = group.owners
            && !owners.is_empty()
        {
            let joined = owners
                .iter()
                .map(|owner| escape_backticks(owner))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(out, "Owners: {joined}\n");
        }
        let body = build_markdown(&group.results, root);
        let sections = body
            .strip_prefix("## Fallow: no issues found\n")
            .or_else(|| body.find("\n\n").map(|pos| &body[pos + 2..]))
            .unwrap_or(&body);
        out.push_str(sections);
    }

    out
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

/// Build markdown output for duplication results.
#[must_use]
pub fn build_duplication_markdown(report: &DuplicationReport, root: &Path) -> String {
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

    write_duplication_groups(&mut out, report, root);
    write_duplication_families(&mut out, report, root);

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

/// Write the clone-groups subsection of the duplication markdown.
fn write_duplication_groups(out: &mut String, report: &DuplicationReport, root: &Path) {
    let rel = |p: &Path| normalize_uri(&relative_path(p, root).display().to_string());
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
}

/// Write the clone-families subsection of the duplication markdown.
fn write_duplication_families(out: &mut String, report: &DuplicationReport, root: &Path) {
    if report.clone_families.is_empty() {
        return;
    }
    let rel = |p: &Path| normalize_uri(&relative_path(p, root).display().to_string());
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

/// Build markdown output for health (complexity) results.
#[must_use]
pub fn build_health_markdown(report: &fallow_output::HealthReport, root: &Path) -> String {
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
        && report.styling_findings.is_empty()
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
    write_styling_findings_section(&mut out, report, root);
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

fn write_styling_findings_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if report.styling_findings.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("## Styling Findings\n\n");
    out.push_str("| File | Rule | Severity | Value |\n");
    out.push_str("|:-----|:-----|:---------|:------|\n");
    for finding in report.styling_findings.iter().take(20) {
        let path = markdown_relative_path(Path::new(&finding.path), root);
        let severity = match finding.effective_severity {
            fallow_output::StylingFindingSeverity::Error => "error",
            fallow_output::StylingFindingSeverity::Warn => "warn",
        };
        let value = escape_table_code_span(&finding.value);
        let _ = writeln!(
            out,
            "| `{path}:{}` | `{}` / `{}` | {severity} | `{value}` |",
            finding.line, finding.code, finding.sub_kind
        );
    }
    if report.styling_findings.len() > 20 {
        let more = report.styling_findings.len() - 20;
        let _ = writeln!(out, "\n... and {more} more styling findings.");
    }
    out.push('\n');
}

/// Render the opt-in `## CSS Health` markdown section (present only with
/// `--css`): a summary of structural metrics, value sprawl, and candidate counts
/// plus a bounded list of the most actionable located candidates.
fn write_css_analytics_section(out: &mut String, report: &fallow_output::HealthReport) {
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
        "- Candidates: {} unreferenced + {} undefined @keyframes | {} duplicate blocks | {} scoped-unused classes | {} Tailwind arbitrary values | {} unused @property | {} unused @layer | {} likely class typos | {} unreferenced classes | {} unused @font-face | {} unused @theme tokens",
        s.keyframes_unreferenced,
        s.keyframes_undefined,
        s.duplicate_declaration_blocks,
        s.scoped_unused_classes,
        s.tailwind_arbitrary_values,
        s.unused_property_registrations,
        s.unused_layers,
        s.unresolved_class_references,
        s.unreferenced_css_classes,
        s.unused_font_faces,
        s.unused_theme_tokens,
    );
    write_css_candidate_details(out, css);
    out.push('\n');
}

fn write_css_candidate_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
    write_css_keyframe_details(out, css);
    write_css_tailwind_details(out, css);
    write_css_class_candidate_details(out, css);
    write_css_font_candidate_details(out, css);
    write_css_font_size_mix_details(out, css);
}

fn write_css_keyframe_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
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
}

fn write_css_tailwind_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
    if !css.tailwind_arbitrary_values.is_empty() {
        let named: Vec<String> = css
            .tailwind_arbitrary_values
            .iter()
            .take(5)
            .map(|a| format!("`{}` ({}x)", a.value, a.count))
            .collect();
        let _ = writeln!(out, "- Top Tailwind arbitrary values: {}", named.join(", "));
    }
}

fn write_css_class_candidate_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
    if !css.unresolved_class_references.is_empty() {
        let named: Vec<String> = css
            .unresolved_class_references
            .iter()
            .take(5)
            .map(|u| {
                format!(
                    "`{}` -> `{}` ({}:{})",
                    u.class, u.suggestion, u.path, u.line
                )
            })
            .collect();
        let _ = writeln!(
            out,
            "- Likely class typos (candidates; verify, may be CSS-in-JS or external): {}",
            named.join(", "),
        );
    }
    if !css.unreferenced_css_classes.is_empty() {
        let named: Vec<String> = css
            .unreferenced_css_classes
            .iter()
            .take(5)
            .map(|u| format!("`.{}` ({}:{})", u.class, u.path, u.line))
            .collect();
        let _ = writeln!(
            out,
            "- Unreferenced global classes (candidates; verify no email / server / CMS / Markdown applies them): {}",
            named.join(", "),
        );
    }
}

fn write_css_font_candidate_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
    if !css.unused_font_faces.is_empty() {
        let named: Vec<String> = css
            .unused_font_faces
            .iter()
            .take(5)
            .map(|u| format!("`{}` ({})", u.family, u.path))
            .collect();
        let _ = writeln!(
            out,
            "- Unused @font-face (dead web-font; candidates, may be set from JS/inline): {}",
            named.join(", "),
        );
    }
    if !css.unused_theme_tokens.is_empty() {
        let named: Vec<String> = css
            .unused_theme_tokens
            .iter()
            .take(5)
            .map(|u| format!("`{}` ({}:{})", u.token, u.path, u.line))
            .collect();
        let _ = writeln!(
            out,
            "- Unused @theme tokens (dead Tailwind v4 design tokens; candidates, may be consumed by a plugin or downstream repo): {}",
            named.join(", "),
        );
    }
}

fn write_css_font_size_mix_details(out: &mut String, css: &fallow_output::CssAnalyticsReport) {
    if let Some(mix) = &css.font_size_unit_mix {
        let breakdown: Vec<String> = mix
            .notations
            .iter()
            .map(|n| format!("{} {}", n.count, n.notation))
            .collect();
        let _ = writeln!(
            out,
            "- Font sizes mix {} units (candidate, standardize unless intentional): {}",
            mix.notations.len(),
            breakdown.join(", "),
        );
    }
}

fn write_coverage_intelligence_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
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
        write_coverage_intelligence_row(out, finding, root);
    }
    out.push('\n');
}

/// Write one coverage-intelligence finding row.
fn write_coverage_intelligence_row(
    out: &mut String,
    finding: &fallow_output::CoverageIntelligenceFinding,
    root: &Path,
) {
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

fn write_runtime_coverage_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let Some(ref production) = report.runtime_coverage else {
        return;
    };
    if !out.is_empty() && !out.ends_with("\n\n") {
        out.push('\n');
    }
    write_runtime_coverage_summary(out, production);
    write_runtime_coverage_findings(out, production, root);
    write_runtime_coverage_hot_paths(out, production, root);
}

/// Write the runtime-coverage summary header and capture-quality lines.
fn write_runtime_coverage_summary(
    out: &mut String,
    production: &fallow_output::RuntimeCoverageReport,
) {
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
        let window = format_window(quality.window_seconds);
        let _ = writeln!(
            out,
            "- Capture quality: short window ({} from {} instance(s), {:.1}% of functions untracked); lazy-parsed scripts may not appear.\n",
            window, quality.instances_observed, quality.untracked_ratio_percent,
        );
    }
}

/// Write the runtime-coverage per-finding table.
fn write_runtime_coverage_findings(
    out: &mut String,
    production: &fallow_output::RuntimeCoverageReport,
    root: &Path,
) {
    if production.findings.is_empty() {
        return;
    }
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
            escape_backticks(&normalize_uri(
                &relative_path(&finding.path, root).display().to_string(),
            )),
            finding.line,
            escape_backticks(&finding.function),
            finding.verdict,
            invocations,
            finding.confidence,
        );
    }
    out.push('\n');
}

/// Write the runtime-coverage hot-paths table.
fn write_runtime_coverage_hot_paths(
    out: &mut String,
    production: &fallow_output::RuntimeCoverageReport,
    root: &Path,
) {
    if production.hot_paths.is_empty() {
        return;
    }
    out.push_str("| ID | Hot path | Function | Invocations | Percentile |\n");
    out.push_str("|:---|:---------|:---------|------------:|-----------:|\n");
    for entry in &production.hot_paths {
        let _ = writeln!(
            out,
            "| `{}` | `{}`:{} | `{}` | {} | {} |",
            escape_backticks(&entry.id),
            escape_backticks(&normalize_uri(
                &relative_path(&entry.path, root).display().to_string(),
            )),
            entry.line,
            escape_backticks(&entry.function),
            entry.invocations,
            entry.percentile,
        );
    }
    out.push('\n');
}

/// Write the trend comparison table to the output.
fn write_trend_section(out: &mut String, report: &fallow_output::HealthReport) {
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
        write_trend_metric_row(out, m);
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

/// Write one trend metric row with unit-aware value and delta formatting.
fn write_trend_metric_row(out: &mut String, m: &fallow_output::TrendMetric) {
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

/// Write the vital signs summary table to the output.
fn write_vital_signs_section(out: &mut String, report: &fallow_output::HealthReport) {
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
fn write_findings_section(out: &mut String, report: &fallow_output::HealthReport, root: &Path) {
    if report.findings.is_empty() {
        return;
    }

    let has_synthetic = report
        .findings
        .iter()
        .any(|finding| matches!(finding.name.as_str(), "<template>" | "<component>"));
    write_findings_heading(out, report, has_synthetic);
    write_findings_table_header(out, has_synthetic);

    for finding in &report.findings {
        write_findings_row(out, finding, report, root);
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

/// Write the heading line for the complexity findings section.
fn write_findings_heading(
    out: &mut String,
    report: &fallow_output::HealthReport,
    has_synthetic: bool,
) {
    let count = report.summary.functions_above_threshold;
    let shown = report.findings.len();
    let subject = if has_synthetic {
        "high complexity finding"
    } else {
        "high complexity function"
    };
    if shown < count {
        let _ = write!(
            out,
            "## Fallow: {count} {subject}{} ({shown} shown)\n\n",
            plural(count),
        );
    } else {
        let _ = write!(out, "## Fallow: {count} {subject}{}\n\n", plural(count));
    }
}

/// Write the table header row for the complexity findings section.
fn write_findings_table_header(out: &mut String, has_synthetic: bool) {
    let name_header = if has_synthetic { "Entry" } else { "Function" };
    let _ = writeln!(
        out,
        "| File | {name_header} | Severity | Cyclomatic | Cognitive | CRAP | Lines |"
    );
    out.push_str("|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n");
}

/// Write one complexity finding row, including threshold-breach markers.
fn write_findings_row(
    out: &mut String,
    finding: &fallow_output::HealthFinding,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    let file_str = escape_backticks(&normalize_uri(
        &relative_path(&finding.path, root).display().to_string(),
    ));
    let thresholds =
        finding
            .effective_thresholds
            .unwrap_or(fallow_output::HealthEffectiveThresholds {
                max_cyclomatic: report.summary.max_cyclomatic_threshold,
                max_cognitive: report.summary.max_cognitive_threshold,
                max_crap: report.summary.max_crap_threshold,
            });
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
        fallow_output::FindingSeverity::Critical => "critical",
        fallow_output::FindingSeverity::High => "high",
        fallow_output::FindingSeverity::Moderate => "moderate",
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
        name = escape_backticks(display_complexity_entry_name(&finding.name).as_ref()),
        cyc = finding.cyclomatic,
        cog = finding.cognitive,
        lines = finding.line_count,
    );
}

fn write_threshold_overrides_section(
    out: &mut String,
    report: &fallow_output::HealthReport,
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
            fallow_output::ThresholdOverrideStatus::Active => "active",
            fallow_output::ThresholdOverrideStatus::Stale => "stale",
            fallow_output::ThresholdOverrideStatus::NoMatch => "no_match",
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
fn write_file_scores_section(out: &mut String, report: &fallow_output::HealthReport, root: &Path) {
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
    report: &fallow_output::HealthReport,
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
    ownership: Option<&fallow_output::OwnershipMetrics>,
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
    if o.ownership_state == fallow_output::OwnershipState::DeclaredInactive {
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

fn write_hotspots_section(out: &mut String, report: &fallow_output::HealthReport, root: &Path) {
    if report.hotspots.is_empty() {
        return;
    }

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
    write_hotspots_table_header(out, any_ownership);

    for entry in &report.hotspots {
        write_hotspots_row(out, entry, any_ownership, root);
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

/// Write the hotspots table header, widening with ownership columns when present.
fn write_hotspots_table_header(out: &mut String, any_ownership: bool) {
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
}

/// Write one hotspot row, including ownership cells when the table is widened.
fn write_hotspots_row(
    out: &mut String,
    entry: &fallow_output::HotspotFinding,
    any_ownership: bool,
    root: &Path,
) {
    let file_str = escape_backticks(&normalize_uri(
        &relative_path(&entry.path, root).display().to_string(),
    ));
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

/// Write the refactoring targets table to the output.
fn write_targets_section(out: &mut String, report: &fallow_output::HealthReport, root: &Path) {
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
fn write_metric_legend(out: &mut String, report: &fallow_output::HealthReport) {
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

/// Build a paste-into-PR markdown rendering of the existing walkthrough guide.
///
/// Mirrors the human terminal tour: a Focus line, Stage 1 (affects code outside the
/// PR) and Stage 2 (self-contained) sections partitioned by `concern_lens`, with synthesized
/// badges as inline code spans, then a collapsible Cleared panel. The JSON guide
/// path is untouched; this is the only NEW walkthrough markdown surface. No ANSI.
///
/// `viewed` is the root-relative file list the local ledger marked viewed (the
/// `--mark-viewed` state). Viewed files collapse out of their stage and into the
/// Cleared panel, and the Cleared summary reports the viewed count, so the
/// markdown surface honors `--mark-viewed` the same way the human surface does
/// instead of silently ignoring it.
#[must_use]
pub fn build_walkthrough_markdown(
    guide: &fallow_output::StandardWalkthroughGuide,
    root: &Path,
    viewed: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("## Fallow Review: Walkthrough\n\n");
    push_walkthrough_focus(&mut out, guide, viewed);

    if guide.direction.order.is_empty() {
        out.push_str("_No reviewable units in this change (orientation only)._\n");
        return out;
    }

    let (stage1, stage2) = partition_walkthrough_stages(guide, viewed);
    push_walkthrough_stage(
        &mut out,
        "Stage 1 \u{00b7} Affects code outside this PR",
        &stage1,
        guide,
        root,
    );
    push_walkthrough_stage(
        &mut out,
        "Stage 2 \u{00b7} Self-contained",
        &stage2,
        guide,
        root,
    );
    push_walkthrough_cleared(&mut out, guide, root, viewed);
    out
}

/// Push the `**Focus:**` line built from the guide's triage, with the reconciled
/// file accounting (staged + cleared + excluded) so the count matches the real
/// changed set and non-source files are surfaced, not silently dropped.
fn push_walkthrough_focus(
    out: &mut String,
    guide: &fallow_output::StandardWalkthroughGuide,
    viewed: &[String],
) {
    let triage = &guide.digest.triage;
    let acc = fallow_output::WalkthroughAccounting::compute(guide, viewed);
    let total = acc.header_total();
    let _ = write!(
        out,
        "**Focus:** {} risk \u{00b7} {} \u{00b7} {} file{}",
        walkthrough_risk_label(triage.risk_class),
        walkthrough_effort_label(triage.review_effort),
        total,
        plural(total),
    );
    let mut parts = vec![format!("{} in stages", acc.staged)];
    if acc.cleared > 0 {
        parts.push(format!("{} cleared", acc.cleared));
    }
    if acc.excluded > 0 {
        parts.push(format!("{} non-source not reviewed", acc.excluded));
    }
    if acc.cleared > 0 || acc.excluded > 0 {
        let _ = write!(out, " ({})", parts.join(" \u{00b7} "));
    }
    out.push_str("\n\n");
}

/// Partition the guide's VISIBLE stage units (de-prioritized AND viewed files
/// collapsed out into Cleared) into (contract-break, orientation), each in
/// `direction.order`.
fn partition_walkthrough_stages<'a>(
    guide: &'a fallow_output::StandardWalkthroughGuide,
    viewed: &[String],
) -> (
    Vec<&'a fallow_output::DirectionUnit>,
    Vec<&'a fallow_output::DirectionUnit>,
) {
    let mut load_bearing = Vec::new();
    let mut mechanical = Vec::new();
    for unit in fallow_output::visible_stage_units(guide, viewed) {
        if unit.concern_lens == "contract-break" {
            load_bearing.push(unit);
        } else {
            mechanical.push(unit);
        }
    }
    (load_bearing, mechanical)
}

/// Push one markdown stage section. Skipped when empty.
fn push_walkthrough_stage(
    out: &mut String,
    title: &str,
    units: &[&fallow_output::DirectionUnit],
    guide: &fallow_output::StandardWalkthroughGuide,
    root: &Path,
) {
    if units.is_empty() {
        return;
    }
    let _ = write!(out, "### {title}\n\n");
    for unit in units {
        let rel = markdown_relative_path_str(&unit.file, root);
        let badges = walkthrough_markdown_badges(unit, guide);
        let suffix = if badges.is_empty() {
            String::new()
        } else {
            format!("  {}", badges.join(" "))
        };
        // The raw composite "(score N)" is omitted: it is an opaque attention total
        // that did not explain the within-stage order. `walkthrough_fact` is the
        // concrete "why" each row carries (out-of-diff count, importer count), which
        // is also the number the within-stage order follows, so a row's position is
        // explained by the count it shows.
        let _ = writeln!(out, "- `{rel}`: {}{suffix}", walkthrough_fact(unit, guide));
    }
    out.push('\n');
}

/// Synthesize the inline-code-span badges for a file in markdown (paste-safe).
fn walkthrough_markdown_badges(
    unit: &fallow_output::DirectionUnit,
    guide: &fallow_output::StandardWalkthroughGuide,
) -> Vec<String> {
    let mut badges: Vec<String> = Vec::new();
    for decision in &guide.digest.decisions.decisions {
        if decision.anchor_file != unit.file {
            continue;
        }
        let token = match decision.category {
            fallow_output::DecisionCategory::CouplingBoundary => "COUPLING",
            fallow_output::DecisionCategory::PublicApiContract => "PUBLIC-API",
            fallow_output::DecisionCategory::Dependency => "DEPENDENCY",
        };
        let chip = format!("`{token}`");
        if !badges.contains(&chip) {
            badges.push(chip);
        }
    }
    if walkthrough_introduced(&unit.file, guide) {
        badges.push("`INTRODUCED`".to_string());
    }
    if unit.concern_lens == "contract-break" {
        badges.push("`OUT-OF-DIFF`".to_string());
    }
    if let Some(owner) = unit.expert.first() {
        badges.push(format!("`OWNER:{}`", escape_backticks(owner)));
    }
    if walkthrough_bus_factor(&unit.file, guide) {
        badges.push("`BUS-FACTOR-1`".to_string());
    }
    if walkthrough_weakened(&unit.file, guide) {
        badges.push("`WEAKENED`".to_string());
    }
    badges
}

/// The one-line "why" for a markdown file row. The cascade is decision question >
/// out-of-diff count > focus reason > orientation only. The concrete count it
/// carries (consumers, importers) is the same number the within-stage order
/// follows, so the order mirrors the human surface (the count it shows).
fn walkthrough_fact(
    unit: &fallow_output::DirectionUnit,
    guide: &fallow_output::StandardWalkthroughGuide,
) -> String {
    if let Some(decision) = guide
        .digest
        .decisions
        .decisions
        .iter()
        .find(|d| d.anchor_file == unit.file)
    {
        // Strip the redundant leading path (the bullet already shows it) and cap
        // the contract-member list, PRESERVING the trailing guidance question. The
        // result is plain prose with no backticks, so it never emits a
        // backslash-backtick sequence and never re-prints the path.
        return fallow_output::clean_decision_fact(
            &decision.question,
            &unit.file,
            fallow_output::MAX_CONTRACT_MEMBERS,
        );
    }
    if !unit.out_of_diff.is_empty() {
        return format!(
            "{} out-of-diff consumer{}",
            unit.out_of_diff.len(),
            plural(unit.out_of_diff.len())
        );
    }
    if let Some(fu) = guide
        .digest
        .focus
        .review_here
        .iter()
        .chain(guide.digest.focus.deprioritized.iter())
        .find(|fu| fu.file == unit.file)
    {
        return escape_backticks(&fu.reason);
    }
    "orientation only".to_string()
}

fn walkthrough_introduced(file: &str, guide: &fallow_output::StandardWalkthroughGuide) -> bool {
    let deltas = &guide.digest.deltas;
    deltas
        .boundary_introduced
        .iter()
        .chain(deltas.cycle_introduced.iter())
        .chain(deltas.public_api_added.iter())
        .any(|entry| entry.contains(file))
}

fn walkthrough_bus_factor(file: &str, guide: &fallow_output::StandardWalkthroughGuide) -> bool {
    guide
        .digest
        .routing
        .units
        .iter()
        .any(|u| u.file == file && u.bus_factor_one)
}

fn walkthrough_weakened(file: &str, guide: &fallow_output::StandardWalkthroughGuide) -> bool {
    guide.digest.weakening.iter().any(|w| w.file == file)
}

/// Push the collapsible Cleared `<details>` panel: de-prioritized files plus any
/// `--mark-viewed` files (collapsed out of their stage), with both counts in the
/// summary so the panel reports the same `N de-prioritized, M viewed` split the
/// human surface does.
fn push_walkthrough_cleared(
    out: &mut String,
    guide: &fallow_output::StandardWalkthroughGuide,
    root: &Path,
    viewed: &[String],
) {
    let deprioritized = &guide.digest.focus.deprioritized;
    // Viewed files NOT already de-prioritized, so a viewed-and-de-prioritized file
    // lands in exactly one bucket (no double count), mirroring the human surface.
    let viewed_only: Vec<&String> = viewed
        .iter()
        .filter(|file| !deprioritized.iter().any(|u| &u.file == *file))
        .collect();
    if deprioritized.is_empty() && viewed_only.is_empty() {
        return;
    }
    let _ = write!(
        out,
        "<details><summary>Cleared ({} de-prioritized, {} viewed)</summary>\n\n",
        deprioritized.len(),
        viewed_only.len(),
    );
    for unit in deprioritized {
        let _ = writeln!(
            out,
            "- `{}`: {}",
            markdown_relative_path_str(&unit.file, root),
            escape_backticks(&unit.reason),
        );
    }
    for file in viewed_only {
        let _ = writeln!(
            out,
            "- `{}`: \u{2713} viewed",
            markdown_relative_path_str(file, root),
        );
    }
    out.push_str("\n</details>\n");
}

/// A file-path string already relative to `root` (the guide stores root-relative
/// paths), normalized + backtick-escaped for a markdown code span.
fn markdown_relative_path_str(file: &str, root: &Path) -> String {
    let path = Path::new(file);
    if path.is_absolute() {
        return markdown_relative_path(path, root);
    }
    escape_backticks(&normalize_uri(file))
}

fn walkthrough_risk_label(risk: fallow_output::RiskClass) -> &'static str {
    match risk {
        fallow_output::RiskClass::Low => "low",
        fallow_output::RiskClass::Medium => "medium",
        fallow_output::RiskClass::High => "high",
    }
}

fn walkthrough_effort_label(effort: fallow_output::ReviewEffort) -> &'static str {
    match effort {
        fallow_output::ReviewEffort::Glance => "glance",
        fallow_output::ReviewEffort::Review => "review",
        fallow_output::ReviewEffort::DeepDive => "deep-dive",
    }
}

#[cfg(test)]
mod health_markdown_tests {
    use std::path::Path;

    use fallow_output::{HealthReport, StylingFinding, StylingFindingSeverity};

    use super::build_health_markdown;

    #[test]
    fn health_markdown_includes_styling_findings() {
        let report = HealthReport {
            styling_findings: vec![StylingFinding {
                code: "css-broken-reference".to_string(),
                sub_kind: "unresolved-class-reference".to_string(),
                path: "src/app.css".to_string(),
                line: 9,
                value: "btn-prmary | btn-primary".to_string(),
                effective_severity: StylingFindingSeverity::Warn,
                blast_radius: None,
                confidence: None,
                agent_disposition: None,
                nearest_token: None,
                fix_hint: None,
                actions: Vec::new(),
            }],
            ..HealthReport::default()
        };

        let output = build_health_markdown(&report, Path::new("/project"));

        assert!(output.contains("## Styling Findings"));
        assert!(output.contains("css-broken-reference"));
        assert!(output.contains("btn-prmary \\| btn-primary"));
    }
}

#[cfg(test)]
mod walkthrough_markdown_tests {
    use super::build_walkthrough_markdown;
    use fallow_output::{
        AgentSchema, Decision, DecisionCategory, DecisionSurface, DiffTriage, DirectionUnit,
        FocusLabel, FocusMap, FocusScore, FocusUnit, GraphFacts, INJECTION_NOTE,
        ImpactClosureFacts, PartitionFacts, ReviewBriefSchemaVersion, ReviewDeltas,
        ReviewDirection, ReviewEffort, RiskClass, RoutingFacts, StandardReviewBriefOutput,
        StandardWalkthroughGuide,
    };
    use std::path::Path;

    fn guide_with_question(file: &str, question: &str) -> StandardWalkthroughGuide {
        let unit = DirectionUnit {
            file: file.to_string(),
            concern_lens: "contract-break".to_string(),
            scoring_budget: 3,
            out_of_diff: vec!["src/consumer.ts".to_string()],
            expert: Vec::new(),
        };
        // The direction unit comes FROM the focus map's review_here in reality, so
        // mirror that here: review_here has the one source unit and triage.files
        // matches it, keeping the excluded bucket at 0 for this synthetic guide.
        let review_unit = FocusUnit {
            file: file.to_string(),
            score: FocusScore::default(),
            label: FocusLabel::ReviewHere,
            reason: "reason".to_string(),
            confidence: Vec::new(),
        };
        let decision = Decision {
            signal_id: "sig:1".to_string(),
            category: DecisionCategory::CouplingBoundary,
            question: question.to_string(),
            anchor_file: file.to_string(),
            anchor_line: 1,
            signal_key: "k".to_string(),
            previous_signal_id: None,
            blast: 1,
            consequence: 2,
            expert: Vec::new(),
            bus_factor_one: false,
            internal_consumer_count: 0,
            tradeoff: String::new(),
        };
        let digest = StandardReviewBriefOutput {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "audit-brief".to_string(),
            triage: DiffTriage {
                files: 1,
                hunks: None,
                net_lines: None,
                risk_class: RiskClass::Low,
                review_effort: ReviewEffort::Glance,
            },
            graph_facts: GraphFacts {
                exports_added: 0,
                api_width_delta: 0,
                reachable_from: Vec::new(),
                boundaries_touched: Vec::new(),
            },
            partition: PartitionFacts::default(),
            impact_closure: ImpactClosureFacts::default(),
            focus: FocusMap {
                review_here: vec![review_unit],
                deprioritized: Vec::new(),
            },
            deltas: ReviewDeltas::default(),
            weakening: Vec::new(),
            routing: RoutingFacts::default(),
            decisions: DecisionSurface {
                decisions: vec![decision],
                truncated: None,
                emitted_signal_ids: Vec::new(),
            },
        };
        StandardWalkthroughGuide {
            schema_version: ReviewBriefSchemaVersion::default(),
            version: "test".to_string(),
            command: "review-walkthrough-guide".to_string(),
            graph_snapshot_hash: "graph:abc".to_string(),
            digest,
            direction: ReviewDirection {
                order: vec![file.to_string()],
                units: vec![unit],
            },
            change_anchors: Vec::new(),
            agent_schema: AgentSchema {
                judgment_shape: "",
                echo_field: "graph_snapshot_hash",
                anchoring_rule: "",
            },
            injection_note: INJECTION_NOTE,
        }
    }

    #[test]
    fn renders_header_stage_and_code_span_badges() {
        let guide = guide_with_question("src/page.ts", "Couple ui to db?");
        let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
        assert!(md.starts_with("## Fallow Review"), "got: {md}");
        assert!(md.contains("### Stage 1"), "got: {md}");
        assert!(md.contains("`COUPLING`"), "badges are code spans: {md}");
        assert!(md.contains("`OUT-OF-DIFF`"), "got: {md}");
        assert!(!md.contains('\u{1b}'), "no ANSI in markdown");
        // The file->description separator is a colon, not the house-style-banned
        // em-dash that the list items used to lead with.
        assert!(
            md.contains("- `src/page.ts`: "),
            "list items use a colon separator: {md}"
        );
        assert!(
            !md.contains("- `src/page.ts` \u{2014} "),
            "no em-dash file separator: {md}"
        );
    }

    // The markdown surface honors `--mark-viewed`: a viewed file collapses out of
    // its stage into the Cleared panel, and the summary reports the viewed count
    // (the same on-disk state the human surface reads), no longer ignored.
    #[test]
    fn viewed_file_collapses_into_cleared_in_markdown() {
        let guide = guide_with_question("src/page.ts", "Couple ui to db?");
        let viewed = vec!["src/page.ts".to_string()];
        let md = build_walkthrough_markdown(&guide, Path::new("/project"), &viewed);
        // The viewed file is no longer rendered in a stage section.
        assert!(
            !md.contains("### Stage 1"),
            "viewed file left its stage: {md}"
        );
        // The Cleared panel reports the viewed count and lists the viewed file.
        assert!(
            md.contains("Cleared (0 de-prioritized, 1 viewed)"),
            "cleared reports viewed count: {md}"
        );
        assert!(
            md.contains("- `src/page.ts`: \u{2713} viewed"),
            "viewed file listed under cleared: {md}"
        );
    }

    // F5/F7: a coordination question must NOT re-print the anchor path inside the
    // fact text, must NOT emit a backslash-backtick sequence, must cap the
    // contract member list, and drops the trailing question in the tour.
    #[test]
    fn fact_does_not_reprint_path_or_emit_escaped_backticks() {
        let q = "`src/page.ts` changes exports (a, b, c, d, e, f, g, h, i) imported by 9 files outside this PR. Does this change break or alter what those callers expect?";
        let guide = guide_with_question("src/page.ts", q);
        let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
        // No backslash-backtick anywhere (the F5 corruption).
        assert!(
            !md.contains("\\`"),
            "fact must never emit a backslash-backtick sequence: {md}"
        );
        // The path is printed once (the bullet lead), not a second time in the fact.
        assert!(
            !md.contains("`src/page.ts` changes exports"),
            "fact must not re-print the path: {md}"
        );
        // The member list is capped with a "+N more".
        assert!(md.contains("+3 more"), "member list capped: {md}");
        // The trailing decision question is dropped in the tour (it lives in the brief).
        assert!(
            !md.contains("break or alter"),
            "the per-file question must be dropped in the tour: {md}"
        );
        // The raw "(score N)" is gone.
        assert!(!md.contains("(score "), "raw score removed: {md}");
    }

    #[test]
    fn empty_order_renders_orientation_only_note() {
        let mut guide = guide_with_question("src/page.ts", "q");
        guide.direction.order.clear();
        guide.direction.units.clear();
        let md = build_walkthrough_markdown(&guide, Path::new("/project"), &[]);
        assert!(md.contains("orientation only"), "got: {md}");
    }
}
