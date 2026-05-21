//! Integration tests for the user-visible `re-export-cycle` finding type.
//!
//! Pinned behaviors per issue #515:
//! - 2-node, 3-node, and self-loop fixtures emit findings with the matching
//!   `kind` discriminator and lexicographically sorted `files`.
//! - Type-only re-export cycles (`export type * from ...` paired
//!   symmetrically) still fire as findings, since chain propagation is the
//!   same no-op as for value cycles.
//! - Every finding ships with a non-empty typed `actions[]` so consumers
//!   have a dispatch handle from day one (panel catch #3, Diego).

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
    // The two-barrel cycle (a <-> b) is the only multi-node SCC; ignore the
    // index.ts barrel (it's the entry point and is on the chain but not in a
    // cycle).
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
    // Panel catch #9 (Aisha): `export type *` chains are structurally
    // identical to value chains for cycle detection: chain propagation is a
    // no-op either way, so the finding fires.
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
