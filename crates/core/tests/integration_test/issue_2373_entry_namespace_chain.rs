//! Issue #2373: an `export * as sub` on the entry-point surface (on the entry
//! itself or on a barrel the entry reaches through plain `export *`) exposes
//! sub's whole namespace object to consumers the graph cannot enumerate. The
//! namespace re-export phase credited sub's direct exports, but neither sub's
//! own `export *` sources nor its own `export * as sub2` sources, so the
//! deeper levels of the chain were reported as unused.
//!
//! ES semantics apply exactly: a plain `export *` never forwards `default`, a
//! namespace object exposes its target's `default`, and a namespace re-export
//! off the entry surface with no consumer exposes nothing.

use super::common::{create_config, fixture_path};
use fallow_core::graph::{ExportSymbol, ModuleNode, ReferenceKind};

const FIXTURE: &str = "issue-2373-entry-namespace-chain";

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

fn export_named<'a>(module: &'a ModuleNode, name: &str) -> &'a ExportSymbol {
    module
        .exports
        .iter()
        .find(|e| e.name.matches_str(name))
        .unwrap_or_else(|| panic!("{} must export `{name}`", module.path.display()))
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

fn is_reported(unused: &[(String, String)], path: &str, name: &str) -> bool {
    unused.iter().any(|(p, n)| p.ends_with(path) && n == name)
}

fn assert_credited(unused: &[(String, String)], path: &str, name: &str, why: &str) {
    assert!(
        !is_reported(unused, path, name),
        "{path}:{name} must be credited: {why}; unused exports: {unused:?}"
    );
}

fn assert_reported(unused: &[(String, String)], path: &str, name: &str, why: &str) {
    assert!(
        is_reported(unused, path, name),
        "{path}:{name} must keep reporting: {why}; unused exports: {unused:?}"
    );
}

#[test]
fn entry_namespace_chain_credits_nested_star_and_namespace_sources() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    // index.ts `export * from './barrel'`; barrel.ts `export * as sub`: the
    // entry surface forwards `sub`, whose namespace object exposes sub.ts's
    // direct exports (credited before this fix), the named exports of its
    // `export *` source, and every export of its `export * as sub2` source,
    // recursively through sub2.ts's own `export *` and `export * as sub3`.
    for name in ["subOne", "sub2", "default"] {
        assert_credited(&unused, "src/sub.ts", name, "`sub` is on the entry surface");
    }
    assert_credited(
        &unused,
        "src/deep.ts",
        "deepX",
        "forwarded by sub.ts's `export *`",
    );
    for name in ["sub2X", "sub3", "default"] {
        assert_credited(
            &unused,
            "src/sub2.ts",
            name,
            "`sub.sub2` is sub2.ts's namespace object",
        );
    }
    assert_credited(
        &unused,
        "src/sub2-deep.ts",
        "sub2DeepX",
        "forwarded by sub2.ts's `export *`",
    );
    for name in ["sub3X", "default"] {
        assert_credited(
            &unused,
            "src/sub3.ts",
            name,
            "`sub.sub2.sub3` is sub3.ts's namespace object",
        );
    }

    // index.ts `export * as top from './top'`: the entry exposes top's
    // namespace object directly, so top.ts's `export *` source is credited.
    for name in ["topOne", "default"] {
        assert_credited(
            &unused,
            "src/top.ts",
            name,
            "`top` is exposed by the entry point",
        );
    }
    assert_credited(
        &unused,
        "src/top-deep.ts",
        "topDeepX",
        "forwarded by top.ts's `export *`",
    );

    // A plain `export *` never forwards `default`: the entry's star does not
    // forward barrel.ts's default, and sub.ts's, sub2.ts's, and top.ts's
    // stars do not forward their sources' defaults.
    for path in [
        "src/barrel.ts",
        "src/deep.ts",
        "src/sub2-deep.ts",
        "src/top-deep.ts",
    ] {
        assert_reported(
            &unused,
            path,
            "default",
            "a plain `export *` never forwards `default`",
        );
    }
}

#[test]
fn namespace_re_exports_off_the_entry_surface_expose_nothing() {
    // Regression pin: shim.ts is reachable but not on the entry surface, and
    // nothing consumes `hidden`, so hidden.ts and its chain keep reporting.
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    assert_credited(
        &unused,
        "src/shim.ts",
        "shimOne",
        "re-exported by name from the entry",
    );
    assert_reported(
        &unused,
        "src/shim.ts",
        "hidden",
        "no consumer observes the namespace",
    );
    for name in ["hiddenOne", "default"] {
        assert_reported(
            &unused,
            "src/hidden.ts",
            name,
            "no consumer observes the namespace",
        );
    }
    for name in ["hiddenDeepX", "default"] {
        assert_reported(
            &unused,
            "src/hidden-deep.ts",
            name,
            "no consumer observes the namespace",
        );
    }
}

#[test]
fn entry_star_chains_that_re_export_themselves_terminate() {
    // Regression pin: index.ts `export * from './cycle-a'`; cycle-a.ts
    // `export * as cycleNs from './cycle-b'`; cycle-b.ts `export * from
    // './cycle-a'`. The closure terminates and the cycle stays fully credited.
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    assert_credited(
        &unused,
        "src/cycle-a.ts",
        "cycleA",
        "forwarded by the entry's `export *`",
    );
    for name in ["cycleB", "default"] {
        assert_credited(
            &unused,
            "src/cycle-b.ts",
            name,
            "`cycleNs` is on the entry surface",
        );
    }
}

#[test]
fn entry_namespace_credit_is_routed_through_the_chain() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    // sub.ts's `export *` source is credited through sub.ts's star chain and
    // its `export * as sub2` source through the namespace object sub.ts
    // exposes; sub2.ts's own chain follows the same two rules.
    let sub = module_named(&graph.modules, "src/sub.ts");
    let deep_x = export_named(module_named(&graph.modules, "src/deep.ts"), "deepX");
    assert!(
        deep_x
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::ReExport && r.from_file == sub.file_id),
        "deepX must be credited through the sub.ts star chain, found {:?}",
        deep_x
            .references
            .iter()
            .map(|r| (r.kind, r.namespace, r.from_file))
            .collect::<Vec<_>>()
    );
    let sub2 = module_named(&graph.modules, "src/sub2.ts");
    for name in ["sub2X", "default"] {
        let export = export_named(sub2, name);
        assert!(
            export
                .references
                .iter()
                .any(|r| r.kind == ReferenceKind::NamespaceImport && r.from_file == sub.file_id),
            "sub2.ts:{name} must be credited through the namespace object sub.ts exposes, \
             found {:?}",
            export
                .references
                .iter()
                .map(|r| (r.kind, r.namespace, r.from_file))
                .collect::<Vec<_>>()
        );
    }
    let sub3_default = export_named(module_named(&graph.modules, "src/sub3.ts"), "default");
    assert!(
        sub3_default
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::NamespaceImport && r.from_file == sub2.file_id),
        "sub3.ts:default must be credited through the namespace object sub2.ts exposes, found {:?}",
        sub3_default
            .references
            .iter()
            .map(|r| (r.kind, r.namespace, r.from_file))
            .collect::<Vec<_>>()
    );
    assert!(
        export_named(module_named(&graph.modules, "src/deep.ts"), "default")
            .references
            .is_empty(),
        "deep.ts:default is not part of sub.ts's `export *` surface"
    );
}
