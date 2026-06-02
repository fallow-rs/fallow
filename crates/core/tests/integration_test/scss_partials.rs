use super::common::{create_config, fixture_path};

#[test]
fn scss_partial_files_resolved_via_underscore_convention() {
    let root = fixture_path("scss-partial-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

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

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|u| u.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"variables"),
        "'variables' should not be an unlisted dep: {unlisted:?}"
    );

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
fn angular_material_scss_package_entrypoint_resolves_external_relative_graph() {
    let root = fixture_path("angular-material-scss-entrypoint");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.contains(&"@angular/material"),
        "Angular Material Sass entrypoint should resolve: {unresolved_specs:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@angular/material"),
        "Angular Material imported via SCSS must not be reported as unused: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"unused-package"),
        "real unused dependencies should still be reported: {unused_dep_names:?}"
    );

    let unlisted_dep_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted_dep_names.contains(&"@angular/cdk"),
        "external package internals should not create unlisted deps: {unlisted_dep_names:?}"
    );
}

#[test]
fn scss_bare_import_does_not_collide_with_sibling_tsx() {
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
