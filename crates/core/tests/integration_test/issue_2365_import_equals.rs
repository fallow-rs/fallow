//! Issue #2365: `import X = require('./x')` is the TypeScript spelling of a
//! CommonJS require binding. It records the same edge a
//! `const X = require('./x')` declaration records, so the target participates
//! in reachability and member accesses through `X` narrow the target's exports
//! the way a namespace import does.
//!
//! `import X = Some.Namespace` stays out of scope: an entity-name reference
//! names a binding declared in the same file, not a module, so it records no
//! edge at all.

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
        "src/inner.ts",
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

/// `export import Inner = require('./inner')` inside a namespace body is still
/// an import of `./inner`, and it narrows the same way.
#[test]
fn export_import_equals_inside_a_namespace_records_the_edge() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    assert_eq!(
        unused_exports_of(&results, "src/inner.ts"),
        vec!["innerUnused".to_string()],
        "the member the namespace body reads is credited and its sibling keeps reporting"
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
