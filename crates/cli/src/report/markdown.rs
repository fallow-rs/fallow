use crate::report::sink::{out, outln};
use std::borrow::Cow;
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

fn display_complexity_entry_name(name: &str) -> Cow<'_, str> {
    match name {
        "<template>" => Cow::Borrowed("<template> (template complexity)"),
        "<component>" => Cow::Borrowed("<component> (component rollup)"),
        _ => Cow::Borrowed(name),
    }
}

pub(super) fn print_markdown(results: &AnalysisResults, root: &Path) {
    outln!("{}", build_markdown(results, root));
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

fn format_markdown_unused_svelte_event(
    e: &fallow_core::results::UnusedSvelteEventFinding,
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
    i: &fallow_core::results::UnusedComponentInputFinding,
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
    o: &fallow_core::results::UnusedComponentOutputFinding,
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

fn write_css_candidate_details(out: &mut String, css: &crate::health_types::CssAnalyticsReport) {
    write_css_keyframe_details(out, css);
    write_css_tailwind_details(out, css);
    write_css_class_candidate_details(out, css);
    write_css_font_candidate_details(out, css);
    write_css_font_size_mix_details(out, css);
}

fn write_css_keyframe_details(out: &mut String, css: &crate::health_types::CssAnalyticsReport) {
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

fn write_css_tailwind_details(out: &mut String, css: &crate::health_types::CssAnalyticsReport) {
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

fn write_css_class_candidate_details(
    out: &mut String,
    css: &crate::health_types::CssAnalyticsReport,
) {
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

fn write_css_font_candidate_details(
    out: &mut String,
    css: &crate::health_types::CssAnalyticsReport,
) {
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

fn write_css_font_size_mix_details(
    out: &mut String,
    css: &crate::health_types::CssAnalyticsReport,
) {
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
        write_coverage_intelligence_row(out, finding, root);
    }
    out.push('\n');
}

/// Write one coverage-intelligence finding row.
fn write_coverage_intelligence_row(
    out: &mut String,
    finding: &crate::health_types::CoverageIntelligenceFinding,
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
    report: &crate::health_types::HealthReport,
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
    production: &crate::health_types::RuntimeCoverageReport,
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
        let window = super::human::health::format_window(quality.window_seconds);
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
    production: &crate::health_types::RuntimeCoverageReport,
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
    production: &crate::health_types::RuntimeCoverageReport,
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
fn write_trend_metric_row(out: &mut String, m: &crate::health_types::TrendMetric) {
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
    report: &crate::health_types::HealthReport,
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
    finding: &crate::health_types::HealthFinding,
    report: &crate::health_types::HealthReport,
    root: &Path,
) {
    let file_str = escape_backticks(&normalize_uri(
        &relative_path(&finding.path, root).display().to_string(),
    ));
    let thresholds =
        finding
            .effective_thresholds
            .unwrap_or(crate::health_types::HealthEffectiveThresholds {
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
        name = escape_backticks(display_complexity_entry_name(&finding.name).as_ref()),
        cyc = finding.cyclomatic,
        cog = finding.cognitive,
        lines = finding.line_count,
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
    entry: &crate::health_types::HotspotFinding,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
    fn health_markdown_labels_template_complexity_entries() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/Card.vue"),
                    name: "<template>".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 8,
                    cognitive: 12,
                    line_count: 40,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: crate::health_types::ExceededThreshold::Cognitive,
                    severity: crate::health_types::FindingSeverity::Moderate,
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
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Fallow: 1 high complexity finding\n"));
        assert!(md.contains("| File | Entry |"));
        assert!(md.contains("`<template> (template complexity)`"));
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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

    // -------------------------------------------------------------------------
    // display_complexity_entry_name: <component> case (lines 24-25)
    // -------------------------------------------------------------------------

    #[test]
    fn display_complexity_entry_name_component_variant() {
        assert_eq!(
            display_complexity_entry_name("<component>"),
            "<component> (component rollup)"
        );
    }

    // -------------------------------------------------------------------------
    // private_type_leaks section (lines 95-99)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_private_type_leak_section() {
        use fallow_types::output_dead_code::PrivateTypeLeakFinding;
        use fallow_types::results::PrivateTypeLeak;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: root.join("src/api.ts"),
                export_name: "publicFn".to_string(),
                type_name: "InternalType".to_string(),
                line: 7,
                col: 0,
                span_start: 0,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Private type leaks (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("`src/api.ts`"));
        assert!(normalized.contains("`publicFn` references private type `InternalType`"));
    }

    // -------------------------------------------------------------------------
    // circular dependency with is_cross_package: true (lines 409-413)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_circular_dep_cross_package_tag() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("pkg-a/src/a.ts"), root.join("pkg-b/src/b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: true,
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("*(cross-package)*"));
    }

    // -------------------------------------------------------------------------
    // boundary coverage violation (lines 460-468)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_boundary_coverage_violation_format() {
        use fallow_types::output_dead_code::BoundaryCoverageViolationFinding;
        use fallow_types::results::BoundaryCoverageViolation;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: root.join("src/orphan.ts"),
                    line: 1,
                    col: 0,
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Boundary coverage (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("src/orphan.ts"));
        assert!(normalized.contains("no matching boundary zone"));
    }

    // -------------------------------------------------------------------------
    // boundary call violation (lines 471-483)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_boundary_call_violation_format() {
        use fallow_types::output_dead_code::BoundaryCallViolationFinding;
        use fallow_types::results::BoundaryCallViolation;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: root.join("src/caller.ts"),
                    line: 42,
                    col: 0,
                    callee: "dangerousCall".to_string(),
                    zone: "public".to_string(),
                    pattern: "dangerous*".to_string(),
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Boundary calls (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("`dangerousCall` forbidden in zone `public`"));
        assert!(normalized.contains("pattern `dangerous*`"));
    }

    // -------------------------------------------------------------------------
    // policy violation (lines 485-502)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_policy_violation_without_message() {
        use fallow_types::output_dead_code::PolicyViolationFinding;
        use fallow_types::results::{PolicyRuleKind, PolicyViolation, PolicyViolationSeverity};
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: root.join("src/banned.ts"),
                line: 3,
                col: 0,
                matched: "eval".to_string(),
                pack: "security".to_string(),
                rule_id: "no-eval".to_string(),
                kind: PolicyRuleKind::BannedCall,
                severity: PolicyViolationSeverity::Error,
                message: None,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Policy violations (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("`eval` banned by `security/no-eval`"));
    }

    #[test]
    fn markdown_policy_violation_with_message() {
        use fallow_types::output_dead_code::PolicyViolationFinding;
        use fallow_types::results::{PolicyRuleKind, PolicyViolation, PolicyViolationSeverity};
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: root.join("src/banned.ts"),
                line: 8,
                col: 0,
                matched: "console.log".to_string(),
                pack: "style".to_string(),
                rule_id: "no-console".to_string(),
                kind: PolicyRuleKind::BannedCall,
                severity: PolicyViolationSeverity::Warn,
                message: Some("Use a logger instead".to_string()),
            }));
        let md = build_markdown(&results, &root);
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("(Use a logger instead)"));
    }

    // -------------------------------------------------------------------------
    // misplaced directive (lines 530-540)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_misplaced_directive_format() {
        use fallow_types::output_dead_code::MisplacedDirectiveFinding;
        use fallow_types::results::MisplacedDirective;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .misplaced_directives
            .push(MisplacedDirectiveFinding::with_actions(
                MisplacedDirective {
                    path: root.join("src/app.ts"),
                    line: 10,
                    col: 0,
                    directive: "use client".to_string(),
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Misplaced directives (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("src/app.ts"));
        assert!(normalized.contains("not in the leading position"));
    }

    // -------------------------------------------------------------------------
    // route collision (lines 651-661)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_route_collision_format() {
        use fallow_types::output_dead_code::RouteCollisionFinding;
        use fallow_types::results::RouteCollision;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: root.join("app/dashboard/page.tsx"),
                url: "/dashboard".to_string(),
                conflicting_paths: vec![root.join("pages/dashboard.tsx")],
                line: 1,
                col: 0,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Route collisions (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("/dashboard"));
        assert!(normalized.contains("dashboard"));
    }

    // -------------------------------------------------------------------------
    // dynamic segment name conflict (lines 663-674)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_dynamic_segment_name_conflict_format() {
        use fallow_types::output_dead_code::DynamicSegmentNameConflictFinding;
        use fallow_types::results::DynamicSegmentNameConflict;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: root.join("app/[slug]/page.tsx"),
                conflicting_segments: vec!["[slug]".to_string(), "[id]".to_string()],
                conflicting_paths: vec![root.join("app/[id]/page.tsx")],
                position: "/product".to_string(),
                line: 1,
                col: 0,
            }),
        );
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Dynamic segment conflicts (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("slug"));
        assert!(normalized.contains("id"));
    }

    // -------------------------------------------------------------------------
    // catalog entry with hardcoded consumers (lines 741-751)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_catalog_entry_with_hardcoded_consumers() {
        use fallow_types::output_dead_code::UnusedCatalogEntryFinding;
        use fallow_types::results::UnusedCatalogEntry;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "lodash".to_string(),
                    catalog_name: "default".to_string(),
                    path: root.join("pnpm-workspace.yaml"),
                    line: 4,
                    hardcoded_consumers: vec![root.join("packages/legacy")],
                },
            ));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unused catalog entries (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("hardcoded in"));
        assert!(normalized.contains("packages/legacy"));
    }

    // -------------------------------------------------------------------------
    // unresolved catalog reference with available_in_catalogs (lines 765-774)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_unresolved_catalog_reference_with_alts() {
        use fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding;
        use fallow_types::results::UnresolvedCatalogReference;
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: root.join("packages/app/package.json"),
                line: 12,
                available_in_catalogs: vec!["shared".to_string()],
            }),
        );
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unresolved catalog references (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("available in: `shared`"));
    }

    // -------------------------------------------------------------------------
    // vital signs section (lines 1536-1571)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_vital_signs_all_optional_fields() {
        use crate::health_types::{HotspotSummary, VitalSigns};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            vital_signs: Some(VitalSigns {
                dead_file_pct: Some(2.5),
                dead_export_pct: Some(10.0),
                avg_cyclomatic: 3.2,
                critical_complexity_pct: None,
                p90_cyclomatic: 8,
                duplication_pct: None,
                hotspot_count: Some(4),
                hotspot_top_pct_count: None,
                maintainability_avg: Some(75.0),
                maintainability_low_pct: None,
                unused_dep_count: Some(3),
                unused_deps_per_k_files: None,
                circular_dep_count: Some(2),
                circular_deps_per_k_files: None,
                counts: None,
                unit_size_profile: None,
                functions_over_60_loc_per_k: None,
                unit_interfacing_profile: None,
                p95_fan_in: None,
                coupling_high_pct: None,
                prop_drilling_chain_count: None,
                prop_drilling_max_depth: None,
                p95_render_fan_in: None,
                render_fan_in_high_pct: None,
                max_render_fan_in: None,
                top_render_fan_in: Vec::new(),
                total_loc: 5000,
            }),
            hotspot_summary: Some(HotspotSummary {
                since: "3 months".to_string(),
                min_commits: 2,
                files_analyzed: 20,
                files_excluded: 0,
                shallow_clone: false,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 10,
                functions_analyzed: 80,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Vital Signs"));
        assert!(md.contains("| Total LOC | 5000 |"));
        assert!(md.contains("| Avg Cyclomatic | 3.2 |"));
        assert!(md.contains("| Dead Files | 2.5% |"));
        assert!(md.contains("| Dead Exports | 10.0% |"));
        assert!(md.contains("| Maintainability (avg) | 75.0 |"));
        assert!(md.contains("| Hotspots (since 3 months) | 4 |"));
        assert!(md.contains("| Circular Deps | 2 |"));
        assert!(md.contains("| Unused Deps | 3 |"));
    }

    #[test]
    fn health_markdown_vital_signs_hotspot_without_summary() {
        use crate::health_types::VitalSigns;
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            vital_signs: Some(VitalSigns {
                avg_cyclomatic: 2.0,
                p90_cyclomatic: 5,
                total_loc: 0,
                dead_file_pct: None,
                dead_export_pct: None,
                critical_complexity_pct: None,
                duplication_pct: None,
                hotspot_count: Some(7),
                hotspot_top_pct_count: None,
                maintainability_avg: None,
                maintainability_low_pct: None,
                unused_dep_count: None,
                unused_deps_per_k_files: None,
                circular_dep_count: None,
                circular_deps_per_k_files: None,
                counts: None,
                unit_size_profile: None,
                functions_over_60_loc_per_k: None,
                unit_interfacing_profile: None,
                p95_fan_in: None,
                coupling_high_pct: None,
                prop_drilling_chain_count: None,
                prop_drilling_max_depth: None,
                p95_render_fan_in: None,
                render_fan_in_high_pct: None,
                max_render_fan_in: None,
                top_render_fan_in: Vec::new(),
            }),
            hotspot_summary: None,
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // When hotspot_summary is None, the label should just say "Hotspots"
        assert!(md.contains("| Hotspots | 7 |"));
        assert!(!md.contains("since"));
    }

    // -------------------------------------------------------------------------
    // health score section (lines 1052-1054 / 1679-1681)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_health_score_header() {
        use crate::health_types::{HealthScore, HealthScorePenalties};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            health_score: Some(HealthScore {
                formula_version: 2,
                score: 82.0,
                grade: "B",
                penalties: HealthScorePenalties {
                    dead_files: None,
                    dead_exports: None,
                    complexity: 5.0,
                    p90_complexity: 3.0,
                    maintainability: None,
                    hotspots: None,
                    unused_deps: None,
                    circular_deps: None,
                    unit_size: None,
                    coupling: None,
                    duplication: None,
                    prop_drilling: None,
                },
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Health Score: 82 (B)"));
    }

    // -------------------------------------------------------------------------
    // threshold overrides section (lines 1696-1747)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_threshold_overrides_active() {
        use crate::health_types::{
            HealthConfiguredThresholds, HealthEffectiveThresholds, ThresholdOverrideMetrics,
            ThresholdOverrideState, ThresholdOverrideStatus,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/complex.ts"),
                    name: "bigFn".to_string(),
                    line: 10,
                    col: 0,
                    cyclomatic: 25,
                    cognitive: 20,
                    line_count: 50,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                files_analyzed: 2,
                functions_analyzed: 5,
                functions_above_threshold: 1,
                ..Default::default()
            },
            threshold_overrides: vec![ThresholdOverrideState {
                status: ThresholdOverrideStatus::Active,
                override_index: 0,
                path: Some(root.join("src/complex.ts")),
                function: Some("bigFn".to_string()),
                configured_thresholds: HealthConfiguredThresholds {
                    max_cyclomatic: Some(30),
                    max_cognitive: Some(25),
                    max_crap: None,
                },
                effective_thresholds: HealthEffectiveThresholds {
                    max_cyclomatic: 30,
                    max_cognitive: 25,
                    max_crap: 30.0,
                },
                metrics: Some(ThresholdOverrideMetrics {
                    cyclomatic: 25,
                    cognitive: 20,
                    crap: None,
                }),
                reason: None,
            }],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Health Threshold Overrides"));
        assert!(md.contains("| Override | Status |"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("active"));
        assert!(normalized.contains("src/complex.ts:bigFn"));
        assert!(normalized.contains("cyclomatic 25, cognitive 20"));
    }

    #[test]
    fn health_markdown_threshold_overrides_stale_no_match() {
        use crate::health_types::{
            HealthConfiguredThresholds, HealthEffectiveThresholds, ThresholdOverrideState,
            ThresholdOverrideStatus,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/x.ts"),
                    name: "fn1".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 40,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                functions_analyzed: 3,
                functions_above_threshold: 1,
                ..Default::default()
            },
            threshold_overrides: vec![
                ThresholdOverrideState {
                    status: ThresholdOverrideStatus::Stale,
                    override_index: 1,
                    path: None,
                    function: None,
                    configured_thresholds: HealthConfiguredThresholds {
                        max_cyclomatic: Some(40),
                        max_cognitive: None,
                        max_crap: None,
                    },
                    effective_thresholds: HealthEffectiveThresholds {
                        max_cyclomatic: 40,
                        max_cognitive: 15,
                        max_crap: 30.0,
                    },
                    metrics: None,
                    reason: None,
                },
                ThresholdOverrideState {
                    status: ThresholdOverrideStatus::NoMatch,
                    override_index: 2,
                    path: None,
                    function: None,
                    configured_thresholds: HealthConfiguredThresholds {
                        max_cyclomatic: None,
                        max_cognitive: None,
                        max_crap: Some(50.0),
                    },
                    effective_thresholds: HealthEffectiveThresholds {
                        max_cyclomatic: 20,
                        max_cognitive: 15,
                        max_crap: 50.0,
                    },
                    metrics: None,
                    reason: None,
                },
            ],
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("stale"));
        assert!(md.contains("no_match"));
        assert!(md.contains("<no matching file or function>"));
        // No metrics so the column is a dash
        assert!(md.contains("| - |"));
    }

    // -------------------------------------------------------------------------
    // file scores section (lines 1749-1791)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_file_scores_section() {
        use crate::health_types::FileHealthScore;
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            file_scores: vec![FileHealthScore {
                path: root.join("src/util.ts"),
                fan_in: 5,
                fan_out: 3,
                dead_code_ratio: 0.1,
                complexity_density: 0.25,
                maintainability_index: 68.0,
                total_cyclomatic: 40,
                total_cognitive: 30,
                function_count: 8,
                lines: 200,
                crap_max: 22.5,
                crap_above_threshold: 1,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                average_maintainability: Some(71.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### File Health Scores (1 files)"));
        assert!(md.contains("| File | Maintainability |"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("`src/util.ts`"));
        assert!(normalized.contains("68.0"));
        // avg maintainability is shown when set
        assert!(md.contains("**Average maintainability index:** 71.0/100"));
    }

    // -------------------------------------------------------------------------
    // coverage gaps: empty files and exports (lines 1810-1813)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_coverage_gaps_empty_files_and_exports() {
        use crate::health_types::{CoverageGapSummary, CoverageGaps};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 5,
                    covered_files: 5,
                    file_coverage_pct: 100.0,
                    untested_files: 0,
                    untested_exports: 0,
                },
                files: vec![],
                exports: vec![],
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Coverage Gaps"));
        assert!(md.contains("_No coverage gaps found in scope._"));
    }

    // -------------------------------------------------------------------------
    // hotspots section without ownership (lines 1889-1928)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_hotspots_without_ownership() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/x.ts"),
                    name: "hotFn".to_string(),
                    line: 5,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 60,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    score: 75.0,
                    commits: 20,
                    weighted_commits: 18.0,
                    lines_added: 200,
                    lines_deleted: 80,
                    complexity_density: 0.7,
                    fan_in: 8,
                    trend: fallow_core::churn::ChurnTrend::Accelerating,
                    ownership: None,
                    is_test_path: false,
                }
                .into(),
            ],
            hotspot_summary: None,
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Hotspots (1 files)"));
        // No "since" when hotspot_summary is None
        assert!(!md.contains("since"));
        // Without ownership the narrow table is used
        assert!(md.contains("| File | Score | Commits | Churn | Density | Fan-in | Trend |"));
        assert!(!md.contains("Bus"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("`src/hot.ts`"));
        assert!(md.contains("75.0"));
        assert!(md.contains("accelerating"));
    }

    #[test]
    fn health_markdown_hotspots_with_summary_since_and_excluded() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/z.ts"),
                    name: "g".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 30,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                    path: root.join("src/churn.ts"),
                    score: 60.0,
                    commits: 15,
                    weighted_commits: 12.0,
                    lines_added: 150,
                    lines_deleted: 40,
                    complexity_density: 0.4,
                    fan_in: 2,
                    trend: fallow_core::churn::ChurnTrend::Cooling,
                    ownership: None,
                    is_test_path: false,
                }
                .into(),
            ],
            hotspot_summary: Some(crate::health_types::HotspotSummary {
                since: "12 months".to_string(),
                min_commits: 5,
                files_analyzed: 100,
                files_excluded: 3,
                shallow_clone: false,
            }),
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("### Hotspots (1 files, since 12 months)"));
        // files_excluded > 0 triggers the excluded message
        assert!(md.contains("3 file"));
        assert!(md.contains("excluded"));
        assert!(md.contains("< 5 commits"));
    }

    // -------------------------------------------------------------------------
    // hotspots with ownership (lines 1854-1887)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_hotspots_with_ownership() {
        use crate::health_types::{
            ContributorEntry, ContributorIdentifierFormat, OwnershipMetrics, OwnershipState,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/owned.ts"),
                    name: "fn2".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 50,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                files_analyzed: 2,
                functions_analyzed: 4,
                functions_above_threshold: 1,
                ..Default::default()
            },
            hotspots: vec![
                crate::health_types::HotspotEntry {
                    path: root.join("src/owned.ts"),
                    score: 80.0,
                    commits: 25,
                    weighted_commits: 22.0,
                    lines_added: 300,
                    lines_deleted: 100,
                    complexity_density: 0.6,
                    fan_in: 5,
                    trend: fallow_core::churn::ChurnTrend::Stable,
                    ownership: Some(OwnershipMetrics {
                        bus_factor: 1,
                        contributor_count: 2,
                        top_contributor: ContributorEntry {
                            identifier: "alice".to_string(),
                            format: ContributorIdentifierFormat::Raw,
                            share: 0.8,
                            stale_days: 5,
                            commits: 20,
                        },
                        recent_contributors: vec![],
                        suggested_reviewers: vec![],
                        declared_owner: Some("@team/core".to_string()),
                        unowned: Some(false),
                        ownership_state: OwnershipState::Active,
                        drift: false,
                        drift_reason: None,
                    }),
                    is_test_path: false,
                }
                .into(),
            ],
            hotspot_summary: None,
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // Ownership widens the table with Bus/Top/Owner/Notes columns
        assert!(md.contains("| Bus | Top | Owner | Notes |"));
        assert!(md.contains("`alice` (80%)"));
        assert!(md.contains("@team/core"));
    }

    // -------------------------------------------------------------------------
    // health trend section (lines 1456-1501)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_trend_section() {
        use crate::health_types::{HealthTrend, TrendDirection, TrendMetric, TrendPoint};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            health_trend: Some(HealthTrend {
                compared_to: TrendPoint {
                    timestamp: "2026-05-01T00:00:00Z".to_string(),
                    git_sha: Some("abc1234".to_string()),
                    score: Some(70.0),
                    grade: Some("B".to_string()),
                    coverage_model: None,
                    snapshot_schema_version: None,
                },
                metrics: vec![TrendMetric {
                    name: "score",
                    label: "Health Score",
                    previous: 70.0,
                    current: 82.0,
                    delta: 12.0,
                    direction: TrendDirection::Improving,
                    unit: "pts",
                    previous_count: None,
                    current_count: None,
                }],
                snapshots_loaded: 3,
                overall_direction: TrendDirection::Improving,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Trend (vs 2026-05-01 (abc1234))"));
        assert!(md.contains("| Metric | Previous | Current | Delta | Direction |"));
        assert!(md.contains("Health Score"));
        assert!(md.contains("+12"));
        assert!(md.contains("improving"));
        assert!(md.contains("3 snapshots available"));
    }

    #[test]
    fn health_markdown_trend_single_snapshot_singular() {
        use crate::health_types::{HealthTrend, TrendDirection, TrendPoint};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            health_trend: Some(HealthTrend {
                compared_to: TrendPoint {
                    timestamp: "2026-06-01T12:00:00Z".to_string(),
                    git_sha: None,
                    score: None,
                    grade: None,
                    coverage_model: None,
                    snapshot_schema_version: None,
                },
                metrics: vec![],
                snapshots_loaded: 1,
                overall_direction: TrendDirection::Stable,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 2,
                functions_analyzed: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("1 snapshot available"));
        assert!(!md.contains("1 snapshots available"));
    }

    // -------------------------------------------------------------------------
    // trend metric with % unit (lines 1506-1507, 1516-1517)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_trend_metric_percent_unit() {
        use crate::health_types::{HealthTrend, TrendDirection, TrendMetric, TrendPoint};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            health_trend: Some(HealthTrend {
                compared_to: TrendPoint {
                    timestamp: "2026-04-01T00:00:00Z".to_string(),
                    git_sha: None,
                    score: None,
                    grade: None,
                    coverage_model: None,
                    snapshot_schema_version: None,
                },
                metrics: vec![TrendMetric {
                    name: "dead_file_pct",
                    label: "Dead Files",
                    previous: 5.0,
                    current: 3.0,
                    delta: -2.0,
                    direction: TrendDirection::Improving,
                    unit: "%",
                    previous_count: None,
                    current_count: None,
                }],
                snapshots_loaded: 2,
                overall_direction: TrendDirection::Improving,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // Percent values include the % sign
        assert!(md.contains("5.0%"));
        assert!(md.contains("3.0%"));
        assert!(md.contains("-2.0%"));
    }

    // -------------------------------------------------------------------------
    // runtime coverage with watermark (lines 1380-1382)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_runtime_coverage_with_watermark() {
        use crate::health_types::{
            RuntimeCoverageReport, RuntimeCoverageReportVerdict, RuntimeCoverageSchemaVersion,
            RuntimeCoverageSummary, RuntimeCoverageWatermark,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
                verdict: RuntimeCoverageReportVerdict::Clean,
                signals: vec![],
                summary: RuntimeCoverageSummary {
                    functions_tracked: 100,
                    functions_hit: 90,
                    functions_unhit: 10,
                    functions_untracked: 5,
                    coverage_percent: 90.0,
                    trace_count: 5000,
                    period_days: 7,
                    deployments_seen: 2,
                    ..Default::default()
                },
                findings: vec![],
                hot_paths: vec![],
                blast_radius: vec![],
                importance: vec![],
                watermark: Some(RuntimeCoverageWatermark::TrialExpired),
                warnings: vec![],
                actionable: true,
                actionability_reason: None,
                actionability_verdict: None,
                provenance: crate::health_types::RuntimeCoverageProvenance::default(),
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 30,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## Runtime Coverage"));
        assert!(md.contains("- Watermark: trial-expired"));
    }

    // -------------------------------------------------------------------------
    // runtime coverage hot paths (lines 1428-1453)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_runtime_coverage_hot_paths() {
        use crate::health_types::{
            RuntimeCoverageHotPath, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
            RuntimeCoverageSchemaVersion, RuntimeCoverageSummary,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            runtime_coverage: Some(RuntimeCoverageReport {
                schema_version: RuntimeCoverageSchemaVersion::V1,
                verdict: RuntimeCoverageReportVerdict::HotPathTouched,
                signals: vec![],
                summary: RuntimeCoverageSummary {
                    functions_tracked: 50,
                    functions_hit: 40,
                    functions_unhit: 10,
                    functions_untracked: 0,
                    coverage_percent: 80.0,
                    trace_count: 2000,
                    period_days: 3,
                    deployments_seen: 1,
                    ..Default::default()
                },
                findings: vec![],
                hot_paths: vec![RuntimeCoverageHotPath {
                    id: "fallow:hot:deadbeef".to_string(),
                    stable_id: None,
                    path: root.join("src/service.ts"),
                    function: "handleRequest".to_string(),
                    line: 42,
                    end_line: 80,
                    invocations: 12_345,
                    percentile: 99,
                    actions: vec![],
                }],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                warnings: vec![],
                actionable: true,
                actionability_reason: None,
                actionability_verdict: None,
                provenance: crate::health_types::RuntimeCoverageProvenance::default(),
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("| ID | Hot path | Function |"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("fallow:hot:deadbeef"));
        assert!(normalized.contains("handleRequest"));
        assert!(normalized.contains("12345"));
    }

    // -------------------------------------------------------------------------
    // CSS analytics section (lines 1100-1270)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_css_analytics_basic() {
        use crate::health_types::{CssAnalyticsReport, CssAnalyticsSummary};
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            css_analytics: Some(CssAnalyticsReport {
                files: vec![],
                summary: CssAnalyticsSummary {
                    files_analyzed: 3,
                    total_rules: 120,
                    total_declarations: 600,
                    important_declarations: 10,
                    empty_rules: 2,
                    max_nesting_depth: 4,
                    unique_colors: 15,
                    unique_font_sizes: 8,
                    unique_z_indexes: 3,
                    unique_box_shadows: 2,
                    unique_border_radii: 5,
                    unique_line_heights: 4,
                    custom_properties_defined: 20,
                    custom_properties_unreferenced: 3,
                    custom_properties_undefined: 1,
                    keyframes_defined: 5,
                    keyframes_unreferenced: 2,
                    keyframes_undefined: 1,
                    scoped_unused_classes: 4,
                    duplicate_declaration_blocks: 1,
                    duplicate_declarations_total: 8,
                    tailwind_arbitrary_values: 3,
                    tailwind_arbitrary_value_uses: 7,
                    unused_property_registrations: 0,
                    unused_layers: 1,
                    unresolved_class_references: 2,
                    unreferenced_css_classes: 0,
                    unused_font_faces: 1,
                    unused_theme_tokens: 0,
                    font_size_units_used: 2,
                    notable_truncated_files: 0,
                },
                scoped_unused: vec![],
                unreferenced_keyframes: vec![],
                undefined_keyframes: vec![],
                duplicate_declaration_blocks: vec![],
                tailwind_arbitrary_values: vec![],
                unused_at_rules: vec![],
                unresolved_class_references: vec![],
                unreferenced_css_classes: vec![],
                unused_font_faces: vec![],
                unused_theme_tokens: vec![],
                font_size_unit_mix: None,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 5,
                functions_analyzed: 20,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("## CSS Health"));
        assert!(md.contains("Stylesheets: 3 | Rules: 120"));
        assert!(md.contains("Value sprawl:"));
        assert!(md.contains("Candidates:"));
    }

    #[test]
    fn health_markdown_css_analytics_undefined_keyframes() {
        use crate::health_types::{
            CssAnalyticsReport, CssAnalyticsSummary, CssCandidateAction, CssCandidateActionType,
            UndefinedKeyframes,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            css_analytics: Some(CssAnalyticsReport {
                files: vec![],
                summary: CssAnalyticsSummary {
                    files_analyzed: 1,
                    total_rules: 10,
                    total_declarations: 50,
                    important_declarations: 0,
                    empty_rules: 0,
                    max_nesting_depth: 2,
                    unique_colors: 5,
                    unique_font_sizes: 2,
                    unique_z_indexes: 1,
                    unique_box_shadows: 0,
                    unique_border_radii: 2,
                    unique_line_heights: 2,
                    custom_properties_defined: 5,
                    custom_properties_unreferenced: 0,
                    custom_properties_undefined: 0,
                    keyframes_defined: 2,
                    keyframes_unreferenced: 0,
                    keyframes_undefined: 1,
                    scoped_unused_classes: 0,
                    duplicate_declaration_blocks: 0,
                    duplicate_declarations_total: 0,
                    tailwind_arbitrary_values: 0,
                    tailwind_arbitrary_value_uses: 0,
                    unused_property_registrations: 0,
                    unused_layers: 0,
                    unresolved_class_references: 0,
                    unreferenced_css_classes: 0,
                    unused_font_faces: 0,
                    unused_theme_tokens: 0,
                    font_size_units_used: 1,
                    notable_truncated_files: 0,
                },
                scoped_unused: vec![],
                unreferenced_keyframes: vec![],
                undefined_keyframes: vec![UndefinedKeyframes {
                    name: "slide-in".to_string(),
                    path: "src/styles.css".to_string(),
                    actions: vec![CssCandidateAction {
                        kind: CssCandidateActionType::VerifyUnused,
                        auto_fixable: false,
                        description: "Verify unused keyframe".to_string(),
                        command: None,
                    }],
                }],
                duplicate_declaration_blocks: vec![],
                tailwind_arbitrary_values: vec![],
                unused_at_rules: vec![],
                unresolved_class_references: vec![],
                unreferenced_css_classes: vec![],
                unused_font_faces: vec![],
                unused_theme_tokens: vec![],
                font_size_unit_mix: None,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 2,
                functions_analyzed: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("Undefined @keyframes"));
        assert!(md.contains("`slide-in`"));
    }

    #[test]
    fn health_markdown_css_analytics_tailwind_and_class_candidates() {
        use crate::health_types::{
            CssAnalyticsReport, CssAnalyticsSummary, CssCandidateAction, CssCandidateActionType,
            TailwindArbitraryValue, UnreferencedCssClass, UnresolvedClassReference,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            css_analytics: Some(CssAnalyticsReport {
                files: vec![],
                summary: CssAnalyticsSummary {
                    files_analyzed: 1,
                    total_rules: 5,
                    total_declarations: 20,
                    important_declarations: 0,
                    empty_rules: 0,
                    max_nesting_depth: 1,
                    unique_colors: 2,
                    unique_font_sizes: 1,
                    unique_z_indexes: 0,
                    unique_box_shadows: 0,
                    unique_border_radii: 1,
                    unique_line_heights: 1,
                    custom_properties_defined: 0,
                    custom_properties_unreferenced: 0,
                    custom_properties_undefined: 0,
                    keyframes_defined: 0,
                    keyframes_unreferenced: 0,
                    keyframes_undefined: 0,
                    scoped_unused_classes: 0,
                    duplicate_declaration_blocks: 0,
                    duplicate_declarations_total: 0,
                    tailwind_arbitrary_values: 2,
                    tailwind_arbitrary_value_uses: 4,
                    unused_property_registrations: 0,
                    unused_layers: 0,
                    unresolved_class_references: 1,
                    unreferenced_css_classes: 1,
                    unused_font_faces: 0,
                    unused_theme_tokens: 0,
                    font_size_units_used: 1,
                    notable_truncated_files: 0,
                },
                scoped_unused: vec![],
                unreferenced_keyframes: vec![],
                undefined_keyframes: vec![],
                duplicate_declaration_blocks: vec![],
                tailwind_arbitrary_values: vec![TailwindArbitraryValue {
                    value: "w-[42px]".to_string(),
                    count: 3,
                    path: "src/App.tsx".to_string(),
                    line: 7,
                    actions: vec![CssCandidateAction {
                        kind: CssCandidateActionType::VerifyUnused,
                        auto_fixable: false,
                        description: "Replace with a scale token".to_string(),
                        command: None,
                    }],
                }],
                unused_at_rules: vec![],
                unresolved_class_references: vec![UnresolvedClassReference {
                    class: "btn-primry".to_string(),
                    suggestion: "btn-primary".to_string(),
                    path: "src/index.html".to_string(),
                    line: 15,
                    actions: vec![],
                }],
                unreferenced_css_classes: vec![UnreferencedCssClass {
                    class: "old-header".to_string(),
                    path: "src/styles.css".to_string(),
                    line: 22,
                    actions: vec![],
                }],
                unused_font_faces: vec![],
                unused_theme_tokens: vec![],
                font_size_unit_mix: None,
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 2,
                functions_analyzed: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("Top Tailwind arbitrary values"));
        assert!(md.contains("`w-[42px]` (3x)"));
        assert!(md.contains("Likely class typos"));
        assert!(md.contains("`btn-primry` -> `btn-primary`"));
        assert!(md.contains("Unreferenced global classes"));
        assert!(md.contains("`.old-header`"));
    }

    #[test]
    fn health_markdown_css_analytics_font_candidates() {
        use crate::health_types::{
            CssAnalyticsReport, CssAnalyticsSummary, CssCandidateAction, CssCandidateActionType,
            CssNotationConsistency, CssNotationCount, UnusedFontFace, UnusedThemeToken,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            css_analytics: Some(CssAnalyticsReport {
                files: vec![],
                summary: CssAnalyticsSummary {
                    files_analyzed: 1,
                    total_rules: 5,
                    total_declarations: 20,
                    important_declarations: 0,
                    empty_rules: 0,
                    max_nesting_depth: 1,
                    unique_colors: 2,
                    unique_font_sizes: 3,
                    unique_z_indexes: 0,
                    unique_box_shadows: 0,
                    unique_border_radii: 1,
                    unique_line_heights: 1,
                    custom_properties_defined: 0,
                    custom_properties_unreferenced: 0,
                    custom_properties_undefined: 0,
                    keyframes_defined: 0,
                    keyframes_unreferenced: 0,
                    keyframes_undefined: 0,
                    scoped_unused_classes: 0,
                    duplicate_declaration_blocks: 0,
                    duplicate_declarations_total: 0,
                    tailwind_arbitrary_values: 0,
                    tailwind_arbitrary_value_uses: 0,
                    unused_property_registrations: 0,
                    unused_layers: 0,
                    unresolved_class_references: 0,
                    unreferenced_css_classes: 0,
                    unused_font_faces: 1,
                    unused_theme_tokens: 1,
                    font_size_units_used: 2,
                    notable_truncated_files: 0,
                },
                scoped_unused: vec![],
                unreferenced_keyframes: vec![],
                undefined_keyframes: vec![],
                duplicate_declaration_blocks: vec![],
                tailwind_arbitrary_values: vec![],
                unused_at_rules: vec![],
                unresolved_class_references: vec![],
                unreferenced_css_classes: vec![],
                unused_font_faces: vec![UnusedFontFace {
                    family: "OldFont".to_string(),
                    path: "src/fonts.css".to_string(),
                    actions: vec![CssCandidateAction {
                        kind: CssCandidateActionType::VerifyUnused,
                        auto_fixable: false,
                        description: "Check if used from JS".to_string(),
                        command: None,
                    }],
                }],
                unused_theme_tokens: vec![UnusedThemeToken {
                    token: "--color-stale".to_string(),
                    namespace: "color".to_string(),
                    path: "src/theme.css".to_string(),
                    line: 10,
                    actions: vec![],
                }],
                font_size_unit_mix: Some(CssNotationConsistency {
                    axis: "Font sizes".to_string(),
                    notations: vec![
                        CssNotationCount {
                            notation: "rem".to_string(),
                            count: 8,
                        },
                        CssNotationCount {
                            notation: "px".to_string(),
                            count: 3,
                        },
                    ],
                    actions: vec![],
                }),
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 2,
                functions_analyzed: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("Unused @font-face"));
        assert!(md.contains("`OldFont` (src/fonts.css)"));
        assert!(md.contains("Unused @theme tokens"));
        assert!(md.contains("`--color-stale` (src/theme.css:10)"));
        assert!(md.contains("Font sizes mix 2 units"));
        assert!(md.contains("8 rem"));
        assert!(md.contains("3 px"));
    }

    // -------------------------------------------------------------------------
    // metric legend (lines 2010-2054)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_metric_legend_shown_when_relevant() {
        use crate::health_types::FileHealthScore;
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            file_scores: vec![FileHealthScore {
                path: root.join("src/x.ts"),
                fan_in: 1,
                fan_out: 1,
                dead_code_ratio: 0.0,
                complexity_density: 0.1,
                maintainability_index: 80.0,
                total_cyclomatic: 5,
                total_cognitive: 4,
                function_count: 2,
                lines: 50,
                crap_max: 5.0,
                crap_above_threshold: 0,
            }],
            summary: crate::health_types::HealthSummary {
                files_analyzed: 1,
                functions_analyzed: 2,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("<details><summary>Metric definitions</summary>"));
        assert!(md.contains("**MI**"));
        assert!(md.contains("[Full metric reference]"));
    }

    #[test]
    fn health_markdown_metric_legend_not_shown_without_sections() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/y.ts"),
                    name: "noLegend".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 22,
                    cognitive: 18,
                    line_count: 30,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // Findings only, no file_scores / hotspots / targets / coverage_gaps
        // so the legend is suppressed
        assert!(!md.contains("<details>"));
    }

    // -------------------------------------------------------------------------
    // coverage_gaps section with single file (plural/singular wording)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_coverage_gaps_files_section_singular() {
        use crate::health_types::{
            CoverageGapSummary, CoverageGaps, UntestedFile, UntestedFileFinding,
        };
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            coverage_gaps: Some(CoverageGaps {
                summary: CoverageGapSummary {
                    runtime_files: 1,
                    covered_files: 0,
                    file_coverage_pct: 0.0,
                    untested_files: 1,
                    untested_exports: 0,
                },
                files: vec![UntestedFileFinding::with_actions(
                    UntestedFile {
                        path: root.join("src/single.ts"),
                        value_export_count: 1,
                    },
                    &root,
                )],
                exports: vec![],
            }),
            summary: crate::health_types::HealthSummary {
                files_analyzed: 2,
                functions_analyzed: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        assert!(md.contains("#### Files"));
        let normalized = md.replace('\\', "/");
        // single export uses no plural "s"
        assert!(normalized.contains("`src/single.ts` (1 value export)"));
    }

    // -------------------------------------------------------------------------
    // health markdown with "shown" subset vs total (lines 1620-1626)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_findings_subset_shown() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/big.ts"),
                    name: "bigFn".to_string(),
                    line: 5,
                    col: 0,
                    cyclomatic: 30,
                    cognitive: 25,
                    line_count: 100,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
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
                files_analyzed: 2,
                functions_analyzed: 10,
                // total > shown triggers the "(N shown)" suffix
                functions_above_threshold: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // 5 total but only 1 shown
        assert!(md.contains("## Fallow: 5 high complexity functions (1 shown)"));
    }

    // -------------------------------------------------------------------------
    // unused type export with is_type_only: true
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_unused_type_export_format() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: root.join("src/types.ts"),
                export_name: "MyInterface".to_string(),
                is_type_only: true,
                line: 3,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let md = build_markdown(&results, &root);
        assert!(md.contains("### Unused type exports (1)"));
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("- `src/types.ts`"));
        assert!(normalized.contains(":3 `MyInterface`"));
    }

    // -------------------------------------------------------------------------
    // dependency with used_in_workspaces (lines 875-882)
    // -------------------------------------------------------------------------

    #[test]
    fn markdown_dep_with_multiple_workspace_consumers() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "shared-utils".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/core/package.json"),
                line: 3,
                used_in_workspaces: vec![root.join("packages/app"), root.join("packages/admin")],
            }));
        let md = build_markdown(&results, &root);
        let normalized = md.replace('\\', "/");
        assert!(normalized.contains("imported in packages/app, packages/admin"));
    }

    // -------------------------------------------------------------------------
    // <component> name in findings (lines 1586-1588 / <component> branch)
    // -------------------------------------------------------------------------

    #[test]
    fn health_markdown_component_rollup_entry_label() {
        let root = PathBuf::from("/project");
        let report = crate::health_types::HealthReport {
            findings: vec![
                crate::health_types::ComplexityViolation {
                    path: root.join("src/Card.vue"),
                    name: "<component>".to_string(),
                    line: 1,
                    col: 0,
                    cyclomatic: 10,
                    cognitive: 14,
                    line_count: 60,
                    param_count: 0,
                    react_hook_count: 0,
                    react_jsx_max_depth: 0,
                    react_prop_count: 0,
                    react_hook_profile: None,
                    exceeded: crate::health_types::ExceededThreshold::Cognitive,
                    severity: crate::health_types::FindingSeverity::Moderate,
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
                ..Default::default()
            },
            ..Default::default()
        };
        let md = build_health_markdown(&report, &root);
        // The table header says "Entry" for synthetic names
        assert!(md.contains("| File | Entry |"));
        assert!(md.contains("`<component> (component rollup)`"));
    }
}
