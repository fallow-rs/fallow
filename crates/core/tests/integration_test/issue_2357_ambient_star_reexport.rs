//! Issue #2357: a star re-export inside a `declare module '<specifier>'` body
//! states that every export of the target is reachable through the declared
//! package name. It must credit each target export without laundering them
//! into the declaring file's export surface, and a bare-specifier re-export
//! inside such a body stays type-only package usage (ambient bodies are erased
//! at runtime).
//!
//! An ambient re-export is erased at runtime and binds no local name, so it
//! cannot be narrowed to one meaning: a forwarded name credits both the type
//! and the value declaration behind it. Plain `export *` follows ES star
//! semantics and never forwards `default`; `export * as ns` exposes the
//! namespace object, whose `default` member is the target's default export.

use super::common::{create_config, create_production_config, fixture_path};
use fallow_core::graph::{ExportNamespace, ModuleNode, ReferenceKind};

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

#[test]
fn ambient_star_re_exports_credit_every_target_export() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    let unused_exports: Vec<(String, &str)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.path.to_string_lossy().replace('\\', "/"),
                e.export.export_name.as_str(),
            )
        })
        .collect();
    let unused_export_names: Vec<&str> = unused_exports.iter().map(|(_, n)| *n).collect();
    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|t| t.export.export_name.as_str())
        .collect();

    // `export * from './impl'` (entry-point ambient.d.ts), `export * as impl
    // from './ns-impl'` (ambient-ns.d.ts), and `export * from './shim-impl'`
    // (shim.ts, a reachable non-entry module) all credit every export of
    // their target. The non-entry module is the case that previously
    // credited nothing: a star re-export only credits through its consumers.
    // `helperE` only reaches impl.ts through impl.ts's own `export *`, the
    // barrel-of-barrels shape real ambient declarations point at.
    // `Merged`, `DeepMerged`, `User`, and `Pair` pair a type with a value of
    // the same name; the ambient re-export forwards both meanings, so neither
    // half may report (the named `Pair` form is the #2349 precedent).
    for name in [
        "helperA",
        "helperB",
        "helperC",
        "helperD",
        "helperE",
        "Merged",
        "DeepMerged",
        "User",
        "Pair",
    ] {
        assert!(
            !unused_export_names.contains(&name) && !unused_type_names.contains(&name),
            "`{name}` is reachable through an ambient-module re-export and must keep its \
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

    // ES `export *` never forwards `default`, neither from the direct target
    // nor along the target's own `export *` chain; `export * as impl` exposes
    // `impl.default`, so the ns-impl.ts default is credited.
    let unused_default_files: Vec<&str> = unused_exports
        .iter()
        .filter(|(_, name)| *name == "default")
        .map(|(path, _)| path.as_str())
        .collect();
    for path in ["src/impl.ts", "src/impl-deep.ts"] {
        assert!(
            unused_default_files.iter().any(|p| p.ends_with(path)),
            "{path}: a plain `export *` inside an ambient body must not credit the target's \
             default export; unused defaults: {unused_default_files:?}"
        );
    }
    assert!(
        !unused_default_files
            .iter()
            .any(|p| p.ends_with("src/ns-impl.ts")),
        "src/ns-impl.ts: `export * as impl` exposes `impl.default`, so the default export is \
         reachable; unused defaults: {unused_default_files:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files.iter().any(|p| {
            p.ends_with("src/impl.ts")
                || p.ends_with("src/impl-deep.ts")
                || p.ends_with("src/ns-impl.ts")
                || p.ends_with("src/shim-impl.ts")
                || p.ends_with("src/named-impl.ts")
        }),
        "re-export targets stay reachable through the ambient declaration: {unused_files:?}"
    );
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
            let export = module
                .exports
                .iter()
                .find(|e| e.name.matches_str(name))
                .unwrap_or_else(|| panic!("{path} must export `{name}`"));
            let kinds: Vec<ReferenceKind> = export.references.iter().map(|r| r.kind).collect();
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
    let deep = module_named(&graph.modules, "src/impl-deep.ts");
    let helper_e = deep
        .exports
        .iter()
        .find(|e| e.name.matches_str("helperE"))
        .expect("impl-deep.ts must export `helperE`");
    assert!(
        helper_e
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::ReExport && r.from_file == impl_module.file_id),
        "helperE must be credited through the impl.ts star chain, found {:?}",
        helper_e
            .references
            .iter()
            .map(|r| (r.kind, r.from_file))
            .collect::<Vec<_>>()
    );
}

#[test]
fn ambient_re_exports_credit_both_declarations_of_a_type_value_pair() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    // Each pair has two export slots: the type declaration is credited in
    // the type namespace and the value declaration in the value namespace.
    // The star forms credit through the whole-module namespace shape, the
    // named form (#2349) through a named import; the entry-star chain from
    // impl.ts to impl-deep.ts marks both lanes as well, so the direct target
    // and the chained target agree.
    for (path, name, kind) in [
        ("src/impl.ts", "Merged", ReferenceKind::NamespaceImport),
        ("src/shim-impl.ts", "User", ReferenceKind::NamespaceImport),
        ("src/named-impl.ts", "Pair", ReferenceKind::NamedImport),
        ("src/impl-deep.ts", "DeepMerged", ReferenceKind::ReExport),
    ] {
        let module = module_named(&graph.modules, path);
        let slots: Vec<_> = module
            .exports
            .iter()
            .filter(|e| e.name.matches_str(name))
            .collect();
        assert_eq!(
            slots.len(),
            2,
            "{path} must export `{name}` as both a type and a value declaration"
        );
        for export in slots {
            let namespace = if export.is_type_only {
                ExportNamespace::Type
            } else {
                ExportNamespace::Value
            };
            assert!(
                export.references_in(namespace).any(|r| r.kind == kind),
                "{path}:{name} ({namespace:?} declaration) must be credited in its own \
                 namespace through {kind:?}, found {:?}",
                export
                    .references
                    .iter()
                    .map(|r| (r.kind, r.namespace))
                    .collect::<Vec<_>>()
            );
        }
    }

    // `export * as impl` forwards the namespace object, so `impl.default`
    // credits the target's default export; plain `export *` never does.
    let ns_default = module_named(&graph.modules, "src/ns-impl.ts")
        .exports
        .iter()
        .find(|e| e.name.matches_str("default"))
        .expect("ns-impl.ts must have a default export");
    assert!(
        ns_default
            .references
            .iter()
            .any(|r| r.kind == ReferenceKind::DefaultImport),
        "ns-impl.ts:default must be credited through the namespace object's `default` \
         member, found {:?}",
        ns_default
            .references
            .iter()
            .map(|r| (r.kind, r.namespace))
            .collect::<Vec<_>>()
    );
    let star_default = module_named(&graph.modules, "src/impl.ts")
        .exports
        .iter()
        .find(|e| e.name.matches_str("default"))
        .expect("impl.ts must have a default export");
    assert!(
        star_default.references.is_empty(),
        "impl.ts:default is not part of the `export *` surface and must carry no \
         reference, found {:?}",
        star_default
            .references
            .iter()
            .map(|r| (r.kind, r.namespace))
            .collect::<Vec<_>>()
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
