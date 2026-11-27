//! A plain import of a value that the importer never reads as a value is not
//! a runtime edge, because TypeScript removes it (issue #5309). Such an edge
//! cannot close a cycle. The edge stays when the importer reads the binding
//! as a value, when an unread binding in a `.tsx` file can be the JSX
//! factory, and when `emitDecoratorMetadata` turns a type annotation of a
//! decorated member into a runtime reference (issue #5362).

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "cycles-type-used-value-imports";

/// The fixture-relative folders of the reported cycles.
fn cycle_groups() -> Vec<String> {
    let root = fixture_path(FIXTURE);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut groups: Vec<String> = results
        .circular_dependencies
        .iter()
        .map(|finding| {
            finding.cycle.files[0]
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
fn imports_that_typescript_removes_close_no_cycle() {
    let groups = cycle_groups();
    for group in [
        "src/class-type",
        "src/unused",
        "src/decorators",
        "src/metadata-no-decorators",
        "src/metadata-not-emitted",
    ] {
        assert!(
            !groups.iter().any(|reported| reported == group),
            "{group} closes only through a removed import, got {groups:#?}"
        );
    }
}

#[test]
fn imports_that_stay_in_the_output_still_close_a_cycle() {
    let groups = cycle_groups();
    for group in [
        "src/unused-tsx",
        "src/reexported",
        "src/metadata",
        "src/generated-extends",
        "src/type-only-tsx",
    ] {
        assert!(
            groups.iter().any(|reported| reported == group),
            "expected a cycle in {group}, got {groups:#?}"
        );
    }
}
