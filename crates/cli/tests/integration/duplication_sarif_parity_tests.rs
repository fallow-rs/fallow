#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The duplication run has one shape in every SARIF document.
//!
//! `fallow dupes`, the bare command, `fallow audit` and `fallow report --from`
//! all emit a duplication run. A code scanning service shows a result only at
//! a file location, so each surface must give one result per clone instance,
//! with a location, and must declare the `fallow/code-duplication` rule.

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

/// A function body long enough to pass the default clone thresholds.
fn cloned_function(name: &str) -> String {
    let mut body = format!("export function {name}(items: number[]): number {{\n");
    body.push_str("  let total = 0;\n");
    for index in 0..12 {
        writeln!(
            body,
            "  if (items[{index}] > {index}) {{ total += items[{index}] * {index}; }}"
        )
        .unwrap();
    }
    body.push_str("  return total;\n}\n");
    body
}

/// A committed base, then a working-tree change that adds two files with one
/// clone between them. The audit of the change reports the clone group.
fn project() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"dupes-sarif","main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const kept = 1;\n");
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    write(
        root,
        "src/index.ts",
        "import { first } from './first';\nimport { second } from './second';\nexport const kept = first([]) + second([]);\n",
    );
    write(root, "src/first.ts", &cloned_function("first"));
    write(root, "src/second.ts", &cloned_function("second"));
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

fn common_args<'a>(root: &'a str, format: &'a str) -> [&'a str; 6] {
    ["--root", root, "--quiet", "--no-cache", "--format", format]
}

fn sarif_of(command: &[&str], root: &str) -> Value {
    let mut args = command.to_vec();
    args.extend(common_args(root, "sarif"));
    parse_json(&run_ok(&args))
}

fn report_from(command: &[&str], root: &str) -> Value {
    let mut args = command.to_vec();
    args.extend(common_args(root, "json"));
    let saved_dir = TempDir::new().expect("saved dir");
    let saved = saved_dir.path().join("saved.json");
    fs::write(&saved, run_ok(&args).stdout).unwrap();
    let output = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().unwrap(),
        "--root",
        root,
        "--format",
        "sarif",
    ]);
    assert_eq!(
        output.code, 0,
        "report --from failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
    parse_json(&output)
}

/// The run that holds the `fallow/code-duplication` rule or results.
fn duplication_run(sarif: &Value) -> &Value {
    sarif["runs"]
        .as_array()
        .expect("runs array")
        .iter()
        .find(|run| {
            run["automationDetails"]["id"]
                .as_str()
                .is_some_and(|id| id.ends_with("dupes/"))
                || run["results"].as_array().is_some_and(|results| {
                    results
                        .iter()
                        .any(|result| result["ruleId"] == "fallow/code-duplication")
                })
        })
        .unwrap_or_else(|| panic!("no duplication run in {sarif}"))
}

/// The results and the declared rules of the duplication run. The category
/// differs per command by design, so the comparison leaves it out.
fn duplication_shape(sarif: &Value) -> (Value, Value) {
    let run = duplication_run(sarif);
    (
        run["results"].clone(),
        run["tool"]["driver"]["rules"].clone(),
    )
}

#[test]
fn every_surface_gives_the_dupes_shape_of_the_duplication_run() {
    let tmp = project();
    let root = tmp.path().to_str().unwrap();
    let audit: &[&str] = &["audit", "--base", "main"];

    let dupes = duplication_shape(&sarif_of(&["dupes"], root));
    let results = dupes.0.as_array().expect("results array");
    assert_eq!(
        results.len(),
        2,
        "one result per clone instance: {results:?}"
    );
    for result in results {
        assert!(
            result
                .pointer("/locations/0/physicalLocation/artifactLocation/uri")
                .is_some(),
            "each duplication result has a file location: {result}"
        );
    }
    assert_eq!(dupes.1[0]["id"], "fallow/code-duplication");

    for (surface, sarif) in [
        ("bare run", sarif_of(&[], root)),
        ("audit", sarif_of(audit, root)),
        ("report --from bare run", report_from(&[], root)),
        ("report --from audit", report_from(audit, root)),
    ] {
        assert_eq!(
            duplication_shape(&sarif),
            dupes,
            "{surface} gives another duplication run than fallow dupes"
        );
    }
}

/// The bare run, `fallow audit` and `fallow report --from` must take their
/// duplication run from `fallow-output`, so the run keeps the shape of
/// `fallow dupes --format sarif`.
#[test]
fn multi_run_sarif_writers_take_the_duplication_run_from_output() {
    let workspace =
        Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join("../..");
    let owner = fs::read_to_string(workspace.join("crates/output/src/analysis_sarif.rs"))
        .expect("read the owner module");
    assert!(owner.contains("pub fn extend_sarif_runs_with_duplication"));
    for source_path in [
        "crates/cli/src/combined/output.rs",
        "crates/cli/src/report/sarif.rs",
        "crates/api/src/audit_output.rs",
    ] {
        let source = fs::read_to_string(workspace.join(source_path))
            .unwrap_or_else(|error| panic!("read {source_path}: {error}"));
        assert!(
            source.contains("extend_sarif_runs_with_duplication"),
            "{source_path} must build its duplication SARIF run with fallow-output"
        );
        assert!(
            !source.contains("\"Clone group "),
            "{source_path} must not build its own duplication SARIF results"
        );
    }
}
