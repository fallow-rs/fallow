use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, WorkspaceInfo, discover_workspaces};
use globset::Glob;
use rustc_hash::FxHashSet;

use crate::error::emit_error;

/// Scope results to the union of the given workspace roots.
///
/// The full cross-workspace graph is still built (so cross-package imports
/// are resolved), but only issues from files under any `ws_root` are reported.
///
/// Any issue whose path starts with one of the roots passes; dependency-level
/// issues are scoped to the matching workspaces' own `package.json` files.
pub fn filter_to_workspaces(
    results: &mut fallow_core::results::AnalysisResults,
    ws_roots: &[PathBuf],
) {
    let any_under = |p: &Path| ws_roots.iter().any(|r| p.starts_with(r));
    let pkg_jsons: Vec<PathBuf> = ws_roots.iter().map(|r| r.join("package.json")).collect();
    let in_pkg_jsons = |p: &Path| pkg_jsons.iter().any(|pkg| p == pkg);

    filter_workspace_source_findings(results, &any_under);
    filter_workspace_dependency_findings(results, &any_under, &in_pkg_jsons);
    filter_workspace_graph_findings(results, &any_under);
    filter_workspace_policy_findings(results, &any_under);
}

fn filter_workspace_source_findings(
    results: &mut fallow_core::results::AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results.unused_files.retain(|f| any_under(&f.file.path));
    results.unused_exports.retain(|e| any_under(&e.export.path));
    results.unused_types.retain(|e| any_under(&e.export.path));
    results
        .private_type_leaks
        .retain(|e| any_under(&e.leak.path));
    results
        .unused_enum_members
        .retain(|m| any_under(&m.member.path));
    results
        .unused_class_members
        .retain(|m| any_under(&m.member.path));
    results
        .unused_store_members
        .retain(|m| any_under(&m.member.path));
    results
        .unprovided_injects
        .retain(|f| any_under(&f.inject.path));
    results
        .unrendered_components
        .retain(|c| any_under(&c.component.path));
    results
        .unused_component_props
        .retain(|p| any_under(&p.prop.path));
    results
        .unused_component_emits
        .retain(|e| any_under(&e.emit.path));
    results
        .unused_component_inputs
        .retain(|i| any_under(&i.input.path));
    results
        .unused_component_outputs
        .retain(|o| any_under(&o.output.path));
    results
        .unused_server_actions
        .retain(|a| any_under(&a.action.path));
    results
        .unused_load_data_keys
        .retain(|k| any_under(&k.key.path));
    results
        .unresolved_imports
        .retain(|i| any_under(&i.import.path));
}

fn filter_workspace_dependency_findings(
    results: &mut fallow_core::results::AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
    in_pkg_jsons: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_dependencies
        .retain(|d| in_pkg_jsons(&d.dep.path));
    results
        .unused_dev_dependencies
        .retain(|d| in_pkg_jsons(&d.dep.path));
    results
        .unused_optional_dependencies
        .retain(|d| in_pkg_jsons(&d.dep.path));
    results
        .type_only_dependencies
        .retain(|d| in_pkg_jsons(&d.dep.path));
    results
        .test_only_dependencies
        .retain(|d| in_pkg_jsons(&d.dep.path));

    results
        .unlisted_dependencies
        .retain(|d| d.dep.imported_from.iter().any(|s| any_under(&s.path)));
    results.unused_dependency_overrides.clear();
    results.misconfigured_dependency_overrides.clear();
}

fn filter_workspace_graph_findings(
    results: &mut fallow_core::results::AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    for dup in &mut results.duplicate_exports {
        dup.export.locations.retain(|loc| any_under(&loc.path));
    }
    results
        .duplicate_exports
        .retain(|d| d.export.locations.len() >= 2);

    results
        .circular_dependencies
        .retain(|c| c.cycle.files.iter().any(|f| any_under(f)));

    results
        .re_export_cycles
        .retain(|c| c.cycle.files.iter().any(|f| any_under(f)));
}

fn filter_workspace_policy_findings(
    results: &mut fallow_core::results::AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .boundary_violations
        .retain(|v| any_under(&v.violation.from_path));
    results
        .boundary_coverage_violations
        .retain(|v| any_under(&v.violation.path));
    results
        .boundary_call_violations
        .retain(|v| any_under(&v.violation.path));
    results
        .policy_violations
        .retain(|v| any_under(&v.violation.path));

    results.stale_suppressions.retain(|s| any_under(&s.path));

    results.security_findings.retain(|f| any_under(&f.path));
    results
        .security_unresolved_callee_diagnostics
        .retain(|d| any_under(&d.path));

    results.unused_catalog_entries.clear();
    results.empty_catalog_groups.clear();
    results
        .unresolved_catalog_references
        .retain(|r| any_under(&r.reference.path));

    results
        .invalid_client_exports
        .retain(|e| any_under(&e.export.path));

    results
        .mixed_client_server_barrels
        .retain(|b| any_under(&b.barrel.path));

    results
        .misplaced_directives
        .retain(|d| any_under(&d.directive_site.path));

    results
        .route_collisions
        .retain(|c| any_under(&c.collision.path));

    results
        .dynamic_segment_name_conflicts
        .retain(|c| any_under(&c.conflict.path));
}

/// Resolve `--workspace <patterns...>` to a set of workspace roots, or exit with
/// an error.
///
/// Patterns support three forms:
/// - Exact package name (`web`) or relative workspace path (`apps/web`)
/// - Glob (`apps/*`, `@scope/*`), matched against BOTH `ws.name` AND the path
///   relative to the repo root; either match counts
/// - Negation (`!apps/legacy`), excludes matching workspaces from the result
///
/// Combination rules (gitignore-style):
/// - Only positive patterns: include matches
/// - Only negative patterns: include all workspaces then remove matches
/// - Mixed: start from union of positive matches, then remove negative matches
///
/// Reserved prefixes for future pnpm-style graph selectors: `^`, `+`, `...`
/// (not yet implemented; reject or repurpose only after panel review).
pub fn resolve_workspace_filters(
    root: &Path,
    patterns: &[String],
    output: OutputFormat,
) -> Result<Vec<PathBuf>, ExitCode> {
    let workspaces = discover_workspaces(root);
    if workspaces.is_empty() {
        let joined = patterns
            .iter()
            .map(|p| format!("'{p}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!(
            "--workspace {joined} specified but no workspaces found. \
             Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, \
             or tsconfig.json has \"references\"."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let rel_paths: Vec<String> = workspaces
        .iter()
        .map(|ws| relative_workspace_path(&ws.root, root))
        .collect();

    let (positive, negative) = split_patterns(patterns);

    let mut matched: FxHashSet<usize> = FxHashSet::default();
    let mut unmatched: Vec<String> = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pat in &positive {
            let hits = find_matches(pat, &workspaces, &rel_paths, output)?;
            if hits.is_empty() {
                unmatched.push(pat.to_string());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        let quoted: Vec<String> = unmatched.iter().map(|p| format!("'{p}'")).collect();
        let available = format_available_workspaces(&workspaces);
        let msg = format!(
            "--workspace: no workspaces matched pattern{}: {}. Available: {available}",
            if unmatched.len() == 1 { "" } else { "s" },
            quoted.join(", "),
        );
        return Err(emit_error(&msg, 2, output));
    }

    for pat in &negative {
        let hits = find_matches(pat, &workspaces, &rel_paths, output)?;
        for idx in hits {
            matched.remove(&idx);
        }
    }

    if matched.is_empty() {
        let include_desc = if positive.is_empty() {
            "<all>".to_owned()
        } else {
            positive
                .iter()
                .map(|p| format!("'{p}'"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let exclude_desc = negative
            .iter()
            .map(|p| format!("'{p}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!(
            "--workspace: all workspaces were excluded by the filter. \
             Included: {include_desc}. Excluded: {exclude_desc}."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let mut roots: Vec<PathBuf> = matched
        .into_iter()
        .map(|i| workspaces[i].root.clone())
        .collect();
    roots.sort();
    Ok(roots)
}

/// Format the workspace list for inclusion in error messages. Caps the
/// displayed names so large monorepos (30+ workspaces) don't produce an
/// unreadable wall of text.
fn format_available_workspaces(workspaces: &[WorkspaceInfo]) -> String {
    const MAX_SHOWN: usize = 10;
    let total = workspaces.len();
    if total <= MAX_SHOWN {
        return workspaces
            .iter()
            .map(|ws| ws.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let shown: Vec<&str> = workspaces
        .iter()
        .take(MAX_SHOWN)
        .map(|ws| ws.name.as_str())
        .collect();
    format!(
        "{shown_list}, ... and {remaining} more ({total} total)",
        shown_list = shown.join(", "),
        remaining = total - MAX_SHOWN,
    )
}

/// Compute the workspace path relative to the repo root, normalized to forward
/// slashes so glob patterns written with `/` work on Windows.
fn relative_workspace_path(ws_root: &Path, root: &Path) -> String {
    ws_root
        .strip_prefix(root)
        .unwrap_or(ws_root)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Split comma-separated patterns into (positive, negative). Whitespace-trimmed;
/// empty entries ignored; leading `!` marks negation.
fn split_patterns(patterns: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for raw in patterns {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(neg) = trimmed.strip_prefix('!') {
            let neg = neg.trim();
            if !neg.is_empty() {
                negative.push(neg);
            }
        } else {
            positive.push(trimmed);
        }
    }
    (positive, negative)
}

/// Find workspace indices matching `pattern`. Exact-name and exact-relative-path
/// matches short-circuit before globbing, so literal names containing glob
/// metacharacters (e.g. `web-[staging]`) still work.
fn find_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
    output: OutputFormat,
) -> Result<Vec<usize>, ExitCode> {
    if let Some(idx) = workspaces.iter().position(|ws| ws.name == pattern) {
        return Ok(vec![idx]);
    }
    if let Some(idx) = rel_paths.iter().position(|p| p == pattern) {
        return Ok(vec![idx]);
    }

    let glob = Glob::new(pattern).map_err(|e| {
        let msg = format!("--workspace: invalid pattern '{pattern}': {e}");
        emit_error(&msg, 2, output)
    })?;
    let matcher = glob.compile_matcher();

    let mut hits = Vec::new();
    for (idx, ws) in workspaces.iter().enumerate() {
        if matcher.is_match(&ws.name) || matcher.is_match(&rel_paths[idx]) {
            hits.push(idx);
        }
    }
    Ok(hits)
}

pub use fallow_core::changed_files::{
    filter_results_by_changed_files as filter_changed_files, get_changed_files,
    try_get_changed_files,
};

/// Drop findings whose source line is not inside an added hunk of the
/// supplied unified diff. Range-shaped findings (clone instances live in
/// dupes, not here; complexity hotspots live in health, not here) are
/// handled in their own subsystems. This filter only governs the per-file
/// findings that live on `AnalysisResults`.
///
/// **Project-level findings bypass the filter.** A PR that deletes the
/// last consumer of `lodash` semantically caused the `unused-dependency`
/// finding even though the diff never touches `package.json`; the same
/// reasoning applies to catalog entries, dependency overrides, type-only
/// deps, and test-only deps. The line filter is a noise-floor reducer for
/// source-anchored findings; CI must still fail on project-level findings
/// the PR caused. Mirrors the default `FALLOW_SUMMARY_SCOPE=all` behavior
/// in typed PR-comment rendering.
///
/// `relative_to_diff_path` normalizes the finding's absolute path to the
/// forward-slashed key shape `git diff` writes (`+++ b/<path>`). When the
/// path cannot be expressed relative to `root` (different drive, traversal
/// escape), the finding is RETAINED rather than silently dropped: an
/// unfilterable path is better surfaced than silently hidden.
pub fn filter_results_by_diff(
    results: &mut fallow_core::results::AnalysisResults,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &Path,
) {
    use crate::report::ci::diff_filter::relative_to_diff_path;

    let touches_file = |path: &Path| -> bool {
        match relative_to_diff_path(path, root) {
            Some(p) => diff_index.touches_file(&p),
            None => true,
        }
    };
    let line_in_diff = |path: &Path, line: u32| -> bool {
        match relative_to_diff_path(path, root) {
            Some(p) => diff_index
                .added_lines_in(&p)
                .is_some_and(|set| set.contains(&u64::from(line))),
            None => true,
        }
    };

    filter_diff_source_findings(results, &touches_file, &line_in_diff);
    filter_diff_security_findings(results, &touches_file, &line_in_diff);
    filter_diff_dependency_findings(results, &line_in_diff);
    filter_diff_graph_findings(results, &touches_file, &line_in_diff);
    filter_diff_framework_findings(results, &line_in_diff);
}

fn filter_diff_source_findings(
    results: &mut fallow_core::results::AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.unused_files.retain(|f| touches_file(&f.file.path));

    results
        .unused_exports
        .retain(|e| line_in_diff(&e.export.path, e.export.line));
    results
        .unused_types
        .retain(|e| line_in_diff(&e.export.path, e.export.line));
    results
        .private_type_leaks
        .retain(|e| line_in_diff(&e.leak.path, e.leak.line));
    results
        .unused_enum_members
        .retain(|m| line_in_diff(&m.member.path, m.member.line));
    results
        .unused_class_members
        .retain(|m| line_in_diff(&m.member.path, m.member.line));
    results
        .unused_store_members
        .retain(|m| line_in_diff(&m.member.path, m.member.line));
    results
        .unprovided_injects
        .retain(|f| line_in_diff(&f.inject.path, f.inject.line));
    results
        .unrendered_components
        .retain(|c| line_in_diff(&c.component.path, c.component.line));
    results
        .unused_component_props
        .retain(|p| line_in_diff(&p.prop.path, p.prop.line));
    results
        .unused_component_emits
        .retain(|e| line_in_diff(&e.emit.path, e.emit.line));
    results
        .unused_component_inputs
        .retain(|i| line_in_diff(&i.input.path, i.input.line));
    results
        .unused_component_outputs
        .retain(|o| line_in_diff(&o.output.path, o.output.line));
    results
        .unused_server_actions
        .retain(|a| line_in_diff(&a.action.path, a.action.line));
    results
        .unused_load_data_keys
        .retain(|k| line_in_diff(&k.key.path, k.key.line));
    results
        .unresolved_imports
        .retain(|i| line_in_diff(&i.import.path, i.import.line));
}

fn filter_diff_security_findings(
    results: &mut fallow_core::results::AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.security_findings.retain(|f| {
        line_in_diff(&f.path, f.line)
            || f.trace.iter().any(|hop| {
                line_in_diff(&hop.path, hop.line)
                    || (matches!(hop.role, fallow_core::results::TraceHopRole::SecretSource)
                        && touches_file(&hop.path))
            })
            || f.reachability.as_ref().is_some_and(|reachability| {
                // Source-trace matching here is deliberately ROLE-AGNOSTIC (any
                // hop on a changed line keeps the finding for advisory display).
                // Do NOT add a role gate: unlike the strict `retain_gate_new`
                // gate, the advisory display must surface a finding whose
                // module-level `ModuleSource` source node sits on a changed line.
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| line_in_diff(&hop.path, hop.line))
            })
    });
    results
        .security_unresolved_callee_diagnostics
        .retain(|d| line_in_diff(&d.path, d.line));
}

fn filter_diff_dependency_findings(
    results: &mut fallow_core::results::AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    for unlisted in &mut results.unlisted_dependencies {
        unlisted
            .dep
            .imported_from
            .retain(|s| line_in_diff(&s.path, s.line));
    }
    results
        .unlisted_dependencies
        .retain(|d| !d.dep.imported_from.is_empty());
}

fn filter_diff_graph_findings(
    results: &mut fallow_core::results::AnalysisResults,
    touches_file: &dyn Fn(&Path) -> bool,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results.duplicate_exports.retain(|d| {
        d.export
            .locations
            .iter()
            .any(|loc| line_in_diff(&loc.path, loc.line))
    });

    results
        .circular_dependencies
        .retain(|c| c.cycle.files.iter().any(|f| touches_file(f)));

    results
        .re_export_cycles
        .retain(|c| c.cycle.files.iter().any(|f| touches_file(f)));

    results
        .boundary_violations
        .retain(|v| line_in_diff(&v.violation.from_path, v.violation.line));

    results
        .stale_suppressions
        .retain(|s| line_in_diff(&s.path, s.line));
}

fn filter_diff_framework_findings(
    results: &mut fallow_core::results::AnalysisResults,
    line_in_diff: &dyn Fn(&Path, u32) -> bool,
) {
    results
        .invalid_client_exports
        .retain(|e| line_in_diff(&e.export.path, e.export.line));

    results
        .mixed_client_server_barrels
        .retain(|b| line_in_diff(&b.barrel.path, b.barrel.line));

    results
        .misplaced_directives
        .retain(|d| line_in_diff(&d.directive_site.path, d.directive_site.line));

    results
        .route_collisions
        .retain(|c| line_in_diff(&c.collision.path, c.collision.line));

    results
        .dynamic_segment_name_conflicts
        .retain(|c| line_in_diff(&c.conflict.path, c.conflict.line));
}

/// Strict gate predicate for `fallow security --gate new` (issue #886): retain a
/// security finding ONLY when the change INTRODUCED it, not merely when it lives
/// in a changed file.
///
/// This is deliberately STRICTER than the advisory `filter_results_by_diff`: a
/// CI gate that fails on "a pre-existing sink in a file the PR happened to touch"
/// is a flapping gate, so this drops the two file-level / pass-through conditions
/// the advisory display keeps:
/// - the `SecretSource && touches_file` FILE-level exception (advisory keeps it so
///   you SEE a leak when you edit the secret-reading file; in a gate it fires on
///   an untouched sink the moment any env-config file is touched), and
/// - the `Intermediate` / `ClientBoundary` pass-through hop lines (editing an
///   unrelated line in a transit module is not new exposure).
///
/// It KEEPS the sink anchor on an added line (a genuinely new sink) AND an
/// `UntrustedSource` / `ModuleSource` / `Sink` trace hop on an added line (a
/// change that wires a new untrusted source INTO an existing sink is real new
/// exposure that anchor-line-only would miss; `ModuleSource` covers the
/// module-level cross-module source node, the role the arg-vs-module split
/// (#1093) renamed it to, so wiring a source-module import on an added line
/// still trips the gate). Unfilterable paths (outside `root`) are RETAINED
/// (`line_in_diff` returns `true` for them): a candidate the gate cannot prove is
/// old fails conservatively, the safe direction for a security gate.
pub fn retain_gate_new(
    results: &mut fallow_core::results::AnalysisResults,
    diff_index: &crate::report::ci::diff_filter::DiffIndex,
    root: &Path,
) {
    use crate::report::ci::diff_filter::relative_to_diff_path;
    use fallow_core::results::TraceHopRole;

    let line_in_diff = |path: &Path, line: u32| -> bool {
        match relative_to_diff_path(path, root) {
            Some(p) => diff_index
                .added_lines_in(&p)
                .is_some_and(|set| set.contains(&u64::from(line))),
            None => true,
        }
    };
    let taint_hop_added = |path: &Path, line: u32, role: TraceHopRole| -> bool {
        matches!(
            role,
            TraceHopRole::UntrustedSource | TraceHopRole::ModuleSource | TraceHopRole::Sink
        ) && line_in_diff(path, line)
    };

    results.security_findings.retain(|f| {
        line_in_diff(&f.path, f.line)
            || f.trace
                .iter()
                .any(|hop| taint_hop_added(&hop.path, hop.line, hop.role))
            || f.reachability.as_ref().is_some_and(|reachability| {
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| taint_hop_added(&hop.path, hop.line, hop.role))
            })
    });
    results
        .security_unresolved_callee_diagnostics
        .retain(|d| line_in_diff(&d.path, d.line));
}

/// Given a list of discovered workspaces and a set of changed file paths,
/// return the indices of workspaces that contain any changed file.
///
/// Pure function, intentionally independent of git / filesystem so the mapping
/// logic can be exercised without a repo. Files outside any workspace (e.g.
/// root-level `package.json`, lockfiles, CI configs) are ignored; they map to
/// zero workspaces, and the caller decides what to do with an empty result.
fn workspaces_containing_any(
    workspaces: &[WorkspaceInfo],
    changed_files: &FxHashSet<std::path::PathBuf>,
) -> Vec<usize> {
    let mut hits: Vec<usize> = Vec::new();
    for (idx, ws) in workspaces.iter().enumerate() {
        if changed_files.iter().any(|f| f.starts_with(&ws.root)) {
            hits.push(idx);
        }
    }
    hits
}

/// Resolve `--changed-workspaces <REF>` to a set of workspace roots containing
/// files changed since `git_ref`.
///
/// Unlike `--changed-since`, which silently falls back to full-scope analysis
/// if git fails, this resolver treats any git failure as a hard error: the
/// flag's entire purpose is to narrow CI scope, so silently widening back to
/// the whole monorepo would defeat the optimization and surprise the user.
///
/// Returns `Ok(vec![])` when git succeeded but no tracked workspace files
/// changed (normal CI outcome: a root-only lockfile bump, for example).
pub fn resolve_changed_workspaces(
    root: &Path,
    git_ref: &str,
    output: OutputFormat,
) -> Result<Vec<PathBuf>, ExitCode> {
    let workspaces = discover_workspaces(root);
    if workspaces.is_empty() {
        let msg = format!(
            "--changed-workspaces '{git_ref}' specified but no workspaces found. \
             Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, \
             or tsconfig.json has \"references\"."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let changed_files = try_get_changed_files(root, git_ref).map_err(|err| {
        let msg = format!(
            "--changed-workspaces failed for ref '{git_ref}': {}",
            err.describe()
        );
        emit_error(&msg, 2, output)
    })?;

    let hits = workspaces_containing_any(&workspaces, &changed_files);
    let mut roots: Vec<PathBuf> = hits
        .into_iter()
        .map(|i| workspaces[i].root.clone())
        .collect();
    roots.sort();
    Ok(roots)
}

/// Resolve whichever workspace scoping flag the user passed. Returns `None`
/// when neither `--workspace` nor `--changed-workspaces` is set, so callers
/// can leave analysis at full scope.
///
/// `--workspace` and `--changed-workspaces` are mutually exclusive at the
/// CLI layer; this helper errors if both are set as a defence-in-depth check.
pub fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
    output: OutputFormat,
) -> Result<Option<Vec<PathBuf>>, ExitCode> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => Ok(Some(resolve_workspace_filters(root, patterns, output)?)),
        (None, Some(git_ref)) => Ok(Some(resolve_changed_workspaces(root, git_ref, output)?)),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            let msg = "--workspace and --changed-workspaces are mutually exclusive. \
                 Pick one: --workspace for explicit package names/globs, \
                 --changed-workspaces for git-derived monorepo CI scoping.";
            Err(emit_error(msg, 2, output))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use fallow_types::extract::{SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason};
    use fallow_types::results::{SecurityReachability, SecuritySeverity};
    use std::path::PathBuf;

    /// Test shim: single-workspace variant on top of `filter_to_workspaces`.
    fn filter_to_workspace(results: &mut AnalysisResults, ws_root: &Path) {
        filter_to_workspaces(results, std::slice::from_ref(&ws_root.to_path_buf()));
    }

    fn unresolved_callee_diagnostic(path: &str, line: u32) -> SecurityUnresolvedCalleeDiagnostic {
        SecurityUnresolvedCalleeDiagnostic {
            path: PathBuf::from(path),
            line,
            col: 0,
            reason: SkippedSecurityCalleeReason::DynamicDispatch,
            expression_kind: SkippedSecurityCalleeExpressionKind::Other,
        }
    }

    #[test]
    fn filter_to_workspace_keeps_files_under_ws_root() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/ui/src/button.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/api/src/handler.ts"),
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            PathBuf::from("/project/packages/ui/src/button.ts")
        );
    }

    #[test]
    fn filter_to_workspace_scopes_unresolved_callee_diagnostics() {
        let mut results = AnalysisResults::default();
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic(
                "/project/packages/ui/src/a.ts",
                3,
            ));
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic(
                "/project/packages/api/src/b.ts",
                3,
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
        assert_eq!(
            results.security_unresolved_callee_diagnostics[0].path,
            PathBuf::from("/project/packages/ui/src/a.ts")
        );
    }

    #[test]
    fn filter_to_workspace_scopes_dependencies_to_ws_package_json() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "react".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/packages/ui/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "vitest".into(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("/project/packages/ui/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dependencies[0].dep.package_name, "react");
        assert_eq!(results.unused_dev_dependencies.len(), 1);
        assert_eq!(
            results.unused_dev_dependencies[0].dep.package_name,
            "vitest"
        );
    }

    #[test]
    fn filter_to_workspace_scopes_unlisted_deps_by_importer() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/packages/ui/src/a.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "debug".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/packages/api/src/b.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unlisted_dependencies.len(), 1);
        assert_eq!(results.unlisted_dependencies[0].dep.package_name, "chalk");
    }

    #[test]
    fn filter_to_workspace_drops_duplicate_exports_below_two_locations() {
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/packages/ui/src/a.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/packages/api/src/b.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "utils".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/packages/ui/src/c.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/packages/ui/src/d.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.duplicate_exports.len(), 1);
        assert_eq!(results.duplicate_exports[0].export.export_name, "utils");
    }

    #[test]
    fn filter_to_workspace_scopes_exports_and_types() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/packages/ui/src/a.ts"),
                export_name: "A".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/packages/api/src/b.ts"),
                export_name: "B".into(),
                is_type_only: false,
                line: 2,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/packages/ui/src/types.ts"),
                export_name: "T".into(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unused_exports[0].export.export_name, "A");
        assert_eq!(results.unused_types.len(), 1);
        assert_eq!(results.unused_types[0].export.export_name, "T");
    }

    #[test]
    fn filter_to_workspace_scopes_type_only_dependencies() {
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".into(),
                    path: PathBuf::from("/project/packages/ui/package.json"),
                    line: 8,
                },
            ));
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "yup".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 8,
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.type_only_dependencies.len(), 1);
        assert_eq!(results.type_only_dependencies[0].dep.package_name, "zod");
    }

    #[test]
    fn filter_to_workspace_scopes_enum_and_class_members() {
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/packages/ui/src/enums.ts"),
                parent_name: "Color".into(),
                member_name: "Red".into(),
                kind: MemberKind::EnumMember,
                line: 2,
                col: 0,
            }));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/packages/api/src/enums.ts"),
                parent_name: "Status".into(),
                member_name: "Active".into(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 0,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/packages/ui/src/service.ts"),
                parent_name: "Svc".into(),
                member_name: "init".into(),
                kind: MemberKind::ClassMethod,
                line: 5,
                col: 0,
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_enum_members.len(), 1);
        assert_eq!(results.unused_enum_members[0].member.member_name, "Red");
        assert_eq!(results.unused_class_members.len(), 1);
        assert_eq!(results.unused_class_members[0].member.member_name, "init");
    }

    #[test]
    fn filter_changed_files_keeps_only_changed() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/b.ts"),
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            PathBuf::from("/project/src/a.ts")
        );
    }

    #[test]
    fn filter_changed_files_preserves_unused_deps() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("/project/package.json"),
                line: 10,
                used_in_workspaces: Vec::new(),
            }));

        let changed = rustc_hash::FxHashSet::default(); // empty set

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dev_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_filters_exports_by_path() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/a.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/b.ts"),
                export_name: "bar".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unused_exports[0].export.export_name, "bar");
    }

    #[test]
    fn filter_changed_files_drops_duplicate_exports_below_two() {
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/b.ts"),
                        line: 2,
                        col: 0,
                    },
                ],
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn filter_changed_files_keeps_circular_deps_if_any_file_changed() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_removes_circular_deps_if_no_file_changed() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/c.ts"));

        filter_changed_files(&mut results, &changed);

        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn filter_changed_files_keeps_unlisted_dep_if_importer_changed() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_removes_unlisted_dep_if_no_importer_changed() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert!(results.unlisted_dependencies.is_empty());
    }

    #[test]
    fn filter_to_workspace_scopes_optional_dependencies() {
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/packages/ui/package.json"),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "esbuild".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 7,
                    used_in_workspaces: Vec::new(),
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_optional_dependencies.len(), 1);
        assert_eq!(
            results.unused_optional_dependencies[0].dep.package_name,
            "fsevents"
        );
    }

    #[test]
    fn filter_to_workspace_scopes_test_only_dependencies() {
        let mut results = AnalysisResults::default();
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "msw".into(),
                    path: PathBuf::from("/project/packages/ui/package.json"),
                    line: 4,
                },
            ));
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "nock".into(),
                    path: PathBuf::from("/project/packages/api/package.json"),
                    line: 6,
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.test_only_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies[0].dep.package_name, "msw");
    }

    #[test]
    fn filter_to_workspace_scopes_circular_dependencies() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/packages/ui/src/a.ts"),
                        PathBuf::from("/project/packages/ui/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/packages/api/src/x.ts"),
                        PathBuf::from("/project/packages/api/src/y.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert_eq!(
            results.circular_dependencies[0].cycle.files[0],
            PathBuf::from("/project/packages/ui/src/a.ts")
        );
    }

    #[test]
    fn filter_to_workspace_keeps_circular_dep_if_any_file_in_workspace() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/packages/ui/src/a.ts"),
                        PathBuf::from("/project/packages/api/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_to_workspace_scopes_unresolved_imports() {
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/packages/ui/src/a.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/packages/api/src/b.ts"),
                specifier: "./gone".into(),
                line: 2,
                col: 0,
                specifier_col: 0,
            }));

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unresolved_imports.len(), 1);
        assert_eq!(results.unresolved_imports[0].import.specifier, "./missing");
    }

    #[test]
    fn filter_to_workspace_on_empty_results_stays_empty() {
        let mut results = AnalysisResults::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);
        assert_eq!(results.total_issues(), 0);
    }

    #[test]
    fn filter_changed_files_filters_types_by_path() {
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/types.ts"),
                export_name: "Foo".into(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/other.ts"),
                export_name: "Bar".into(),
                is_type_only: true,
                line: 2,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/types.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_types.len(), 1);
        assert_eq!(results.unused_types[0].export.export_name, "Foo");
    }

    #[test]
    fn filter_changed_files_filters_enum_members_by_path() {
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/enums.ts"),
                parent_name: "Color".into(),
                member_name: "Red".into(),
                kind: MemberKind::EnumMember,
                line: 2,
                col: 0,
            }));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/other.ts"),
                parent_name: "Status".into(),
                member_name: "Active".into(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 0,
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/enums.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_enum_members.len(), 1);
        assert_eq!(results.unused_enum_members[0].member.member_name, "Red");
    }

    #[test]
    fn filter_changed_files_filters_class_members_by_path() {
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/service.ts"),
                parent_name: "Svc".into(),
                member_name: "init".into(),
                kind: MemberKind::ClassMethod,
                line: 5,
                col: 0,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/other.ts"),
                parent_name: "Other".into(),
                member_name: "run".into(),
                kind: MemberKind::ClassMethod,
                line: 10,
                col: 0,
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/service.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_class_members.len(), 1);
        assert_eq!(results.unused_class_members[0].member.member_name, "init");
    }

    #[test]
    fn filter_changed_files_preserves_optional_and_type_only_and_test_only_deps() {
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 8,
                },
            ));
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "msw".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 12,
                },
            ));

        let changed = rustc_hash::FxHashSet::default();

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_optional_dependencies.len(), 1);
        assert_eq!(results.type_only_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_keeps_duplicate_exports_when_both_changed() {
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/b.ts"),
                        line: 2,
                        col: 0,
                    },
                ],
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.duplicate_exports.len(), 1);
        assert_eq!(results.duplicate_exports[0].export.locations.len(), 2);
    }

    #[test]
    fn filter_changed_files_empty_set_clears_file_scoped_issues() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/b.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/c.ts"),
                export_name: "T".into(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/d.ts"),
                parent_name: "E".into(),
                member_name: "A".into(),
                kind: MemberKind::EnumMember,
                line: 1,
                col: 0,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/e.ts"),
                parent_name: "C".into(),
                member_name: "m".into(),
                kind: MemberKind::ClassMethod,
                line: 1,
                col: 0,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/f.ts"),
                specifier: "./x".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));

        let changed = rustc_hash::FxHashSet::default();

        filter_changed_files(&mut results, &changed);

        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.unused_enum_members.is_empty());
        assert!(results.unused_class_members.is_empty());
        assert!(results.unresolved_imports.is_empty());
    }

    #[test]
    fn filter_changed_files_on_empty_results_stays_empty() {
        let mut results = AnalysisResults::default();
        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.total_issues(), 0);
    }

    #[test]
    fn filter_changed_files_unlisted_dep_with_multiple_importers_keeps_if_any_changed() {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![
                        ImportSite {
                            path: PathBuf::from("/project/src/a.ts"),
                            line: 1,
                            col: 0,
                        },
                        ImportSite {
                            path: PathBuf::from("/project/src/b.ts"),
                            line: 5,
                            col: 0,
                        },
                    ],
                },
            ));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_filters_unresolved_imports_by_path() {
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/a.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/b.ts"),
                specifier: "./gone".into(),
                line: 2,
                col: 0,
                specifier_col: 0,
            }));

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unresolved_imports.len(), 1);
        assert_eq!(results.unresolved_imports[0].import.specifier, "./missing");
    }

    fn ws(name: &str, rel: &str) -> fallow_config::WorkspaceInfo {
        fallow_config::WorkspaceInfo {
            root: PathBuf::from("/project").join(rel),
            name: name.to_owned(),
            is_internal_dependency: false,
        }
    }

    fn rel(workspaces: &[fallow_config::WorkspaceInfo]) -> Vec<String> {
        workspaces
            .iter()
            .map(|w| relative_workspace_path(&w.root, Path::new("/project")))
            .collect()
    }

    #[test]
    fn split_patterns_separates_positive_and_negative() {
        let input = vec![
            "web".to_owned(),
            "apps/*".to_owned(),
            "!apps/legacy".to_owned(),
            "  ".to_owned(),
            String::new(),
            "!  ".to_owned(),
        ];
        let (pos, neg) = split_patterns(&input);
        assert_eq!(pos, vec!["web", "apps/*"]);
        assert_eq!(neg, vec!["apps/legacy"]);
    }

    #[test]
    fn find_matches_exact_name_short_circuits_glob_metachars() {
        let workspaces = vec![ws("web-[staging]", "apps/web-staging")];
        let rels = rel(&workspaces);
        let hits = find_matches(
            "web-[staging]",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn find_matches_glob_against_name_and_path() {
        let workspaces = vec![
            ws("@scope/ui", "packages/ui"),
            ws("admin", "apps/admin"),
            ws("web", "apps/web"),
        ];
        let rels = rel(&workspaces);

        let hits = find_matches(
            "@scope/*",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![0]);

        let hits = find_matches(
            "apps/*",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![1, 2]);
    }

    #[test]
    fn find_matches_invalid_glob_after_no_literal_match_errors() {
        let workspaces = vec![ws("web", "apps/web")];
        let rels = rel(&workspaces);
        assert!(
            find_matches(
                "web-[bad",
                &workspaces,
                &rels,
                fallow_config::OutputFormat::Human,
            )
            .is_err()
        );
    }

    #[test]
    fn format_available_workspaces_truncates_when_above_cap() {
        let workspaces: Vec<WorkspaceInfo> = (0..15)
            .map(|i| ws(&format!("pkg-{i}"), &format!("packages/pkg-{i}")))
            .collect();
        let rendered = format_available_workspaces(&workspaces);
        assert!(rendered.starts_with("pkg-0, pkg-1,"));
        assert!(rendered.contains("and 5 more"));
        assert!(rendered.contains("15 total"));
    }

    #[test]
    fn format_available_workspaces_does_not_truncate_below_cap() {
        let workspaces = vec![ws("a", "packages/a"), ws("b", "packages/b")];
        assert_eq!(format_available_workspaces(&workspaces), "a, b");
    }

    #[test]
    fn filter_to_workspaces_unions_multiple_roots() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/ui/src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/api/src/b.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/legacy/src/c.ts"),
            }));

        let roots = [
            PathBuf::from("/project/packages/ui"),
            PathBuf::from("/project/packages/api"),
        ];
        filter_to_workspaces(&mut results, &roots);

        assert_eq!(results.unused_files.len(), 2);
    }

    #[test]
    fn filter_to_workspaces_scopes_deps_to_matched_package_jsons() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/packages/ui/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "react".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/packages/api/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "axios".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/packages/legacy/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));

        let roots = [
            PathBuf::from("/project/packages/ui"),
            PathBuf::from("/project/packages/api"),
        ];
        filter_to_workspaces(&mut results, &roots);

        assert_eq!(results.unused_dependencies.len(), 2);
        let names: Vec<&str> = results
            .unused_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_ref())
            .collect();
        assert!(names.contains(&"lodash"));
        assert!(names.contains(&"react"));
        assert!(!names.contains(&"axios"));
    }

    #[test]
    fn filter_to_workspaces_empty_slice_drops_everything() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/packages/ui/src/a.ts"),
            }));
        filter_to_workspaces(&mut results, &[]);
        assert_eq!(results.unused_files.len(), 0);
    }

    #[test]
    fn workspaces_containing_any_returns_only_hits() {
        let workspaces = vec![
            ws("ui", "packages/ui"),
            ws("api", "packages/api"),
            ws("legacy", "packages/legacy"),
        ];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/packages/ui/src/a.ts"));
        changed.insert(PathBuf::from("/project/packages/api/src/b.ts"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert_eq!(hits, vec![0, 1]);
    }

    #[test]
    fn workspaces_containing_any_ignores_root_only_changes() {
        let workspaces = vec![ws("ui", "packages/ui"), ws("api", "packages/api")];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/package.json"));
        changed.insert(PathBuf::from("/project/pnpm-lock.yaml"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert!(hits.is_empty());
    }

    #[test]
    fn workspaces_containing_any_empty_changed_set_is_no_hits() {
        let workspaces = vec![ws("ui", "packages/ui")];
        let changed = FxHashSet::default();

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert!(hits.is_empty());
    }

    #[test]
    fn workspaces_containing_any_single_changed_file_maps_to_one_workspace() {
        let workspaces = vec![
            ws("ui", "packages/ui"),
            ws("api", "packages/api"),
            ws("cli", "packages/cli"),
        ];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/packages/api/src/b.ts"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert_eq!(hits, vec![1]);
    }

    #[test]
    fn resolve_workspace_scope_neither_flag_returns_none() {
        let root = Path::new("/project");
        let got = resolve_workspace_scope(root, None, None, OutputFormat::Human).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn resolve_workspace_scope_both_flags_is_error() {
        let root = Path::new("/project");
        let patterns = ["web".to_owned()];
        let got = resolve_workspace_scope(root, Some(&patterns), Some("main"), OutputFormat::Human);
        assert!(
            got.is_err(),
            "--workspace + --changed-workspaces must error out"
        );
    }

    fn build_diff(text: &str) -> crate::report::ci::diff_filter::DiffIndex {
        crate::report::ci::diff_filter::DiffIndex::from_unified_diff(text)
    }

    #[test]
    fn filter_by_diff_drops_unused_export_not_on_added_line() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -10,1 +10,2 @@\n\
              ctx\n\
             +touched\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/a.ts"),
                export_name: "kept".into(),
                is_type_only: false,
                line: 11,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/a.ts"),
                export_name: "dropped".into(),
                is_type_only: false,
                line: 30,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        filter_results_by_diff(&mut results, &diff, root);
        let names: Vec<&str> = results
            .unused_exports
            .iter()
            .map(|e| e.export.export_name.as_str())
            .collect();
        assert_eq!(names, vec!["kept"]);
    }

    #[test]
    fn filter_by_diff_scopes_unresolved_callee_diagnostics_to_added_lines() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -10,1 +10,2 @@\n\
              ctx\n\
             +client[method](req.body.name);\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic("/project/src/a.ts", 11));
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic("/project/src/a.ts", 30));

        filter_results_by_diff(&mut results, &diff, root);

        assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
        assert_eq!(results.security_unresolved_callee_diagnostics[0].line, 11);
    }

    #[test]
    fn filter_by_diff_keeps_project_level_deps_even_when_diff_misses_package_json() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -1,3 +1,2 @@\n\
              keep\n\
             -import 'lodash';\n\
              keep\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 42,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "react".into(),
                    catalog_name: "default".into(),
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 5,
                    hardcoded_consumers: Vec::new(),
                },
            ));
        filter_results_by_diff(&mut results, &diff, root);
        assert_eq!(
            results.unused_dependencies.len(),
            1,
            "unused-dependency must bypass the diff filter"
        );
        assert_eq!(
            results.unused_catalog_entries.len(),
            1,
            "unused-catalog-entry must bypass the diff filter"
        );
    }

    #[test]
    fn filter_by_diff_keeps_security_finding_when_secret_source_changed() {
        let diff = build_diff(
            "diff --git a/src/server.ts b/src/server.ts\n\
             --- a/src/server.ts\n\
             +++ b/src/server.ts\n\
             @@ -8,1 +8,2 @@\n\
              const keep = true;\n\
             +export const db = process.env.DATABASE_URL;\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path: PathBuf::from("/project/src/client.tsx"),
            line: 2,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![
                TraceHop {
                    path: PathBuf::from("/project/src/client.tsx"),
                    line: 2,
                    col: 0,
                    role: TraceHopRole::ClientBoundary,
                },
                TraceHop {
                    path: PathBuf::from("/project/src/server.ts"),
                    line: 1,
                    col: 0,
                    role: TraceHopRole::SecretSource,
                },
            ],
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            runtime: None,
        });

        filter_results_by_diff(&mut results, &diff, root);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn filter_by_diff_keeps_security_finding_when_untrusted_source_trace_changed() {
        let diff = build_diff(
            "diff --git a/src/route.ts b/src/route.ts\n\
             --- a/src/route.ts\n\
             +++ b/src/route.ts\n\
             @@ -3,0 +3,1 @@\n\
             +import { run } from './runner';\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("command-injection".into()),
            cwe: Some(78),
            path: PathBuf::from("/project/src/runner.ts"),
            line: 10,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: Vec::new(),
            dead_code: None,
            reachability: Some(SecurityReachability {
                reachable_from_entry: false,
                reachable_from_untrusted_source: true,
                taint_confidence: None,
                untrusted_source_hop_count: Some(1),
                untrusted_source_trace: vec![
                    TraceHop {
                        path: PathBuf::from("/project/src/route.ts"),
                        line: 3,
                        col: 0,
                        role: TraceHopRole::UntrustedSource,
                    },
                    TraceHop {
                        path: PathBuf::from("/project/src/runner.ts"),
                        line: 10,
                        col: 0,
                        role: TraceHopRole::Sink,
                    },
                ],
                blast_radius: 0,
                crosses_boundary: false,
            }),
            runtime: None,
        });

        filter_results_by_diff(&mut results, &diff, root);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn gate_keeps_new_sink_anchor_on_added_line() {
        let diff = build_diff(
            "diff --git a/src/render.ts b/src/render.ts\n\
             --- a/src/render.ts\n\
             +++ b/src/render.ts\n\
             @@ -11,0 +12,1 @@\n\
             +el.innerHTML = req.query.html;\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".into()),
            cwe: Some(79),
            path: PathBuf::from("/project/src/render.ts"),
            line: 12,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            runtime: None,
        });

        retain_gate_new(&mut results, &diff, root);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn gate_new_scopes_unresolved_callee_diagnostics_to_added_lines() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -2,1 +2,2 @@\n\
              ctx\n\
             +factory()(req.body.name);\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic("/project/src/a.ts", 3));
        results
            .security_unresolved_callee_diagnostics
            .push(unresolved_callee_diagnostic("/project/src/a.ts", 20));

        retain_gate_new(&mut results, &diff, root);

        assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
        assert_eq!(results.security_unresolved_callee_diagnostics[0].line, 3);
    }

    #[test]
    fn gate_drops_secret_source_file_touched_when_anchor_unchanged() {
        // The SAME shape the advisory `filter_by_diff` KEEPS (a SecretSource hop
        // whose FILE is touched, anchor line unchanged). The gate MUST DROP it:
        // editing a file that merely reads a secret is not a new sink, and the
        // forbidden "a sink in a changed file fails the gate" false positive.
        let diff = build_diff(
            "diff --git a/src/server.ts b/src/server.ts\n\
             --- a/src/server.ts\n\
             +++ b/src/server.ts\n\
             @@ -8,1 +8,2 @@\n\
              const keep = true;\n\
             +export const db = process.env.DATABASE_URL;\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path: PathBuf::from("/project/src/client.tsx"),
            line: 2,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![
                TraceHop {
                    path: PathBuf::from("/project/src/client.tsx"),
                    line: 2,
                    col: 0,
                    role: TraceHopRole::ClientBoundary,
                },
                TraceHop {
                    path: PathBuf::from("/project/src/server.ts"),
                    line: 1,
                    col: 0,
                    role: TraceHopRole::SecretSource,
                },
            ],
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            runtime: None,
        });

        // Advisory keeps it; the gate drops it.
        let mut advisory = results.clone();
        filter_results_by_diff(&mut advisory, &diff, root);
        assert_eq!(advisory.security_findings.len(), 1);

        retain_gate_new(&mut results, &diff, root);
        assert_eq!(results.security_findings.len(), 0);
    }

    #[test]
    fn gate_keeps_untrusted_source_hop_on_added_line() {
        // A pre-existing sink whose anchor is NOT added, but the PR wires a new
        // UntrustedSource into it on an added line: genuine new exposure, kept.
        let diff = build_diff(
            "diff --git a/src/route.ts b/src/route.ts\n\
             --- a/src/route.ts\n\
             +++ b/src/route.ts\n\
             @@ -3,0 +3,1 @@\n\
             +import { run } from './runner';\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("command-injection".into()),
            cwe: Some(78),
            path: PathBuf::from("/project/src/runner.ts"),
            line: 10,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: Vec::new(),
            dead_code: None,
            reachability: Some(SecurityReachability {
                reachable_from_entry: false,
                reachable_from_untrusted_source: true,
                taint_confidence: None,
                untrusted_source_hop_count: Some(1),
                untrusted_source_trace: vec![TraceHop {
                    path: PathBuf::from("/project/src/route.ts"),
                    line: 3,
                    col: 0,
                    role: TraceHopRole::UntrustedSource,
                }],
                blast_radius: 0,
                crosses_boundary: false,
            }),
            runtime: None,
        });

        retain_gate_new(&mut results, &diff, root);

        assert_eq!(results.security_findings.len(), 1);
    }

    #[test]
    fn gate_drops_intermediate_hop_pass_through() {
        // Only an Intermediate (pass-through) hop sits on an added line; the sink
        // anchor and any UntrustedSource/Sink hop are unchanged. Editing a transit
        // module is not new exposure, so the gate drops it.
        let diff = build_diff(
            "diff --git a/src/transit.ts b/src/transit.ts\n\
             --- a/src/transit.ts\n\
             +++ b/src/transit.ts\n\
             @@ -4,0 +4,1 @@\n\
             +const passthrough = next;\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results.security_findings.push(SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_types::results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".into()),
            cwe: Some(79),
            path: PathBuf::from("/project/src/sink.ts"),
            line: 20,
            col: 0,
            evidence: "candidate".into(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![TraceHop {
                path: PathBuf::from("/project/src/transit.ts"),
                line: 4,
                col: 0,
                role: TraceHopRole::Intermediate,
            }],
            actions: Vec::new(),
            dead_code: None,
            reachability: None,
            runtime: None,
        });

        retain_gate_new(&mut results, &diff, root);

        assert!(results.security_findings.is_empty());
    }

    #[test]
    fn filter_by_diff_drops_duplicate_export_when_no_location_in_diff() {
        let diff = build_diff(
            "diff --git a/src/other.ts b/src/other.ts\n\
             --- a/src/other.ts\n\
             +++ b/src/other.ts\n\
             @@ -1,0 +1,1 @@\n\
             +untouched-by-dups\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 5,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            }));
        filter_results_by_diff(&mut results, &diff, root);
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn filter_by_diff_keeps_duplicate_export_when_both_locations_in_diff() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -1,0 +1,1 @@\n\
             +line1\n\
             diff --git a/src/b.ts b/src/b.ts\n\
             --- a/src/b.ts\n\
             +++ b/src/b.ts\n\
             @@ -1,0 +1,1 @@\n\
             +line1\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/b.ts"),
                        line: 1,
                        col: 0,
                    },
                ],
            }));
        filter_results_by_diff(&mut results, &diff, root);
        assert_eq!(results.duplicate_exports.len(), 1);
        assert_eq!(results.duplicate_exports[0].export.locations.len(), 2);
    }

    #[test]
    fn filter_by_diff_keeps_duplicate_export_when_pr_adds_one_against_off_diff_existing() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +export const helper = 1;\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            }));
        filter_results_by_diff(&mut results, &diff, root);
        assert_eq!(
            results.duplicate_exports.len(),
            1,
            "PR-introduced duplicate must surface even when only one location is in the diff"
        );
        assert_eq!(
            results.duplicate_exports[0].export.locations.len(),
            2,
            "both locations must be retained so the renderer can show the conflict pair"
        );
        let paths: Vec<&Path> = results.duplicate_exports[0]
            .export
            .locations
            .iter()
            .map(|loc| loc.path.as_path())
            .collect();
        assert!(paths.contains(&Path::new("/project/src/a.ts")));
        assert!(paths.contains(&Path::new("/project/src/b.ts")));
    }

    #[test]
    fn filter_by_diff_keeps_unlisted_dep_only_when_at_least_one_import_site_in_diff() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +import 'chalk';\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![
                        ImportSite {
                            path: PathBuf::from("/project/src/a.ts"),
                            line: 1,
                            col: 0,
                        },
                        ImportSite {
                            path: PathBuf::from("/project/src/b.ts"),
                            line: 5,
                            col: 0,
                        },
                    ],
                },
            ));
        filter_results_by_diff(&mut results, &diff, root);
        assert_eq!(results.unlisted_dependencies.len(), 1);
        assert_eq!(results.unlisted_dependencies[0].dep.imported_from.len(), 1);
        assert_eq!(
            results.unlisted_dependencies[0].dep.imported_from[0].path,
            PathBuf::from("/project/src/a.ts")
        );
    }

    #[test]
    fn filter_by_diff_drops_unlisted_dep_when_no_import_sites_in_diff() {
        let diff = build_diff(
            "diff --git a/src/elsewhere.ts b/src/elsewhere.ts\n\
             --- a/src/elsewhere.ts\n\
             +++ b/src/elsewhere.ts\n\
             @@ -0,0 +1,1 @@\n\
             +nothing\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/a.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        filter_results_by_diff(&mut results, &diff, root);
        assert!(results.unlisted_dependencies.is_empty());
    }

    #[test]
    fn filter_by_diff_unused_files_use_file_level_membership() {
        let diff = build_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +touched\n",
        );
        let root = Path::new("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/b.ts"),
            }));
        filter_results_by_diff(&mut results, &diff, root);
        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            PathBuf::from("/project/src/a.ts")
        );
    }
}
