use rustc_hash::FxHashMap;
use std::path::Path;

use fallow_config::OutputFormat;
use fallow_types::results::UnusedDependency;

use super::plan::{CapturedHashes, FixPlan};

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

    let mut deps_by_pkg: FxHashMap<&Path, Vec<(&str, &str)>> = FxHashMap::default();
    for dep in &input.results.unused_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "dependencies");
    }
    for dep in &input.results.unused_dev_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "devDependencies");
    }
    for dep in &input.results.unused_optional_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "optionalDependencies");
    }

    let _ = input.root; // root was previously used to construct the path; now deps carry their own path

    for (&pkg_path, removals) in &deps_by_pkg {
        process_package_dependency_removals(input, pkg_path, removals.as_slice());
    }
}

/// Read, edit, and (when not dry-run) stage one `package.json` for the
/// queued dependency removals targeting it. Pushes a fix entry per removed
/// package and corrects `applied` to false on a serialization failure.
fn process_package_dependency_removals(
    input: &mut DependencyFixInput<'_>,
    pkg_path: &Path,
    removals: &[(&str, &str)],
) {
    let Ok(content) = std::fs::read_to_string(pkg_path) else {
        return;
    };
    let Ok(mut pkg_value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    let mut changed = false;
    for &(package_name, location) in removals {
        if let Some(deps) = pkg_value.get_mut(location)
            && let Some(obj) = deps.as_object_mut()
            && obj.remove(package_name).is_some()
        {
            if input.dry_run {
                if !matches!(input.output, OutputFormat::Json) {
                    eprintln!(
                        "Would remove `{package_name}` from {location} in {}",
                        pkg_path.display()
                    );
                }
                input.fixes.push(serde_json::json!({
                    "type": "remove_dependency",
                    "package": package_name,
                    "location": location,
                    "file": pkg_path.display().to_string(),
                }));
            } else {
                changed = true;
                input.fixes.push(serde_json::json!({
                    "type": "remove_dependency",
                    "package": package_name,
                    "location": location,
                    "file": pkg_path.display().to_string(),
                    "applied": true,
                    "__target": pkg_path.display().to_string(),
                }));
            }
        }
    }

    if changed && !input.dry_run {
        stage_package_dependency_edit(input, pkg_path, &pkg_value);
    }
}

/// Serialize the edited `package.json` value and stage it for write, or
/// flip the corresponding fix entries to `applied: false` on failure.
fn stage_package_dependency_edit(
    input: &mut DependencyFixInput<'_>,
    pkg_path: &Path,
    pkg_value: &serde_json::Value,
) {
    match serde_json::to_string_pretty(pkg_value) {
        Ok(new_json) => {
            let pkg_content = new_json + "\n";
            input
                .plan
                .stage(pkg_path.to_path_buf(), pkg_content.into_bytes());
        }
        Err(e) => {
            eprintln!("Error: failed to serialize {}: {e}", pkg_path.display());
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
    deps_by_pkg: &mut FxHashMap<&'a Path, Vec<(&'a str, &'static str)>>,
    dep: &'a UnusedDependency,
    location: &'static str,
) {
    if dep.used_in_workspaces.is_empty() {
        deps_by_pkg
            .entry(&dep.path)
            .or_default()
            .push((&dep.package_name, location));
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
        let mut plan = FixPlan::new();
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
}
