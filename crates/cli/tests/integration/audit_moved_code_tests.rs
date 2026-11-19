//! `audit --gate new-only` attribution of unused code and clone groups that
//! a change moved to another file (#4670).
//!
//! Git reports a rename only for a whole file. A refactor that splits a file
//! gives the moved code a new path, and the path is part of each attribution
//! key. A moved finding must still match its base finding.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

/// A repository with `files` on the `main` branch, and a `head` branch
/// checked out on top of it. `config` is the content of `.fallowrc.json`.
fn repository(config: &str, files: &[(&str, String)]) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-moved-code", "private": true, "type": "module"}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    write_files(dir, files);
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "base");
    git(dir, &["checkout", "-b", "head"]);
    tmp
}

fn write_files(dir: &Path, files: &[(&str, String)]) {
    for (file, source) in files {
        fs::write(dir.join("src").join(file), source).unwrap();
    }
}

fn audit(dir: &Path) -> Value {
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--no-css",
    ]);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(&output)
}

const DEAD_CODE_CONFIG: &str = r#"{"entry": ["src/index.js"], "duplicates": {"minLines": 1000}}"#;

/// `(export name, introduced)` of each unused export, sorted.
fn unused_exports(json: &Value) -> Vec<(String, bool)> {
    let mut exports: Vec<(String, bool)> = json["dead_code"]["unused_exports"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["export_name"].as_str().unwrap().to_string(),
                        item["introduced"].as_bool().expect("introduced flag"),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    exports.sort();
    exports
}

fn index(imports: &[(&str, &str)]) -> String {
    let mut source = String::new();
    for (name, file) in imports {
        writeln!(source, "import {{ {name} }} from './{file}.js';").unwrap();
    }
    let names: Vec<&str> = imports.iter().map(|(name, _)| *name).collect();
    writeln!(source, "console.log({});", names.join(", ")).unwrap();
    source
}

fn library() -> String {
    "export const usedOne = 1;\nexport const usedTwo = 2;\nexport const unusedOne = 3;\nexport function unusedTwo() {\n  return 4;\n}\n"
        .to_string()
}

#[test]
fn unused_exports_of_a_split_file_stay_inherited() {
    let tmp = repository(
        DEAD_CODE_CONFIG,
        &[
            ("index.js", index(&[("usedOne", "lib"), ("usedTwo", "lib")])),
            ("lib.js", library()),
        ],
    );
    let dir = tmp.path();
    fs::remove_file(dir.join("src/lib.js")).unwrap();
    write_files(
        dir,
        &[
            ("index.js", index(&[("usedOne", "one"), ("usedTwo", "two")])),
            (
                "one.js",
                "export const usedOne = 1;\nexport const unusedOne = 3;\n".to_string(),
            ),
            (
                "two.js",
                "export const usedTwo = 2;\nexport function unusedTwo() {\n  return 4;\n}\n"
                    .to_string(),
            ),
        ],
    );
    commit_all(dir, "split");

    let json = audit(dir);

    assert_eq!(
        unused_exports(&json),
        [
            ("unusedOne".to_string(), false),
            ("unusedTwo".to_string(), false)
        ],
        "{json:#}"
    );
    assert_eq!(json["attribution"]["dead_code_introduced"], 0, "{json:#}");
    assert_eq!(json["verdict"], "pass", "{json:#}");
}

#[test]
fn new_unused_export_with_the_name_of_one_in_an_unchanged_file_is_introduced() {
    let tmp = repository(
        DEAD_CODE_CONFIG,
        &[
            ("index.js", index(&[("usedOne", "lib"), ("usedTwo", "lib")])),
            ("lib.js", library()),
        ],
    );
    let dir = tmp.path();
    write_files(
        dir,
        &[
            (
                "index.js",
                index(&[
                    ("usedOne", "lib"),
                    ("usedTwo", "lib"),
                    ("usedThree", "other"),
                ]),
            ),
            (
                "other.js",
                "export const usedThree = 5;\nexport const unusedOne = 6;\n".to_string(),
            ),
        ],
    );
    commit_all(dir, "same-named unused export in a new file");

    let json = audit(dir);

    assert_eq!(json["attribution"]["dead_code_introduced"], 1, "{json:#}");
    assert_eq!(json["verdict"], "fail", "{json:#}");
}

#[test]
fn one_moved_unused_export_does_not_cover_two_same_named_exports() {
    let tmp = repository(
        DEAD_CODE_CONFIG,
        &[
            ("index.js", index(&[("usedOne", "lib"), ("usedTwo", "lib")])),
            ("lib.js", library()),
        ],
    );
    let dir = tmp.path();
    fs::remove_file(dir.join("src/lib.js")).unwrap();
    write_files(
        dir,
        &[
            (
                "index.js",
                index(&[
                    ("usedOne", "one"),
                    ("usedTwo", "two"),
                    ("usedThree", "copy"),
                ]),
            ),
            (
                "one.js",
                "export const usedOne = 1;\nexport const unusedOne = 3;\n".to_string(),
            ),
            (
                "two.js",
                "export const usedTwo = 2;\nexport function unusedTwo() {\n  return 4;\n}\n"
                    .to_string(),
            ),
            (
                "copy.js",
                "export const usedThree = 5;\nexport const unusedOne = 3;\n".to_string(),
            ),
        ],
    );
    commit_all(dir, "split and copy");

    let json = audit(dir);

    // The copy also makes a duplicate export, so the test reads the unused
    // exports and not the dead-code counts.
    assert_eq!(
        unused_exports(&json),
        [
            ("unusedOne".to_string(), false),
            ("unusedOne".to_string(), true),
            ("unusedTwo".to_string(), false)
        ],
        "{json:#}"
    );
}

#[test]
fn export_that_the_change_made_unused_is_introduced() {
    let tmp = repository(
        DEAD_CODE_CONFIG,
        &[
            (
                "index.js",
                index(&[("kept", "old"), ("helper", "shared"), ("other", "shared")]),
            ),
            (
                "old.js",
                "export const kept = 1;\nexport const helper = 2;\n".to_string(),
            ),
            (
                "shared.js",
                "export const helper = 3;\nexport const other = 4;\n".to_string(),
            ),
        ],
    );
    let dir = tmp.path();
    // `old.js` goes away with its unused `helper`. `shared.js` gets a new
    // line, and its `helper` loses the only import. The change did not write
    // that export, so it is no moved code.
    fs::remove_file(dir.join("src/old.js")).unwrap();
    write_files(
        dir,
        &[
            (
                "index.js",
                index(&[("other", "shared"), ("extra", "shared")]),
            ),
            (
                "shared.js",
                "export const helper = 3;\nexport const other = 4;\nexport const extra = 5;\n"
                    .to_string(),
            ),
        ],
    );
    commit_all(dir, "remove the last import");

    let json = audit(dir);

    assert_eq!(
        unused_exports(&json),
        [("helper".to_string(), true)],
        "{json:#}"
    );
    assert_eq!(json["verdict"], "fail", "{json:#}");
}

const DUPLICATION_CONFIG: &str = r#"{"entry": ["src/*.js"]}"#;

/// A function body long enough to form a clone group with a second copy.
fn cloned(name: &str, factor: usize) -> String {
    let mut source = format!("export function {name}(items) {{\n  const out = [];\n");
    for step in 0..12 {
        writeln!(
            source,
            "  out.push(items.map((item) => item.value * {}).filter((value) => value > {step}));",
            step * factor
        )
        .unwrap();
    }
    source.push_str("  return out;\n}\n");
    source
}

fn clone_groups(json: &Value) -> Vec<bool> {
    json["duplication"]["clone_groups"]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .map(|group| group["introduced"].as_bool().expect("introduced flag"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn clone_group_of_a_split_file_stays_inherited() {
    let tmp = repository(
        DUPLICATION_CONFIG,
        &[(
            "index.js",
            format!("{}{}", cloned("zeta", 1), cloned("alpha", 1)),
        )],
    );
    let dir = tmp.path();
    fs::remove_file(dir.join("src/index.js")).unwrap();
    // The file names put the copies in the opposite order of the base file.
    write_files(
        dir,
        &[
            ("first.js", cloned("alpha", 1)),
            ("second.js", cloned("zeta", 1)),
        ],
    );
    commit_all(dir, "split");

    let json = audit(dir);

    assert_eq!(clone_groups(&json), [false], "{json:#}");
    assert_eq!(json["attribution"]["duplication_introduced"], 0, "{json:#}");
    assert_eq!(json["verdict"], "pass", "{json:#}");
}

#[test]
fn clone_group_with_other_text_in_new_files_is_introduced() {
    let tmp = repository(
        DUPLICATION_CONFIG,
        &[(
            "index.js",
            format!("{}{}", cloned("zeta", 1), cloned("alpha", 1)),
        )],
    );
    let dir = tmp.path();
    fs::remove_file(dir.join("src/index.js")).unwrap();
    write_files(
        dir,
        &[
            ("first.js", cloned("alpha", 3)),
            ("second.js", cloned("zeta", 3)),
        ],
    );
    commit_all(dir, "replace with other clones");

    let json = audit(dir);

    assert_eq!(clone_groups(&json), [true], "{json:#}");
}
