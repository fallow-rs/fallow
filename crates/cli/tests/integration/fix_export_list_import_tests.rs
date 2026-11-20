#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` and the imports of a local export list (issue #4833).
//!
//! When the fixer removes a name from `export { a, used };` and `a` came
//! from an import that nothing else in the file uses, the fixer also removes
//! that import. A kept import is not a consumer for the analysis, so the
//! next run removes the `export` of `a` in its source module, and the import
//! then fails the type check with TS2306 or TS2459.

use std::path::Path;

use crate::common::run_fallow_in_root;

fn write_project(root: &Path, files: &[(&str, &str)]) {
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "export-list-import", "version": "1.0.0", "type": "module", "main": "src/index.ts" }"#,
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

fn apply(root: &Path) {
    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(applied.code, 0, "{}", applied.stderr);
}

#[test]
fn two_fix_runs_remove_the_import_of_a_removed_list_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used } from './barrel';\nimport { other } from './a';\n\nused(other);\n",
            ),
            (
                "src/barrel.ts",
                "import { a } from './a';\nimport { used } from './used';\n\nexport { a, used };\n",
            ),
            (
                "src/a.ts",
                "export function a() {}\nexport function other() {}\n",
            ),
            ("src/used.ts", "export function used() {}\n"),
        ],
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        "import { used } from './used';\n\nexport { used };\n"
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        "import { used } from './used';\n\nexport { used };\n"
    );
    assert_eq!(
        read(root, "src/a.ts"),
        "function a() {}\nexport function other() {}\n"
    );
}

#[test]
fn fix_removes_only_the_unused_specifier_of_a_shared_import() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(
        root,
        &[
            (
                "src/index.ts",
                "import { used, c } from './barrel';\n\nused(c);\n",
            ),
            (
                "src/barrel.ts",
                "import {\n  a,\n  c,\n  used,\n} from './mod';\n\nexport { a, c, used };\n",
            ),
            (
                "src/mod.ts",
                "export function a() {}\nexport const c = 1;\nexport function used() {}\n",
            ),
        ],
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        "import {\n  c,\n  used,\n} from './mod';\n\nexport { c, used };\n"
    );
}

#[test]
fn fix_keeps_an_import_that_other_code_uses() {
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
                "import a, { b } from './a';\nimport './polyfill';\n\nexport const twice = () => b() + b();\nexport { a, b, b as used };\n",
            ),
            (
                "src/a.ts",
                "export default function a() {}\nexport function b() { return 1; }\n",
            ),
            ("src/polyfill.ts", "globalThis.x = 1;\n"),
        ],
    );

    let preview = run_fallow_in_root("fix", root, &["--dry-run", "--quiet"]);
    assert_eq!(preview.code, 0, "{}", preview.stderr);
    assert!(
        preview
            .stderr
            .contains("Would remove import from src/barrel.ts:1 `a`"),
        "{}",
        preview.stderr
    );

    apply(root);
    assert_eq!(
        read(root, "src/barrel.ts"),
        "import { b } from './a';\nimport './polyfill';\n\nconst twice = () => b() + b();\nexport { b as used };\n"
    );
}
