//! Issue #754: ESLint plugins pulled in transitively through a meta-preset
//! (e.g. `@antfu/eslint-config`) were reported as `unused-dev-dependency`. The
//! flat config calls the preset factory (`config({...})`) and names no individual
//! plugins, so the flat-config `plugins` object-key credit has nothing to match.
//!
//! The fix reads the preset's own `package.json` and credits the eslint-ecosystem
//! `dependencies` / `peerDependencies` it declares. Crediting is scoped to the
//! preset's declared set and to eslint-shaped names, so a plugin the preset does
//! not declare, and a general-purpose runtime dep, both stay reportable.

use super::common::{create_config, fixture_path};

fn unused_dev_deps(results: &fallow_core::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect()
}

#[test]
fn preset_declared_plugins_are_credited() {
    let root = fixture_path("issue-754-eslint-meta-preset");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_deps(&results);

    // The preset declares these as (peer) dependencies; they must be credited.
    assert!(
        !unused.contains(&"eslint-plugin-alpha"),
        "eslint-plugin-alpha is a peerDependency of @scope/eslint-config and must \
         be credited via the preset. Got unused_dev_dependencies: {unused:?}"
    );
    assert!(
        !unused.contains(&"@scope2/eslint-plugin"),
        "@scope2/eslint-plugin is a peerDependency of @scope/eslint-config and must \
         be credited via the preset. Got: {unused:?}"
    );
}

#[test]
fn plugin_not_declared_by_preset_stays_flagged() {
    // Non-vacuous control: a genuinely-unused eslint plugin the preset does NOT
    // declare must remain reported, proving the credit is scoped to the preset's
    // declared set rather than blanket-crediting every eslint plugin.
    let root = fixture_path("issue-754-eslint-meta-preset");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_deps(&results);

    assert!(
        unused.contains(&"eslint-plugin-orphan"),
        "eslint-plugin-orphan is not declared by the preset and is not wired into \
         the config, so it must stay reported. Got: {unused:?}"
    );
}

#[test]
fn preset_non_eslint_dependency_stays_flagged() {
    // The preset declares `picocolors` as a regular dependency. It is not an
    // eslint-ecosystem package, so crediting it would risk masking a genuinely
    // unused general-purpose dep the user declared independently.
    let root = fixture_path("issue-754-eslint-meta-preset");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dev_deps(&results);

    assert!(
        unused.contains(&"picocolors"),
        "picocolors is a non-eslint dependency of the preset and must not be \
         credited as an eslint plugin. Got: {unused:?}"
    );
}
