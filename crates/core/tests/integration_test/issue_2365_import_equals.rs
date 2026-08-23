//! Issue #2365: `import X = require('./x')` is the TypeScript spelling of a
//! CommonJS require binding. It records the same edge a
//! `const X = require('./x')` declaration records, so the target participates
//! in reachability and member accesses through `X` narrow the target's exports
//! the way a namespace import does, in type space as well as value space.
//!
//! The exported form, `export import X = require('./x')`, additionally hands
//! the module object to consumers the graph cannot enumerate, so it credits
//! every export of the target exactly as `import * as X; export { X }` does,
//! whatever else the file binds under that name.
//!
//! The one place the two spellings still differ is the consumer's own export
//! surface: `export { X }` records an export row named `X`, so an unconsumed
//! re-export reports; the import-equals form records no export row, so it never
//! does. That is a deliberate miss, pinned below, not an invented row.
//!
//! A binding nothing references is elided by TypeScript, so it credits nothing
//! and the target keeps every unused-export and unused-type row it earns,
//! exactly as an unreferenced `import * as X` does.
//!
//! `import type X = require('pkg')` is the one spelling TypeScript erases
//! entirely, so it keeps the type-space edge but never claims the package is
//! imported at runtime, matching `import type * as X from 'pkg'`.
//!
//! `import X = Some.Namespace` stays out of scope: an entity-name reference
//! names a binding declared in the same file, not a module, so it records no
//! edge at all.
//!
//! Every shape in the fixture is TypeScript the compiler accepts. The
//! namespace-body spelling (`namespace N { export import X = require('./x') }`)
//! is TS1147 and therefore lives only in the extractor's lenient-parse unit
//! test, not here.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-2365-import-equals";

fn unused_files(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
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

/// Exports reported on one file, in report order.
fn unused_exports_of(results: &fallow_core::results::AnalysisResults, file: &str) -> Vec<String> {
    unused_export_pairs(results)
        .into_iter()
        .filter(|(path, _)| path.ends_with(file))
        .map(|(_, name)| name)
        .collect()
}

/// Type exports reported on one file, in report order.
fn unused_types_of(results: &fallow_core::results::AnalysisResults, file: &str) -> Vec<String> {
    results
        .unused_types
        .iter()
        .filter(|e| {
            e.export
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(file)
        })
        .map(|e| e.export.export_name.clone())
        .collect()
}

#[test]
fn import_equals_require_makes_the_target_reachable() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_files = unused_files(&results);

    // Every one of these is reached through an `import X = require(...)`
    // binding alone; the issue reported all of them as unused files.
    for path in [
        "src/assigned.ts",
        "src/narrowed.ts",
        "src/whole.ts",
        "src/whole-deep.ts",
    ] {
        assert!(
            !unused_files.iter().any(|p| p.ends_with(path)),
            "{path} is imported through `import X = require(...)` and must be reachable; unused \
             files: {unused_files:?}"
        );
    }

    // The issue's own shape: a namespace exported with `export =` and consumed
    // through the member the consumer writes.
    assert!(
        !unused_export_pairs(&results)
            .iter()
            .any(|(path, name)| path.ends_with("src/assigned.ts") && name == "viaAssignment"),
        "the member the consumer accesses through the binding must be credited: {:?}",
        unused_export_pairs(&results)
    );
}

/// The binding narrows to the members the consumer writes, exactly like the
/// equivalent `import * as ns`. Both halves of the fixture declare the same
/// shape (one accessed export, one untouched sibling) and must report the same
/// way: the sibling reports, the accessed export does not.
#[test]
fn import_equals_narrows_members_like_a_namespace_import() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    assert_eq!(
        unused_exports_of(&results, "src/narrowed.ts"),
        vec!["unusedByNarrowing".to_string()],
        "an `import X = require(...)` binding credits only the members it accesses"
    );
    assert_eq!(
        unused_exports_of(&results, "src/narrowed-esm.ts"),
        vec!["esmUnusedByNarrowing".to_string()],
        "the equivalent namespace import reports the same way"
    );
}

/// A binding used as a whole object (`Object.values(Whole)`) observes every
/// name on the namespace object, including the names the target only exposes
/// through its own `export *` chain, so the #2372 whole-object seed applies to
/// this binding as it does to a namespace import.
#[test]
fn import_equals_used_as_a_whole_object_credits_the_star_chain() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let pairs = unused_export_pairs(&results);
    let unused_files = unused_files(&results);

    for (file, name) in [
        ("src/whole.ts", "wholeDirect"),
        ("src/whole-deep.ts", "wholeDeep"),
    ] {
        // An unreachable file stacks no unused-export rows underneath its
        // unused-file row, so reachability is asserted first: without it the
        // export assertion would hold vacuously.
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "{file} must be reachable before its exports can be credited: {unused_files:?}"
        );
        assert!(
            !pairs
                .iter()
                .any(|(path, export)| path.ends_with(file) && export == name),
            "{file}: `{name}` is on the namespace object a whole-object use observes: {pairs:?}"
        );
    }
}

/// The binding narrows in type space too. `Typed.UsedShape` in an annotation
/// credits the interface; the untouched sibling interface still reports. The
/// `import * as TypedEsm` half of the fixture declares the same shape and must
/// report the same way: crediting only the value lane would invent an
/// `unused-type` row the namespace import never produces.
#[test]
fn import_equals_narrows_type_members_like_a_namespace_import() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    assert_eq!(
        unused_types_of(&results, "src/typed.ts"),
        vec!["UnusedShape".to_string()],
        "a type reached through `Typed.UsedShape` is credited, its sibling is not"
    );
    assert_eq!(
        unused_types_of(&results, "src/typed-esm.ts"),
        vec!["EsmUnusedShape".to_string()],
        "the equivalent namespace import reports the same way"
    );

    // The value export on the same target is credited through the same
    // binding, so the type lane is not bought by dropping the value lane.
    assert!(
        unused_exports_of(&results, "src/typed.ts").is_empty(),
        "the value member read through the binding stays credited: {:?}",
        unused_exports_of(&results, "src/typed.ts")
    );
}

/// Object destructuring off the binding (`const { used } = X`) resolves through
/// the namespace binding name, so only the destructured member is credited.
/// Without that binding name the whole target would report.
#[test]
fn import_equals_destructuring_off_the_binding_narrows_like_a_namespace_import() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    assert_eq!(
        unused_exports_of(&results, "src/destructured.ts"),
        vec!["destructuredSibling".to_string()],
        "destructuring off the binding credits the destructured member alone"
    );
    assert_eq!(
        unused_exports_of(&results, "src/destructured-esm.ts"),
        vec!["esmDestructuredSibling".to_string()],
        "the equivalent namespace import reports the same way"
    );
}

/// `export import X = require('./x')` hands the required module object to
/// consumers the graph cannot enumerate, so every export of the target keeps
/// its credit. On an entry point there is no local member access to narrow
/// with, and without the whole-object credit `is_entry_with_no_access` would
/// turn every export of the target into a false `unused-export` row (#2373).
/// The file-level form is pinned against the `import * as X; export { X }`
/// twin; it is the only spelling TypeScript accepts outside a `declare module`
/// body.
#[test]
fn export_import_equals_on_an_entry_point_credits_the_whole_module() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_files = unused_files(&results);

    for file in ["src/entry-reexport.ts", "src/entry-reexport-esm.ts"] {
        // An unreachable file stacks no unused-export rows underneath its
        // unused-file row, so reachability is asserted first.
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "{file} must be reachable before its exports can be credited: {unused_files:?}"
        );
        assert!(
            unused_exports_of(&results, file).is_empty(),
            "{file}: every export is reachable through the re-exported binding: {:?}",
            unused_exports_of(&results, file)
        );
    }
}

/// An `import X = require('./x')` binding nothing references is elided by
/// TypeScript, so it credits nothing: the target stays reachable through the
/// edge, and every export and type on it still reports. Crediting the whole
/// module object there deleted rows the analyzer is right about, which is worse
/// than the missing edge the issue started from. Pinned against the
/// `import * as X` twin, which reports exactly the same way.
#[test]
fn an_unreferenced_import_equals_binding_credits_nothing() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_files = unused_files(&results);

    for (file, export, type_name) in [
        ("src/stale-target.ts", "staleAlpha", "StaleShape"),
        ("src/stale-esm-target.ts", "staleEsmAlpha", "StaleEsmShape"),
    ] {
        // The edge still resolves, so the target is reachable; an unreachable
        // file would stack no rows underneath its unused-file row and the
        // assertions below would hold vacuously.
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "{file} is imported and must stay reachable: {unused_files:?}"
        );
        assert_eq!(
            unused_exports_of(&results, file),
            vec![export.to_string()],
            "{file}: an unreferenced binding credits no export"
        );
        assert_eq!(
            unused_types_of(&results, file),
            vec![type_name.to_string()],
            "{file}: an unreferenced binding credits no type either"
        );
    }
}

/// The unmasked shadowed shape: an exported import-equals whose name a
/// parameter binds again, with no `Object.values(...)` to mask the difference.
/// A shadow guard on the whole-object credit reported every export of the
/// target as an auto-fixable `unused-export`, rows main never produced and rows
/// the `import * as X; export { X }` twin next to it never produces either.
///
/// The two files also pin the one deviation that remains between the spellings:
/// `export { X }` is an export row, so an unconsumed re-export reports on the
/// consumer, while the import-equals binding records no export row and never
/// does. That direction loses a finding, it does not invent one.
#[test]
fn a_shadowed_export_import_equals_keeps_the_targets_credit() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_files = unused_files(&results);

    for file in [
        "src/bare-shadowed-target.ts",
        "src/bare-shadowed-esm-target.ts",
    ] {
        // An unreachable file stacks no unused-export rows underneath its
        // unused-file row, so reachability is asserted first.
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "{file} must be reachable before its exports can be credited: {unused_files:?}"
        );
        assert!(
            unused_exports_of(&results, file).is_empty(),
            "{file}: the re-exported binding hands the module object on, so every export keeps \
             its credit: {:?}",
            unused_exports_of(&results, file)
        );
    }

    assert!(
        unused_exports_of(&results, "src/bare-shadowed.ts").is_empty(),
        "the import-equals binding is no export row, so only `parseBareShadowed` can report and \
         the entry consumes it: {:?}",
        unused_exports_of(&results, "src/bare-shadowed.ts")
    );
    assert_eq!(
        unused_exports_of(&results, "src/bare-shadowed-esm.ts"),
        vec!["BareShadowedEsmTarget".to_string()],
        "the twin's `export {{ X }}` is an export row nothing imports, which is the one row the \
         import-equals spelling does not produce"
    );
}

/// A whole-object use the consumer genuinely wrote survives a same-named
/// binding elsewhere in the file. An earlier revision withdrew the exported
/// form's credit by name, which deleted the genuine use with it and turned
/// every export of the target into a false `unused-export` row. The
/// `import * as X` twin next to it writes the same `Object.values(X)` and the
/// same shadowing parameter, and must report the same way.
#[test]
fn a_shadowed_export_import_equals_keeps_a_genuine_whole_object_use() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused_files = unused_files(&results);

    for file in ["src/shadowed-target.ts", "src/shadowed-esm-target.ts"] {
        assert!(
            !unused_files.iter().any(|p| p.ends_with(file)),
            "{file} must be reachable before its exports can be credited: {unused_files:?}"
        );
        assert!(
            unused_exports_of(&results, file).is_empty(),
            "{file}: `Object.values(...)` observes every name on the module object: {:?}",
            unused_exports_of(&results, file)
        );
    }
}

/// A `declare module '...'` body is the only context outside file scope where
/// TypeScript accepts `export import X = require('...')`, and a relative
/// specifier is TS2439 there, so the reference names a package. The edge still
/// has to be recorded: without it the package is a false `unused-dependency`.
#[test]
fn export_import_equals_inside_an_ambient_module_credits_the_package() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_dependencies
        .iter()
        .map(|f| f.dep.package_name.clone())
        .collect();
    assert!(
        !unused.iter().any(|name| name == "ambient-dep"),
        "the ambient body's import edge credits the package: {unused:?}"
    );
    let unlisted: Vec<String> = results
        .unlisted_dependencies
        .iter()
        .map(|f| f.dep.package_name.clone())
        .collect();
    assert!(
        unlisted.is_empty(),
        "the package is declared in the manifest, so nothing is unlisted: {unlisted:?}"
    );
}

/// `import type X = require('pkg')` is erased by TypeScript: the emitted
/// JavaScript holds no `require` call, so the package is a type-space
/// reference and never a runtime import. It must therefore report exactly what
/// `import type * as X from 'pkg'` reports, which is nothing, while the
/// unerased `import X = require('pkg')` next to it still reports the
/// devDependency as production usage.
#[test]
fn type_only_import_equals_is_not_a_runtime_dependency() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    let in_production: Vec<String> = results
        .dev_dependencies_in_production
        .iter()
        .map(|f| f.dep.package_name.clone())
        .collect();
    let unused_dev: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|f| f.dep.package_name.clone())
        .collect();

    for package in ["type-only-dep", "type-only-esm-dep"] {
        assert!(
            !in_production.iter().any(|name| name == package),
            "{package} is erased at compile time and is not imported at runtime: \
             {in_production:?}"
        );
        assert!(
            !unused_dev.iter().any(|name| name == package),
            "{package} is used in a type position and is not an unused devDependency: \
             {unused_dev:?}"
        );
    }

    // Positive control: the same lane on the unerased spelling still fires, so
    // the assertions above are not passing because the lane is dead.
    assert!(
        in_production.iter().any(|name| name == "runtime-dep"),
        "an unerased `import X = require('pkg')` on a devDependency is production usage: \
         {in_production:?}"
    );
}

/// Deliberate negative control: `import ShapesAlias = Shapes` names a namespace
/// declared in the same file. It is a local alias, not a module reference, so
/// no import edge is invented. A `Shapes` specifier would surface as an
/// unresolved import or an unlisted dependency.
#[test]
fn import_equals_entity_name_is_a_local_alias_not_an_import() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    let unresolved: Vec<String> = results
        .unresolved_imports
        .iter()
        .map(|f| f.import.specifier.clone())
        .collect();
    assert!(
        unresolved.is_empty(),
        "an entity-name import-equals must not become a module specifier: {unresolved:?}"
    );
    let unlisted: Vec<String> = results
        .unlisted_dependencies
        .iter()
        .map(|f| f.dep.package_name.clone())
        .collect();
    assert!(
        unlisted.is_empty(),
        "an entity-name import-equals must not become a package dependency: {unlisted:?}"
    );

    // The file holding the alias is analyzed and its export is credited, so the
    // control is not passing vacuously.
    assert!(
        !unused_files(&results)
            .iter()
            .any(|p| p.ends_with("src/local-alias.ts")),
        "the aliasing file is imported and must be reachable"
    );
    assert!(
        unused_exports_of(&results, "src/local-alias.ts").is_empty(),
        "the aliasing file's only export is consumed by the entry"
    );
}
