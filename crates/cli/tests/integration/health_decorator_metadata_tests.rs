//! Coverage gaps through decorator metadata (issue #5362).
//!
//! Under `emitDecoratorMetadata`, TypeScript turns some type annotations of a
//! decorated class into runtime references. A test that imports the class
//! then also loads and reads the classes that those annotations name.

use tempfile::tempdir;

use crate::common::{
    commit_all, fixture_path, git, parse_json, run_fallow_in_root, run_fallow_raw,
};

const FIXTURE: &str = "decorator-metadata-test-reach";

/// The exports that no test reaches, sorted.
fn untested_exports(root: &std::path::Path) -> Vec<String> {
    let output = run_fallow_in_root(
        "health",
        root,
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "coverage gaps default to warn severity: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let mut names: Vec<String> = json["coverage_gaps"]["exports"]
        .as_array()
        .expect("coverage_gaps.exports should be an array")
        .iter()
        .filter_map(|item| item["export_name"].as_str())
        .map(str::to_owned)
        .collect();
    names.sort_unstable();
    names
}

fn copy_fixture() -> tempfile::TempDir {
    let dir = tempdir().expect("create temp dir");
    crate::common::copy_fixture_tree(&fixture_path(FIXTURE), dir.path());
    dir
}

#[test]
fn decorator_metadata_types_are_test_reached() {
    let dir = copy_fixture();
    assert_eq!(
        untested_exports(dir.path()),
        ["NullableDto", "PlainDto", "PromiseDto", "TypeOnlyDto"],
        "only the type names that TypeScript emits as metadata reach the test"
    );
}

#[test]
fn decorator_metadata_types_are_stable_on_a_warm_cache() {
    let dir = copy_fixture();
    let cold = untested_exports(dir.path());
    assert!(
        dir.path().join(".fallow").exists(),
        "the cold run should write a cache"
    );
    assert_eq!(untested_exports(dir.path()), cold);
}

#[test]
fn nullable_union_is_metadata_without_strict_null_checks() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "experimentalDecorators": true, "emitDecoratorMetadata": true } }"#,
    )
    .expect("write tsconfig");
    assert_eq!(
        untested_exports(dir.path()),
        ["PlainDto", "PromiseDto", "TypeOnlyDto"],
        "without strictNullChecks, TypeScript drops null from the union"
    );
}

#[test]
fn types_are_not_test_reached_without_decorator_metadata() {
    let dir = copy_fixture();
    let cold = untested_exports(dir.path());
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true, "experimentalDecorators": true } }"#,
    )
    .expect("write tsconfig");
    let mut expected: Vec<String> = [
        "AccessorDto",
        "BarrelDto",
        "CtorDto",
        "NullableDto",
        "ParamDto",
        "PlainDto",
        "PromiseDto",
        "PropertyDto",
        "RestDto",
        "ReturnDto",
        "TypeOnlyDto",
        "UnionDto",
    ]
    .map(str::to_owned)
    .to_vec();
    expected.sort_unstable();
    assert_ne!(cold, expected, "the first run uses decorator metadata");
    assert_eq!(
        untested_exports(dir.path()),
        expected,
        "a tsconfig edit changes the result on a warm cache"
    );
}

#[test]
fn a_config_that_keeps_each_import_still_reads_decorator_metadata() {
    let dir = copy_fixture();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true, "experimentalDecorators": true, "emitDecoratorMetadata": true, "verbatimModuleSyntax": true } }"#,
    )
    .expect("write tsconfig");
    assert_eq!(
        untested_exports(dir.path()),
        ["NullableDto", "PlainDto", "PromiseDto", "TypeOnlyDto"],
        "verbatimModuleSyntax keeps the imports, and the metadata still reads them"
    );
}

#[test]
fn audit_test_reach_follows_decorator_metadata() {
    let dir = copy_fixture();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    for name in ["param", "plain"] {
        let path = root.join(format!("src/dtos/{name}.dto.ts"));
        let source = std::fs::read_to_string(&path).expect("read dto");
        std::fs::write(&path, source.replace("{}", "{\n  id = 1;\n}")).expect("write dto");
    }
    commit_all(root, "head");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--brief",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let brief = parse_json(&output);
    let reach = |name: &str| -> String {
        brief["test_reach"]["exports"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["export_name"] == name))
            .and_then(|row| row["reach"].as_str())
            .unwrap_or_else(|| panic!("no test reach row for {name}: {brief:#}"))
            .to_owned()
    };
    assert_eq!(reach("ParamDto"), "transitive");
    assert_eq!(reach("PlainDto"), "none");
}
