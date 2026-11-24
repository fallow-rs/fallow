//! A `duplicates.ignore` or `health.ignore` glob that matches nothing gives a
//! note in the run that reads the setting: `fallow dupes` and `fallow health`.
//! The dead-code run gives the same notes (issue #5167).

#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use super::check_tests::{stale_ignore_list_project, unmatched_ignore_list_entries};
use crate::common::{parse_json, run_fallow_in_root};

fn entry(kind: &str, pattern: &str) -> (String, String) {
    (kind.to_owned(), pattern.to_owned())
}

#[test]
fn dupes_reports_each_duplicates_ignore_glob_that_matches_nothing() {
    let dir = stale_ignore_list_project();

    let json = parse_json(&run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert_eq!(
        unmatched_ignore_list_entries(&json),
        vec![entry("duplicates-ignore-unmatched", "src/never/**")],
        "src/gen/** matches a module, and a dupes run does not read the other lists: {json}"
    );

    let human = run_fallow_in_root("dupes", dir.path(), &[]);
    assert!(
        human
            .stderr
            .contains("Note: duplicates.ignore glob matched no file this run: src/never/**"),
        "{}",
        human.stderr
    );
    assert!(!human.stderr.contains("health.ignore"), "{}", human.stderr);

    let production = parse_json(&run_fallow_in_root(
        "dupes",
        dir.path(),
        &["--format", "json", "--quiet", "--production"],
    ));
    assert!(
        unmatched_ignore_list_entries(&production).is_empty(),
        "a production run analyzes fewer files, so it reports no unmatched entry: {production}"
    );
}

#[test]
fn health_reports_each_health_ignore_glob_that_matches_nothing() {
    let dir = stale_ignore_list_project();

    let json = parse_json(&run_fallow_in_root(
        "health",
        dir.path(),
        &["--format", "json", "--quiet"],
    ));
    assert_eq!(
        unmatched_ignore_list_entries(&json),
        vec![entry("health-ignore-unmatched", "src/nothing/**")],
        "src/gen/** matches a module, docs/** matches a Markdown file that the health \
         pipeline reads, and a health run does not read the other lists: {json}"
    );

    let human = run_fallow_in_root("health", dir.path(), &[]);
    assert!(
        human
            .stderr
            .contains("Note: health.ignore glob matched no file this run: src/nothing/**"),
        "{}",
        human.stderr
    );
    assert!(
        !human.stderr.contains("duplicates.ignore"),
        "{}",
        human.stderr
    );

    let production = parse_json(&run_fallow_in_root(
        "health",
        dir.path(),
        &["--format", "json", "--quiet", "--production"],
    ));
    assert!(
        unmatched_ignore_list_entries(&production).is_empty(),
        "a production run analyzes fewer files, so it reports no unmatched entry: {production}"
    );
}

#[test]
fn combined_run_reports_each_unmatched_ignore_list_entry_once() {
    let dir = stale_ignore_list_project();

    let json = parse_json(&crate::common::run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("utf-8 path"),
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(
        unmatched_ignore_list_entries(&json),
        vec![
            entry("ignore-exports-file-unmatched", "src/missing.ts"),
            entry("ignore-exports-name-unmatched", "gone in src/a.ts"),
            entry("duplicates-ignore-unmatched", "src/never/**"),
            entry("health-ignore-unmatched", "src/nothing/**"),
        ],
        "the dead-code, dupes and health sections can state the same entry: {json}"
    );
}

#[test]
fn combined_human_run_prints_each_ignore_list_note_once() {
    let dir = stale_ignore_list_project();
    let root = dir.path().to_str().expect("utf-8 path");
    let notes = [
        "Note: duplicates.ignore glob matched no file this run: src/never/**",
        "Note: health.ignore glob matched no file this run: src/nothing/**",
    ];

    let full = crate::common::run_fallow_raw(&["--root", root]);
    for note in notes {
        assert_eq!(
            full.stderr.matches(note).count(),
            1,
            "{note}\n{}",
            full.stderr
        );
    }

    for (only, note) in [("dupes", notes[0]), ("health", notes[1])] {
        let run = crate::common::run_fallow_raw(&["--root", root, "--only", only]);
        assert_eq!(
            run.stderr.matches(note).count(),
            1,
            "a --only {only} run has no dead-code section that states the entry: {}",
            run.stderr
        );
    }
}
