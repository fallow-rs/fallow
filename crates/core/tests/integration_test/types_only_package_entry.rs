//! A project package whose manifest has only a `types` or `typings` entry
//! resolves through that entry. The fixture has one package whose `types`
//! names a source file, and one whose `typings` names a declaration file in
//! build output that maps to a source file. Without the entry, a type import
//! of the package is an unresolved import and its dependency is unused.

use super::common::{create_config, fixture_path};

#[test]
fn types_entry_resolves_a_package_without_main() {
    let config = create_config(fixture_path("types-only-package-entry"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec!["@fixture/no-entry"],
        "a package without any entry stays unresolved"
    );

    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unused.is_empty(),
        "each type import credits its dependency: {unused:?}"
    );
}
