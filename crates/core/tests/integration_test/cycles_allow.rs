//! `circularDependencies.allow` (issue #4623) accepts a cycle that has
//! exactly the files of an entry, in any rotation. A cycle that shares an
//! edge with an accepted cycle but has another file set stays reported.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "cycles-allow";

fn entry(files: &[&str]) -> Vec<String> {
    files.iter().map(|file| (*file).to_owned()).collect()
}

/// The reported cycles in cycle order as fixture-relative paths, and the
/// `allow` entries that matched no cycle.
fn analyze(allow: Vec<Vec<String>>) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    let root = fixture_path(FIXTURE);
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
