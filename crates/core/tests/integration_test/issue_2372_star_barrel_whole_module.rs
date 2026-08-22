//! Issue #2372: a consumer that observes a whole namespace object sees every
//! name on it, including the names the target only exposes through its own
//! `export *` and `export * as ns` chains. Two consumer shapes used to credit
//! the target's direct exports only: a namespace import the graph cannot
//! narrow (a whole-object use such as `Object.values(ns)`, a binding handed on
//! without member access, or a binding re-exported from a non-entry module)
//! and a dynamic-import pattern match (`import()` with a template,
//! `import.meta.glob`, `require.context`).
//!
//! ES semantics apply exactly: the namespace object includes the target's
//! `default`, a plain `export *` never forwards a `default`, and an
//! `export * as sub` exposes sub's namespace object, `default` included.
//! Member-narrowed namespace imports keep their narrowing and never seed the
//! closure.

use super::common::{create_config, fixture_path};
use fallow_core::graph::{ExportSymbol, ModuleNode, ReferenceKind};

const FIXTURE: &str = "issue-2372-star-barrel-whole-module";

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
fn whole_object_namespace_use_credits_star_and_namespace_chains() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    // `Object.values(ns)` in index.ts observes barrel.ts's namespace object:
    // its direct exports (`default` included), the named exports of its
    // `export *` source, and every export of its `export * as sub` source,
    // recursively through sub.ts's own `export *` and `export * as sub2`
    // chains (a three-level chain).
    assert_credited(&unused, "src/barrel.ts", "direct", "a direct export");
    assert_credited(
        &unused,
        "src/barrel.ts",
        "default",
        "`ns.default` is on the object",
    );
    assert_credited(
        &unused,
        "src/deep.ts",
        "deepHelper",
        "forwarded by barrel.ts's `export *`",
    );
    for name in ["subOne", "sub2", "default"] {
        assert_credited(
            &unused,
            "src/sub.ts",
            name,
            "`ns.sub` is sub.ts's namespace object",
        );
    }
    assert_credited(
        &unused,
        "src/sub-deep.ts",
        "subDeepOne",
        "forwarded by sub.ts's `export *`",
    );
    for name in ["sub2One", "default"] {
        assert_credited(
            &unused,
            "src/sub2.ts",
            name,
            "`ns.sub.sub2` is sub2.ts's namespace",
        );
    }
    assert_credited(
        &unused,
        "src/sub2-deep.ts",
        "sub2DeepOne",
        "forwarded by sub2.ts's `export *`",
    );

    // A plain `export *` never forwards `default`.
    for path in ["src/deep.ts", "src/sub-deep.ts", "src/sub2-deep.ts"] {
        assert_reported(
            &unused,
            path,
            "default",
            "a plain `export *` never forwards `default`",
        );
    }
}

#[test]
fn every_unnarrowed_namespace_import_seeds_the_closure() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    // shim.ts (reachable, not an entry point): `Object.keys(shimNs)`.
    assert_credited(
        &unused,
        "src/shim-barrel.ts",
        "shimDirect",
        "a direct export",
    );
    assert_credited(
        &unused,
        "src/shim-deep.ts",
        "shimDeepOne",
        "forwarded by shim-barrel.ts's `export *` behind a non-entry whole-object use",
    );
    for name in ["shimSubOne", "default"] {
        assert_credited(
            &unused,
            "src/shim-sub.ts",
            name,
            "`shimNs.shimSub` is shim-sub.ts's namespace object",
        );
    }
    assert_reported(
        &unused,
        "src/shim-deep.ts",
        "default",
        "never forwarded by `export *`",
    );

    // passer.ts: `consume(passed)` hands the binding on without any member
    // access, so the graph cannot narrow it.
    assert_credited(
        &unused,
        "src/passed-barrel.ts",
        "passedDirect",
        "a direct export",
    );
    assert_credited(
        &unused,
        "src/passed-deep.ts",
        "passedDeepOne",
        "forwarded by passed-barrel.ts's `export *` behind an unnarrowed binding",
    );
    assert_reported(
        &unused,
        "src/passed-deep.ts",
        "default",
        "never forwarded by `export *`",
    );

    // re-shim.ts: `export { reNs }` hands the namespace object to consumers
    // the graph cannot enumerate.
    assert_credited(&unused, "src/re-barrel.ts", "reDirect", "a direct export");
    assert_credited(
        &unused,
        "src/re-deep.ts",
        "reDeepOne",
        "forwarded by re-barrel.ts's `export *` behind a re-exported namespace binding",
    );
    assert_reported(
        &unused,
        "src/re-deep.ts",
        "default",
        "never forwarded by `export *`",
    );
}

#[test]
fn dynamic_import_pattern_targets_credit_their_chains() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    // `import.meta.glob('./mods/*.ts')` hands the consumer each matched
    // module's namespace object.
    assert_credited(&unused, "src/mods/a.ts", "aOwn", "a direct export");
    for name in ["deepStarOne", "deepStarTwo"] {
        assert_credited(
            &unused,
            "src/mod-deps/a-deep.ts",
            name,
            "forwarded by mods/a.ts's `export *`",
        );
    }
    assert_reported(
        &unused,
        "src/mod-deps/a-deep.ts",
        "default",
        "never forwarded by `export *`",
    );
    assert_credited(
        &unused,
        "src/mod-deps/a-named.ts",
        "namedOnly",
        "named re-export on mods/a.ts",
    );
    assert_reported(
        &unused,
        "src/mod-deps/a-named.ts",
        "namedSibling",
        "not forwarded by any re-export",
    );
    for name in ["bNsOne", "default"] {
        assert_credited(
            &unused,
            "src/mod-deps/b-ns.ts",
            name,
            "`bNs` is b-ns.ts's namespace object",
        );
    }
    assert_credited(
        &unused,
        "src/mod-deps/b-ns-deep.ts",
        "bNsDeepOne",
        "forwarded by b-ns.ts's `export *`",
    );
    assert_reported(
        &unused,
        "src/mod-deps/b-ns-deep.ts",
        "default",
        "never forwarded by `export *`",
    );

    // A template `import()` and `require.context` follow the same rule.
    assert_credited(
        &unused,
        "src/plugins/one.ts",
        "pluginOne",
        "a direct export",
    );
    assert_credited(
        &unused,
        "src/plugin-deps/one-deep.ts",
        "pluginDeepOne",
        "forwarded by plugins/one.ts's `export *` behind a template `import()`",
    );
    assert_reported(
        &unused,
        "src/plugin-deps/one-deep.ts",
        "default",
        "never forwarded by `export *`",
    );
    assert_credited(&unused, "src/icons/star.ts", "starIcon", "a direct export");
    assert_credited(
        &unused,
        "src/icon-deps/star-deep.ts",
        "iconDeepOne",
        "forwarded by icons/star.ts's `export *` behind `require.context`",
    );
    assert_reported(
        &unused,
        "src/icon-deps/star-deep.ts",
        "default",
        "never forwarded by `export *`",
    );
}

#[test]
fn member_narrowed_namespace_imports_do_not_seed_the_closure() {
    // Regression pin: `narrow.one()` keeps narrowing to the accessed member,
    // so nothing else behind narrow-barrel.ts is credited.
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    assert_credited(
        &unused,
        "src/narrow-deep.ts",
        "one",
        "accessed as `narrow.one`",
    );
    assert_reported(
        &unused,
        "src/narrow-deep.ts",
        "two",
        "never accessed on the binding",
    );
    assert_reported(
        &unused,
        "src/narrow-barrel.ts",
        "narrowSub",
        "never accessed on the binding",
    );
    for name in ["narrowSubOne", "default"] {
        assert_reported(
            &unused,
            "src/narrow-sub.ts",
            name,
            "the `narrowSub` namespace is never accessed",
        );
    }
}

#[test]
fn alias_sourced_namespace_bindings_do_not_seed_the_closure() {
    // Regression pin: alias.ts places `aliasNs` in `export const aliasApi =
    // { aliasNs }` without touching it otherwise. The direct-export mark-all
    // still fires for that unnarrowed binding (as before), but the namespace
    // object alias phase follows `aliasApi.aliasNs.aliasOne` precisely, so the
    // star chain behind the aliased barrel stays narrowed.
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    assert_credited(
        &unused,
        "src/alias-barrel.ts",
        "aliasDirect",
        "a direct export behind an unnarrowed binding",
    );
    assert_credited(
        &unused,
        "src/alias-deep.ts",
        "aliasOne",
        "accessed as `aliasApi.aliasNs.aliasOne`",
    );
    assert_reported(
        &unused,
        "src/alias-deep.ts",
        "aliasTwo",
        "never accessed through the alias",
    );
}

#[test]
fn star_chains_that_re_export_themselves_terminate() {
    // cycle-a.ts `export * from './cycle-b'`; cycle-b.ts `export * as cycleNs
    // from './cycle-a'`. `Object.keys(cyc)` on cycle-a.ts sees `cycleA`,
    // `cycleB`, and `cycleNs`; cycle-b.ts's default is on neither namespace
    // object.
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    assert_credited(&unused, "src/cycle-a.ts", "cycleA", "a direct export");
    for name in ["cycleB", "cycleNs"] {
        assert_credited(
            &unused,
            "src/cycle-b.ts",
            name,
            "forwarded by cycle-a.ts's `export *`",
        );
    }
    assert_reported(
        &unused,
        "src/cycle-b.ts",
        "default",
        "never forwarded by `export *`",
    );
}

#[test]
fn whole_module_credit_is_routed_through_the_chain() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    // The `export *` source is credited through the barrel's star chain, the
    // `export * as sub` source through the namespace object the barrel
    // exposes, and so on down the chain.
    let barrel = module_named(&graph.modules, "src/barrel.ts");
    let deep_helper = export_named(module_named(&graph.modules, "src/deep.ts"), "deepHelper");
    assert!(
        deep_helper
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::ReExport && r.from_file == barrel.file_id),
        "deepHelper must be credited through the barrel.ts star chain, found {:?}",
        deep_helper
            .references
            .iter()
            .map(|r| (r.kind, r.namespace, r.from_file))
            .collect::<Vec<_>>()
    );
    let sub = module_named(&graph.modules, "src/sub.ts");
    for name in ["subOne", "default"] {
        let export = export_named(sub, name);
        assert!(
            export
                .references
                .iter()
                .any(|r| r.kind == ReferenceKind::NamespaceImport && r.from_file == barrel.file_id),
            "sub.ts:{name} must be credited through the namespace object barrel.ts exposes, \
             found {:?}",
            export
                .references
                .iter()
                .map(|r| (r.kind, r.namespace, r.from_file))
                .collect::<Vec<_>>()
        );
    }
    let sub2 = module_named(&graph.modules, "src/sub2.ts");
    let sub2_default = export_named(sub2, "default");
    assert!(
        sub2_default
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::NamespaceImport && r.from_file == sub.file_id),
        "sub2.ts:default must be credited through the namespace object sub.ts exposes, found {:?}",
        sub2_default
            .references
            .iter()
            .map(|r| (r.kind, r.namespace, r.from_file))
            .collect::<Vec<_>>()
    );
    assert!(
        export_named(module_named(&graph.modules, "src/sub2-deep.ts"), "default")
            .references
            .is_empty(),
        "sub2-deep.ts:default is not part of sub2.ts's `export *` surface"
    );
}

#[test]
fn plain_star_members_of_the_closure_do_not_forward_a_default_namespace() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    // barrel.ts `export * from './star-default'`: the star carries
    // star-default.ts's named exports onto the observed namespace object and
    // leaves its `default` behind, so `export * as default` there hands its
    // target to nobody.
    assert_credited(
        &unused,
        "src/star-default.ts",
        "starDefaultOne",
        "a plain `export *` forwards every named export",
    );
    assert_reported(
        &unused,
        "src/star-default.ts",
        "default",
        "a plain `export *` never forwards `default`",
    );
    assert_reported(
        &unused,
        "src/star-default-sub.ts",
        "starDefaultSubOne",
        "the namespace object named `default` is not on the barrel's namespace object",
    );
    assert_reported(
        &unused,
        "src/star-default-deep.ts",
        "starDefaultDeepOne",
        "nothing behind the `default` namespace object is reachable",
    );
}

#[test]
fn a_whole_object_use_in_an_unreachable_file_credits_no_chain() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    // dead-consumer.ts observes dead-barrel.ts's whole namespace object, but
    // no entry point reaches either file. The report already calls all four
    // files unused, so crediting the chain would only stack unused-export
    // rows underneath those rows.
    for path in [
        "src/dead-consumer.ts",
        "src/dead-barrel.ts",
        "src/dead-deep.ts",
        "src/dead-sub.ts",
    ] {
        assert!(
            unused_files.iter().any(|p| p.ends_with(path)),
            "{path} must stay an unused file: {unused_files:?}"
        );
    }
    for (path, name) in [
        ("src/dead-deep.ts", "deadDeepOne"),
        ("src/dead-sub.ts", "deadSubOne"),
        ("src/dead-sub.ts", "default"),
    ] {
        assert!(
            !is_reported(&unused, path, name),
            "{path}:{name} must not be reported on top of the unused-file row; \
             unused exports: {unused:?}"
        );
    }
}
