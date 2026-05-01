use super::common::{create_config, fixture_path};

#[test]
fn basic_project_detects_unused_files() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // orphan.ts should be detected as unused
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn basic_project_detects_unused_exports() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unusedFunction"),
        "unusedFunction should be detected as unused export, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"anotherUnused"),
        "anotherUnused should be detected as unused export, found: {unused_export_names:?}"
    );
    // usedFunction should NOT be in unused
    assert!(
        !unused_export_names.contains(&"usedFunction"),
        "usedFunction should NOT be detected as unused"
    );
}

#[test]
fn basic_project_detects_unused_types() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"UnusedType"),
        "UnusedType should be detected as unused type, found: {unused_type_names:?}"
    );
    assert!(
        unused_type_names.contains(&"UnusedInterface"),
        "UnusedInterface should be detected as unused type, found: {unused_type_names:?}"
    );
    // UsedType should NOT be in unused
    assert!(
        !unused_type_names.contains(&"UsedType"),
        "UsedType should NOT be detected as unused"
    );
}

#[test]
fn basic_project_detects_unused_dependencies() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();

    assert!(
        unused_dep_names.contains(&"unused-dep"),
        "unused-dep should be detected as unused dependency, found: {unused_dep_names:?}"
    );
}

#[test]
fn analysis_returns_correct_total_count() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(results.has_issues(), "basic-project should have issues");
    assert!(results.total_issues() > 0, "total_issues should be > 0");
}

#[test]
fn analyze_project_convenience_function() {
    let root = fixture_path("basic-project");
    let results = fallow_core::analyze_project(&root).expect("analysis should succeed");
    assert!(results.has_issues());
}

#[test]
fn cjs_project_detects_orphan() {
    let root = fixture_path("cjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        unused_file_names.contains(&"orphan.js".to_string()),
        "orphan.js should be detected as unused, found: {unused_file_names:?}"
    );
}

// ── Namespace imports ─────────────────────────────────────────

#[test]
fn namespace_import_makes_all_exports_used() {
    let root = fixture_path("namespace-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // With import * as utils, only members accessed via utils.member are used.
    // In the fixture, only utils.foo is accessed; bar and baz are unused.
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"foo"),
        "foo should be used via utils.foo member access"
    );
    assert!(
        unused_export_names.contains(&"bar"),
        "bar should be unused (not accessed via utils.bar)"
    );
    assert!(
        unused_export_names.contains(&"baz"),
        "baz should be unused (not accessed via utils.baz)"
    );
}

// ── Namespace exports (issue #52) ────────────────────────────

#[test]
fn namespace_export_members_not_reported_as_unused() {
    let root = fixture_path("namespace-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The namespace export `BusinessHelper` is imported and its members
    // accessed via `BusinessHelper.inviteSupplier()` etc. Neither the
    // namespace nor its inner functions should be reported as unused.
    assert!(
        results.unused_exports.is_empty(),
        "No unused exports expected, got: {:?}",
        results
            .unused_exports
            .iter()
            .map(|e| e.export_name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        results.unused_types.is_empty(),
        "No unused types expected, got: {:?}",
        results
            .unused_types
            .iter()
            .map(|e| e.export_name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(results.unused_files.is_empty(), "No unused files expected");
}

// ── Duplicate exports ─────────────────────────────────────────

#[test]
fn duplicate_exports_detected() {
    let root = fixture_path("duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dup_names: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|d| d.export_name.as_str())
        .collect();

    assert!(
        dup_names.contains(&"shared"),
        "shared should be detected as duplicate export, found: {dup_names:?}"
    );
}

#[test]
fn go_project_detects_used_and_unused_exports() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("pkg/utils")).expect("create pkg/utils");
    fs::create_dir_all(root.join("pkg/orphan")).expect("create pkg/orphan");

    fs::write(
        root.join("go.mod"),
        "module github.com/acme/example\n\ngo 1.25\n",
    )
    .expect("write go.mod");
    fs::write(
        root.join("main.go"),
        r#"package main

import "github.com/acme/example/pkg/utils"

func main() {
    utils.Foo()
}
"#,
    )
    .expect("write main.go");
    fs::write(
        root.join("pkg/utils/foo.go"),
        r#"package utils

func Foo() {}
"#,
    )
    .expect("write foo.go");
    fs::write(
        root.join("pkg/utils/bar.go"),
        r#"package utils

func Bar() {}
"#,
    )
    .expect("write bar.go");
    fs::write(
        root.join("pkg/orphan/orphan.go"),
        r#"package orphan

func Lost() {}
"#,
    )
    .expect("write orphan.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(&str, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export_name.as_str(),
                e.path.file_name().unwrap().to_string_lossy().to_string(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(name, file)| *name == "Foo" && file == "foo.go"),
        "Foo should be used via utils.Foo(), found unused exports: {unused_exports:?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|(name, file)| *name == "Bar" && file == "bar.go"),
        "Bar should be reported as unused, found unused exports: {unused_exports:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        unused_files.contains(&"orphan.go".to_string()),
        "orphan.go should be reported as unused, found: {unused_files:?}"
    );
}

#[test]
fn go_work_workspace_resolves_cross_module_package_usage() {
    use std::fs;

    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    fs::create_dir_all(root.join("app")).expect("create app");
    fs::create_dir_all(root.join("lib/pkg/shared")).expect("create lib/pkg/shared");
    fs::create_dir_all(root.join("lib/pkg/orphan")).expect("create lib/pkg/orphan");

    fs::write(
        root.join("go.work"),
        "go 1.25\n\nuse (\n    ./app\n    ./lib\n)\n",
    )
    .expect("write go.work");
    fs::write(
        root.join("app/go.mod"),
        "module github.com/acme/app\n\ngo 1.25\n",
    )
    .expect("write app/go.mod");
    fs::write(
        root.join("app/main.go"),
        r#"package main

import "github.com/acme/lib/pkg/shared"

func main() {
    shared.Foo()
}
"#,
    )
    .expect("write app/main.go");
    fs::write(
        root.join("lib/go.mod"),
        "module github.com/acme/lib\n\ngo 1.25\n",
    )
    .expect("write lib/go.mod");
    fs::write(
        root.join("lib/pkg/shared/foo.go"),
        r#"package shared

func Foo() {}
"#,
    )
    .expect("write foo.go");
    fs::write(
        root.join("lib/pkg/shared/bar.go"),
        r#"package shared

func Bar() {}
"#,
    )
    .expect("write bar.go");
    fs::write(
        root.join("lib/pkg/orphan/orphan.go"),
        r#"package orphan

func Lost() {}
"#,
    )
    .expect("write orphan.go");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(&str, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export_name.as_str(),
                e.path.file_name().unwrap().to_string_lossy().to_string(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(name, file)| *name == "Foo" && file == "foo.go"),
        "Foo should be used across go.work modules, found unused exports: {unused_exports:?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|(name, file)| *name == "Bar" && file == "bar.go"),
        "Bar should remain unused across go.work modules, found unused exports: {unused_exports:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        unused_files.contains(&"orphan.go".to_string()),
        "orphan.go should be reported as unused inside the workspace, found: {unused_files:?}"
    );
}

// ── Default export detection ───────────────────────────────────

#[test]
fn default_export_flagged_when_not_imported() {
    let root = fixture_path("default-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // unused-default.ts is never imported, so it should be an unused file
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        unused_file_names.contains(&"unused-default.ts".to_string()),
        "unused-default.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn default_export_flagged_when_only_named_imported() {
    let root = fixture_path("default-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // component.ts is imported for { usedNamed } only, so its default export
    // should be flagged as unused
    let unused_export_entries: Vec<(&str, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export_name.as_str(),
                e.path.file_name().unwrap().to_string_lossy().to_string(),
            )
        })
        .collect();

    assert!(
        unused_export_entries
            .iter()
            .any(|(name, file)| *name == "default" && file == "component.ts"),
        "default export on component.ts should be flagged as unused, found: {unused_export_entries:?}"
    );

    // usedNamed should NOT be flagged
    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export_name == "usedNamed"),
        "usedNamed should NOT be detected as unused"
    );
}

// ── Side-effect imports ────────────────────────────────────────

#[test]
fn side_effect_import_makes_file_reachable() {
    let root = fixture_path("side-effect-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // setup.ts is imported via side-effect import, so it should be reachable
    assert!(
        !unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be reachable via side-effect import, unused files: {unused_file_names:?}"
    );

    // orphan.ts is never imported, so it should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn circular_import_does_not_crash() {
    // Create temporary fixture with circular imports
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let temp_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();

    std::fs::write(
        temp_dir.join("package.json"),
        r#"{"name": "circular", "main": "src/a.ts"}"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = b + 1;\n",
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/b.ts"),
        "import { a } from './a';\nexport const b = a + 1;\n",
    )
    .unwrap();

    let config = create_config(temp_dir);
    // This should not crash or infinite loop
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        !results.circular_dependencies.is_empty(),
        "should detect circular dependency between a.ts and b.ts"
    );
    assert_eq!(results.circular_dependencies[0].length, 2);
}
