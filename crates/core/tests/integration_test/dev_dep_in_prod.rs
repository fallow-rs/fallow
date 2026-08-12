use super::common::{create_config, fixture_path};

/// The promote-side mirror of `test-only-dependency`: a devDependency imported
/// by production code with a runtime/value import should be reported so it can
/// move to `dependencies`.
#[test]
fn dev_dependency_used_in_production_detected() {
    let root = fixture_path("dev-dep-in-prod");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        flagged.contains(&"yaml"),
        "yaml is value-imported from a production file and should be flagged, found: {flagged:?}"
    );

    assert!(
        flagged.contains(&"runtime-helper"),
        "a devDependency reached through an indirect npm start script should be flagged, found: {flagged:?}"
    );

    assert!(
        !flagged.contains(&"svgo"),
        "a devDependency imported only by build tooling must not be flagged, found: {flagged:?}"
    );

    assert!(
        !results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with("scripts/build.ts")),
        "build tooling referenced from package.json must remain reachable for dead-code analysis"
    );

    // type-fest is imported from production code but ONLY via `import type`,
    // which is erased at build time, so it must NOT be flagged.
    assert!(
        !flagged.contains(&"type-fest"),
        "type-only production imports must not be flagged, found: {flagged:?}"
    );

    // vitest is imported only from a test file (excluded from production), so
    // it is correctly placed in devDependencies and must NOT be flagged.
    assert!(
        !flagged.contains(&"vitest"),
        "test-only imports must not be flagged (that is the demote rule's job), found: {flagged:?}"
    );

    // left-pad is a real production dependency, not a devDependency, so it is
    // out of scope for this rule.
    assert!(
        !flagged.contains(&"left-pad"),
        "production dependencies are out of scope, found: {flagged:?}"
    );
}
