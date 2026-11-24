//! Integration tests for the user-visible `re-export-cycle` finding type.
//!
//! Pinned behaviors per issue #515:
//! - cycle kind matches the fixture shape
//! - type-only re-export cycles still fire as findings
//! - every finding includes non-empty typed `actions[]`

use super::common::{create_config, fixture_path};
use fallow_core::results::ReExportCycleKind;

#[test]
fn two_node_cycle_fires_as_multi_node_finding() {
    let root = fixture_path("re-export-cycle-2-node");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let cycles = &results.re_export_cycles;
    assert!(
        !cycles.is_empty(),
        "expected at least one re-export cycle finding, got none"
    );
    let two_node = cycles
        .iter()
        .find(|c| matches!(c.cycle.kind, ReExportCycleKind::MultiNode) && c.cycle.files.len() == 2)
        .expect("expected a 2-node multi-node cycle");
    let names: Vec<String> = two_node
        .cycle
        .files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        names,
        vec!["barrel-a.ts", "barrel-b.ts"],
        "files should be sorted lexicographically by display string"
    );
    assert!(
        !two_node.actions.is_empty(),
        "every cycle finding must ship with at least one IssueAction"
    );
}

#[test]
fn three_node_cycle_fires_with_three_files() {
    let root = fixture_path("re-export-cycle-3-node");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let three_node = results
        .re_export_cycles
        .iter()
        .find(|c| matches!(c.cycle.kind, ReExportCycleKind::MultiNode) && c.cycle.files.len() == 3)
        .expect("expected a 3-node multi-node cycle (a -> b -> c -> a)");
    let names: Vec<String> = three_node
        .cycle
        .files
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["a.ts", "b.ts", "c.ts"]);
    assert!(!three_node.actions.is_empty());
}

#[test]
fn self_loop_fires_with_self_loop_kind() {
    let root = fixture_path("re-export-cycle-self-loop");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let self_loop = results
        .re_export_cycles
        .iter()
        .find(|c| matches!(c.cycle.kind, ReExportCycleKind::SelfLoop))
        .expect("expected a self-loop finding for barrel.ts");
    assert_eq!(
        self_loop.cycle.files.len(),
        1,
        "self-loop must carry exactly one member file"
    );
    assert!(
        self_loop
            .cycle
            .files
            .first()
            .unwrap()
            .ends_with("barrel.ts")
    );
    assert!(!self_loop.actions.is_empty());
}

#[test]
fn type_only_re_export_cycle_still_fires_as_finding() {
    let root = fixture_path("re-export-cycle-type-only");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let type_only_cycle = results
        .re_export_cycles
        .iter()
        .find(|c| matches!(c.cycle.kind, ReExportCycleKind::MultiNode) && c.cycle.files.len() == 2);
    assert!(
        type_only_cycle.is_some(),
        "type-only re-export cycle should still produce a finding"
    );
}

/// The line and column of a cycle finding point at the `export ... from`
/// statement in `files[0]` that re-exports from another member. A SARIF or
/// CI annotation without them can only name the file.
#[test]
fn cycle_finding_points_at_the_re_export_that_closes_the_cycle() {
    let root = fixture_path("re-export-cycle-anchor-line");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let anchor = |kind: ReExportCycleKind| {
        let finding = results
            .re_export_cycles
            .iter()
            .find(|c| c.cycle.kind == kind)
            .unwrap_or_else(|| panic!("expected a {kind:?} cycle"));
        let value = serde_json::to_value(&finding.cycle).expect("cycle serializes");
        (value["line"].as_u64(), value["col"].as_u64())
    };

    // `barrel-a.ts` line 2 re-exports from `helper.ts`, which is not a member.
    // Line 3 re-exports from `barrel-b.ts` after two spaces of indent.
    assert_eq!(anchor(ReExportCycleKind::MultiNode), (Some(3), Some(2)));
    assert_eq!(anchor(ReExportCycleKind::SelfLoop), (Some(3), Some(0)));
}
