use fallow_types::results::AnalysisResults;

/// Pushes a pluralized `"<count> <label>"` part onto `parts` when `count > 0`.
///
/// The noun must be the last word of `label`: a trailing `s` or `ies` is read
/// as the plural suffix. Use [`push_summary_part_forms`] for other labels.
fn push_summary_part(parts: &mut Vec<String>, count: usize, label: &str) {
    if let Some(stem) = label.strip_suffix("ies") {
        push_summary_part_forms(parts, count, &format!("{stem}y"), label);
    } else if let Some(singular) = label.strip_suffix('s') {
        push_summary_part_forms(parts, count, singular, label);
    } else {
        push_summary_part_forms(parts, count, label, &format!("{label}s"));
    }
}

/// Pushes `"<count> <singular|plural>"` onto `parts` when `count > 0`.
fn push_summary_part_forms(parts: &mut Vec<String>, count: usize, singular: &str, plural: &str) {
    match count {
        0 => {}
        1 => parts.push(format!("1 {singular}")),
        _ => parts.push(format!("{count} {plural}")),
    }
}

/// Counts distinct file pairs participating in duplicate-export findings.
fn count_duplicate_export_pairs(results: &AnalysisResults) -> usize {
    let mut pair_set = rustc_hash::FxHashSet::default();
    for dup in &results.duplicate_exports {
        let dup = &dup.export;
        if dup.locations.len() >= 2 {
            let mut paths: Vec<&std::path::Path> =
                dup.locations.iter().map(|l| l.path.as_path()).collect();
            paths.sort();
            paths.dedup();
            if paths.len() >= 2 {
                pair_set.insert((paths[0].to_path_buf(), paths[1].to_path_buf()));
            }
        }
    }
    pair_set.len()
}

/// Appends the unused-code and dependency summary parts.
fn push_summary_core_parts(
    parts: &mut Vec<String>,
    results: &AnalysisResults,
    suppressed_exports: usize,
    suppressed_types: usize,
) {
    push_summary_part(parts, results.unused_files.len(), "file");
    push_summary_part(
        parts,
        results
            .unused_exports
            .len()
            .saturating_sub(suppressed_exports),
        "export",
    );
    push_summary_part(
        parts,
        results.unused_types.len().saturating_sub(suppressed_types),
        "type",
    );
    push_summary_part(
        parts,
        results.private_type_leaks.len(),
        "private type leaks",
    );
    push_summary_part_forms(
        parts,
        results.deprecated_exports_in_use.len(),
        "deprecated export in use",
        "deprecated exports in use",
    );
    push_summary_member_parts(parts, results);
    push_summary_dependency_parts(parts, results);
    push_summary_graph_parts(parts, results);
}

fn push_summary_member_parts(parts: &mut Vec<String>, results: &AnalysisResults) {
    push_summary_part(parts, results.unused_enum_members.len(), "enum members");
    push_summary_part(parts, results.unused_class_members.len(), "class members");
    push_summary_part(parts, results.unused_store_members.len(), "store members");
}

fn push_summary_dependency_parts(parts: &mut Vec<String>, results: &AnalysisResults) {
    push_summary_part(
        parts,
        results.unused_dependencies.len(),
        "unused dependencies",
    );
    push_summary_part(
        parts,
        results.unused_dev_dependencies.len() + results.unused_optional_dependencies.len(),
        "dev/optional dependencies",
    );
    push_summary_part(
        parts,
        results.unresolved_imports.len(),
        "unresolved imports",
    );
    push_summary_part(
        parts,
        results.unlisted_dependencies.len(),
        "unlisted dependencies",
    );
    push_summary_part(
        parts,
        count_duplicate_export_pairs(results),
        "duplicate pair",
    );
    push_summary_part(
        parts,
        results.type_only_dependencies.len(),
        "type-only dependencies",
    );
    push_summary_part(
        parts,
        results.test_only_dependencies.len(),
        "test-only dependencies",
    );
    push_summary_part(
        parts,
        results.misconfigured_dependency_overrides.len(),
        "misconfigured dependency overrides",
    );
}

fn push_summary_graph_parts(parts: &mut Vec<String>, results: &AnalysisResults) {
    push_summary_part_forms(
        parts,
        results.dev_dependencies_in_production.len(),
        "dev dependency in production",
        "dev dependencies in production",
    );
    push_summary_part(
        parts,
        results.circular_dependencies.len(),
        "circular dependencies",
    );
    push_summary_part(parts, results.re_export_cycles.len(), "re-export cycles");
    push_summary_part(parts, results.package_cycles.len(), "package cycles");
    push_summary_part(
        parts,
        results.boundary_violations.len(),
        "boundary violations",
    );
    push_summary_part(
        parts,
        results.boundary_coverage_violations.len(),
        "boundary coverage violations",
    );
    push_summary_part(
        parts,
        results.boundary_call_violations.len(),
        "boundary call violations",
    );
    push_summary_part(parts, results.policy_violations.len(), "policy violations");
}

/// Appends the framework-specific summary parts.
fn push_summary_framework_parts(parts: &mut Vec<String>, results: &AnalysisResults) {
    push_summary_part(
        parts,
        results.unprovided_injects.len(),
        "unprovided injects",
    );
    push_summary_part(
        parts,
        results.unrendered_components.len(),
        "unrendered components",
    );
    push_summary_part(
        parts,
        results.unused_component_props.len(),
        "unused component props",
    );
    push_summary_part(
        parts,
        results.absent_component_props.len(),
        "optional prop review candidates",
    );
    push_summary_part(
        parts,
        results.unused_component_emits.len(),
        "unused component emits",
    );
    push_summary_part(
        parts,
        results.unused_component_inputs.len(),
        "unused component inputs",
    );
    push_summary_part(
        parts,
        results.unused_component_outputs.len(),
        "unused component outputs",
    );
    push_summary_part(
        parts,
        results.unused_svelte_events.len(),
        "unused Svelte events",
    );
    push_summary_part(
        parts,
        results.unused_effect_layers.len(),
        "unused Effect layers",
    );
    push_summary_part(
        parts,
        results.unused_tagged_errors.len(),
        "unused tagged errors",
    );
    push_summary_part(
        parts,
        results.unproduced_catch_tags.len(),
        "unproduced catch tags",
    );
    push_summary_part(
        parts,
        results.unused_server_actions.len(),
        "unused server actions",
    );
    push_summary_part(
        parts,
        results.unused_load_data_keys.len(),
        "unused load data keys",
    );
    push_summary_part(
        parts,
        results.stale_suppressions.len(),
        "stale suppressions",
    );
}

/// Build a one-line summary footer showing counts per issue type.
///
/// `suppressed_exports` / `suppressed_types` are subtracted from the raw
/// counts so the footer reflects the *visible* items when export suppression
/// is active (exports from unused files are hidden).
pub(super) fn build_summary_footer(
    results: &AnalysisResults,
    suppressed_exports: usize,
    suppressed_types: usize,
) -> String {
    let mut parts = Vec::new();
    push_summary_core_parts(&mut parts, results, suppressed_exports, suppressed_types);
    push_summary_framework_parts(&mut parts, results);
    parts.join(" \u{00b7} ")
}
