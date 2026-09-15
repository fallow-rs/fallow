//! A built-in discovery ignore pattern removes candidate source files from
//! every surface, and until issue #2638 nothing counted or named the drop: a
//! run rooted at a directory a default pattern matches reported a clean result
//! with exit 0 and no way to tell that its files were never analyzed.
//!
//! The walk now records one `excluded-by-default-ignore` diagnostic per
//! built-in pattern that excluded at least one candidate source file. These
//! tests are written on the serialized diagnostic rather than on the typed
//! variant, because the wire shape (`kind`, `pattern`, `file_count`, `path`)
//! is the contract the issue asks for.

use std::fs;
use std::path::Path;

use fallow_config::ResolvedConfig;

use super::common::{create_config, create_config_with_ignore_patterns};

const KIND: &str = "excluded-by-default-ignore";

/// Every `excluded-by-default-ignore` entry this walk recorded, serialized and
/// with `path` rewritten project-relative with forward slashes.
fn exclusion_diagnostics(config: &ResolvedConfig) -> Vec<serde_json::Value> {
    fallow_core::discover::discover_files_config_candidates_and_diagnostics(config, &[])
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.kind.id() == KIND)
        .map(|diagnostic| {
            let relative = diagnostic.into_root_relative(&config.root);
            serde_json::to_value(&relative).expect("diagnostic serializes")
        })
        .collect()
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// R1: the reported entry names the pattern verbatim, counts the excluded
/// source files exactly, and anchors at the directory that holds them.
#[test]
fn a_built_in_pattern_that_excluded_source_files_is_reported_once_with_an_exact_count() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "packages/web/build/src/a.ts", "export const a = 1;\n");
    write_file(root, "packages/web/build/src/b.ts", "export const b = 1;\n");

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert_eq!(
        reported.len(),
        1,
        "one entry per excluding pattern: {reported:?}"
    );
    assert_eq!(reported[0]["kind"], KIND);
    assert_eq!(reported[0]["pattern"], "**/build/**");
    assert_eq!(reported[0]["file_count"], 2);
    assert_eq!(reported[0]["path"], "packages/web/build");
    let message = reported[0]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.contains("**/build/**") && message.contains("--root"),
        "the message names the pattern and the only remedy that analyzes the tree: {message}"
    );
}

/// R2: two patterns hitting one tree produce two entries, each with its own
/// count, ordered by the fixed built-in pattern order rather than by the
/// nondeterministic walk order.
#[test]
fn two_built_in_patterns_produce_two_entries_in_built_in_pattern_order() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-two" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "coverage/report.ts", "export const report = 1;\n");
    write_file(root, "dist/one.ts", "export const one = 1;\n");
    write_file(root, "dist/two.ts", "export const two = 1;\n");

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    let patterns: Vec<&str> = reported
        .iter()
        .map(|entry| entry["pattern"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        patterns,
        vec!["**/dist/**", "**/coverage/**"],
        "entries follow the built-in pattern order: {reported:?}"
    );
    assert_eq!(reported[0]["file_count"], 2);
    assert_eq!(reported[0]["path"], "dist");
    assert_eq!(reported[1]["file_count"], 1);
    assert_eq!(reported[1]["path"], "coverage");
}

/// P5 (AC3): a file the project's own `ignorePatterns` also matched was an
/// explicit choice, so reporting it as a surprise would be wrong.
#[test]
fn a_file_the_user_also_ignored_contributes_to_no_entry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-user" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "dist/generated.ts", "export const generated = 1;\n");

    let config = create_config_with_ignore_patterns(root.to_path_buf(), &["dist/**"]);
    let reported = exclusion_diagnostics(&config);

    assert!(
        reported.is_empty(),
        "a user-ignored file is not a surprise: {reported:?}"
    );
}

/// P6: gitignore prunes before the visitor runs, so a generated tree the
/// repository already hides counts zero. The honest population this feature
/// reports is "candidate source files git did not already hide".
#[test]
fn a_gitignored_tree_yields_no_entry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(
        root,
        "package.json",
        r#"{ "name": "issue-2638-gitignored" }"#,
    );
    write_file(root, ".gitignore", "dist/\n");
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "dist/generated.ts", "export const generated = 1;\n");
    fs::create_dir_all(root.join(".git")).expect("create .git marker");

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert!(
        reported.is_empty(),
        "gitignored output never reaches the visitor: {reported:?}"
    );
}

/// The count means SOURCE files. The walk's type filter also admits config
/// candidates, and a `tsconfig.json` inside an excluded tree has no imports or
/// exports the message could claim were lost.
#[test]
fn non_source_files_inside_an_excluded_directory_are_not_counted() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-config" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "dist/only.ts", "export const only = 1;\n");
    write_file(root, "dist/tsconfig.json", "{}\n");

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0]["file_count"], 1);
}

/// A project no built-in pattern touches stays silent, which is the case that
/// keeps `workspace_diagnostics[]` quiet on a well-kept repository.
#[test]
fn a_project_with_no_excluded_source_reports_nothing() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-clean" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");

    let config = create_config(root.to_path_buf());
    assert!(exclusion_diagnostics(&config).is_empty());
}
