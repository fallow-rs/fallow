//! Issue #2357: a star re-export inside a `declare module '<specifier>'` body
//! states that every export of the target is reachable through the declared
//! package name. It must credit the target's full ES star surface without
//! laundering anything into the declaring file's export surface, and a
//! bare-specifier re-export inside such a body stays type-only package usage
//! (ambient bodies are erased at runtime).
//!
//! The star forms (`export *`, `export * as ns`) forward every named export in
//! both meanings, so a same-name type and value pair keeps both halves, and
//! they follow the target's own `export *` and `export * as ns` chains. Plain
//! `export *` never forwards `default`; a namespace re-export exposes the
//! namespace object, whose `default` member is the target's default export.
//! Every other type-only form (the named ambient re-export from #2349,
//! explicitly type-only ambient re-exports, `import()` type references in TS
//! and JSDoc) keeps its type-space credit, so the value half of a pair reached
//! only that way still reports, exactly as before.

use super::common::{create_config, create_production_config, fixture_path};
use fallow_core::graph::{ExportNamespace, ExportSymbol, ModuleNode, ReferenceKind};

const FIXTURE: &str = "issue-2357-ambient-star-reexport";

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

fn reference_shapes(export: &ExportSymbol) -> Vec<(ReferenceKind, ExportNamespace)> {
    export
        .references
        .iter()
        .map(|r| (r.kind, r.namespace))
        .collect()
}

/// The type and the value declaration slot of a same-name pair.
fn pair_slots<'a>(module: &'a ModuleNode, name: &str) -> (&'a ExportSymbol, &'a ExportSymbol) {
    let slots: Vec<&ExportSymbol> = module
        .exports
        .iter()
        .filter(|e| e.name.matches_str(name))
        .collect();
    assert_eq!(
        slots.len(),
        2,
        "{} must export `{name}` as both a type and a value declaration",
        module.path.display()
    );
    let type_slot = slots
        .iter()
        .find(|e| e.is_type_only)
        .expect("the pair has a type declaration");
    let value_slot = slots
        .iter()
        .find(|e| !e.is_type_only)
        .expect("the pair has a value declaration");
    (type_slot, value_slot)
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

fn unused_type_names(results: &fallow_core::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_types
        .iter()
        .map(|t| t.export.export_name.as_str())
        .collect()
}

#[test]
fn ambient_star_re_exports_credit_every_target_export() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_exports = unused_export_pairs(&results);
    let unused_export_names: Vec<&str> = unused_exports.iter().map(|(_, n)| n.as_str()).collect();
    let unused_type_names = unused_type_names(&results);

    // `export * from './impl'` (entry-point ambient.d.ts), `export * as impl
    // from './ns-impl'` (ambient-ns.d.ts), and `export * from './shim-impl'`
    // (shim.ts, a reachable non-entry module) credit every named export of
    // their target. The non-entry module is the case that previously credited
    // nothing. `helperE` and `DeepMerged` only reach impl.ts through its own
    // `export *`; `subOne`, `SubPair`, `nested`, `nestedOne`, and
    // `subDeepOne` only through its `export * as sub` namespace and sub.ts's
    // own star and namespace chains; `shimSubOne` through shim-impl.ts's
    // `export * as shimSub`. The star forms forward both meanings of a name,
    // so neither half of the `Merged`, `DeepMerged`, `User`, and `SubPair`
    // pairs may report.
    for name in [
        "helperA",
        "helperB",
        "helperC",
        "helperD",
        "helperE",
        "Merged",
        "DeepMerged",
        "User",
        "sub",
        "subOne",
        "SubPair",
        "nested",
        "nestedOne",
        "subDeepOne",
        "shimSub",
        "shimSubOne",
    ] {
        assert!(
            !unused_export_names.contains(&name) && !unused_type_names.contains(&name),
            "`{name}` is reachable through an ambient-module star re-export and must keep its \
             export credit; exports: {unused_exports:?}, types: {unused_type_names:?}"
        );
    }
    for name in ["unusedSibling", "namedSibling"] {
        assert!(
            unused_export_names.contains(&name),
            "`{name}` is not forwarded by any ambient re-export and must keep reporting: \
             {unused_exports:?}"
        );
    }

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    for path in [
        "src/impl.ts",
        "src/impl-deep.ts",
        "src/ns-impl.ts",
        "src/shim-impl.ts",
        "src/named-impl.ts",
        "src/sub.ts",
        "src/sub-deep.ts",
        "src/sub-nested.ts",
        "src/shim-sub.ts",
    ] {
        assert!(
            !unused_files.iter().any(|p| p.ends_with(path)),
            "{path} stays reachable through the ambient declaration: {unused_files:?}"
        );
    }
}

#[test]
fn ambient_star_forms_follow_es_default_semantics() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_default_files: Vec<String> = unused_export_pairs(&results)
        .into_iter()
        .filter(|(_, name)| name == "default")
        .map(|(path, _)| path)
        .collect();

    // Plain `export *` never forwards `default`: the direct target (impl.ts),
    // its own `export *` source (impl-deep.ts), and sub.ts's `export *`
    // source (sub-deep.ts) keep reporting their unused defaults.
    for path in ["src/impl.ts", "src/impl-deep.ts", "src/sub-deep.ts"] {
        assert!(
            unused_default_files.iter().any(|p| p.ends_with(path)),
            "{path}: a plain `export *` must not credit the target's default export; unused \
             defaults: {unused_default_files:?}"
        );
    }
    // A namespace object exposes `default`: the ambient `export * as impl`
    // (ns-impl.ts), impl.ts's `export * as sub` (sub.ts), sub.ts's
    // `export * as nested` (sub-nested.ts), and shim-impl.ts's
    // `export * as shimSub` (shim-sub.ts) credit their target's default.
    for path in [
        "src/ns-impl.ts",
        "src/sub.ts",
        "src/sub-nested.ts",
        "src/shim-sub.ts",
    ] {
        assert!(
            !unused_default_files.iter().any(|p| p.ends_with(path)),
            "{path}: `export * as ns` exposes `ns.default`, so the default export is \
             reachable; unused defaults: {unused_default_files:?}"
        );
    }
}

#[test]
fn ambient_named_and_type_only_forms_keep_type_space_credit() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");
    let unused_exports = unused_export_pairs(&output.results);
    let unused_type_names = unused_type_names(&output.results);

    // The named ambient form (#2349, `export { Pair } from`), explicitly
    // type-only ambient re-exports (`export type { TypedPair } from`,
    // `export { type InlinePair } from`), a TS `import('./x').ImportPair`
    // type, and a JSDoc `@type {import('./x').JsdocPair}` all credit type
    // space only: the type half of each pair carries a named-import
    // reference in the type namespace, the value half carries no value-space
    // reference and keeps reporting as an unused export (never as an unused
    // type), exactly as before this change.
    for (path, name) in [
        ("src/named-impl.ts", "Pair"),
        ("src/typed-impl.ts", "TypedPair"),
        ("src/typed-impl.ts", "InlinePair"),
        ("src/import-impl.ts", "ImportPair"),
        ("src/import-impl.ts", "JsdocPair"),
    ] {
        let (type_slot, value_slot) = pair_slots(module_named(&graph.modules, path), name);
        assert!(
            type_slot
                .references_in(ExportNamespace::Type)
                .any(|r| r.kind == ReferenceKind::NamedImport),
            "{path}:{name} (type declaration) must be credited in type space, found {:?}",
            reference_shapes(type_slot)
        );
        assert!(
            value_slot
                .references_in(ExportNamespace::Value)
                .next()
                .is_none(),
            "{path}:{name} (value declaration) must not be credited in value space by a \
             type-only reference, found {:?}",
            reference_shapes(value_slot)
        );
        assert!(
            unused_exports
                .iter()
                .any(|(p, n)| p.ends_with(path) && n == name),
            "{path}:{name}: the value half of the pair must keep reporting: {unused_exports:?}"
        );
        assert!(
            !unused_type_names.contains(&name),
            "{path}:{name}: the type half of the pair is credited: {unused_type_names:?}"
        );
    }
}

#[test]
fn ambient_star_re_exports_leave_no_surface_on_the_declaring_file() {
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
         that is where the star surface used to launder target exports"
    );
    let shim = module_named(&graph.modules, "src/shim.ts");
    assert!(
        shim.is_reachable() && !shim.is_entry_point(),
        "the fixture must keep shim.ts a reachable non-entry module so the credit cannot \
         come from entry-point star propagation"
    );
    for path in ["src/ambient.d.ts", "src/ambient-ns.d.ts", "src/shim.ts"] {
        let module = module_named(&graph.modules, path);
        let re_exports: Vec<(&str, &str)> = module
            .re_exports
            .iter()
            .map(|e| (e.imported_name.as_str(), e.exported_name.as_str()))
            .collect();
        assert!(
            re_exports.is_empty(),
            "{path}: an ambient-body star re-export must not become part of the declaring \
             file's export surface, found re-export edges {re_exports:?}"
        );
        assert!(
            module.exports.is_empty(),
            "{path}: an ambient body contributes no file-level exports, found {:?}",
            module.exports.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
    }

    for (path, names) in [
        ("src/impl.ts", &["helperA", "helperB"][..]),
        ("src/ns-impl.ts", &["helperC"][..]),
        ("src/shim-impl.ts", &["helperD"][..]),
    ] {
        let module = module_named(&graph.modules, path);
        for name in names {
            let kinds: Vec<ReferenceKind> = export_named(module, name)
                .references
                .iter()
                .map(|r| r.kind)
                .collect();
            assert!(
                kinds.contains(&ReferenceKind::NamespaceImport),
                "{path}:{name} must be credited through the whole-module namespace shape the \
                 ambient declaration emits, found {kinds:?}"
            );
            assert!(
                !kinds.contains(&ReferenceKind::ReExport),
                "{path}:{name} must not be credited through a re-export surface of the \
                 declaring file, found {kinds:?}"
            );
        }
    }

    // The whole-module credit follows the target's own `export *` chain: the
    // graph propagates it from impl.ts (the barrel) to impl-deep.ts.
    let impl_module = module_named(&graph.modules, "src/impl.ts");
    let helper_e = export_named(module_named(&graph.modules, "src/impl-deep.ts"), "helperE");
    assert!(
        helper_e
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::ReExport && r.from_file == impl_module.file_id),
        "helperE must be credited through the impl.ts star chain, found {:?}",
        reference_shapes(helper_e)
    );
}

#[test]
fn ambient_star_chain_covers_the_targets_namespace_re_exports() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    let credited_from = |module: &ModuleNode, name: &str, from: &ModuleNode| {
        let export = export_named(module, name);
        assert!(
            export.references.iter().any(|r| {
                r.kind == ReferenceKind::NamespaceImport
                    && r.from_file == from.file_id
                    && r.namespace == ExportNamespace::Value
            }),
            "{}:{name} must be credited through the namespace object {} exposes, found {:?}",
            module.path.display(),
            from.path.display(),
            export
                .references
                .iter()
                .map(|r| (r.kind, r.namespace, r.from_file))
                .collect::<Vec<_>>()
        );
    };

    // impl.ts `export * as sub from './sub'`: the ambient star reaches the
    // namespace object, so every export of sub.ts, `default` included, is
    // credited from impl.ts by the namespace re-export phase.
    let impl_module = module_named(&graph.modules, "src/impl.ts");
    let sub = module_named(&graph.modules, "src/sub.ts");
    for name in ["subOne", "nested", "default"] {
        credited_from(sub, name, impl_module);
    }

    // sub.ts's own `export *` source gets the star treatment (named exports,
    // never `default`) and its own `export * as nested` source the namespace
    // treatment, recursively.
    let sub_deep = module_named(&graph.modules, "src/sub-deep.ts");
    let sub_deep_one = export_named(sub_deep, "subDeepOne");
    assert!(
        sub_deep_one
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::ReExport && r.from_file == sub.file_id),
        "subDeepOne must be credited through the sub.ts star chain, found {:?}",
        reference_shapes(sub_deep_one)
    );
    assert!(
        export_named(sub_deep, "default").references.is_empty(),
        "sub-deep.ts:default is not part of sub.ts's `export *` surface"
    );
    let nested = module_named(&graph.modules, "src/sub-nested.ts");
    for name in ["nestedOne", "default"] {
        credited_from(nested, name, sub);
    }

    // The reachable non-entry shim follows the same rule for its target's
    // `export * as shimSub` namespace.
    let shim_impl = module_named(&graph.modules, "src/shim-impl.ts");
    let shim_sub = module_named(&graph.modules, "src/shim-sub.ts");
    for name in ["shimSubOne", "default"] {
        credited_from(shim_sub, name, shim_impl);
    }
}

#[test]
fn ambient_star_forms_credit_both_declarations_of_a_type_value_pair() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    // Each pair has two export slots: the type declaration is credited in
    // the type namespace and the value declaration in the value namespace.
    // The direct star targets credit through the whole-module namespace
    // shape, the `export * as sub` target through the namespace re-export
    // phase, and the `export *` chain from impl.ts to impl-deep.ts marks both
    // lanes as well, so direct and chained targets agree.
    for (path, name, kind) in [
        ("src/impl.ts", "Merged", ReferenceKind::NamespaceImport),
        ("src/shim-impl.ts", "User", ReferenceKind::NamespaceImport),
        ("src/sub.ts", "SubPair", ReferenceKind::NamespaceImport),
        ("src/impl-deep.ts", "DeepMerged", ReferenceKind::ReExport),
    ] {
        let (type_slot, value_slot) = pair_slots(module_named(&graph.modules, path), name);
        for (export, namespace) in [
            (type_slot, ExportNamespace::Type),
            (value_slot, ExportNamespace::Value),
        ] {
            assert!(
                export.references_in(namespace).any(|r| r.kind == kind),
                "{path}:{name} ({namespace:?} declaration) must be credited in its own \
                 namespace through {kind:?}, found {:?}",
                reference_shapes(export)
            );
        }
    }

    // `export * as impl` forwards the namespace object, so `impl.default`
    // credits the target's default export; plain `export *` never does.
    let ns_default = export_named(module_named(&graph.modules, "src/ns-impl.ts"), "default");
    assert!(
        ns_default
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::DefaultImport),
        "ns-impl.ts:default must be credited through the namespace object's `default` \
         member, found {:?}",
        reference_shapes(ns_default)
    );
    let star_default = export_named(module_named(&graph.modules, "src/impl.ts"), "default");
    assert!(
        star_default.references.is_empty(),
        "impl.ts:default is not part of the `export *` surface and must carry no \
         reference, found {:?}",
        reference_shapes(star_default)
    );
}

#[test]
fn ambient_bare_specifier_re_exports_are_type_only_dependency_usage() {
    let root = fixture_path(FIXTURE);

    // deps.d.ts re-exports `Named` from `named-dep` and `*` from `star-dep`
    // inside a `declare module 'typed-shim'` body. Both are the only usages of
    // those production dependencies.
    let production = fallow_core::analyze(&create_production_config(root.clone()))
        .expect("production analysis should succeed");
    let type_only_names: Vec<&str> = production
        .type_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    let unused_dep_names: Vec<&str> = production
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    for dep in ["named-dep", "star-dep"] {
        assert!(
            type_only_names.contains(&dep),
            "`{dep}` is referenced only from an ambient-module re-export, which is erased at \
             runtime, so it must classify as type-only usage: {type_only_names:?}"
        );
        assert!(
            !unused_dep_names.contains(&dep),
            "`{dep}` is referenced and must not be reported unused: {unused_dep_names:?}"
        );
    }

    // Outside production mode the finding is not emitted, and the package
    // usage still keeps the dependency out of the unused list.
    let default = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    assert!(
        default.type_only_dependencies.is_empty(),
        "type-only dependency findings are production-mode only: {:?}",
        default
            .type_only_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );
    let unused_dep_names: Vec<&str> = default
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"named-dep") && !unused_dep_names.contains(&"star-dep"),
        "ambient-body re-exports keep their package referenced: {unused_dep_names:?}"
    );
}
