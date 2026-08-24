//! Issue #2375: `export type *` and `export type * as ns` inside a
//! `declare module '<specifier>'` body state that every export of the target
//! is reachable through the declared module name in type space alone.
//!
//! Issue #2357 routed the plain `export *` forms through a bindingless
//! whole-module import so the declaring file gains no export surface, but the
//! type-only spellings kept the pre-existing file-level type-only star
//! re-export: on an entry-point `.d.ts` that laundered the target's value
//! exports into the entry surface, and on a non-entry shim it credited
//! nothing, so the type half of a same-name pair reported. Both spellings now
//! take the same whole-module shape, flagged type-only, and the graph credits
//! the target's star surface in the type namespace alone: the type half of a
//! pair is credited while its value half keeps reporting, `default` stays
//! unforwarded for the plain star, and `export type * as ns` forwards the
//! namespace object's `default` in type space.

use super::common::{create_config, fixture_path};
use fallow_core::graph::{ExportNamespace, ExportSymbol, ModuleNode, ReferenceKind};

const FIXTURE: &str = "issue-2375-ambient-type-star";

fn module_named<'a>(modules: &'a [ModuleNode], suffix: &str) -> &'a ModuleNode {
    modules
        .iter()
        .find(|m| {
            m.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} must be part of the module graph"))
}

fn slot<'a>(module: &'a ModuleNode, name: &str, is_type_only: bool) -> &'a ExportSymbol {
    module
        .exports
        .iter()
        .find(|e| e.name.matches_str(name) && e.is_type_only == is_type_only)
        .unwrap_or_else(|| {
            panic!(
                "{} must export `{name}` (type-only: {is_type_only})",
                module.path.display()
            )
        })
}

fn reference_shapes(export: &ExportSymbol) -> Vec<(ReferenceKind, ExportNamespace)> {
    export
        .references
        .iter()
        .map(|r| (r.kind, r.namespace))
        .collect()
}

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.path.to_string_lossy().replace('\\', "/"),
                e.export.export_name.clone(),
            )
        })
        .collect()
}

fn unused_type_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_types
        .iter()
        .map(|t| {
            (
                t.export.path.to_string_lossy().replace('\\', "/"),
                t.export.export_name.clone(),
            )
        })
        .collect()
}

/// Whether a finding list holds `name` on the file ending in `suffix`.
/// Finding paths are absolute, so the fixture file is matched by suffix.
fn holds(findings: &[(String, String)], suffix: &str, name: &str) -> bool {
    findings
        .iter()
        .any(|(path, found)| path.ends_with(suffix) && found == name)
}

#[test]
fn ambient_type_star_credits_the_target_type_surface() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_exports = unused_export_pairs(&results);
    let unused_types = unused_type_pairs(&results);

    // The type half of every same-name pair is forwarded by the star, in the
    // entry-point `.d.ts` direction (`entry-pair.ts`) and in the non-entry
    // `shim.ts` direction alike. The `shim-pair.ts` half is the false positive
    // the issue reports: nothing credited it before.
    for (path, name) in [
        ("src/entry-pair.ts", "EntryPair"),
        ("src/shim-pair.ts", "ShimPair"),
        ("src/ns-pair.ts", "NsPair"),
        ("src/ns-pair.ts", "NsAlias"),
    ] {
        assert!(
            !holds(&unused_types, path, name),
            "{path}:{name}: the type half is forwarded by the ambient type star: {unused_types:?}"
        );
    }

    // A value-only export has no type declaration to forward, so the star
    // reaches its value declaration through the type-space fallback lane, the
    // same credit the named ambient `export type { plain }` form has given
    // since #2349.
    for (path, name) in [
        ("src/entry-pair.ts", "entryPlain"),
        ("src/shim-pair.ts", "shimPlain"),
        ("src/value-only.ts", "valueOne"),
        ("src/value-only.ts", "valueTwo"),
        ("src/ns-value.ts", "nsValueOne"),
    ] {
        assert!(
            !holds(&unused_exports, path, name),
            "{path}:{name}: a value-only export stays reachable as `typeof {name}` through the \
             ambient type star: {unused_exports:?}"
        );
    }

    // `export type * as ns` exposes the namespace object in type space, whose
    // `default` member is the target's default export, so both a type default
    // (`export default interface`) and a value default are credited.
    for path in ["src/ns-pair.ts", "src/ns-value.ts"] {
        assert!(
            !holds(&unused_exports, path, "default") && !holds(&unused_types, path, "default"),
            "{path}: `export type * as ns` forwards `ns.default`: {unused_exports:?} \
             {unused_types:?}"
        );
    }
}

#[test]
fn ambient_type_star_leaves_the_value_meaning_reporting() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_exports = unused_export_pairs(&results);

    // The value half of a same-name pair is not forwarded: `export type *`
    // erases it. On `entry-pair.ts` the entry `.d.ts` star used to launder it
    // into the entry surface, so this row is new.
    for (path, name) in [
        ("src/entry-pair.ts", "EntryPair"),
        ("src/shim-pair.ts", "ShimPair"),
        ("src/ns-pair.ts", "NsPair"),
    ] {
        assert!(
            holds(&unused_exports, path, name),
            "{path}:{name}: the value half of the pair must keep reporting: {unused_exports:?}"
        );
    }

    // A plain star forwards no `default`, whichever spelling it carries.
    for path in ["src/entry-pair.ts", "src/shim-pair.ts"] {
        assert!(
            holds(&unused_exports, path, "default"),
            "{path}: `export type *` forwards no `default`: {unused_exports:?}"
        );
    }
}

#[test]
fn ambient_type_star_leaves_no_surface_on_the_declaring_file() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    let ambient = module_named(&graph.modules, "src/ambient.d.ts");
    assert!(
        ambient.is_entry_point(),
        "the fixture must keep the declaring file an entry point (package.json `types`): \
         that is where the type star used to launder the target's value exports"
    );
    let shim = module_named(&graph.modules, "src/shim.ts");
    assert!(
        shim.is_reachable() && !shim.is_entry_point(),
        "the fixture must keep shim.ts a reachable non-entry module so the credit cannot \
         come from entry-point star propagation"
    );

    for path in [
        "src/ambient.d.ts",
        "src/ambient-ns.d.ts",
        "src/ambient-value.d.ts",
        "src/shim.ts",
    ] {
        let module = module_named(&graph.modules, path);
        let re_exports: Vec<(&str, &str)> = module
            .re_exports
            .iter()
            .map(|e| (e.imported_name.as_str(), e.exported_name.as_str()))
            .collect();
        assert!(
            re_exports.is_empty(),
            "{path}: an ambient-body type star must not become part of the declaring file's \
             export surface, found re-export edges {re_exports:?}"
        );
        assert!(
            module.exports.is_empty(),
            "{path}: an ambient body contributes no file-level exports, found {:?}",
            module.exports.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }
}

#[test]
fn ambient_type_star_references_land_in_the_type_namespace() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    let entry_pair = module_named(&graph.modules, "src/entry-pair.ts");
    assert_eq!(
        reference_shapes(slot(entry_pair, "EntryPair", true)),
        vec![(ReferenceKind::NamespaceImport, ExportNamespace::Type)],
        "the interface half carries exactly the type-space star reference"
    );
    assert!(
        reference_shapes(slot(entry_pair, "EntryPair", false)).is_empty(),
        "the const half of the pair carries no reference at all"
    );

    let ns_pair = module_named(&graph.modules, "src/ns-pair.ts");
    assert!(
        reference_shapes(slot(ns_pair, "default", false))
            .contains(&(ReferenceKind::DefaultImport, ExportNamespace::Type)),
        "`export type * as ns` reaches `ns.default` in the type namespace"
    );
}
