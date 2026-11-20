//! Each example project of `fallow explain` reports the rule that it
//! illustrates.
//!
//! The examples are in `crates/api/rule-examples/`. `fallow explain` and the
//! rule pages of the docs show their files, so an example that no longer
//! reports its rule would teach the wrong code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{ExampleFile, RuleExample, all_rules, bare_rule_id, rule_command, rule_examples};
use tempfile::TempDir;

use crate::common::{commit_all, git, run_fallow_in_root};

fn write_files(root: &Path, files: &[ExampleFile]) {
    for file in files {
        let path = root.join(file.path);
        fs::create_dir_all(path.parent().expect("a file has a parent directory")).unwrap();
        fs::write(path, file.code).unwrap();
    }
}

/// A finding of a run: the rule id and the path of the file that it is on.
type Finding = (String, String);

/// A SARIF artifact URI as a project path. The SARIF writer encodes `[` and
/// `]`, which a route file such as `app/blog/[id]/page.tsx` has.
fn uri_to_path(uri: &str) -> String {
    uri.replace("%5B", "[").replace("%5D", "]")
}

/// The findings that `fallow <command>` reports for the example, or the
/// reason why the run gave no report.
fn findings(example: &RuleExample, command: &str) -> Result<Vec<Finding>, String> {
    let project = TempDir::new().map_err(|error| format!("no temp dir: {error}"))?;
    let root = project.path();
    let mut args: Vec<&str> = example.args.to_vec();
    let is_change = !example.base.is_empty();
    if is_change {
        write_files(root, example.base);
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "core.autocrlf", "false"]);
        commit_all(root, "base");
        write_files(root, example.files);
        commit_all(root, "change");
        args.extend(["--base", "HEAD~1", "--format", "json", "--quiet"]);
    } else {
        write_files(root, example.files);
        args.extend(["--format", "sarif", "--quiet"]);
    }
    let output = run_fallow_in_root(command, root, &args);
    let report: serde_json::Value = serde_json::from_str(&output.stdout).map_err(|error| {
        format!(
            "no JSON report (exit code {}): {error}\nstderr: {}",
            output.code, output.stderr
        )
    })?;
    let found = if is_change {
        report["diff_risk"]["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|finding| {
                let rule = finding["rule"].as_str()?;
                let path = finding["path"].as_str()?;
                Some((format!("fallow/{rule}"), path.to_string()))
            })
            .collect()
    } else {
        report["runs"][0]["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|result| {
                let rule = result["ruleId"].as_str()?;
                let uri = result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                    .as_str()?;
                Some((rule.to_string(), uri_to_path(uri)))
            })
            .collect()
    };
    Ok(found)
}

/// Why `example` does not show its rule, or `None` when it does: the rule
/// reports a finding on a file that the explain output shows.
fn failure(example: &RuleExample) -> Option<String> {
    let Some(rule) = all_rules().find(|rule| bare_rule_id(rule) == example.rule) else {
        return Some(format!("{}: no registered rule has this id", example.rule));
    };
    let is_visible = |path: &str| {
        example
            .files
            .iter()
            .any(|file| !file.hidden && file.path == path)
    };
    match findings(example, rule_command(rule)) {
        Ok(found)
            if found
                .iter()
                .any(|(reported, path)| reported == rule.id && is_visible(path)) =>
        {
            None
        }
        Ok(found) => Some(format!(
            "{}: no finding of {} is on a visible file of the example. Reported: {found:?}",
            example.rule, rule.id
        )),
        Err(error) => Some(format!("{}: {error}", example.rule)),
    }
}

#[test]
fn each_example_project_reports_its_rule() {
    let examples = rule_examples();
    let workers = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk_size = examples.len().div_ceil(workers).max(1);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in examples.chunks(chunk_size) {
            handles.push(scope.spawn(|| chunk.iter().filter_map(failure).collect::<Vec<_>>()));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("an example run panicked"))
            .collect()
    });
    assert!(
        failures.is_empty(),
        "{} of {} example projects do not show their rule:\n{}",
        failures.len(),
        examples.len(),
        failures.join("\n")
    );
}
