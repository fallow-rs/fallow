//! `--group-by` keeps every issue type that the flat report shows.
//!
//! The grouping builder partitions each result list into buckets. A list the
//! builder does not know is dropped, so the envelope `total_issues` counts
//! findings that no group lists. These tests run real fixtures flat and grouped
//! and compare the per-type counts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{copy_fixture, parse_json, run_fallow_in_root};
use tempfile::TempDir;

/// GitLab-style CODEOWNERS for `--group-by owner` and `--group-by section`.
const CODEOWNERS: &str = "* @root\n\n[Pages] @pages\n/app/ @pages\n\n[Components] @ui\n/src/ @ui\n";

/// Fixture, result key, and human section title of each checked issue type.
const CASES: [(&str, &str, &str); 6] = [
    ("nextjs-route-tree", "route_collisions", "Route collisions"),
    (
        "nextjs-route-tree",
        "dynamic_segment_name_conflicts",
        "Dynamic segment conflicts",
    ),
    (
        "svelte-dead-event",
        "unused_svelte_events",
        "Unused Svelte events",
    ),
    ("prop-drilling", "prop_drilling_chains", "Prop drilling"),
    ("thin-wrapper", "thin_wrappers", "Thin wrappers"),
    (
        "duplicate-prop-shape",
        "duplicate_prop_shapes",
        "Duplicate prop shapes",
    ),
];

/// Copy a fixture and give it a CODEOWNERS file.
fn project(fixture: &str) -> TempDir {
    let dir = copy_fixture(fixture);
    std::fs::write(dir.path().join("CODEOWNERS"), CODEOWNERS).expect("write CODEOWNERS");
    dir
}

/// Turns on the opt-in component health rules so their fixtures report.
fn config() -> (TempDir, String) {
    let dir = TempDir::new().expect("config dir");
    let path = dir.path().join("fallow.json");
    std::fs::write(
        &path,
        r#"{"rules":{"prop-drilling":"warn","thin-wrapper":"warn","duplicate-prop-shape":"warn"}}"#,
    )
    .expect("write config");
    let path = path.to_str().expect("utf-8 config path").to_string();
    (dir, path)
}

fn json(root: &Path, config: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--config", config, "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("dead-code", root, &args);
    assert!(
        matches!(output.code, 0 | 1),
        "{extra:?}: unexpected exit {}\nstderr:\n{}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

fn len(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map_or(0, Vec::len)
}

#[test]
fn grouped_json_lists_every_issue_type_of_the_flat_report() {
    let (_dir, config) = config();
    for (fixture, key, _) in CASES {
        let project = project(fixture);
        let root = project.path();
        let flat = json(root, &config, &[]);
        for mode in ["directory", "owner", "section"] {
            let flat_count = len(&flat, key);
            assert!(flat_count > 0, "{fixture}: the fixture must report {key}");

            let grouped = json(root, &config, &["--group-by", mode]);
            let groups = grouped["groups"].as_array().expect("groups array");
            let grouped_count: usize = groups.iter().map(|group| len(group, key)).sum();
            assert_eq!(
                grouped_count, flat_count,
                "{fixture} --group-by {mode}: groups must list every {key} finding: {grouped}"
            );

            let group_total: u64 = groups
                .iter()
                .map(|group| group["total_issues"].as_u64().expect("group total"))
                .sum();
            assert_eq!(
                group_total,
                grouped["total_issues"].as_u64().expect("envelope total"),
                "{fixture} --group-by {mode}: group totals must add up to the envelope total"
            );
        }
    }
}

#[test]
fn grouped_human_shows_every_issue_type_of_the_flat_report() {
    let (_dir, config) = config();
    for (fixture, _, title) in CASES {
        let project = project(fixture);
        for mode in ["directory", "owner", "section"] {
            let output = run_fallow_in_root(
                "dead-code",
                project.path(),
                &["--config", &config, "--group-by", mode, "--quiet"],
            );
            assert!(
                output.stdout.contains(title),
                "{fixture} --group-by {mode}: grouped human output must show the {title:?} \
                 section:\n{}",
                output.stdout
            );
        }
    }
}

/// Compact and markdown list the issue types that count toward
/// `total_issues`. The component health signals are JSON and human only.
#[test]
fn grouped_compact_and_markdown_show_counted_issue_types() {
    let cases = [
        (
            "nextjs-route-tree",
            ["route-collision:", "dynamic-segment-name-conflict:"].as_slice(),
            ["Route collisions", "Dynamic segment conflicts"].as_slice(),
        ),
        (
            "svelte-dead-event",
            ["unused-svelte-event:"].as_slice(),
            ["Unused Svelte events"].as_slice(),
        ),
    ];
    for (fixture, tags, titles) in cases {
        let project = project(fixture);
        let root = project.path();
        let args = |format| ["--group-by", "directory", "--format", format, "--quiet"];
        let compact = run_fallow_in_root("dead-code", root, &args("compact"));
        for tag in tags {
            assert!(
                compact.stdout.contains(tag),
                "{fixture}: grouped compact output must list {tag}:\n{}",
                compact.stdout
            );
        }
        let markdown = run_fallow_in_root("dead-code", root, &args("markdown"));
        for title in titles {
            assert!(
                markdown.stdout.contains(title),
                "{fixture}: grouped markdown output must show {title:?}:\n{}",
                markdown.stdout
            );
        }
    }
}
