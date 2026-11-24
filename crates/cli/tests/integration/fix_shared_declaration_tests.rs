#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` and export declarations that bind more than one name
//! (issue #4659).
//!
//! One `export` keyword exports every name of `export const { a, b } = x;`,
//! `export const [a, b] = x;` and `export const a = 1, b = 2;`. The fixer
//! must never remove that keyword while one of the names is in use: the
//! imports of that name then fail to type-check (TS2306 or TS2459).

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write_project(root: &Path, store: &str, index: &str) {
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "shared-declaration", "version": "1.0.0", "type": "module", "main": "src/index.ts" }"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/store.ts"), store).unwrap();
    std::fs::write(root.join("src/index.ts"), index).unwrap();
}

fn read_store(root: &Path) -> String {
    std::fs::read_to_string(root.join("src/store.ts")).unwrap()
}

/// The `remove_export` entries of a `--dry-run` JSON run, split into the
/// planned `(line, name)` pairs and the `shared_declaration_in_use` pairs,
/// plus the `skipped_shared_declarations` counter.
struct Preview {
    planned: Vec<(u64, String)>,
    withheld: Vec<(u64, String)>,
    counter: u64,
}

fn preview(root: &Path) -> Preview {
    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let envelope = parse_json(&output);
    let mut planned = Vec::new();
    let mut withheld = Vec::new();
    for fix in envelope["fixes"].as_array().unwrap() {
        if fix["type"] != "remove_export" {
            continue;
        }
        let entry = (
            fix["line"].as_u64().unwrap(),
            fix["name"].as_str().unwrap().to_owned(),
        );
        if fix["skip_reason"] == "kept_re_export" {
            continue;
        }
        if fix["skipped"] == true {
            assert_eq!(fix["skip_reason"], "shared_declaration_in_use", "{fix}");
            assert_eq!(fix["applied"], false);
            assert!(fix["help"].as_str().is_some_and(|help| !help.is_empty()));
            withheld.push(entry);
        } else {
            planned.push(entry);
        }
    }
    planned.sort();
    withheld.sort();
    let counter = envelope["skipped_shared_declarations"]
        .as_u64()
        .expect("skipped_shared_declarations is always present");
    Preview {
        planned,
        withheld,
        counter,
    }
}

fn apply(root: &Path) {
    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(applied.code, 0, "{}", applied.stderr);
}

fn entry(line: u64, name: &str) -> (u64, String) {
    (line, name.to_owned())
}

/// Assert that `fix` keeps `store` unchanged and reports `withheld`.
fn assert_withheld(store: &str, index: &str, withheld: &[(u64, &str)]) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root, store, index);

    let preview = preview(root);
    assert_eq!(preview.planned, Vec::new(), "nothing may be removed");
    let expected: Vec<(u64, String)> = withheld
        .iter()
        .map(|(line, name)| entry(*line, name))
        .collect();
    assert_eq!(preview.withheld, expected);
    assert_eq!(preview.counter, expected.len() as u64);

    apply(root);
    assert_eq!(read_store(root), store);
}

#[test]
fn fix_keeps_an_object_pattern_export_while_one_name_is_in_use() {
    assert_withheld(
        "const hooks = { useA: () => 1, useB: () => 2 };\nexport const { useA, useB } = hooks;\n",
        "import { useA } from './store';\n\nuseA();\n",
        &[(2, "useB")],
    );
}

#[test]
fn fix_keeps_an_array_pattern_export_while_one_name_is_in_use() {
    assert_withheld(
        "export const [first, second] = [1, 2];\n",
        "import { first } from './store';\n\nfirst;\n",
        &[(1, "second")],
    );
}

#[test]
fn fix_keeps_a_pattern_with_a_default_value() {
    assert_withheld(
        "const config: { a?: number; b?: number } = {};\nexport const { a = 1, b = 2 } = config;\n",
        "import { a } from './store';\n\na;\n",
        &[(2, "b")],
    );
}

#[test]
fn fix_keeps_a_pattern_with_a_rest_element() {
    assert_withheld(
        "const config = { a: 1, b: 2, c: 3 };\nexport const { a, ...rest } = config;\n",
        "import { a } from './store';\n\na;\n",
        &[(2, "rest")],
    );
}

#[test]
fn fix_keeps_a_nested_pattern() {
    assert_withheld(
        "const config = { inner: { a: 1, b: 2 } };\nexport const { inner: { a, b } } = config;\n",
        "import { a } from './store';\n\na;\n",
        &[(2, "b")],
    );
}

#[test]
fn fix_keeps_a_pattern_over_several_lines() {
    assert_withheld(
        "const hooks = { useA: () => 1, useB: () => 2 };\nexport const {\n  useA,\n  useB,\n} = hooks;\n",
        "import { useA } from './store';\n\nuseA();\n",
        &[(4, "useB")],
    );
}

#[test]
fn fix_keeps_let_and_var_patterns() {
    assert_withheld(
        "const hooks = { a: 1, b: 2 };\nexport let { a, b } = hooks;\nexport var [c, d] = [1, 2];\n",
        "import { a, c } from './store';\n\na;\nc;\n",
        &[(2, "b"), (3, "d")],
    );
}

#[test]
fn fix_keeps_a_declaration_with_several_declarators() {
    assert_withheld(
        "export const x1 = 1, x2 = 2;\n",
        "import { x1 } from './store';\n\nx1;\n",
        &[(1, "x2")],
    );
}

#[test]
fn fix_removes_the_export_when_every_name_of_the_pattern_is_unused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "const hooks = { useA: () => 1, useB: () => 2 };\nexport const {\n  useA,\n  useB,\n} = hooks;\nexport let [c, d] = [1, 2];\nexport const used = 1;\n",
        "import { used } from './store';\n\nused;\n",
    );

    let preview = preview(root);
    assert_eq!(
        preview.planned,
        vec![
            entry(3, "useA"),
            entry(4, "useB"),
            entry(6, "c"),
            entry(6, "d")
        ]
    );
    assert_eq!(preview.withheld, Vec::new());
    assert_eq!(preview.counter, 0);

    apply(root);
    assert_eq!(
        read_store(root),
        "const hooks = { useA: () => 1, useB: () => 2 };\nconst {\n  useA,\n  useB,\n} = hooks;\nlet [c, d] = [1, 2];\nexport const used = 1;\n"
    );
}

#[test]
fn fix_keeps_every_name_of_a_pattern_when_a_kept_re_export_names_one() {
    // The barrel sits in a `fixtures` directory, so `fix` keeps its
    // re-export of `useA`. The declaration must then keep `useB` too.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "const hooks = { useA: () => 1, useB: () => 2 };\nexport const { useA, useB } = hooks;\n",
        "import { used } from './fixtures/barrel';\n\nused;\n",
    );
    std::fs::create_dir_all(root.join("src/fixtures")).unwrap();
    std::fs::write(
        root.join("src/fixtures/barrel.ts"),
        "export { useA } from '../store';\nexport const used = 1;\n",
    )
    .unwrap();

    let preview = preview(root);
    assert_eq!(preview.planned, Vec::new());
    assert_eq!(preview.withheld, vec![entry(2, "useB")]);

    apply(root);
    assert!(read_store(root).contains("export const { useA, useB } = hooks;"));
}

/// Write `files` next to a `package.json` whose entry is `src/index.ts`.
fn write_files(root: &Path, files: &[(&str, &str)]) {
    write_project(root, "", "");
    for (path, content) in files {
        std::fs::write(root.join(path), content).unwrap();
    }
}

#[test]
fn fix_removes_the_keyword_of_a_declaration_that_does_not_start_its_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "const o = { a: 1, b: 2 };\nconst y = 1; export const { a, b } = o;\nexport const used = y;\n",
        "import { used } from './store';\n\nused;\n",
    );

    let preview = preview(root);
    assert_eq!(preview.planned, vec![entry(2, "a"), entry(2, "b")]);

    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(applied.code, 0, "{}", applied.stderr);
    assert_eq!(parse_json(&applied)["total_fixed"], 2);
    assert_eq!(
        read_store(root),
        "const o = { a: 1, b: 2 };\nconst y = 1; const { a, b } = o;\nexport const used = y;\n"
    );
}

#[test]
fn fix_removes_the_keyword_of_the_declaration_after_a_used_export_on_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "const o = { a: 1, b: 2 };\nexport const z = 1; export const { a, b } = o;\n",
        "import { z } from './store';\n\nz;\n",
    );

    let preview = preview(root);
    assert_eq!(preview.planned, vec![entry(2, "a"), entry(2, "b")]);

    apply(root);
    assert_eq!(
        read_store(root),
        "const o = { a: 1, b: 2 };\nexport const z = 1; const { a, b } = o;\n"
    );
}

#[test]
fn fix_keeps_a_shared_declaration_at_the_line_start_for_another_name_on_the_line() {
    // Only `z` is unused. The `export` keyword at the line start belongs to
    // the declaration of `a` and `b`, so the fix removes the second keyword.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "const o = { a: 1, b: 2 };\nexport const { a, b } = o; export const z = 1;\n",
        "import { a, b } from './store';\n\na;\nb;\n",
    );

    let preview = preview(root);
    assert_eq!(preview.planned, vec![entry(2, "z")]);
    assert_eq!(preview.withheld, Vec::new());

    apply(root);
    assert_eq!(
        read_store(root),
        "const o = { a: 1, b: 2 };\nexport const { a, b } = o; const z = 1;\n"
    );
}

#[test]
fn fix_keeps_a_vue_declaration_with_several_declarators_while_one_name_is_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = "<script lang=\"ts\">\nexport const c = 1, d = 2;\n</script>\n\n<template><div /></template>\n";
    write_files(
        root,
        &[
            ("src/Store.vue", store),
            ("src/index.ts", "import { c } from './Store.vue';\n\nc;\n"),
        ],
    );

    let preview = preview(root);
    assert_eq!(preview.planned, Vec::new());
    assert_eq!(preview.withheld, vec![entry(2, "d")]);

    apply(root);
    assert_eq!(
        std::fs::read_to_string(root.join("src/Store.vue")).unwrap(),
        store
    );
}

#[test]
fn fix_keeps_an_astro_declaration_with_several_declarators_while_one_name_is_in_use() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = "---\nexport const c = 1, d = 2;\n---\n<p>{c}</p>\n";
    write_files(
        root,
        &[
            ("src/Store.astro", store),
            ("src/index.ts", "import { c } from './Store.astro';\n\nc;\n"),
        ],
    );

    let preview = preview(root);
    assert_eq!(preview.planned, Vec::new());
    assert_eq!(preview.withheld, vec![entry(2, "d")]);

    apply(root);
    assert_eq!(
        std::fs::read_to_string(root.join("src/Store.astro")).unwrap(),
        store
    );
}

#[test]
fn fix_removes_the_export_of_a_single_name_pattern_in_a_vue_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_files(
        root,
        &[
            (
                "src/Store.vue",
                "<script lang=\"ts\">\nconst x = { a: 1 };\nexport const { a } = x;\nexport const used = 1;\n</script>\n",
            ),
            (
                "src/index.ts",
                "import { used } from './Store.vue';\n\nused;\n",
            ),
        ],
    );

    let preview = preview(root);
    assert_eq!(preview.planned, vec![entry(3, "a")]);
    assert_eq!(preview.withheld, Vec::new());

    apply(root);
    assert_eq!(
        std::fs::read_to_string(root.join("src/Store.vue")).unwrap(),
        "<script lang=\"ts\">\nconst x = { a: 1 };\nconst { a } = x;\nexport const used = 1;\n</script>\n"
    );
}

#[test]
fn fix_removes_the_export_of_the_second_function_on_one_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        "export function live() {} export function dead() {}\n",
        "import { live } from './store';\n\nlive();\n",
    );

    let preview = preview(root);
    assert_eq!(preview.planned, vec![entry(1, "dead")]);

    apply(root);
    assert_eq!(
        read_store(root),
        "export function live() {} function dead() {}\n"
    );
}

#[test]
fn fix_keeps_a_shared_declaration_with_a_comment_after_export() {
    assert_withheld(
        "export /* values */ const live = 1, dead = 2;\n",
        "import { live } from './store';\n\nlive;\n",
        &[(1, "dead")],
    );
}

#[test]
fn fix_keeps_a_shared_declaration_with_a_tab_after_export() {
    assert_withheld(
        "export\tconst live = 1, dead = 2;\n",
        "import { live } from './store';\n\nlive;\n",
        &[(1, "dead")],
    );
}
