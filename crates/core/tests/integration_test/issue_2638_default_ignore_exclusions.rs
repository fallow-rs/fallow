//! A built-in discovery ignore pattern removes candidate source files from
//! every surface, and until issue #2638 nothing counted or named the drop: a
//! run rooted at a directory a default pattern matches reported a clean result
//! with exit 0 and no way to tell that its files were never analyzed.
//!
//! The walk now records one `excluded-by-default-ignore` diagnostic per
//! built-in pattern that excluded at least one candidate source file. These
//! tests are written on the serialized diagnostic rather than on the typed
//! variant, because the wire shape (`kind`, `pattern`, `file_count`,
//! `directory_count`, `path`) is the contract the issue asks for.

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
    assert_eq!(reported[0]["directory_count"], 1);
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

/// The flat monorepo shape the issue is about: ten packages each losing one
/// file makes every `dist/` directory "the largest", so the entry has to say
/// how many directories it is leaving unnamed instead of implying `path` holds
/// most of them.
#[test]
fn an_exclusion_spread_over_sibling_packages_counts_its_directories() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-flat" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    for package in ["a", "b", "c"] {
        write_file(
            root,
            &format!("packages/{package}/dist/gen.ts"),
            "export const gen = 1;\n",
        );
    }

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0]["file_count"], 3);
    assert_eq!(reported[0]["directory_count"], 3);
    assert_eq!(reported[0]["path"], "packages/a/dist");
    let message = reported[0]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.contains("across 3 directories, the largest group under 'packages/a/dist'"),
        "a max-of-group is not a majority: {message}"
    );
}

/// A file-shaped built-in that matched a file at the analysis root anchors at
/// the root itself, and the entry has to render that as a location rather than
/// as an empty string or an absolute host path.
#[test]
fn a_root_anchored_exclusion_is_reported_relative_to_the_root() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-root" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(root, "app.min.js", "var a = 1;\n");

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0]["pattern"], "**/*.min.js");
    assert_eq!(reported[0]["path"], ".");
    let message = reported[0]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.starts_with("Skipped 1 source file under '.'"),
        "{message}"
    );
    assert!(
        !message.contains("fallow --root"),
        "the message explains why re-rooting fails, it does not prescribe it: {message}"
    );
}

/// Installed dependencies are not the first-party source this diagnostic is
/// about. A project that does not gitignore `node_modules` would otherwise
/// report a five-figure count whose only honest remedy is "that is your
/// dependency tree".
#[test]
fn a_non_gitignored_node_modules_tree_is_never_reported() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    write_file(root, "package.json", r#"{ "name": "issue-2638-deps" }"#);
    write_file(root, "src/app.ts", "export const app = 1;\n");
    write_file(
        root,
        "node_modules/left-pad/index.js",
        "module.exports = 1;\n",
    );
    write_file(
        root,
        "node_modules/left-pad/lib/pad.js",
        "module.exports = 2;\n",
    );

    let config = create_config(root.to_path_buf());
    let reported = exclusion_diagnostics(&config);

    assert!(
        reported.is_empty(),
        "dependencies are not a surprise exclusion: {reported:?}"
    );
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
