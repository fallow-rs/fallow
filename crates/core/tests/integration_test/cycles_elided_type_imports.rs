//! A plain import whose names all resolve to type-only exports of the
//! target is not a runtime edge, because TypeScript removes it from the
//! output (issue #5262). Such an edge cannot close a cycle. The edge stays a
//! runtime edge when the tsconfig of the importer keeps imports
//! (`verbatimModuleSyntax`, `preserveValueImports`), for an importer that is
//! not TypeScript, and for namespace, side-effect and value imports.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "cycles-elided-type-imports";

/// The fixture-relative folders of the reported cycles.
fn cycle_groups() -> Vec<String> {
    let root = fixture_path(FIXTURE);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut groups: Vec<String> = results
        .circular_dependencies
        .iter()
        .map(|finding| {
            let first = &finding.cycle.files[0];
            first
                .parent()
                .and_then(|dir| dir.strip_prefix(&root).ok())
                .map(|dir| dir.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default()
        })
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

#[test]
fn imports_of_type_only_exports_close_no_cycle() {
    let groups = cycle_groups();
    for group in [
        "src/interfaces",
        "src/alias",
        "src/barrel",
        "src/default",
        "src/dynamic",
    ] {
        assert!(
            !groups.iter().any(|reported| reported == group),
            "{group} has only erased type imports on one side, got {groups:#?}"
        );
    }
}

#[test]
fn imports_that_stay_in_the_output_still_close_a_cycle() {
    let groups = cycle_groups();
    for group in [
        "src/value",
        "src/merged",
        "src/mixed",
        "src/namespace",
        "src/side-effect",
        "src/verbatim",
        "src/preserve",
        "src/solution",
        "src/references",
        "src/js",
    ] {
        assert!(
            groups.iter().any(|reported| reported == group),
            "expected a cycle in {group}, got {groups:#?}"
        );
    }
}
