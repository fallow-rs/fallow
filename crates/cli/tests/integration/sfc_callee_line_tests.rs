//! A call site in a single-file component reports its line in the whole file.
//!
//! The script body of a `.vue`, `.svelte`, `.astro` or `.mdx` file starts after
//! other source. A call finding and its suppression comment must use the line
//! of the file, the same as an import finding in the same block.

use std::path::Path;

use serde_json::Value;

use crate::common::{parse_json, run_fallow_in_root};

const RULES: &str = r#"{ "version": 1, "name": "repro", "rules": [
  { "id": "no-eval", "kind": "banned-call", "callees": ["eval"], "message": "No eval." },
  { "id": "no-function", "kind": "banned-call", "callees": ["Function"], "message": "No Function." },
  { "id": "no-lodash", "kind": "banned-import", "specifiers": ["lodash"], "message": "No lodash." }
] }"#;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

fn project(file: &str, source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "repro", "private": true, "type": "module" }"#,
    );
    write(root, "pack.json", RULES);
    write(
        root,
        ".fallowrc.json",
        r#"{ "entry": ["src/**/*"], "rulePacks": ["pack.json"], "rules": { "policy-violation": "error" } }"#,
    );
    write(root, file, source);
    dir
}

fn architecture(root: &Path) -> Value {
    let output = run_fallow_in_root(
        "architecture",
        root,
        &["--no-cache", "--quiet", "--policy", "--format", "json"],
    );
    parse_json(&output)
}

/// `(line, col, rule_id)` of each policy finding, sorted by line and column.
fn findings(json: &Value) -> Vec<(u64, u64, String)> {
    let mut found: Vec<(u64, u64, String)> = json["policy_violations"]
        .as_array()
        .expect("policy_violations")
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().expect("line"),
                finding["col"].as_u64().expect("col"),
                finding["rule_id"].as_str().expect("rule_id").to_owned(),
            )
        })
        .collect();
    found.sort();
    found
}

fn finding(line: u64, col: u64, rule_id: &str) -> (u64, u64, String) {
    (line, col, rule_id.to_owned())
}

#[test]
fn vue_script_setup_on_the_first_line_reports_the_file_line() {
    let dir = project(
        "src/one.vue",
        "<script setup lang=\"ts\">\neval(\"x\");\n</script>\n<template><div /></template>\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(2, 0, "no-eval")]
    );
}

#[test]
fn vue_with_two_script_blocks_reports_the_file_line_for_each_form() {
    let dir = project(
        "src/two.vue",
        "<template><div /></template>\n\
         <script lang=\"ts\">\n\
         export default {};\n\
         </script>\n\
         <script setup lang=\"ts\">\n\
         import lodash from \"lodash\";\n\
         eval(\"x\");\n\
         new Function(\"x\");\n\
         console.log(lodash);\n\
         </script>\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![
            finding(6, 7, "no-lodash"),
            finding(7, 0, "no-eval"),
            finding(8, 0, "no-function"),
        ]
    );
}

#[test]
fn vue_call_in_the_second_script_block_uses_the_offset_of_that_block() {
    let dir = project(
        "src/both.vue",
        "<script lang=\"ts\">\n\
         eval(\"first\");\n\
         export default {};\n\
         </script>\n\
         <template><div /></template>\n\
         <script setup lang=\"ts\">\n\
         new Function(\"second\");\n\
         </script>\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(2, 0, "no-eval"), finding(7, 0, "no-function")]
    );
}

#[test]
fn svelte_script_after_markup_reports_the_file_line() {
    let dir = project(
        "src/comp.svelte",
        "<h1>header</h1>\n\n<script lang=\"ts\">\n  import lodash from \"lodash\";\n  eval(\"x\");\n  console.log(lodash);\n</script>\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(4, 9, "no-lodash"), finding(5, 2, "no-eval")]
    );
}

#[test]
fn astro_frontmatter_reports_the_file_line() {
    let dir = project(
        "src/page.astro",
        "---\nimport lodash from \"lodash\";\n\neval(\"x\");\nconsole.log(lodash);\n---\n<div />\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(2, 7, "no-lodash"), finding(4, 0, "no-eval")]
    );
}

#[test]
fn mdx_statement_after_prose_reports_the_file_position() {
    let dir = project(
        "src/note.mdx",
        "# Title\n\nimport lodash from \"lodash\";\n\nexport const x = eval(\"x\");\n\n<div />\n",
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(3, 7, "no-lodash"), finding(5, 17, "no-eval")]
    );
}

#[test]
fn banned_effect_in_a_vue_script_reports_the_file_line() {
    let dir = project(
        "src/effect.vue",
        "<template><div /></template>\n\n<script setup lang=\"ts\">\n\neval(\"x\");\n</script>\n",
    );
    write(
        dir.path(),
        "pack.json",
        r#"{ "version": 1, "name": "repro", "rules": [
  { "id": "no-effect", "kind": "banned-effect", "effects": ["process"], "message": "No process." }
] }"#,
    );
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(5, 0, "no-effect")]
    );
}

#[test]
fn suppression_above_the_call_in_a_vue_script_covers_the_finding() {
    let dir = project(
        "src/suppress.vue",
        "<template><div /></template>\n\n<script setup lang=\"ts\">\n// fallow-ignore-next-line policy-violation\neval(\"x\");\n</script>\n",
    );
    assert_eq!(findings(&architecture(dir.path())), vec![]);

    let control = project(
        "src/suppress.vue",
        "<template><div /></template>\n\n<script setup lang=\"ts\">\n// a plain comment\neval(\"x\");\n</script>\n",
    );
    assert_eq!(
        findings(&architecture(control.path())),
        vec![finding(5, 0, "no-eval")]
    );
}

#[test]
fn suppression_on_an_unrelated_line_does_not_cover_the_finding() {
    // The call is 56 bytes into its script body. A body-relative offset lands on
    // line 2 of the file, which is the line that the comment on line 1 covers.
    let padding = "x".repeat(35);
    let source = format!(
        "<!-- fallow-ignore-next-line policy-violation -->\n<template><div /></template>\n\n<script setup lang=\"ts\">\nconst padding = \"{padding}\";\neval(padding);\n</script>\n",
    );
    let dir = project("src/unrelated.vue", &source);
    assert_eq!(
        findings(&architecture(dir.path())),
        vec![finding(6, 0, "no-eval")]
    );
}

#[test]
fn forbidden_call_in_a_vue_script_reports_the_file_line() {
    let dir = project(
        "src/zone/call.vue",
        "<template><div /></template>\n\n<script setup lang=\"ts\">\neval(\"x\");\n</script>\n",
    );
    let root = dir.path();
    write(
        root,
        ".fallowrc.json",
        r#"{
  "entry": ["src/**/*"],
  "rules": { "boundary-violation": "error" },
  "boundaries": {
    "zones": [{ "name": "z", "patterns": ["src/zone/**"] }],
    "rules": [],
    "calls": { "forbidden": [{ "from": "z", "callee": "eval" }] }
  }
}"#,
    );
    let json = parse_json(&run_fallow_in_root(
        "architecture",
        root,
        &["--no-cache", "--quiet", "--boundaries", "--format", "json"],
    ));
    let lines: Vec<u64> = json["boundary_call_violations"]
        .as_array()
        .expect("boundary_call_violations")
        .iter()
        .map(|violation| violation["line"].as_u64().expect("line"))
        .collect();
    assert_eq!(lines, vec![4], "report: {json}");
}
