//! Integration tests for the `package-cycle` finding (issue #2955).
//!
//! A package cycle is a cycle between workspace packages, built from resolved
//! cross-package imports. It can exist when no file-level cycle exists.

use super::common::{create_config, fixture_path};
use fallow_core::results::PackageCycle;

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .expect("path has a file name")
        .to_string_lossy()
        .into_owned()
}

fn cycle_with_packages<'a>(cycles: &'a [PackageCycle], packages: &[&str]) -> &'a PackageCycle {
    cycles
        .iter()
        .find(|cycle| cycle.packages == packages)
        .unwrap_or_else(|| panic!("expected a cycle {packages:?}, got {cycles:#?}"))
}

#[test]
fn reproduction_reports_package_cycle_without_file_cycle() {
    let config = create_config(fixture_path("package-cycle-workspace"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.circular_dependencies.is_empty(),
        "the file graph has no cycle: {:#?}",
        results.circular_dependencies
    );
    let cycles: Vec<PackageCycle> = results
        .package_cycles
        .iter()
        .map(|finding| finding.cycle.clone())
        .collect();
    assert_eq!(cycles.len(), 1, "{cycles:#?}");
    let cycle = cycle_with_packages(&cycles, &["@repro/a", "@repro/b"]);
    assert_eq!(cycle.length, 2);
    assert_eq!(cycle.edges.len(), 2);

    let first = &cycle.edges[0];
    assert_eq!(first.from_package, "@repro/a");
    assert_eq!(first.to_package, "@repro/b");
    assert_eq!(file_name(&first.path), "x.ts");
    assert_eq!(file_name(&first.target_path), "y.ts");
    assert_eq!(first.line, 1);
    assert!(!first.type_only);

    let second = &cycle.edges[1];
    assert_eq!(second.from_package, "@repro/b");
    assert_eq!(second.to_package, "@repro/a");
    assert_eq!(file_name(&second.path), "z.ts");
    assert_eq!(file_name(&second.target_path), "w.ts");
    assert!(!second.type_only);

    assert!(!results.package_cycles[0].actions.is_empty());
}

/// Package c imports package a only from a test file and a tooling config
/// file. Those files are not part of the package build, so a -> b -> c -> a
/// is not a cycle.
#[test]
fn acyclic_workspace_reports_no_package_cycle() {
    let config = create_config(fixture_path("package-cycle-acyclic"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.package_cycles.is_empty(),
        "{:#?}",
        results.package_cycles
    );
}

#[test]
fn project_without_workspaces_reports_no_package_cycle() {
    let config = create_config(fixture_path("re-export-cycle-2-node"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.package_cycles.is_empty());
}

#[test]
fn three_package_cycle_and_type_only_hop_are_reported() {
    let config = create_config(fixture_path("package-cycle-3-node"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let cycles: Vec<PackageCycle> = results
        .package_cycles
        .iter()
        .map(|finding| finding.cycle.clone())
        .collect();
    assert_eq!(cycles.len(), 2, "{cycles:#?}");

    // Shorter cycles sort first.
    assert_eq!(cycles[0].packages, ["@tri/a", "@tri/b"]);
    let type_hop = &cycles[0].edges[1];
    assert_eq!(type_hop.from_package, "@tri/b");
    assert_eq!(type_hop.to_package, "@tri/a");
    assert!(type_hop.type_only, "b -> a only has a type import");
    assert_eq!(file_name(&type_hop.path), "types.ts");
    assert!(!cycles[0].edges[0].type_only);

    let tri = cycle_with_packages(&cycles, &["@tri/a", "@tri/b", "@tri/c"]);
    assert_eq!(tri.length, 3);
    assert!(tri.edges.iter().all(|edge| !edge.type_only));
    assert_eq!(tri.edges[2].from_package, "@tri/c");
    assert_eq!(tri.edges[2].to_package, "@tri/a");
    assert_eq!(file_name(&tri.edges[2].target_path), "util.ts");
}

#[test]
fn suppressing_every_import_on_a_hop_removes_the_cycle() {
    let config = create_config(fixture_path("package-cycle-suppressed"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.package_cycles.is_empty(),
        "{:#?}",
        results.package_cycles
    );
    assert!(
        results.stale_suppressions.is_empty(),
        "the suppression is used: {:#?}",
        results.stale_suppressions
    );
}

/// Two workspace packages share the name `example`. Each finding must still
/// name one package: the label carries the package root, and `package_roots`
/// gives the root of every package in cycle order.
#[test]
fn duplicate_package_names_carry_the_package_root() {
    let root = fixture_path("package-cycle-duplicate-names");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let cycles: Vec<PackageCycle> = results
        .package_cycles
        .iter()
        .map(|finding| finding.cycle.clone())
        .collect();
    assert_eq!(cycles.len(), 2, "{cycles:#?}");

    let one = cycle_with_packages(&cycles, &["@dup/lib", "example (examples/one)"]);
    let two = cycle_with_packages(&cycles, &["@dup/lib", "example (examples/two)"]);
    for (cycle, example_root) in [(one, "examples/one"), (two, "examples/two")] {
        let roots: Vec<std::path::PathBuf> = cycle
            .package_roots
            .iter()
            .map(|path| path.strip_prefix(&root).unwrap_or(path).to_path_buf())
            .collect();
        assert_eq!(
            roots,
            [
                std::path::PathBuf::from("packages/lib"),
                std::path::PathBuf::from(example_root)
            ]
        );
        assert_eq!(cycle.edges[0].from_package, "@dup/lib");
        assert_eq!(cycle.edges[0].to_package, cycle.packages[1]);
        assert!(!cycle.group_truncated);
    }
}

#[test]
fn unique_package_names_stay_plain() {
    let root = fixture_path("package-cycle-workspace");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let cycle = &results.package_cycles[0].cycle;
    assert_eq!(cycle.packages, ["@repro/a", "@repro/b"]);
    let roots: Vec<std::path::PathBuf> = cycle
        .package_roots
        .iter()
        .map(|path| path.strip_prefix(&root).unwrap_or(path).to_path_buf())
        .collect();
    assert_eq!(
        roots,
        [
            std::path::PathBuf::from("packages/a"),
            std::path::PathBuf::from("packages/b")
        ]
    );
    assert!(!cycle.group_truncated);
}
