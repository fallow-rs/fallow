//! `--group-by` human output prints each section footer as one unit.
//!
//! A section footer is a description and a docs link. A grouped report prints
//! a footer the first time that it sees it and skips it in later groups. Two
//! sections that share a docs link but have different descriptions must both
//! keep their link.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::run_fallow_in_root;
use tempfile::TempDir;

const DEPS_URL: &str = "https://fallow.tools/docs/explanations/dead-code/#unused-dependencies";
const FILES_URL: &str = "https://fallow.tools/docs/explanations/dead-code/#unused-files";
const FILES_DESCRIPTION: &str = "Files not reachable from any entry point";

/// A project with an unused dependency, an unused devDependency, and an unused
/// file in each of two directories.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "footer-dedup",
  "private": true,
  "main": "src/index.ts",
  "dependencies": { "left-pad": "1.0.0" },
  "devDependencies": { "right-pad": "1.0.0" }
}
"#,
    )
    .expect("write package.json");
    for sub in ["src", "lib"] {
        std::fs::create_dir_all(root.join(sub)).expect("create dir");
    }
    std::fs::write(root.join("src/index.ts"), "export {};\n").expect("write index");
    std::fs::write(root.join("src/orphan.ts"), "export const a = 1;\n").expect("write orphan");
    std::fs::write(root.join("lib/orphan.ts"), "export const b = 1;\n").expect("write orphan");
    dir
}

fn grouped_output(root: &Path) -> String {
    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--group-by",
            "directory",
            "--unused-files",
            "--unused-deps",
            "--quiet",
        ],
    );
    assert!(
        matches!(output.code, 0 | 1),
        "unexpected exit {}\nstderr:\n{}",
        output.code,
        output.stderr
    );
    output.stdout
}

/// The trimmed line that follows each line that contains `needle`.
fn lines_after(stdout: &str, needle: &str) -> Vec<String> {
    let lines: Vec<&str> = stdout.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(i, _)| {
            lines
                .get(i + 1)
                .map_or_else(String::new, |line| line.trim().to_string())
        })
        .collect()
}

#[test]
fn grouped_footers_that_share_a_url_keep_their_link() {
    let dir = project();
    let stdout = grouped_output(dir.path());

    for description in [
        "Listed in dependencies but never imported",
        "Listed in devDependencies but never imported or referenced",
    ] {
        assert_eq!(
            lines_after(&stdout, description),
            vec![DEPS_URL.to_string()],
            "footer {description:?} must print once and keep its link\n{stdout}",
        );
    }
}

#[test]
fn grouped_footer_repeated_in_a_later_group_prints_no_orphan_description() {
    let dir = project();
    let stdout = grouped_output(dir.path());

    assert_eq!(
        stdout.matches(FILES_DESCRIPTION).count(),
        1,
        "the unused-files footer description must print once\n{stdout}",
    );
    assert_eq!(
        stdout.matches(FILES_URL).count(),
        1,
        "the unused-files footer link must print once\n{stdout}",
    );
    assert_eq!(
        lines_after(&stdout, FILES_DESCRIPTION),
        vec![FILES_URL.to_string()],
        "{stdout}",
    );
}
