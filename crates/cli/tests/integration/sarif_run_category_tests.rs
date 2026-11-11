#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Each run of a multi-analysis SARIF document has its own category.
//!
//! GitHub code scanning takes the category of a run from
//! `automationDetails.id`: the category is the text before the last `/`, and
//! the text after it is a run id. GitHub rejects an upload with two runs that
//! have the same tool and the same category. The bare command, `fallow audit`
//! and `fallow report --from` all merge several analyses into one document, so
//! the ids of the runs must differ before the last `/`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};
use serde_json::Value;
use tempfile::TempDir;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    fs::write(path, content).expect("write fixture file");
}

/// A function body long enough to pass the default clone thresholds and the
/// default complexity thresholds.
fn complex_function(name: &str) -> String {
    let mut body = format!("export function {name}(items: number[]): number {{\n");
    body.push_str("  let total = 0;\n");
    for index in 0..24 {
        writeln!(
            body,
            "  if (items[{index}] > {index} && items[{index}] < {}) {{ total += items[{index}] * {index}; }}",
            index + 100
        )
        .unwrap();
    }
    body.push_str("  return total;\n}\n");
    body
}

/// A committed base, then a working-tree change with an unused export, a
/// complex function and a clone. The audit of the change reports dead code,
/// duplication and health.
fn project() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"sarif-runs","main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const kept = 1;\n");
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    write(
        root,
        "src/index.ts",
        "import { first } from './first';\nimport { second } from './second';\nexport const kept = first([]) + second([]);\nexport const unusedValue = 2;\n",
    );
    write(root, "src/first.ts", &complex_function("first"));
    write(root, "src/second.ts", &complex_function("second"));
    write(
        root,
        "src/unused.ts",
        "export const orphan = (n: number): number => n;\n",
    );
    tmp
}

fn run_ok(args: &[&str]) -> CommandOutput {
    let output = run_fallow_raw(args);
    assert!(
        matches!(output.code, 0 | 1),
        "fallow {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    output
}

/// The category that GitHub code scanning derives from an
/// `automationDetails.id`: all text before the last `/`.
fn github_category(id: &str) -> &str {
    id.rsplit_once('/').map_or("", |(category, _)| category)
}

/// The `automationDetails.id` of each run, in order.
fn run_ids(sarif: &Value) -> Vec<String> {
    sarif["runs"]
        .as_array()
        .expect("runs array")
        .iter()
        .map(|run| {
            assert_eq!(run["tool"]["driver"]["name"], "fallow", "{run}");
            run["automationDetails"]["id"]
                .as_str()
                .expect("each run has an automationDetails.id")
                .to_owned()
        })
        .collect()
}

/// Assert the id of each run, in order, and assert that GitHub derives a
/// different category for each run. All runs have the tool name `fallow`, so
/// a unique category gives a unique `(tool, category)` pair.
fn assert_distinct_categories(sarif: &Value, expected: &[&str]) {
    let ids = run_ids(sarif);
    assert_eq!(ids, expected);
    let mut categories: Vec<&str> = ids.iter().map(|id| github_category(id)).collect();
    categories.sort_unstable();
    let count = categories.len();
    categories.dedup();
    assert_eq!(
        categories.len(),
        count,
        "two runs share a GitHub category: {ids:?}"
    );
}

#[test]
fn github_category_is_the_text_before_the_last_slash() {
    assert_eq!(github_category("fallow/dead-code/"), "fallow/dead-code");
    assert_eq!(github_category("fallow/dead-code"), "fallow");
    assert_eq!(github_category("fallow"), "");
}

fn bare_args<'a>(root: &'a str, format: &'a str) -> Vec<&'a str> {
    vec!["--root", root, "--quiet", "--no-cache", "--format", format]
}

fn audit_args<'a>(root: &'a str, format: &'a str) -> Vec<&'a str> {
    vec![
        "audit",
        "--root",
        root,
        "--base",
        "main",
        "--quiet",
        "--no-cache",
        "--format",
        format,
    ]
}

fn report_from(saved: &Path, root: &str) -> Value {
    let saved = saved.to_str().unwrap();
    let output = run_fallow_raw(&[
        "report", "--from", saved, "--root", root, "--format", "sarif",
    ]);
    assert_eq!(
        output.code, 0,
        "report --from failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    parse_json(&output)
}

const BARE_CATEGORIES: [&str; 3] = ["fallow/dead-code/", "fallow/dupes/", "fallow/health/"];
const AUDIT_CATEGORIES: [&str; 3] = [
    "fallow/audit/dead-code/",
    "fallow/audit/dupes/",
    "fallow/audit/health/",
];

#[test]
fn bare_sarif_gives_each_run_a_distinct_category() {
    let tmp = project();
    let root = tmp.path().to_str().unwrap();
    let sarif = parse_json(&run_ok(&bare_args(root, "sarif")));
    assert_distinct_categories(&sarif, &BARE_CATEGORIES);
}

#[test]
fn audit_sarif_gives_each_run_a_distinct_category() {
    let tmp = project();
    let root = tmp.path().to_str().unwrap();
    let sarif = parse_json(&run_ok(&audit_args(root, "sarif")));
    assert_distinct_categories(&sarif, &AUDIT_CATEGORIES);
}

#[test]
fn report_from_sarif_keeps_the_categories_of_direct_output() {
    let tmp = project();
    let root = tmp.path().to_str().unwrap();
    let saved_dir = TempDir::new().expect("saved dir");

    let bare = saved_dir.path().join("bare.json");
    fs::write(&bare, run_ok(&bare_args(root, "json")).stdout).unwrap();
    assert_distinct_categories(&report_from(&bare, root), &BARE_CATEGORIES);

    let audit = saved_dir.path().join("audit.json");
    fs::write(&audit, run_ok(&audit_args(root, "json")).stdout).unwrap();
    assert_distinct_categories(&report_from(&audit, root), &AUDIT_CATEGORIES);
}
