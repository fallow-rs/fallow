//! An import of a SvelteKit virtual module (`$app/*`, `$env/*`,
//! `$service-worker`) or of a generated `./$types` module uses
//! `@sveltejs/kit`, also when no `svelte.config.js` or `vite.config.*`
//! exists. Without this credit, `fallow fix` removes the package and the
//! imports stop to resolve. See issue #5019.

use super::common::{create_config, fixture_path};

fn unused_dev_dependencies(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    let mut names: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| {
            let manifest = dep.dep.path.to_string_lossy().replace('\\', "/");
            let workspace = manifest
                .rsplit_once("/packages/")
                .map_or("<root>", |(_, rest)| {
                    rest.split('/').next().unwrap_or("<root>")
                })
                .to_string();
            format!("{workspace}:{}", dep.dep.package_name)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn sveltekit_virtual_imports_credit_kit_without_config() {
    let config = create_config(fixture_path("sveltekit-project"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !names.contains(&"@sveltejs/kit"),
        "the $app and ./$types imports use @sveltejs/kit: {names:?}"
    );
}

#[test]
fn sveltekit_virtual_import_credit_stays_in_its_workspace() {
    let config = create_config(fixture_path("sveltekit-virtual-module-credit"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_dev_dependencies(&results),
        vec!["plain:@sveltejs/kit".to_string()],
        "a $app/state import (app) and a ./$types import (types-only) credit the kit of \
         their own workspace only"
    );
}

#[test]
fn a_specifier_that_only_starts_with_service_worker_does_not_credit_kit() {
    let config = create_config(fixture_path("sveltekit-service-worker-prefix"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        names.contains(&"@sveltejs/kit"),
        "$service-workerx is not the $service-worker module: {names:?}"
    );
}
