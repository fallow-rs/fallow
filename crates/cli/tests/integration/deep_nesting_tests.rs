#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Deep nesting without brackets must not abort a run. Each analysis that
//! parses the files (dead code, duplication, health) runs on them.

use std::fmt::Write as _;

use crate::common::{parse_json, run_fallow_raw};

/// Far past the depth at which these shapes overflow a 16 MiB stack.
const DEEP: usize = 20_000;

/// Files that push the parse from the main thread to the worker pool.
const FILLER_FILES: usize = 40;

fn write_deep_project(filler_files: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name": "deep", "main": "src/index.ts"}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    let deep_files = [
        (
            "generic.ts",
            format!(
                "export type T = {}B{};\n",
                "A<".repeat(DEEP),
                ">".repeat(DEEP)
            ),
        ),
        (
            "assign.ts",
            format!("export let a = 0;\n{}1;\n", "a=".repeat(DEEP)),
        ),
        (
            "ternary.ts",
            format!("export const t = {}c;\n", "a?b:".repeat(DEEP)),
        ),
        (
            "element.tsx",
            format!(
                "export const e = {}{};\n",
                "<a>".repeat(DEEP),
                "</a>".repeat(DEEP)
            ),
        ),
    ];
    let mut index = String::new();
    for (name, source) in &deep_files {
        std::fs::write(root.join("src").join(name), source).unwrap();
        let stem = name.split('.').next().unwrap();
        writeln!(index, "export * from './{stem}';").unwrap();
    }
    for n in 0..filler_files {
        std::fs::write(
            root.join("src").join(format!("filler{n}.ts")),
            format!("export const filler{n} = {n};\n"),
        )
        .unwrap();
        writeln!(index, "export * from './filler{n}';").unwrap();
    }
    std::fs::write(root.join("src/index.ts"), index).unwrap();
    dir
}

fn assert_run_survives(filler_files: usize) {
    let dir = write_deep_project(filler_files);
    let root = dir.path().to_str().unwrap();
    let output = run_fallow_raw(&["--root", root, "--format", "json", "--quiet", "--no-cache"]);
    assert!(
        matches!(output.code, 0 | 1),
        "the run must finish, got exit {}. stderr:\n{}",
        output.code,
        output.stderr
    );
    assert!(
        !output.stderr.contains("overflowed its stack"),
        "stderr:\n{}",
        output.stderr
    );
    parse_json(&output);
}

#[test]
fn deep_nesting_does_not_abort_a_small_project() {
    assert_run_survives(0);
}

#[test]
fn deep_nesting_does_not_abort_a_project_on_the_worker_pool() {
    assert_run_survives(FILLER_FILES);
}
