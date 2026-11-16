//! `circularDependencies.allow` (issue #4623) accepts a cycle that has
//! exactly the files of an entry, in any rotation. A cycle that shares an
//! edge with an accepted cycle but has another file set stays reported.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "cycles-allow";
/// Five files that all import each other: one SCC with more cycles than the
/// detector lists.
const DENSE_FIXTURE: &str = "cycles-allow-dense";

fn entry(files: &[&str]) -> Vec<String> {
    files.iter().map(|file| (*file).to_owned()).collect()
}

/// The reported cycles in cycle order as fixture-relative paths, and the
/// `allow` entries that matched no cycle.
fn analyze(allow: Vec<Vec<String>>) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    analyze_fixture(FIXTURE, allow)
}

fn analyze_fixture(fixture: &str, allow: Vec<Vec<String>>) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let root = fixture_path(fixture);
    let mut config = create_config(root.clone());
    config.circular_dependencies.allow = allow;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut cycles: Vec<Vec<String>> = results
        .circular_dependencies
        .iter()
        .map(|finding| {
            finding
                .cycle
                .files
                .iter()
                .map(|path| {
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        })
        .collect();
    cycles.sort();
    let unmatched = config
        .circular_dependencies_allow_effect
        .ineffective(&config.circular_dependencies.allow)
        .into_iter()
        .cloned()
        .collect();
    (cycles, unmatched)
}

#[test]
fn without_allow_both_cycles_are_reported() {
    let (cycles, unmatched) = analyze(Vec::new());
    assert_eq!(cycles.len(), 2, "{cycles:#?}");
    assert!(unmatched.is_empty());
}

#[test]
fn an_entry_accepts_only_the_cycle_with_exactly_its_files() {
    let (cycles, unmatched) = analyze(vec![entry(&["src/a.ts", "src/b.ts"])]);
    assert_eq!(
        cycles,
        vec![entry(&["src/a.ts", "src/b.ts", "src/c.ts"])],
        "the longer cycle shares the a -> b edge and stays reported"
    );
    assert!(unmatched.is_empty(), "{unmatched:#?}");
}

#[test]
fn an_entry_matches_any_rotation_of_the_cycle() {
    let (cycles, unmatched) = analyze(vec![
        entry(&["src/b.ts", "src/a.ts"]),
        entry(&["./src/c.ts", "src/a.ts", "src/b.ts"]),
    ]);
    assert!(cycles.is_empty(), "{cycles:#?}");
    assert!(unmatched.is_empty(), "{unmatched:#?}");
}

#[test]
fn an_entry_in_another_order_or_with_other_files_is_unmatched() {
    let reversed = entry(&["src/a.ts", "src/c.ts", "src/b.ts"]);
    let missing = entry(&["src/a.ts", "src/missing.ts"]);
    let (cycles, unmatched) = analyze(vec![reversed.clone(), missing.clone()]);
    assert_eq!(cycles.len(), 2, "{cycles:#?}");
    assert_eq!(unmatched, vec![reversed, missing]);
}

/// Whether `cycle` is `entry` from some start file.
fn is_rotation(entry: &[String], cycle: &[String]) -> bool {
    entry.len() == cycle.len()
        && (0..cycle.len()).any(|start| {
            entry
                .iter()
                .enumerate()
                .all(|(offset, file)| cycle[(start + offset) % cycle.len()] == *file)
        })
}

#[test]
fn accepted_cycles_do_not_use_the_per_group_cycle_limit() {
    let (listed, _) = analyze_fixture(DENSE_FIXTURE, Vec::new());
    assert_eq!(
        listed.len(),
        20,
        "the detector lists 20 cycles: {listed:#?}"
    );
    let unlisted = entry(&["src/b.ts", "src/c.ts", "src/d.ts"]);
    let unlisted = if listed.iter().any(|cycle| is_rotation(&unlisted, cycle)) {
        entry(&["src/b.ts", "src/d.ts", "src/c.ts"])
    } else {
        unlisted
    };
    assert!(
        !listed.iter().any(|cycle| is_rotation(&unlisted, cycle)),
        "{unlisted:?} must be a cycle that the first run does not list: {listed:#?}"
    );

    let mut allow = listed.clone();
    allow.push(unlisted.clone());
    let (cycles, unmatched) = analyze_fixture(DENSE_FIXTURE, allow);
    assert!(
        !cycles.is_empty(),
        "other cycles of the group are still reported: {cycles:#?}"
    );
    assert!(
        !cycles.iter().any(|cycle| listed
            .iter()
            .chain([&unlisted])
            .any(|e| is_rotation(e, cycle))),
        "an accepted cycle is not reported: {cycles:#?}"
    );
    assert!(unmatched.is_empty(), "{unmatched:#?}");
}

#[test]
fn an_entry_that_the_detector_cannot_check_is_not_reported_as_unmatched() {
    let in_full_group = entry(&["src/a.ts", "src/b.ts", "src/c.ts", "src/d.ts", "src/e.ts"]);
    let too_long: Vec<String> = (0..13).map(|index| format!("src/f{index}.ts")).collect();
    let not_a_cycle = entry(&["src/a.ts", "src/index.ts"]);
    let (_, unmatched) = analyze_fixture(
        DENSE_FIXTURE,
        vec![in_full_group, too_long, not_a_cycle.clone()],
    );
    assert_eq!(
        unmatched,
        vec![not_a_cycle],
        "only an entry outside a full group and within the length limit is unmatched"
    );
}
