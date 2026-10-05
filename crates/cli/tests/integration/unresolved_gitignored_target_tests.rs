#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{copy_fixture, git, parse_json, run_fallow_in_root};

const FIXTURE: &str = "unresolved-gitignored-target";

/// The `(path, specifier)` pairs of the unresolved-import findings, sorted.
fn unresolved_imports(root: &Path) -> Vec<(String, String)> {
    unresolved_imports_with(root, &["--no-cache"])
}

/// The unresolved-import findings of a run with the extra `args`, sorted.
fn unresolved_imports_with(root: &Path, args: &[&str]) -> Vec<(String, String)> {
    let mut all_args = args.to_vec();
    all_args.extend(["--format", "json", "--quiet", "--unresolved-imports"]);
    let out = run_fallow_in_root("dead-code", root, &all_args);
    let json = parse_json(&out);
    let mut pairs: Vec<(String, String)> = json["unresolved_imports"]
        .as_array()
        .expect("unresolved_imports array")
        .iter()
        .map(|finding| {
            (
                finding["path"].as_str().unwrap().replace('\\', "/"),
                finding["specifier"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

fn pair(path: &str, specifier: &str) -> (String, String) {
    (path.to_owned(), specifier.to_owned())
}

/// Every relative import in the fixture, as the analysis reports it when no
/// gitignore rule applies.
fn all_fixture_imports() -> Vec<(String, String)> {
    vec![
        pair("lib/helper.ts", "./generated/client"),
        pair("next-env.d.ts", "./.next/types/routes.d.ts"),
        pair("src/index.ts", "../dist/out.js"),
        pair("src/index.ts", "../lib/generated/client"),
        pair("src/index.ts", "./missing"),
    ]
}

#[test]
fn relative_import_of_missing_gitignored_path_is_not_unresolved() {
    let dir = copy_fixture(FIXTURE);
    git(dir.path(), &["init", "-q"]);

    assert_eq!(
        unresolved_imports(dir.path()),
        vec![pair("src/index.ts", "./missing")],
        "a missing target below a gitignored directory is build output, \
         but a missing target that no rule ignores stays unresolved"
    );
}

#[test]
fn relative_import_of_missing_path_stays_unresolved_without_gitignore_rule() {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(dir.path().join(".gitignore"), "").unwrap();
    std::fs::write(dir.path().join("lib/.gitignore"), "").unwrap();
    git(dir.path(), &["init", "-q"]);

    assert_eq!(unresolved_imports(dir.path()), all_fixture_imports());
}

#[test]
fn gitignore_rules_do_not_apply_outside_a_git_repository() {
    let dir = copy_fixture(FIXTURE);

    assert_eq!(unresolved_imports(dir.path()), all_fixture_imports());
}

const SUBPATH_EXPORT_FIXTURE: &str = "unresolved-gitignored-subpath-export";

/// Every unresolved workspace subpath import in the fixture, as the analysis
/// reports it when no gitignore rule applies.
fn all_subpath_export_imports() -> Vec<(String, String)> {
    vec![
        pair("packages/consumer/src/index.ts", "store/enums"),
        pair("packages/consumer/src/index.ts", "store/missing"),
        pair("packages/consumer/src/index.ts", "store/schema"),
    ]
}

#[test]
fn workspace_subpath_export_to_missing_gitignored_path_is_not_unresolved() {
    let dir = copy_fixture(SUBPATH_EXPORT_FIXTURE);
    git(dir.path(), &["init", "-q"]);

    assert_eq!(
        unresolved_imports(dir.path()),
        vec![
            pair("packages/consumer/src/index.ts", "store/missing"),
            pair("packages/consumer/src/index.ts", "store/schema"),
        ],
        "an exports target below a gitignored directory is generated code, \
         but a missing target that no rule ignores and a subpath without an \
         exports key stay unresolved"
    );
}

#[test]
fn workspace_subpath_export_to_missing_gitignored_path_stays_silent_on_a_cache_hit() {
    let dir = copy_fixture(SUBPATH_EXPORT_FIXTURE);
    git(dir.path(), &["init", "-q"]);
    let expected = vec![
        pair("packages/consumer/src/index.ts", "store/missing"),
        pair("packages/consumer/src/index.ts", "store/schema"),
    ];

    assert_eq!(unresolved_imports_with(dir.path(), &[]), expected);
    assert!(
        dir.path().join(".fallow").is_dir(),
        "the first run must write the cache"
    );
    assert_eq!(
        unresolved_imports_with(dir.path(), &[]),
        expected,
        "the second run restores the resolved graph from the cache"
    );
}

#[test]
fn workspace_subpath_export_to_missing_path_stays_unresolved_without_gitignore_rule() {
    let dir = copy_fixture(SUBPATH_EXPORT_FIXTURE);
    std::fs::write(dir.path().join("packages/store/.gitignore"), "").unwrap();
    git(dir.path(), &["init", "-q"]);

    assert_eq!(unresolved_imports(dir.path()), all_subpath_export_imports());
}

#[test]
fn workspace_subpath_export_keeps_dependency_findings() {
    let dir = copy_fixture(SUBPATH_EXPORT_FIXTURE);
    git(dir.path(), &["init", "-q"]);

    let out = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let json = parse_json(&out);
    for kind in ["unused_dependencies", "unlisted_dependencies"] {
        assert_eq!(
            json[kind].as_array().map(Vec::len),
            Some(0),
            "the listed and used workspace dependency must not appear in {kind}: {json}"
        );
    }
}
