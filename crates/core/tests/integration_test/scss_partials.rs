use super::common::{create_config, fixture_path};

#[test]
fn scss_partial_files_resolved_via_underscore_convention() {
    let root = fixture_path("scss-partial-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // _variables.scss and _mixins.scss should NOT be reported as unused files
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .filter_map(|n| n.to_str())
        .map(ToString::to_string)
        .collect();
    assert!(
        !unused_file_names.contains(&"_variables.scss".to_string()),
        "_variables.scss should be used via @use: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"_mixins.scss".to_string()),
        "_mixins.scss should be used via @use: {unused_file_names:?}"
    );

    // No unresolved imports for SCSS partial references
    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.iter().any(|s| s.contains("variables")),
        "variables should be resolved: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs.iter().any(|s| s.contains("mixins")),
        "mixins should be resolved: {unresolved_specs:?}"
    );

    // No unlisted dependencies for SCSS partials
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|u| u.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"variables"),
        "'variables' should not be an unlisted dep: {unlisted:?}"
    );

    // Directory index: _index.scss should be resolved via @use 'components'
    assert!(
        !unused_file_names.contains(&"_index.scss".to_string()),
        "_index.scss should be used via @use 'components': {unused_file_names:?}"
    );
    assert!(
        !unresolved_specs.iter().any(|s| s.contains("components")),
        "components should be resolved via _index.scss: {unresolved_specs:?}"
    );
}

#[test]
fn angular_style_preprocessor_include_paths_resolve_bare_scss_imports() {
    // Issue #103: Angular's `stylePreprocessorOptions.includePaths` allows bare
    // SCSS `@import 'variables'` / `@use 'mixins'` to resolve against extra
    // directories. The Angular plugin extracts these from angular.json and the
    // graph resolver retries failing bare SCSS specifiers against each path.
    let root = fixture_path("angular-scss-include-paths");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specs.iter().any(|s| s.contains("variables")),
        "@import 'variables' should resolve via includePaths: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs.iter().any(|s| s.contains("mixins")),
        "@use 'mixins' should resolve via includePaths: {unresolved_specs:?}"
    );

    // Partial files reached only via includePaths must not be flagged unused.
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .filter_map(|n| n.to_str())
        .map(ToString::to_string)
        .collect();
    assert!(
        !unused_file_names.contains(&"_variables.scss".to_string()),
        "_variables.scss should be reachable via includePaths: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"_mixins.scss".to_string()),
        "_mixins.scss should be reachable via includePaths: {unused_file_names:?}"
    );
}

#[test]
fn scss_bare_specifiers_resolve_from_node_modules() {
    // Issue #125: Sass's `@import` / `@use` resolution searches `node_modules/`
    // for bare specifiers. `@import 'bootstrap/scss/functions'` should resolve
    // to `node_modules/bootstrap/scss/_functions.scss` (partial convention) and
    // `@import 'animate.css/animate.min'` should resolve to
    // `node_modules/animate.css/animate.min.css` (CSS extension append).
    let root = fixture_path("scss-node-modules-resolution");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("bootstrap/scss/functions")),
        "@import 'bootstrap/scss/functions' should resolve via node_modules: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("bootstrap/scss/variables")),
        "@import 'bootstrap/scss/variables' should resolve via node_modules: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("bootstrap/scss/mixins")),
        "@use 'bootstrap/scss/mixins' should resolve via node_modules: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("animate.css/animate.min")),
        "@import 'animate.css/animate.min' should resolve via node_modules \
         (CSS extension append): {unresolved_specs:?}"
    );

    // Packages resolved via node_modules must be tracked as used so that
    // `unused-dependencies` does not flag them.
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"bootstrap"),
        "bootstrap imported via SCSS must not be reported as unused: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"animate.css"),
        "animate.css imported via SCSS must not be reported as unused: {unused_dep_names:?}"
    );
}

#[test]
fn external_package_scss_subpaths_credit_nested_style_dependencies() {
    // Real-world styleguide packages often expose raw SCSS entrypoints from
    // node_modules. When the consumer imports that SCSS, nested imports like
    // `bootstrap/scss/functions` and `/node_modules/@vuepic/vue-datepicker/dist/main`
    // are build-time requirements of the app even though they live inside the
    // external package source tree.
    let root = fixture_path("external-style-package-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"@acme/style-lib"),
        "external SCSS entrypoint owner package must be treated as used: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"bootstrap"),
        "bootstrap imported inside external SCSS must not be reported as unused: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"@vuepic/vue-datepicker"),
        "@vuepic/vue-datepicker imported via external SCSS must not be reported as unused: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"unused-package"),
        "real unused dependencies should still be reported: {unused_dep_names:?}"
    );
}

#[test]
fn scss_bare_import_does_not_collide_with_sibling_tsx() {
    // Issue #245: `@use 'Widget'` from a `.scss` file MUST resolve only to
    // `Widget.scss` / `_Widget.scss` etc. Sass never sees JS/TS files; a
    // sibling `Widget.tsx` is invisible to the Sass resolver. The bug
    // manifested as a phantom 3-file circular dependency chain when both a
    // `.tsx` component file and its `.scss` style sheet existed alongside.
    let root = fixture_path("scss-bare-import-tsx-collision");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.circular_dependencies.is_empty(),
        "expected no circular dependencies, got: {:?}",
        results
            .circular_dependencies
            .iter()
            .map(|c| c
                .cycle
                .files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>()
    );

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved_specs.is_empty(),
        "expected no unresolved imports, got: {unresolved_specs:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .filter_map(|n| n.to_str())
        .map(ToString::to_string)
        .collect();
    assert!(
        !unused_files.contains(&"Widget.scss".to_string()),
        "Widget.scss must be reachable via Helper.scss `@use 'Widget'`: {unused_files:?}"
    );
}
