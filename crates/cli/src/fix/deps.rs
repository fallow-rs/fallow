use rustc_hash::FxHashMap;
use std::path::Path;

use fallow_config::OutputFormat;
use fallow_core::results::UnusedDependency;

use super::io::atomic_write;

/// Apply dependency fixes to package.json files (root and workspace), returning JSON fix entries.
pub(super) fn apply_dependency_fixes(
    root: &Path,
    results: &fallow_core::results::AnalysisResults,
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> bool {
    let mut had_write_error = false;

    if results.unused_dependencies.is_empty()
        && results.unused_dev_dependencies.is_empty()
        && results.unused_optional_dependencies.is_empty()
    {
        return had_write_error;
    }

    // Group all unused deps by their package.json path so we can batch edits per file
    let mut deps_by_pkg: FxHashMap<&Path, Vec<(&str, &str)>> = FxHashMap::default();
    for dep in &results.unused_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "dependencies");
    }
    for dep in &results.unused_dev_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "devDependencies");
    }
    for dep in &results.unused_optional_dependencies {
        queue_dependency_removal(&mut deps_by_pkg, &dep.dep, "optionalDependencies");
    }

    let _ = root; // root was previously used to construct the path; now deps carry their own path

    for (pkg_path, removals) in &deps_by_pkg {
        if let Ok(content) = std::fs::read_to_string(pkg_path)
            && let Ok(mut pkg_value) = serde_json::from_str::<serde_json::Value>(&content)
        {
            let mut changed = false;

            for &(package_name, location) in removals {
                if let Some(deps) = pkg_value.get_mut(location)
                    && let Some(obj) = deps.as_object_mut()
                    && obj.remove(package_name).is_some()
                {
                    if dry_run {
                        if !matches!(output, OutputFormat::Json) {
                            eprintln!(
                                "Would remove `{package_name}` from {location} in {}",
                                pkg_path.display()
                            );
                        }
                        fixes.push(serde_json::json!({
                            "type": "remove_dependency",
                            "package": package_name,
                            "location": location,
                            "file": pkg_path.display().to_string(),
                        }));
                    } else {
                        changed = true;
                        fixes.push(serde_json::json!({
                            "type": "remove_dependency",
                            "package": package_name,
                            "location": location,
                            "file": pkg_path.display().to_string(),
                            "applied": true,
                        }));
                    }
                }
            }

            if changed && !dry_run {
                match serde_json::to_string_pretty(&pkg_value) {
                    Ok(new_json) => {
                        let pkg_content = new_json + "\n";
                        if let Err(e) = atomic_write(pkg_path, pkg_content.as_bytes()) {
                            had_write_error = true;
                            eprintln!("Error: failed to write {}: {e}", pkg_path.display());
                        }
                    }
                    Err(e) => {
                        had_write_error = true;
                        eprintln!("Error: failed to serialize {}: {e}", pkg_path.display());
                    }
                }
            }
        }
    }

    had_write_error
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

    #[test]
    fn dependency_fix_dry_run_does_not_modify_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original =
            r#"{"dependencies": {"lodash": "^4.0.0"}, "devDependencies": {"jest": "^29.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 5,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        apply_dependency_fixes(root, &results, OutputFormat::Json, true, &mut fixes);

        // package.json should not change
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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 5,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 5,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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
        let results = fallow_core::results::AnalysisResults::default();
        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);
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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dev_dependencies.push(
            fallow_core::results::UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: fallow_core::results::DependencyLocation::DevDependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_optional_dependencies.push(
            fallow_core::results::UnusedOptionalDependencyFinding::with_actions(UnusedDependency {
                package_name: "sharp".into(),
                location: fallow_core::results::DependencyLocation::OptionalDependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );
        results.unused_dev_dependencies.push(
            fallow_core::results::UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: fallow_core::results::DependencyLocation::DevDependencies,
                path: pkg_path.clone(),
                line: 5,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "nonexistent".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path,
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        // No fix was applied (dep not found)
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_dry_run_with_human_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json");
        let original = r#"{"dependencies": {"lodash": "^4.0.0"}}"#;
        std::fs::write(&pkg_path, original).unwrap();

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        apply_dependency_fixes(root, &results, OutputFormat::Human, true, &mut fixes);

        // File should not be modified
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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path,
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

        // Invalid JSON: the let-chain fails, so this path is just skipped
        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn dependency_fix_nonexistent_package_json_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_path = root.join("package.json"); // Does not exist

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path,
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path,
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        let had_error =
            apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

        assert!(!had_error);
        // No dependencies section -> no fix
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

        let mut results = fallow_core::results::AnalysisResults::default();
        results.unused_dependencies.push(
            fallow_core::results::UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_core::results::DependencyLocation::Dependencies,
                path: pkg_path.clone(),
                line: 3,
                used_in_workspaces: Vec::new(),
            }),
        );

        let mut fixes = Vec::new();
        apply_dependency_fixes(root, &results, OutputFormat::Human, false, &mut fixes);

        let content = std::fs::read_to_string(&pkg_path).unwrap();
        assert!(content.ends_with('\n'), "output should end with newline");
    }
}
