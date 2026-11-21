//! Each example project of `fallow explain` reports the rule that it
//! illustrates, at the same location in each output format.
//!
//! The examples are in `crates/api/rule-examples/`. `fallow explain` and the
//! rule pages of the docs show their files, so an example that no longer
//! reports its rule would teach the wrong code.
//!
//! The test runs each example in SARIF, JSON, CodeClimate and compact
//! output. SARIF is the reference: each SARIF result of the rule is one
//! finding. Each other format must report each finding in the same file and
//! on the same line, and the location must match the anchor that the example
//! declares. The LSP is not part of this test: the CLI test crate does not
//! depend on `fallow-lsp`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use fallow_api::{
    ExampleAnchor, ExampleFile, RuleExample, all_rules, bare_rule_id, rule_command, rule_examples,
};
use serde_json::Value;
use tempfile::TempDir;

use crate::common::{CommandOutput, commit_all, git, run_fallow_in_root};

/// The output formats that the test compares with SARIF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Format {
    Json,
    CodeClimate,
    Compact,
}

impl Format {
    const ALL: [Self; 3] = [Self::Json, Self::CodeClimate, Self::Compact];

    const fn flag(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::CodeClimate => "codeclimate",
            Self::Compact => "compact",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.flag())
    }
}

/// One check of an example: the SARIF location against the anchor of the
/// example, or one format against SARIF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Check {
    Anchor,
    Output(Format),
}

impl fmt::Display for Check {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anchor => formatter.write_str("the anchor"),
            Self::Output(format) => write!(formatter, "{format}"),
        }
    }
}

/// A known failure: the rule, the check that fails, and the reason. The test
/// fails when an entry no longer matches a failure, so the list cannot go
/// stale.
const KNOWN_GAPS: &[(&str, Check, &str)] = &[
    (
        "css-broken-reference",
        Check::Anchor,
        "the undefined-keyframes finding has no line, so it reports line 1",
    ),
    (
        "dev-dependency-in-production",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "duplicate-export",
        Check::Output(Format::Compact),
        "the compact line names the export only, with no file and no line",
    ),
    (
        "duplicate-prop-shape",
        Check::Output(Format::CodeClimate),
        "CodeClimate reports a component-health finding only at the `error` severity",
    ),
    (
        "prop-drilling",
        Check::Output(Format::CodeClimate),
        "CodeClimate reports a component-health finding only at the `error` severity",
    ),
    (
        "refactoring-target",
        Check::Output(Format::CodeClimate),
        "the CodeClimate output of `fallow health` has no refactoring targets",
    ),
    (
        "test-only-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "thin-wrapper",
        Check::Output(Format::CodeClimate),
        "CodeClimate reports a component-health finding only at the `error` severity",
    ),
    (
        "type-only-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "unlisted-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "unused-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "unused-dev-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
    (
        "unused-optional-dependency",
        Check::Output(Format::Compact),
        "the compact line names the package only, with no file and no line",
    ),
];

/// The compact tag of each rule whose tag is not its rule id. Compact output
/// starts each line with the tag.
const COMPACT_TAGS: &[(&str, &str)] = &[
    ("boundary-call-violation", "boundary-call"),
    ("dev-dependency-in-production", "dev-dep-in-prod"),
    ("feature-flag", "feature-flag-env"),
    ("flag-retirement-candidate", "flag-retire"),
    ("high-cognitive-complexity", "high-complexity"),
    ("high-crap-score", "high-complexity"),
    ("high-cyclomatic-complexity", "high-complexity"),
    ("long-file", "high-complexity"),
    ("missing-suppression-reason", "stale-suppression"),
    ("test-only-dependency", "test-only-dep"),
    ("type-only-dependency", "type-only-dep"),
    ("unlisted-dependency", "unlisted-dep"),
    ("unused-dependency", "unused-dep"),
    ("unused-dev-dependency", "unused-devdep"),
    ("unused-optional-dependency", "unused-optionaldep"),
];

fn compact_tag(rule: &str) -> &str {
    COMPACT_TAGS
        .iter()
        .find(|(id, _)| *id == rule)
        .map_or(rule, |(_, tag)| tag)
}

fn write_files(root: &Path, files: &[ExampleFile]) {
    for file in files {
        let path = root.join(file.path);
        fs::create_dir_all(path.parent().expect("a file has a parent directory")).unwrap();
        fs::write(path, file.code).unwrap();
    }
}

/// A finding of the SARIF reference: the project path and the 1-based line
/// and column. `None` when the result has no region.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Location {
    path: String,
    line: Option<u64>,
    column: Option<u64>,
}

impl fmt::Display for Location {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(formatter, "{}:{line}:{column}", self.path),
            (Some(line), None) => write!(formatter, "{}:{line}", self.path),
            _ => write!(formatter, "{} (no line)", self.path),
        }
    }
}

/// A SARIF artifact URI as a project path. The SARIF writer encodes `[` and
/// `]`, which a route file such as `app/blog/[id]/page.tsx` has.
fn uri_to_path(uri: &str) -> String {
    uri.replace("%5B", "[").replace("%5D", "]")
}

/// A project with the files of the example. For a rule that reports on a
/// change, the project is a Git repository with the base and the change.
fn project(example: &RuleExample) -> TempDir {
    let project = TempDir::new().expect("a temp dir");
    let root = project.path();
    if example.base.is_empty() {
        write_files(root, example.files);
    } else {
        write_files(root, example.base);
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "core.autocrlf", "false"]);
        commit_all(root, "base");
        write_files(root, example.files);
        commit_all(root, "change");
    }
    project
}

fn run(example: &RuleExample, command: &str, root: &Path, format: &str) -> CommandOutput {
    let mut args: Vec<&str> = example.args.to_vec();
    if !example.base.is_empty() {
        args.extend(["--base", "HEAD~1"]);
    }
    args.extend(["--format", format, "--quiet"]);
    run_fallow_in_root(command, root, &args)
}

/// Whether the command rejects the format, for example `fallow security
/// --format compact`.
fn rejects_format(output: &CommandOutput) -> bool {
    output.code == 2 && output.stderr.contains("supports --format")
}

fn parse(output: &CommandOutput, format: &str) -> Result<Value, String> {
    serde_json::from_str(&output.stdout).map_err(|error| {
        format!(
            "no {format} report (exit code {}): {error}\nstderr: {}",
            output.code, output.stderr
        )
    })
}

/// The SARIF results of `rule_id` in all runs. `fallow audit` writes one
/// run for each analysis.
fn sarif_locations(report: &Value, rule_id: &str) -> Vec<Location> {
    let mut found: Vec<Location> = report["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|run| run["results"].as_array())
        .flatten()
        .filter(|result| result["ruleId"].as_str() == Some(rule_id))
        .filter_map(|result| {
            let physical = &result["locations"][0]["physicalLocation"];
            let uri = physical["artifactLocation"]["uri"].as_str()?;
            Some(Location {
                path: uri_to_path(uri),
                line: physical["region"]["startLine"].as_u64(),
                column: physical["region"]["startColumn"].as_u64(),
            })
        })
        .collect();
    found.sort();
    found
}

/// Each `(path, line)` pair in a JSON report. A row names its file as
/// `path`, `file` or `from_path`, and its line as `line` or `start_line`. A
/// row with a path and no line gives `None`.
fn json_locations(node: &Value, found: &mut BTreeSet<(String, Option<u64>)>) {
    match node {
        Value::Array(items) => items.iter().for_each(|item| json_locations(item, found)),
        Value::Object(object) => {
            let path = object
                .get("path")
                .or_else(|| object.get("file"))
                .or_else(|| object.get("from_path"))
                .and_then(Value::as_str);
            if let Some(path) = path {
                let line = object
                    .get("line")
                    .or_else(|| object.get("start_line"))
                    .and_then(Value::as_u64);
                found.insert((path.to_string(), line));
            }
            object
                .values()
                .for_each(|value| json_locations(value, found));
        }
        _ => {}
    }
}

/// Whether the compact line `text` has the tag `tag` and holds
/// `:<path>:<line>`, or `:<path>` with no line after it when `line` is `None`.
fn compact_line_has(text: &str, tag: &str, path: &str, line: Option<u64>) -> bool {
    if text.split(':').next() != Some(tag) {
        return false;
    }
    let text = uri_to_path(text);
    let needle = format!(":{path}");
    text.match_indices(&needle).any(|(start, _)| {
        let rest = &text[start + needle.len()..];
        match line {
            Some(line) => rest
                .strip_prefix(&format!(":{line}"))
                .is_some_and(|after| !after.starts_with(|next: char| next.is_ascii_digit())),
            None => rest.is_empty() || rest.starts_with(':') || rest.starts_with(' '),
        }
    })
}

/// The line that a format must report for `location`. A file-level finding
/// has no line in SARIF. CodeClimate needs a line, and it uses line 1.
fn expected_line(location: &Location, format: Format) -> Option<u64> {
    match (location.line, format) {
        (None, Format::CodeClimate) => Some(1),
        (line, _) => line,
    }
}

/// The SARIF locations that `format` does not report.
fn missing_in(
    format: Format,
    rule: &str,
    report: &str,
    sarif: &[Location],
) -> Result<Vec<Location>, String> {
    let missing = match format {
        Format::Json => {
            let value: Value = serde_json::from_str(report).map_err(|error| error.to_string())?;
            let mut found = BTreeSet::new();
            json_locations(&value, &mut found);
            sarif
                .iter()
                .filter(|location| {
                    !found.contains(&(location.path.clone(), expected_line(location, format)))
                })
                .cloned()
                .collect()
        }
        Format::CodeClimate => {
            let value: Value = serde_json::from_str(report).map_err(|error| error.to_string())?;
            let mut issues: Vec<(String, Option<u64>)> = value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|issue| {
                    let location = &issue["location"];
                    let line = location["lines"]["begin"]
                        .as_u64()
                        .or_else(|| location["positions"]["begin"]["line"].as_u64());
                    Some((uri_to_path(location["path"].as_str()?), line))
                })
                .collect();
            let mut missing = Vec::new();
            for location in sarif {
                let wanted = (location.path.clone(), expected_line(location, format));
                match issues.iter().position(|issue| *issue == wanted) {
                    Some(index) => {
                        issues.swap_remove(index);
                    }
                    None => missing.push(location.clone()),
                }
            }
            missing
        }
        Format::Compact => sarif
            .iter()
            .filter(|location| {
                !report.lines().any(|text| {
                    compact_line_has(
                        text,
                        compact_tag(rule),
                        &location.path,
                        expected_line(location, format),
                    )
                })
            })
            .cloned()
            .collect(),
    };
    Ok(missing)
}

/// Why the SARIF location does not match the anchor of the example, or
/// `None` when it does.
fn anchor_failure(anchor: ExampleAnchor, location: &Location) -> Option<String> {
    match anchor {
        ExampleAnchor::FileLevel => match location.line {
            None | Some(1) => None,
            Some(line) => Some(format!(
                "the example is file-level, but SARIF reports line {line} at {location}"
            )),
        },
        ExampleAnchor::Line => match (location.line, location.column) {
            (None, _) => Some(format!("SARIF reports no line at {location}")),
            (Some(1), None | Some(1)) => Some(format!(
                "SARIF reports line 1, column 1 at {location}. The example code is not there, \
                 so the location is a fill-in for a missing span. If the rule is about a whole \
                 file, set `\"anchor\": \"file-level\"` in example.json"
            )),
            _ => None,
        },
    }
}

/// The problems of one example: the setup errors, and the checks that fail,
/// each with a message.
#[derive(Default)]
struct Outcome {
    errors: Vec<String>,
    failed: Vec<(Check, String)>,
}

fn check(example: &RuleExample) -> Outcome {
    let mut outcome = Outcome::default();
    let Some(rule) = all_rules().find(|rule| bare_rule_id(rule) == example.rule) else {
        outcome
            .errors
            .push(format!("{}: no registered rule has this id", example.rule));
        return outcome;
    };
    let command = rule_command(rule);
    let project = project(example);
    let root = project.path();

    let sarif = match parse(&run(example, command, root, "sarif"), "SARIF") {
        Ok(report) => sarif_locations(&report, rule.id),
        Err(error) => {
            outcome.errors.push(format!("{}: {error}", example.rule));
            return outcome;
        }
    };
    let is_visible = |path: &str| {
        example
            .files
            .iter()
            .any(|file| !file.hidden && file.path == path)
    };
    if !sarif.iter().any(|location| is_visible(&location.path)) {
        outcome.errors.push(format!(
            "{}: no SARIF result of {} is on a visible file of the example. Reported: {sarif:?}",
            example.rule, rule.id
        ));
        return outcome;
    }
    for location in &sarif {
        if let Some(error) = anchor_failure(example.anchor, location) {
            outcome
                .failed
                .push((Check::Anchor, format!("{}: {error}", example.rule)));
        }
    }

    for format in Format::ALL {
        let output = run(example, command, root, format.flag());
        if rejects_format(&output) {
            continue;
        }
        match missing_in(format, example.rule, &output.stdout, &sarif) {
            Ok(missing) if missing.is_empty() => {}
            Ok(missing) => {
                let missing: Vec<String> = missing.iter().map(ToString::to_string).collect();
                outcome.failed.push((
                    Check::Output(format),
                    format!(
                        "{}: {format} does not report the SARIF location {}",
                        example.rule,
                        missing.join(", ")
                    ),
                ));
            }
            Err(error) => outcome.errors.push(format!(
                "{}: no {format} report (exit code {}): {error}\nstderr: {}",
                example.rule, output.code, output.stderr
            )),
        }
    }
    outcome
}

#[test]
fn each_example_reports_its_rule_at_one_location_in_each_format() {
    let examples = rule_examples();
    let workers = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk_size = examples.len().div_ceil(workers).max(1);
    let outcomes: Vec<(&str, Outcome)> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in examples.chunks(chunk_size) {
            handles.push(scope.spawn(|| {
                chunk
                    .iter()
                    .map(|example| (example.rule, check(example)))
                    .collect::<Vec<_>>()
            }));
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("an example run panicked"))
            .collect()
    });

    let mut failures = Vec::new();
    let mut seen_gaps = BTreeSet::new();
    for (rule, outcome) in &outcomes {
        failures.extend(outcome.errors.iter().cloned());
        for (check, message) in &outcome.failed {
            if KNOWN_GAPS
                .iter()
                .any(|(gap_rule, gap_check, _)| gap_rule == rule && gap_check == check)
            {
                seen_gaps.insert((*rule, *check));
            } else {
                failures.push(message.clone());
            }
        }
    }
    for (rule, check, reason) in KNOWN_GAPS {
        if !seen_gaps.contains(&(*rule, *check)) {
            failures.push(format!(
                "{rule}: the KNOWN_GAPS entry for {check} (\"{reason}\") matches no \
                 failure. Remove the entry."
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} problems in {} example projects:\n{}",
        failures.len(),
        examples.len(),
        failures.join("\n")
    );
}

#[test]
fn compact_line_has_matches_a_path_and_a_line() {
    let has = |text: &str, path: &str, line: Option<u64>| {
        let tag = text.split(':').next().unwrap();
        compact_line_has(text, tag, path, line)
    };
    assert!(has("unused-export:src/a.ts:3:x", "src/a.ts", Some(3)));
    assert!(has("code-duplication:src/a.ts:3-11:f", "src/a.ts", Some(3)));
    assert!(!has("unused-export:src/a.ts:31:x", "src/a.ts", Some(3)));
    assert!(!has(
        "re-export-cycle:src/a.ts:src/a.ts <-> src/b.ts",
        "src/a.ts",
        Some(1)
    ));
    assert!(has("unused-file:src/a.ts", "src/a.ts", None));
    assert!(has(
        "route-collision:app/%5Bid%5D/page.tsx:1 (url)",
        "app/[id]/page.tsx",
        Some(1)
    ));
    assert!(!compact_line_has(
        "unused-type:src/a.ts:3:x",
        "unused-export",
        "src/a.ts",
        Some(3)
    ));
}
