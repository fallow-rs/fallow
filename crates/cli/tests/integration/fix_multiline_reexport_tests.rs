#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` and export lists that span several lines (issue #4636).
//!
//! The fixer must never remove the `export` of a declaration while a
//! re-export that it keeps still names that declaration: the fixed code then
//! fails to type-check with TS2459.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write_project(root: &Path, files: &[(&str, &str)]) {
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "multiline-reexport", "version": "1.0.0", "type": "module", "main": "src/index.ts" }"#,
    )
    .unwrap();
    for (path, content) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap()
}

/// `(path, line, name)` of every planned `remove_export` entry, sorted. A
/// withheld removal is a `remove_export` entry with `skipped: true` and does
/// not count.
fn planned_removals(root: &Path) -> Vec<(String, u64, String)> {
    let preview = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(preview.code, 0, "{}", preview.stderr);
    let mut planned: Vec<(String, u64, String)> = parse_json(&preview)["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_export" && fix["skipped"] != true)
        .map(|fix| {
            (
                fix["path"].as_str().unwrap().to_owned(),
                fix["line"].as_u64().unwrap(),
                fix["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    planned.sort();
    planned
}

fn apply(root: &Path) {
    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(applied.code, 0, "{}", applied.stderr);
}

fn removal(path: &str, line: u64, name: &str) -> (String, u64, String) {
    (path.to_owned(), line, name.to_owned())
}

const MOD_TS: &str = "export function a() {}\nexport function b() {}\nexport function c() {}\nexport function used() {}\n";

#[test]
fn fix_removes_a_multi_line_re_export_with_its_source_exports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './barrel';\n\nused();\n",
            ),
            (
                "src/barrel.ts",
                "export {\n\ta,\n\tb\n} from './mod';\nexport { c } from './mod';\nexport { used } from './mod';\n",
            ),
            ("src/mod.ts", MOD_TS),
        ],
    );

    assert_eq!(
        planned_removals(root),
        vec![
            removal("src/barrel.ts", 2, "a"),
            removal("src/barrel.ts", 3, "b"),
            removal("src/barrel.ts", 5, "c"),
            removal("src/mod.ts", 1, "a"),
            removal("src/mod.ts", 2, "b"),
            removal("src/mod.ts", 3, "c"),
        ]
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        "export { used } from './mod';\n"
    );
    assert_eq!(
        read(root, "src/mod.ts"),
        "function a() {}\nfunction b() {}\nfunction c() {}\nexport function used() {}\n"
    );
}

#[test]
fn fix_removes_only_the_unused_names_of_a_multi_line_re_export() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used, c } from './barrel';\nimport type { Kept } from './barrel';\n\nused(c);\nexport const k: Kept = 1;\n",
            ),
            (
                "src/barrel.ts",
                concat!(
                    "export {\n",
                    "  // helpers\n",
                    "  a, // not used\n",
                    "  used,\n",
                    "  /* also used */ c,\n",
                    "  b,\n",
                    "} from './mod';\n",
                    "export type {\n",
                    "  Kept,\n",
                    "  Dropped,\n",
                    "} from './types';\n",
                    "export { d } from './more';\n",
                ),
            ),
            ("src/mod.ts", MOD_TS),
            (
                "src/types.ts",
                "export type Kept = number;\nexport type Dropped = string;\n",
            ),
            ("src/more.ts", "export const d = 1;\n"),
        ],
    );

    assert_eq!(
        planned_removals(root),
        vec![
            removal("src/barrel.ts", 3, "a"),
            removal("src/barrel.ts", 6, "b"),
            removal("src/barrel.ts", 12, "d"),
            removal("src/mod.ts", 1, "a"),
            removal("src/mod.ts", 2, "b"),
            removal("src/more.ts", 1, "d"),
        ]
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        concat!(
            "export {\n",
            "  // helpers\n",
            "  used,\n",
            "  /* also used */ c,\n",
            "} from './mod';\n",
            "export type {\n",
            "  Kept,\n",
            "  Dropped,\n",
            "} from './types';\n",
        )
    );
    // `fix` does not remove unused type exports, so the type list and its
    // source stay as they are.
    assert_eq!(
        read(root, "src/types.ts"),
        "export type Kept = number;\nexport type Dropped = string;\n"
    );
}

#[test]
fn fix_keeps_a_source_export_while_a_kept_re_export_names_it() {
    // The barrel sits in a `fixtures` directory, so `fix` withholds its
    // rewrite as low confidence. The source export it re-exports must stay.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './fixtures/barrel';\n\nused();\n",
            ),
            (
                "src/fixtures/barrel.ts",
                "export {\n  a,\n  used,\n} from '../mod';\n",
            ),
            ("src/mod.ts", MOD_TS),
        ],
    );

    let planned = planned_removals(root);
    assert!(
        !planned.contains(&removal("src/mod.ts", 1, "a")),
        "the export of `a` must stay while src/fixtures/barrel.ts re-exports it: {planned:?}"
    );
    assert!(
        planned.contains(&removal("src/mod.ts", 2, "b")),
        "{planned:?}"
    );

    apply(root);
    assert_eq!(
        read(root, "src/fixtures/barrel.ts"),
        "export {\n  a,\n  used,\n} from '../mod';\n"
    );
    assert!(read(root, "src/mod.ts").starts_with("export function a() {}\n"));
}

/// The `kept_re_export` entries of a `--dry-run` JSON run, as
/// `(path, line, name)`, and the `skipped_kept_re_exports` counter.
fn kept_re_export_entries(root: &Path) -> (Vec<(String, u64, String)>, u64) {
    let preview = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(preview.code, 0, "{}", preview.stderr);
    let envelope = parse_json(&preview);
    let mut entries: Vec<(String, u64, String)> = envelope["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["skip_reason"] == "kept_re_export")
        .map(|fix| {
            assert_eq!(fix["type"], "remove_export");
            assert_eq!(fix["applied"], false);
            assert_eq!(fix["skipped"], true);
            assert!(fix["help"].as_str().is_some_and(|help| !help.is_empty()));
            (
                fix["path"].as_str().unwrap().to_owned(),
                fix["line"].as_u64().unwrap(),
                fix["name"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    entries.sort();
    let counter = envelope["skipped_kept_re_exports"]
        .as_u64()
        .expect("skipped_kept_re_exports is always present");
    (entries, counter)
}

#[test]
fn fix_reports_each_export_that_a_kept_re_export_holds_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './fixtures/barrel';\n\nused();\n",
            ),
            (
                "src/fixtures/barrel.ts",
                "export {\n  a,\n  used,\n} from '../mod';\n",
            ),
            ("src/mod.ts", MOD_TS),
        ],
    );

    assert_eq!(
        kept_re_export_entries(root),
        (vec![removal("src/mod.ts", 1, "a")], 1)
    );

    let human = run_fallow_in_root("fix", root, &["--dry-run"]);
    assert!(
        human.stderr.contains(
            "Would keep export in src/mod.ts:1 `a`: a re-export that this run keeps still names it"
        ),
        "{}",
        human.stderr
    );
    assert!(
        human
            .stderr
            .contains("Kept 1 unused export that a kept re-export still names."),
        "{}",
        human.stderr
    );
}

#[test]
fn fix_reports_an_export_behind_an_ignored_re_export() {
    // `ignoreExports` hides the barrel names from the report, so `fix` keeps
    // the re-export and must keep the source export with it.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './barrel';\n\nused();\n",
            ),
            (
                "src/barrel.ts",
                "export {\n  a,\n  used,\n} from './mod';\n",
            ),
            ("src/mod.ts", MOD_TS),
            (
                ".fallowrc.json",
                r#"{"ignoreExports": [{"file": "src/barrel.ts", "exports": ["*"]}]}"#,
            ),
        ],
    );

    assert_eq!(
        kept_re_export_entries(root),
        (vec![removal("src/mod.ts", 1, "a")], 1)
    );
    apply(root);
    assert!(read(root, "src/mod.ts").starts_with("export function a() {}\n"));
}

#[test]
fn fix_reports_an_export_behind_a_list_that_it_cannot_rewrite() {
    // Other code follows the list on its last line, so no safe edit can
    // delete the full statement.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            ("src/index.ts", "import './barrel';\n"),
            (
                "src/barrel.ts",
                "export {\n  a\n} from './mod'; console.log(1);\n",
            ),
            ("src/mod.ts", "export function a() {}\n"),
        ],
    );

    assert_eq!(
        kept_re_export_entries(root),
        (vec![removal("src/mod.ts", 1, "a")], 1)
    );
    apply(root);
    assert_eq!(read(root, "src/mod.ts"), "export function a() {}\n");
    assert_eq!(
        read(root, "src/barrel.ts"),
        "export {\n  a\n} from './mod'; console.log(1);\n"
    );
}

#[test]
fn fix_keeps_a_re_export_chain_whose_outer_link_stays() {
    // `outer.ts` is in a `fixtures` directory, so its re-export stays. The
    // inner re-export and the source export it names must stay with it.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './fixtures/outer';\n\nused();\n",
            ),
            (
                "src/fixtures/outer.ts",
                "export {\n  a,\n  used,\n} from '../inner';\n",
            ),
            ("src/inner.ts", "export {\n  a,\n  used,\n} from './mod';\n"),
            ("src/mod.ts", MOD_TS),
        ],
    );

    let planned = planned_removals(root);
    assert!(
        !planned.iter().any(|(_, _, name)| name == "a"),
        "no link of the chain for `a` may go: {planned:?}"
    );
}
