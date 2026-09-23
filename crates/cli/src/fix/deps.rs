use rustc_hash::FxHashMap;
use std::path::Path;

use fallow_config::OutputFormat;
use fallow_types::output_dead_code::ReachabilityCaveat;
use fallow_types::results::UnusedDependency;

use super::plan::{CapturedHashes, FixPlan, SkipReason};

/// One queued `package.json` edit: which package to drop from which array,
/// plus the reachability caveats the analysis put on the finding.
///
/// `remove-dependency` is the most destructive write `fallow fix` performs,
/// and a dependency is only reported unused when NO module imports its
/// specifier. A module that did not parse cleanly can hide exactly that
/// import, so a caveated finding is withheld instead of applied, in the same
/// intentional family as the off-graph export skip: the exit code does not
/// move and `fallow dead-code` keeps reporting the finding.
struct QueuedRemoval<'a> {
    package_name: &'a str,
    location: &'static str,
    caveats: &'a [ReachabilityCaveat],
}

/// Apply dependency fixes to package.json files and return JSON fix entries.
///
/// `hashes` is accepted for signature uniformity; `package.json` files are
/// re-read and reparsed here, so the hash check is a no-op.
pub(super) struct DependencyFixInput<'a> {
    pub(super) root: &'a Path,
    pub(super) results: &'a fallow_types::results::AnalysisResults,
    pub(super) hashes: &'a CapturedHashes,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

pub(super) fn apply_dependency_fixes(input: &mut DependencyFixInput<'_>) {
    let _ = input.hashes; // see doc above

    if input.results.unused_dependencies.is_empty()
        && input.results.unused_dev_dependencies.is_empty()
        && input.results.unused_optional_dependencies.is_empty()
    {
        return;
    }

    let mut deps_by_pkg: FxHashMap<&Path, Vec<QueuedRemoval<'_>>> = FxHashMap::default();
    for dep in &input.results.unused_dependencies {
        queue_dependency_removal(
            &mut deps_by_pkg,
            &dep.dep,
            "dependencies",
            &dep.reachability_caveats,
        );
    }
    for dep in &input.results.unused_dev_dependencies {
        queue_dependency_removal(
            &mut deps_by_pkg,
            &dep.dep,
            "devDependencies",
            &dep.reachability_caveats,
        );
    }
    for dep in &input.results.unused_optional_dependencies {
        queue_dependency_removal(
            &mut deps_by_pkg,
            &dep.dep,
            "optionalDependencies",
            &dep.reachability_caveats,
        );
    }

    for (&pkg_path, removals) in &deps_by_pkg {
        process_package_dependency_removals(input, pkg_path, removals.as_slice());
    }
}

/// The project-root-relative, forward-slash form of a `package.json` path.
///
/// Every other fix entry reports a project-relative path: `remove_export`
/// strips the root, and the catalog fixers normalize separators on top of
/// that. A dependency entry used to carry the absolute host path instead, so a
/// single `fixes` array mixed `"path": "src/index.ts"` with an absolute
/// `"file"`, and an agent holding fallow to its project-root-relative contract
/// read a machine path that means nothing on its side of the wire. The
/// absolute path the applier needs stays on the orchestrator-private
/// `__target` field.
fn relative_package_path(root: &Path, pkg_path: &Path) -> String {
    pkg_path
        .strip_prefix(root)
        .unwrap_or(pkg_path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read, edit, and (when not dry-run) stage one `package.json` for the
/// queued dependency removals targeting it. Pushes a fix entry per removed
/// package and corrects `applied` to false on a serialization failure.
fn process_package_dependency_removals(
    input: &mut DependencyFixInput<'_>,
    pkg_path: &Path,
    removals: &[QueuedRemoval<'_>],
) {
    let Ok(content) = std::fs::read_to_string(pkg_path) else {
        return;
    };
    let Ok(mut pkg_value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    let mut changed = false;
    for removal in removals {
        if !removal.caveats.is_empty() {
            // Mirror the applied path, which only reports a removal it could
            // actually perform: a finding for a package the manifest no longer
            // declares must not be counted as a withheld write.
            if dependency_is_declared(&pkg_value, removal) {
                push_withheld_dependency_entry(input, pkg_path, removal);
            }
            continue;
        }
        let package_name = removal.package_name;
        let location = removal.location;
        if let Some(deps) = pkg_value.get_mut(location)
            && let Some(obj) = deps.as_object_mut()
            && obj.remove(package_name).is_some()
        {
            let relative = relative_package_path(input.root, pkg_path);
            if input.dry_run {
                if !matches!(input.output, OutputFormat::Json) {
                    eprintln!("Would remove `{package_name}` from {location} in {relative}");
                }
                input.fixes.push(serde_json::json!({
                    "type": "remove_dependency",
                    "package": package_name,
                    "location": location,
                    "file": relative,
                }));
            } else {
                changed = true;
                input.fixes.push(serde_json::json!({
                    "type": "remove_dependency",
                    "package": package_name,
                    "location": location,
                    "file": relative,
                    "applied": true,
                    "__target": pkg_path.display().to_string(),
                }));
            }
        }
    }

    if changed && !input.dry_run {
        stage_package_dependency_edit(input, pkg_path, &content, &pkg_value);
    }
}

/// Whether `package.json` still declares the queued package in the array the
/// finding named.
fn dependency_is_declared(pkg_value: &serde_json::Value, removal: &QueuedRemoval<'_>) -> bool {
    pkg_value
        .get(removal.location)
        .and_then(serde_json::Value::as_object)
        .is_some_and(|deps| deps.contains_key(removal.package_name))
}

/// Emit the skip entry for a dependency whose verdict the run itself flagged,
/// and say so on stderr. The entry carries both the shared `skip_reason` an
/// agent already branches on and the caveat tokens, so a caller can gate on
/// the marker instead of inferring it from the reason string.
fn push_withheld_dependency_entry(
    input: &mut DependencyFixInput<'_>,
    pkg_path: &Path,
    removal: &QueuedRemoval<'_>,
) {
    let package_name = removal.package_name;
    let location = removal.location;
    let relative = relative_package_path(input.root, pkg_path);
    // Gated exactly like the `Would remove` line above: format only, never
    // `--quiet`. A removal fallow WILL make survived `--quiet` while a removal
    // it REFUSED did not, so the quiet plan read as complete when it was
    // partial. A refusal is a measurement, not progress chatter.
    if !matches!(input.output, OutputFormat::Json) {
        // The message names the caveat the finding actually carries. Naming
        // one cause ("did not parse cleanly") sent a reader of a run degraded
        // by the size guard hunting for parse errors that do not exist.
        let reason = fallow_types::output_dead_code::caveat_labels(removal.caveats)
            .unwrap_or_else(|| "incomplete analysis".to_string());
        eprintln!(
            "Kept `{package_name}` in {location} in {relative}: {reason}, so the import that would credit it may never have been seen."
        );
    }
    let tokens: Vec<&str> = removal
        .caveats
        .iter()
        .map(|caveat| ReachabilityCaveat::token(*caveat))
        .collect();
    input.fixes.push(serde_json::json!({
        "type": "remove_dependency",
        "package": package_name,
        "location": location,
        "file": relative,
        "applied": false,
        "skipped": true,
        "skip_reason": SkipReason::LowConfidenceIncompleteAnalysis.as_wire_str(),
        "reachability_caveats": tokens,
    }));
}

/// Serialize the edited `package.json` value and stage it for write, or
/// flip the corresponding fix entries to `applied: false` on failure.
fn stage_package_dependency_edit(
    input: &mut DependencyFixInput<'_>,
    pkg_path: &Path,
    original_content: &str,
    pkg_value: &serde_json::Value,
) {
    match serde_json::to_string_pretty(pkg_value) {
        Ok(new_json) => {
            let pkg_content = new_json + "\n";
            input.plan.stage_existing(
                pkg_path.to_path_buf(),
                original_content.as_bytes(),
                pkg_content.into_bytes(),
            );
        }
        Err(e) => {
            eprintln!(
                "Error: failed to serialize {}: {e}",
                relative_package_path(input.root, pkg_path)
            );
            for entry in input.fixes.iter_mut() {
                let matches = entry
                    .get("__target")
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| t == pkg_path.display().to_string());
                if matches {
                    entry["applied"] = serde_json::json!(false);
                }
            }
        }
    }
}

fn queue_dependency_removal<'a>(
    deps_by_pkg: &mut FxHashMap<&'a Path, Vec<QueuedRemoval<'a>>>,
    dep: &'a UnusedDependency,
    location: &'static str,
    caveats: &'a [ReachabilityCaveat],
) {
    if dep.used_in_workspaces.is_empty() {
        deps_by_pkg
            .entry(&dep.path)
            .or_default()
            .push(QueuedRemoval {
                package_name: &dep.package_name,
                location,
                caveats,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_fix_deps(
        root: &Path,
        results: &fallow_types::results::AnalysisResults,
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> bool {
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = CapturedHashes::default();
        apply_dependency_fixes(&mut DependencyFixInput {
            root,
            results,
            hashes: &hashes,
            plan: &mut plan,
            output,
            dry_run,
            fixes,
        });
        if dry_run {
            return false;
        }
        !plan.commit().failed.is_empty()
    }

    fn unused_dep(pkg_path: &Path, name: &str) -> UnusedDependency {
        UnusedDependency {
            package_name: name.into(),
            location: fallow_types::results::DependencyLocation::Dependencies,
            path: pkg_path.to_path_buf(),
            line: 5,
            used_in_workspaces: Vec::new(),
        }
    }

    /// `remove-dependency` is the most destructive write `fallow fix` has, and
    /// a package is reported unused only because no module imported it. A file
    /// that did not parse cleanly can hide exactly that import, so a caveated
    /// finding must leave `package.json` untouched.
    #[test]
    fn a_caveated_dependency_is_not_removed_from_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies": {"lodash": "^4.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        let mut finding = fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
            unused_dep(&pkg_path, "lodash"),
        );
        finding.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_dependencies.push(finding);

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Json, false, &mut fixes);

        assert_eq!(
            std::fs::read_to_string(&pkg_path).unwrap(),
            original,
            "the manifest must be byte-identical after a withheld removal"
        );
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["applied"], false);
        assert_eq!(fixes[0]["skipped"], true);
        assert_eq!(
            fixes[0]["skip_reason"],
            "low_confidence_incomplete_analysis"
        );
        assert_eq!(
            fixes[0]["reachability_caveats"],
            serde_json::json!(["incomplete-import-graph"]),
            "the entry carries the marker so a caller gates on it instead of inferring"
        );
    }

    /// The preview an agent runs first must not plan the removal either.
    #[test]
    fn a_caveated_dependency_is_withheld_in_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies": {"lodash": "^4.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        let mut finding = fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
            unused_dep(&pkg_path, "lodash"),
        );
        finding.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_dependencies.push(finding);

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Json, true, &mut fixes);

        assert_eq!(std::fs::read_to_string(&pkg_path).unwrap(), original);
        assert_eq!(
            fixes[0]["skip_reason"],
            "low_confidence_incomplete_analysis"
        );
        assert_ne!(
            fixes[0]["applied"],
            serde_json::json!(true),
            "a preview must never advertise a withheld removal as planned"
        );
    }

    /// The withholding is per finding, not per run: a package with no caveat
    /// still gets removed even when a sibling entry carries one.
    #[test]
    fn an_uncaveated_dependency_is_still_removed_alongside_a_caveated_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"dependencies": {"lodash": "^4.0.0", "left-pad": "^1.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        let mut caveated = fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
            unused_dep(&pkg_path, "lodash"),
        );
        caveated.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        results.unused_dependencies.push(caveated);
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(unused_dep(
                &pkg_path, "left-pad",
            )),
        );

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Json, false, &mut fixes);

        let written = std::fs::read_to_string(&pkg_path).unwrap();
        assert!(written.contains("lodash"), "the caveated package survives");
        assert!(
            !written.contains("left-pad"),
            "the clean package is still removed: {written}"
        );
    }

    #[test]
    fn dependency_fix_dry_run_does_not_modify_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original =
            r#"{"dependencies": {"lodash": "^4.0.0"}, "devDependencies": {"jest": "^29.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Json, true, &mut fixes);

        assert_eq!(std::fs::read_to_string(&pkg_path).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_dependency");
        assert_eq!(fixes[0]["package"], "lodash");
    }

    #[test]
    fn dependency_fix_removes_unused_dep_from_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"dependencies": {"lodash": "^4.0.0", "react": "^18.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let deps = parsed["dependencies"].as_object().unwrap();
        assert!(!deps.contains_key("lodash"));
        assert!(deps.contains_key("react"));
    }

    #[test]
    fn dependency_fix_preserves_manifest_changed_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies":{"lodash":"^4.0.0"}}"#;
        let external = r#"{"dependencies":{"react":"^18.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 1,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );
        let hashes = CapturedHashes::default();
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        apply_dependency_fixes(&mut DependencyFixInput {
            root,
            results: &results,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });
        std::fs::write(&pkg_path, external).unwrap();

        let outcome = plan.commit();

        assert!(outcome.written.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(outcome.failed[0].0, pkg_path);
        assert_eq!(std::fs::read_to_string(&pkg_path).unwrap(), external);
    }

    #[test]
    fn dependency_fix_skips_dep_used_in_another_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("packages/shared/package.json");
        std::fs::create_dir_all(pkg_path.parent().unwrap()).unwrap();
        std::fs::write(
            &pkg_path,
            r#"{"dependencies": {"lodash-es": "^4.17.21", "react": "^18.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash-es".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 5,
                    used_in_workspaces: vec![root.join("packages/consumer")],
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        assert!(fixes.is_empty());
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let deps = parsed["dependencies"].as_object().unwrap();
        assert!(deps.contains_key("lodash-es"));
        assert!(deps.contains_key("react"));
    }

    #[test]
    fn dependency_fix_empty_results_returns_early() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let results = fallow_types::results::AnalysisResults::default();
        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);
        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_removes_dev_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"devDependencies": {"jest": "^29.0.0", "vitest": "^1.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dev_dependencies.push(
            fallow_types::output_dead_code::UnusedDevDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "jest".into(),
                    location: fallow_types::results::DependencyLocation::DevDependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let dev_deps = parsed["devDependencies"].as_object().unwrap();
        assert!(!dev_deps.contains_key("jest"));
        assert!(dev_deps.contains_key("vitest"));
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["location"], "devDependencies");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn dependency_fix_removes_optional_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"optionalDependencies": {"sharp": "^0.33.0", "canvas": "^2.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_optional_dependencies.push(
            fallow_types::output_dead_code::UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "sharp".into(),
                    location: fallow_types::results::DependencyLocation::OptionalDependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let opt_deps = parsed["optionalDependencies"].as_object().unwrap();
        assert!(!opt_deps.contains_key("sharp"));
        assert!(opt_deps.contains_key("canvas"));
    }

    #[test]
    fn dependency_fix_removes_from_multiple_sections() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"dependencies": {"lodash": "^4.0.0"}, "devDependencies": {"jest": "^29.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );
        results.unused_dev_dependencies.push(
            fallow_types::output_dead_code::UnusedDevDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "jest".into(),
                    location: fallow_types::results::DependencyLocation::DevDependencies,
                    path: pkg_path.clone(),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let deps = parsed["dependencies"].as_object().unwrap();
        assert!(!deps.contains_key("lodash"));
        let dev_deps = parsed["devDependencies"].as_object().unwrap();
        assert!(!dev_deps.contains_key("jest"));
        assert_eq!(fixes.len(), 2);
    }

    #[test]
    fn dependency_fix_removes_last_dep_leaves_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(&pkg_path, r#"{"dependencies": {"lodash": "^4.0.0"}}"#).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        let content = std::fs::read_to_string(&pkg_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        let deps = parsed["dependencies"].as_object().unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn dependency_fix_dep_not_in_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies": {"react": "^18.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "nonexistent".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path,
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_dry_run_with_human_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies": {"lodash": "^4.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Human, true, &mut fixes);

        assert_eq!(std::fs::read_to_string(&pkg_path).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn dependency_fix_invalid_json_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(&pkg_path, "not valid json").unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path,
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_nonexistent_package_json_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json"); // Does not exist

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path,
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_missing_section_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"name": "test"}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path,
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        let had_error = run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_output_has_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(
            &pkg_path,
            r#"{"dependencies": {"lodash": "^4.0.0", "react": "^18.0.0"}}"#,
        )
        .unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "lodash".into(),
                    location: fallow_types::results::DependencyLocation::Dependencies,
                    path: pkg_path.clone(),
                    line: 3,
                    used_in_workspaces: Vec::new(),
                },
            ),
        );

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Human, false, &mut fixes);

        let content = std::fs::read_to_string(&pkg_path).unwrap();
        assert!(content.ends_with('\n'), "output should end with newline");
    }

    /// Every `remove_dependency` entry, in all three of its states, must report
    /// the manifest the same way `remove_export` reports a source file: as a
    /// project-relative path. An agent consuming one `fixes` array cannot be
    /// asked to guess which entries are relative to the project and which are
    /// absolute on the machine that ran the analysis.
    #[test]
    fn every_dependency_entry_reports_a_project_relative_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_dir = root.join("packages").join("app");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        let pkg_path = pkg_dir.join("package.json");

        for (dry_run, caveated) in [(true, false), (false, false), (true, true)] {
            std::fs::write(&pkg_path, r#"{"dependencies": {"lodash": "^4.0.0"}}"#).unwrap();

            let mut results = fallow_types::results::AnalysisResults::default();
            let mut finding = fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
                unused_dep(&pkg_path, "lodash"),
            );
            if caveated {
                finding.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
            }
            results.unused_dependencies.push(finding);

            let mut fixes = Vec::new();
            run_fix_deps(root, &results, OutputFormat::Json, dry_run, &mut fixes);

            assert_eq!(fixes.len(), 1, "dry_run={dry_run} caveated={caveated}");
            assert_eq!(
                fixes[0]["file"], "packages/app/package.json",
                "dry_run={dry_run} caveated={caveated}: {}",
                fixes[0]
            );
        }
    }

    /// Relativizing the reported path must not disturb the private correlation
    /// field the applier matches on, which is an absolute host path.
    #[test]
    fn the_applied_entry_keeps_an_absolute_private_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        std::fs::write(&pkg_path, r#"{"dependencies": {"lodash": "^4.0.0"}}"#).unwrap();

        let mut results = fallow_types::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(unused_dep(
                &pkg_path, "lodash",
            )),
        );

        let mut fixes = Vec::new();
        run_fix_deps(root, &results, OutputFormat::Json, false, &mut fixes);

        assert_eq!(fixes[0]["file"], "package.json");
        assert_eq!(
            fixes[0]["__target"],
            serde_json::json!(pkg_path.display().to_string()),
            "the applier resolves the real file through __target"
        );
    }

    /// A manifest outside the project root has no relative form. Reporting the
    /// absolute path is better than reporting a wrong relative one, so the
    /// fallback must be the path itself rather than a truncated stem.
    #[test]
    fn a_manifest_outside_the_root_keeps_its_full_path() {
        let outside = Path::new("/elsewhere/package.json");
        assert_eq!(
            relative_package_path(Path::new("/project"), outside),
            "/elsewhere/package.json"
        );
    }
}
