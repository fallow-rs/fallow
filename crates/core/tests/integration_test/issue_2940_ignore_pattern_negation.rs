//! Issues #2940 and #2452: a `!` entry in `ignorePatterns` lifts a built-in
//! discovery ignore or the hidden-directory skip for the paths it matches.
//!
//! The order is: built-in defaults, then the project's own patterns, then the
//! `!` exceptions. `node_modules` and `.git` can never be lifted.

use std::fs;
use std::path::Path;

use fallow_config::{FallowConfig, ResolvedConfig};

use super::common::create_config_with_ignore_patterns;

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// Project-relative, forward-slash paths of every discovered source file.
fn discovered(config: &ResolvedConfig) -> Vec<String> {
    let mut paths: Vec<String> = fallow_core::discover::discover_files(config)
        .into_iter()
        .map(|file| {
            file.path
                .strip_prefix(&config.root)
                .expect("discovered file is under the root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    paths.sort();
    paths
}

fn write_coverage_fixture(root: &Path) {
    write_file(root, "package.json", r#"{ "name": "issue-2940" }"#);
    write_file(root, "src/index.ts", "export const app = 1;\n");
    write_file(
        root,
        "src/policy/coverage/limit.ts",
        "export const limit = 1;\n",
    );
    write_file(root, "src/policy/coverage/a.ts", "export const a = 1;\n");
    write_file(root, "coverage/lcov-report/prettify.js", "var x = 1;\n");
}

#[test]
fn a_negation_lifts_a_built_in_ignore_for_its_subtree_only() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_coverage_fixture(root);

    let config =
        create_config_with_ignore_patterns(root.to_path_buf(), &["!src/policy/coverage/**"]);

    assert_eq!(
        discovered(&config),
        vec![
            "src/index.ts",
            "src/policy/coverage/a.ts",
            "src/policy/coverage/limit.ts",
        ],
        "the lifted subtree is discovered and the real coverage output stays excluded"
    );
}

#[test]
fn a_lifted_file_is_not_reported_as_excluded_by_a_default_ignore() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_coverage_fixture(root);
    write_file(root, "coverage/src/report.ts", "export const r = 1;\n");

    let config =
        create_config_with_ignore_patterns(root.to_path_buf(), &["!src/policy/coverage/**"]);
    let reported: Vec<serde_json::Value> =
        fallow_core::discover::discover_files_config_candidates_and_diagnostics(&config, &[])
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.kind.id() == "excluded-by-default-ignore")
            .map(|diagnostic| {
                serde_json::to_value(diagnostic.into_root_relative(&config.root))
                    .expect("diagnostic serializes")
            })
            .collect();

    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(
        reported[0]["file_count"], 2,
        "only the two files under the top-level coverage directory"
    );
    assert_eq!(reported[0]["path"], "coverage");
}

#[test]
fn a_negation_comes_after_the_projects_own_patterns() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_coverage_fixture(root);

    let config = create_config_with_ignore_patterns(
        root.to_path_buf(),
        &["src/policy/**", "!src/policy/coverage/limit.ts"],
    );

    assert_eq!(
        discovered(&config),
        vec!["src/index.ts", "src/policy/coverage/limit.ts"],
    );
}

#[test]
fn node_modules_stays_excluded_under_a_broad_negation() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(root, "package.json", r#"{ "name": "issue-2940" }"#);
    write_file(root, "src/index.ts", "export const app = 1;\n");
    write_file(root, "node_modules/dep/index.ts", "export const dep = 1;\n");
    write_file(root, "dist/out.ts", "export const out = 1;\n");

    let config = create_config_with_ignore_patterns(root.to_path_buf(), &["!**/*.ts"]);

    assert_eq!(discovered(&config), vec!["dist/out.ts", "src/index.ts"]);
}

#[test]
fn a_negation_adds_a_hidden_directory_to_traversal() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(root, "package.json", r#"{ "name": "issue-2452" }"#);
    write_file(root, "src/index.ts", "export const app = 1;\n");
    write_file(root, ".config/build.ts", "export const build = 1;\n");
    write_file(root, ".config/nested/deep.ts", "export const deep = 1;\n");
    write_file(root, ".other/skip.ts", "export const skip = 1;\n");

    let without = create_config_with_ignore_patterns(root.to_path_buf(), &[]);
    assert_eq!(discovered(&without), vec!["src/index.ts"]);

    let config = create_config_with_ignore_patterns(root.to_path_buf(), &["!.config/**"]);
    assert_eq!(
        discovered(&config),
        vec![".config/build.ts", ".config/nested/deep.ts", "src/index.ts"],
    );
}

#[test]
fn a_hidden_directory_negation_admits_only_the_files_it_matches() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(root, "package.json", r#"{ "name": "issue-2452" }"#);
    write_file(root, "src/index.ts", "export const app = 1;\n");
    write_file(root, ".config/build.ts", "export const build = 1;\n");
    write_file(root, ".config/other.ts", "export const other = 1;\n");
    write_file(
        root,
        "packages/web/.config/vite.ts",
        "export const v = 1;\n",
    );

    let config = create_config_with_ignore_patterns(root.to_path_buf(), &["!.config/build.ts"]);
    assert_eq!(
        discovered(&config),
        vec![".config/build.ts", "src/index.ts"]
    );

    let any_depth = create_config_with_ignore_patterns(root.to_path_buf(), &["!**/.config/**"]);
    assert_eq!(
        discovered(&any_depth),
        vec![
            ".config/build.ts",
            ".config/other.ts",
            "packages/web/.config/vite.ts",
            "src/index.ts",
        ],
    );
}

fn load_err(body: &str) -> String {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = tmp.path().join(".fallowrc.json");
    fs::write(&path, body).expect("write config");
    FallowConfig::load(&path)
        .expect_err("load should reject the negation")
        .to_string()
}

#[test]
fn a_negation_that_targets_node_modules_or_git_is_a_config_error() {
    for pattern in [
        "!node_modules/pkg/**",
        "!**/.git/**",
        "!packages/*/node_modules/x.ts",
    ] {
        let msg = load_err(&format!(r#"{{ "ignorePatterns": ["{pattern}"] }}"#));
        assert!(msg.contains("ignorePatterns"), "msg: {msg}");
        assert!(msg.contains(pattern), "msg: {msg}");
        assert!(msg.contains("can not be lifted"), "msg: {msg}");
    }
}

#[test]
fn an_empty_negation_is_a_config_error() {
    let msg = load_err(r#"{ "ignorePatterns": ["!"] }"#);
    assert!(msg.contains("ignorePatterns"), "msg: {msg}");
}
