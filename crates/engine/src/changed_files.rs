//! Changed-file helpers owned by the engine boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use fallow_types::{
    output_dead_code::{
        CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
        PropDrillingChainFinding, ReExportCycleFinding, UnlistedDependencyFinding,
    },
    results::{AnalysisResults, SecurityFinding},
};
use rustc_hash::FxHashSet;

use crate::duplicates::{self, DuplicationReport};

pub use crate::git_env::{AMBIENT_GIT_ENV_VARS, clear_ambient_git_env};

/// Function pointer signature used to intercept short-running git
/// subprocesses spawned by changed-file helpers.
pub type ChangedFilesSpawnHook = fn(&mut std::process::Command) -> std::io::Result<Output>;

static SPAWN_HOOK: OnceLock<ChangedFilesSpawnHook> = OnceLock::new();

/// Classification of a changed-file git failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedFilesError {
    /// Git ref failed validation before invoking `git`.
    InvalidRef(String),
    /// `git` binary not found or not executable.
    GitMissing(String),
    /// Command ran but the directory is not a git repository.
    NotARepository,
    /// Command ran but the ref is invalid or another git error occurred.
    GitFailed(String),
}

impl ChangedFilesError {
    /// Human-readable clause suitable for embedding in an error message.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::InvalidRef(err) => format!("invalid git ref: {err}"),
            Self::GitMissing(err) => format!("failed to run git: {err}"),
            Self::NotARepository => "not a git repository".to_owned(),
            Self::GitFailed(stderr) => augment_git_failed(stderr),
        }
    }
}

fn augment_git_failed(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("not a valid object name")
        || lower.contains("unknown revision")
        || lower.contains("ambiguous argument")
    {
        format!(
            "{stderr} (shallow clone? try `git fetch --unshallow`, or set `fetch-depth: 0` on actions/checkout / `GIT_DEPTH: 0` in GitLab CI)"
        )
    } else {
        stderr.to_owned()
    }
}

/// Install a spawn-hook for changed-file git subprocesses.
pub fn set_spawn_hook(hook: ChangedFilesSpawnHook) {
    let _ = SPAWN_HOOK.set(hook);
}

/// Validate a user-supplied git ref before passing it to git.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    if s.is_empty() {
        return Err("git ref cannot be empty".to_string());
    }
    if s.starts_with('-') {
        return Err("git ref cannot start with '-'".to_string());
    }
    let mut in_braces = false;
    for c in s.chars() {
        match c {
            '{' => in_braces = true,
            '}' => in_braces = false,
            ':' | ' ' if in_braces => {}
            c if c.is_ascii_alphanumeric()
                || matches!(c, '.' | '_' | '-' | '/' | '~' | '^' | '@' | '{' | '}') => {}
            _ => return Err(format!("git ref contains disallowed character: '{c}'")),
        }
    }
    if in_braces {
        return Err("git ref has unclosed '{'".to_string());
    }
    Ok(s)
}

/// Resolve the canonical git toplevel for `cwd`.
pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, &["rev-parse", "--show-toplevel"]))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --show-toplevel returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// Resolve the canonical git common directory for `cwd`.
pub fn resolve_git_common_dir(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    let output = spawn_output(&mut git_command(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ChangedFilesError::GitFailed(
            "git rev-parse --git-common-dir returned empty output".to_owned(),
        ));
    }

    let path = PathBuf::from(trimmed);
    Ok(dunce::canonicalize(&path).unwrap_or(path))
}

/// Get files changed since a git ref.
pub fn try_get_changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let toplevel = resolve_git_toplevel(root)?;
    try_get_changed_files_with_toplevel(root, &toplevel, git_ref)
}

/// Resolve changed files for a git ref relative to a project root.
///
/// # Errors
///
/// Returns an error when git cannot resolve the ref or repository state.
pub fn changed_files(root: &Path, git_ref: &str) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    try_get_changed_files(root, git_ref)
}

/// Get changed files and the git toplevel used to resolve them.
pub fn try_get_changed_files_with_toplevel(
    cwd: &Path,
    toplevel: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;

    let mut files = collect_git_paths(
        cwd,
        toplevel,
        &[
            "diff",
            "--name-only",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    )?;
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["diff", "--name-only", "HEAD"],
    )?);
    files.extend(collect_git_paths(
        cwd,
        toplevel,
        &["ls-files", "--full-name", "--others", "--exclude-standard"],
    )?);
    Ok(files)
}

/// Return the raw git diff for a ref.
pub fn try_get_changed_diff(root: &Path, git_ref: &str) -> Result<String, ChangedFilesError> {
    validate_git_ref(git_ref).map_err(ChangedFilesError::InvalidRef)?;
    let output = spawn_output(&mut git_command(
        root,
        &[
            "diff",
            "--relative",
            "--unified=0",
            "--end-of-options",
            &format!("{git_ref}...HEAD"),
        ],
    ))
    .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get changed files if git can resolve them, otherwise return `None`.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "intentional user-facing warning for the CLI's --changed-since fallback path; typed callers use try_get_changed_files instead"
)]
pub fn get_changed_files(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    match try_get_changed_files(root, git_ref) {
        Ok(files) => Some(files),
        Err(ChangedFilesError::InvalidRef(e)) => {
            eprintln!("Warning: --changed-since ignored: invalid git ref: {e}");
            None
        }
        Err(ChangedFilesError::GitMissing(e)) => {
            eprintln!("Warning: --changed-since ignored: failed to run git: {e}");
            None
        }
        Err(ChangedFilesError::NotARepository) => {
            eprintln!("Warning: --changed-since ignored: not a git repository");
            None
        }
        Err(ChangedFilesError::GitFailed(stderr)) => {
            eprintln!("Warning: --changed-since failed for ref '{git_ref}': {stderr}");
            None
        }
    }
}

fn spawn_output(command: &mut Command) -> std::io::Result<Output> {
    if let Some(hook) = SPAWN_HOOK.get() {
        hook(command)
    } else {
        command.output()
    }
}

fn collect_git_paths(
    cwd: &Path,
    toplevel: &Path,
    args: &[&str],
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    let output = spawn_output(&mut git_command(cwd, args))
        .map_err(|e| ChangedFilesError::GitMissing(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.contains("not a git repository") {
            ChangedFilesError::NotARepository
        } else {
            ChangedFilesError::GitFailed(stderr.trim().to_owned())
        });
    }

    #[cfg(windows)]
    let normalise_segment = |line: &str| line.replace('/', "\\");
    #[cfg(not(windows))]
    let normalise_segment = |line: &str| line.to_owned();

    let files = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| toplevel.join(normalise_segment(line)))
        .collect();

    Ok(files)
}

#[expect(
    clippy::disallowed_methods,
    reason = "canonical engine-owned git spawn wrapper for changed-file orchestration"
)]
fn git_command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    clear_ambient_git_env(&mut command);
    command.args(args).current_dir(cwd);
    command
}

/// Scope dead-code results to findings affected by changed files.
///
/// Dependency-level issues stay unfiltered because whether a dependency is
/// unused is a graph-global fact, not a changed-file-local fact.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_results_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let cf = normalize_changed_files_set(changed_files);
    classify_changed_file_filter_fields(results);
    retain_basic_issue_findings_by_changed_path(results, &cf);
    retain_graph_findings_by_changed_files(results, &cf);
    retain_boundary_policy_and_suppression_findings(results, &cf);
    retain_security_and_workspace_findings(results, &cf);
    retain_framework_findings_by_changed_files(results, &cf);
}

fn classify_changed_file_filter_fields(results: &AnalysisResults) {
    let AnalysisResults {
        unused_files: _unused_files,
        unused_exports: _unused_exports,
        unused_types: _unused_types,
        private_type_leaks: _private_type_leaks,
        unused_dependencies: _unused_dependencies,
        unused_dev_dependencies: _unused_dev_dependencies,
        unused_optional_dependencies: _unused_optional_dependencies,
        unused_enum_members: _unused_enum_members,
        unused_class_members: _unused_class_members,
        unused_store_members: _unused_store_members,
        unresolved_imports: _unresolved_imports,
        unlisted_dependencies: _unlisted_dependencies,
        duplicate_exports: _duplicate_exports,
        type_only_dependencies: _type_only_dependencies,
        test_only_dependencies: _test_only_dependencies,
        dev_dependencies_in_production: _dev_dependencies_in_production,
        circular_dependencies: _circular_dependencies,
        re_export_cycles: _re_export_cycles,
        boundary_violations: _boundary_violations,
        boundary_coverage_violations: _boundary_coverage_violations,
        boundary_call_violations: _boundary_call_violations,
        policy_violations: _policy_violations,
        stale_suppressions: _stale_suppressions,
        unused_catalog_entries: _unused_catalog_entries,
        empty_catalog_groups: _empty_catalog_groups,
        unresolved_catalog_references: _unresolved_catalog_references,
        unused_dependency_overrides: _unused_dependency_overrides,
        misconfigured_dependency_overrides: _misconfigured_dependency_overrides,
        invalid_client_exports: _invalid_client_exports,
        mixed_client_server_barrels: _mixed_client_server_barrels,
        misplaced_directives: _misplaced_directives,
        unprovided_injects: _unprovided_injects,
        unrendered_components: _unrendered_components,
        route_collisions: _route_collisions,
        dynamic_segment_name_conflicts: _dynamic_segment_name_conflicts,
        unused_component_props: _unused_component_props,
        unused_component_emits: _unused_component_emits,
        unused_component_inputs: _unused_component_inputs,
        unused_component_outputs: _unused_component_outputs,
        unused_svelte_events: _unused_svelte_events,
        unused_server_actions: _unused_server_actions,
        unused_load_data_keys: _unused_load_data_keys,
        unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
        prop_drilling_chains: _prop_drilling_chains,
        thin_wrappers: _thin_wrappers,
        duplicate_prop_shapes: _duplicate_prop_shapes,
        suppression_count: _suppression_count,
        unused_component_props_exempted: _unused_component_props_exempted,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
        render_fan_in: _render_fan_in,
        react_component_intel: _react_component_intel,
    } = results;
}

fn retain_basic_issue_findings_by_changed_path(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_by_changed_path(&mut results.unused_files, changed_files, |f| &f.file.path);
    retain_by_changed_path(&mut results.unused_exports, changed_files, |e| {
        &e.export.path
    });
    retain_by_changed_path(&mut results.unused_types, changed_files, |e| &e.export.path);
    retain_by_changed_path(&mut results.private_type_leaks, changed_files, |e| {
        &e.leak.path
    });
    retain_by_changed_path(&mut results.unused_enum_members, changed_files, |m| {
        &m.member.path
    });
    retain_by_changed_path(&mut results.unused_class_members, changed_files, |m| {
        &m.member.path
    });
    retain_by_changed_path(&mut results.unused_store_members, changed_files, |m| {
        &m.member.path
    });
    retain_by_changed_path(&mut results.unresolved_imports, changed_files, |i| {
        &i.import.path
    });
}

fn retain_graph_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_unlisted_dependencies_by_import_site(&mut results.unlisted_dependencies, changed_files);
    retain_duplicate_exports_by_changed_locations(&mut results.duplicate_exports, changed_files);
    retain_circular_dependencies_by_changed_file(&mut results.circular_dependencies, changed_files);
    retain_re_export_cycles_by_changed_file(&mut results.re_export_cycles, changed_files);
}

fn retain_boundary_policy_and_suppression_findings(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_by_changed_path(&mut results.boundary_violations, changed_files, |v| {
        &v.violation.from_path
    });
    retain_by_changed_path(
        &mut results.boundary_coverage_violations,
        changed_files,
        |v| &v.violation.path,
    );
    retain_by_changed_path(&mut results.boundary_call_violations, changed_files, |v| {
        &v.violation.path
    });
    retain_by_changed_path(&mut results.policy_violations, changed_files, |v| {
        &v.violation.path
    });
    retain_by_changed_path(&mut results.stale_suppressions, changed_files, |s| &s.path);
}

fn retain_security_and_workspace_findings(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_security_findings_by_changed_path(&mut results.security_findings, changed_files);
    retain_by_changed_path(
        &mut results.security_unresolved_callee_diagnostics,
        changed_files,
        |d| &d.path,
    );
    retain_by_changed_path(
        &mut results.unresolved_catalog_references,
        changed_files,
        |r| &r.reference.path,
    );
    results
        .empty_catalog_groups
        .retain(|g| normalized_set_contains_path(changed_files, &g.group.path));
    retain_by_changed_path(
        &mut results.unused_dependency_overrides,
        changed_files,
        |o| &o.entry.path,
    );
    retain_by_changed_path(
        &mut results.misconfigured_dependency_overrides,
        changed_files,
        |o| &o.entry.path,
    );
}

fn retain_framework_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_client_boundary_findings_by_changed_files(results, changed_files);
    retain_component_contract_findings_by_changed_files(results, changed_files);
    retain_react_health_findings_by_changed_files(results, changed_files);
    retain_nextjs_findings_by_changed_files(results, changed_files);
}

fn retain_client_boundary_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let AnalysisResults {
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
        ..
    } = results;

    retain_by_changed_path(invalid_client_exports, changed_files, |e| &e.export.path);
    retain_by_changed_path(mixed_client_server_barrels, changed_files, |b| {
        &b.barrel.path
    });
    retain_by_changed_path(misplaced_directives, changed_files, |d| {
        &d.directive_site.path
    });
}

fn retain_component_contract_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let AnalysisResults {
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_server_actions,
        unused_load_data_keys,
        ..
    } = results;

    retain_by_changed_path(unprovided_injects, changed_files, |i| &i.inject.path);
    retain_by_changed_path(unrendered_components, changed_files, |c| &c.component.path);
    retain_by_changed_path(unused_component_props, changed_files, |p| &p.prop.path);
    retain_by_changed_path(unused_component_emits, changed_files, |e| &e.emit.path);
    retain_by_changed_path(unused_component_inputs, changed_files, |i| &i.input.path);
    retain_by_changed_path(unused_component_outputs, changed_files, |o| &o.output.path);
    retain_by_changed_path(unused_svelte_events, changed_files, |e| &e.event.path);
    retain_by_changed_path(unused_server_actions, changed_files, |a| &a.action.path);
    retain_by_changed_path(unused_load_data_keys, changed_files, |k| &k.key.path);
}

fn retain_react_health_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let AnalysisResults {
        prop_drilling_chains,
        thin_wrappers,
        duplicate_prop_shapes,
        ..
    } = results;

    retain_prop_drilling_chains_by_anchor(prop_drilling_chains, changed_files);
    retain_by_changed_path(thin_wrappers, changed_files, |w| &w.wrapper.file);
    retain_duplicate_prop_shapes_by_anchor(duplicate_prop_shapes, changed_files);
}

fn retain_nextjs_findings_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    let AnalysisResults {
        route_collisions,
        dynamic_segment_name_conflicts,
        ..
    } = results;

    retain_by_changed_path(route_collisions, changed_files, |c| &c.collision.path);
    retain_by_changed_path(dynamic_segment_name_conflicts, changed_files, |c| {
        &c.conflict.path
    });
}

fn retain_unlisted_dependencies_by_import_site(
    dependencies: &mut Vec<UnlistedDependencyFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    dependencies.retain(|dependency| {
        dependency
            .dep
            .imported_from
            .iter()
            .any(|site| contains_normalized(changed_files, &site.path))
    });
}

fn retain_duplicate_exports_by_changed_locations(
    duplicate_exports: &mut Vec<DuplicateExportFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    for duplicate in &mut *duplicate_exports {
        duplicate
            .export
            .locations
            .retain(|location| contains_normalized(changed_files, &location.path));
    }
    duplicate_exports.retain(|duplicate| duplicate.export.locations.len() >= 2);
}

fn retain_circular_dependencies_by_changed_file(
    cycles: &mut Vec<CircularDependencyFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    cycles.retain(|cycle| {
        cycle
            .cycle
            .files
            .iter()
            .any(|file| contains_normalized(changed_files, file))
    });
}

fn retain_re_export_cycles_by_changed_file(
    cycles: &mut Vec<ReExportCycleFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    cycles.retain(|cycle| {
        cycle
            .cycle
            .files
            .iter()
            .any(|file| contains_normalized(changed_files, file))
    });
}

fn retain_security_findings_by_changed_path(
    findings: &mut Vec<SecurityFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    findings.retain(|finding| security_finding_touches_changed_path(finding, changed_files));
}

fn retain_prop_drilling_chains_by_anchor(
    chains: &mut Vec<PropDrillingChainFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    chains.retain(|chain| {
        chain
            .chain
            .hops
            .first()
            .is_some_and(|hop| contains_normalized(changed_files, &hop.file))
    });
}

fn retain_duplicate_prop_shapes_by_anchor(
    shapes: &mut Vec<DuplicatePropShapeFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    retain_by_changed_path(shapes, changed_files, |shape| &shape.shape.file);
}

fn retain_by_changed_path<T>(
    items: &mut Vec<T>,
    changed_files: &FxHashSet<PathBuf>,
    path: impl Fn(&T) -> &Path,
) {
    items.retain(|item| contains_normalized(changed_files, path(item)));
}

fn security_finding_touches_changed_path(
    finding: &SecurityFinding,
    changed_files: &FxHashSet<PathBuf>,
) -> bool {
    contains_normalized(changed_files, &finding.path)
        || finding
            .trace
            .iter()
            .any(|hop| contains_normalized(changed_files, &hop.path))
        || finding.reachability.as_ref().is_some_and(|reachability| {
            reachability
                .untrusted_source_trace
                .iter()
                .any(|hop| contains_normalized(changed_files, &hop.path))
        })
}

fn normalize_changed_files_set(changed_files: &FxHashSet<PathBuf>) -> FxHashSet<PathBuf> {
    changed_files
        .iter()
        .map(|p| dunce::simplified(p).to_path_buf())
        .collect()
}

fn contains_normalized(normalized: &FxHashSet<PathBuf>, path: &Path) -> bool {
    normalized.contains(dunce::simplified(path))
}

fn normalized_set_contains_path(normalized: &FxHashSet<PathBuf>, path: &Path) -> bool {
    contains_normalized(normalized, path)
        || (path.is_relative() && normalized.iter().any(|changed| changed.ends_with(path)))
}

/// Scope duplication groups to clone groups touching at least one changed file.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_duplication_by_changed_files(
    report: &mut DuplicationReport,
    changed_files: &FxHashSet<PathBuf>,
    root: &Path,
) {
    let cf = normalize_changed_files_set(changed_files);
    report.clone_groups.retain(|group| {
        group
            .instances
            .iter()
            .any(|instance| contains_normalized(&cf, &instance.file))
    });
    duplicates::refresh_clone_families(report, root);
    report.stats = duplicates::recompute_stats(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::{
        duplicates::{CloneGroup, CloneInstance, DuplicationStats},
        output_dead_code::{
            EmptyCatalogGroupFinding, UnusedDependencyFinding, UnusedExportFinding,
            UnusedFileFinding,
        },
        results::{
            DependencyLocation, EmptyCatalogGroup, UnusedDependency, UnusedExport, UnusedFile,
        },
    };

    #[test]
    fn validate_git_ref_rejects_option_like_ref() {
        assert!(validate_git_ref("--upload-pack=evil").is_err());
        assert!(validate_git_ref("-flag").is_err());
    }

    #[test]
    fn validate_git_ref_allows_reflog_relative_date() {
        assert!(validate_git_ref("HEAD@{1 week ago}").is_ok());
    }

    #[test]
    fn git_command_clears_parent_git_environment() {
        let command = git_command(Path::new("."), &["status"]);
        let envs: Vec<_> = command.get_envs().collect();

        for var in AMBIENT_GIT_ENV_VARS {
            assert!(
                envs.iter()
                    .any(|(key, value)| key.to_str() == Some(*var) && value.is_none()),
                "{var} should be cleared from the command env",
            );
        }
    }

    #[test]
    fn try_get_changed_files_not_a_repository() {
        let temp = tempfile::tempdir().expect("tempdir");
        let result = try_get_changed_files(temp.path(), "main");
        assert!(matches!(result, Err(ChangedFilesError::NotARepository)));
    }

    #[test]
    fn changed_files_error_describe_matches_core_contract() {
        assert_eq!(
            ChangedFilesError::InvalidRef("bad ref".to_string()).describe(),
            "invalid git ref: bad ref"
        );
        assert_eq!(
            ChangedFilesError::GitMissing("not found".to_string()).describe(),
            "failed to run git: not found"
        );
        assert_eq!(
            ChangedFilesError::NotARepository.describe(),
            "not a git repository"
        );
        assert!(
            ChangedFilesError::GitFailed("unknown revision main".to_string())
                .describe()
                .contains("fetch-depth: 0")
        );
    }

    #[test]
    fn filter_results_keeps_only_changed_file_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/repo/a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/repo/b.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/repo/a.ts"),
                export_name: "foo".to_owned(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/repo/a.ts"));

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            PathBuf::from("/repo/a.ts")
        );
        assert_eq!(results.unused_exports.len(), 1);
    }

    #[test]
    fn filter_results_preserves_graph_global_dependency_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_owned(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/package.json"),
                line: 3,
                used_in_workspaces: Vec::new(),
            }));

        let changed = FxHashSet::default();
        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.unused_dependencies.len(), 1);
    }

    #[test]
    fn filter_results_keeps_relative_manifest_finding_when_manifest_changed() {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "legacy".to_owned(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 4,
            }));

        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/repo/pnpm-workspace.yaml"));

        filter_results_by_changed_files(&mut results, &changed);

        assert_eq!(results.empty_catalog_groups.len(), 1);
    }

    #[test]
    fn filter_duplication_keeps_groups_with_changed_instances_and_recomputes_stats() {
        let mut report = DuplicationReport {
            clone_groups: vec![
                CloneGroup {
                    instances: vec![
                        CloneInstance {
                            file: PathBuf::from("/repo/a.ts"),
                            start_line: 1,
                            end_line: 5,
                            start_col: 0,
                            end_col: 10,
                            fragment: "code".to_owned(),
                        },
                        CloneInstance {
                            file: PathBuf::from("/repo/b.ts"),
                            start_line: 1,
                            end_line: 5,
                            start_col: 0,
                            end_col: 10,
                            fragment: "code".to_owned(),
                        },
                    ],
                    token_count: 20,
                    line_count: 5,
                },
                CloneGroup {
                    instances: vec![
                        CloneInstance {
                            file: PathBuf::from("/repo/c.ts"),
                            start_line: 1,
                            end_line: 5,
                            start_col: 0,
                            end_col: 10,
                            fragment: "other".to_owned(),
                        },
                        CloneInstance {
                            file: PathBuf::from("/repo/d.ts"),
                            start_line: 1,
                            end_line: 5,
                            start_col: 0,
                            end_col: 10,
                            fragment: "other".to_owned(),
                        },
                    ],
                    token_count: 20,
                    line_count: 5,
                },
            ],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats {
                total_files: 4,
                files_with_clones: 4,
                total_lines: 100,
                duplicated_lines: 20,
                total_tokens: 200,
                duplicated_tokens: 80,
                clone_groups: 2,
                clone_instances: 4,
                duplication_percentage: 20.0,
                clone_groups_below_min_occurrences: 0,
            },
        };

        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/repo/a.ts"));

        filter_duplication_by_changed_files(&mut report, &changed, Path::new("/repo"));

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.stats.clone_groups, 1);
        assert_eq!(report.stats.clone_instances, 2);
    }
}
