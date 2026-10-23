//! Vite credits Sass files that no module imports directly: `additionalData`
//! `@use` targets (#3819), `@use` through `resolve.alias` (#3820), and the
//! `vite-plugin-vuetify` `styles.configFile` (#3821).

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let mut paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    paths
}

fn unresolved_specifiers(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let mut specifiers: Vec<String> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.clone())
        .collect();
    specifiers.sort();
    specifiers
}

#[test]
fn issue_3819_additional_data_use_credits_partials_and_extensionless_files() {
    let config = create_config(fixture_path("issue-3819-vite-additional-data-partials"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_paths(&results);
    for credited in [
        "src/styles/_tokens.scss",
        "src/styles/theme.scss",
        "src/styles/mixins/_index.scss",
    ] {
        assert!(
            !unused.iter().any(|path| path.ends_with(credited)),
            "{credited} is loaded through additionalData: {unused:?}"
        );
    }
    assert!(
        unused
            .iter()
            .any(|path| path.ends_with("src/styles/_orphan.scss")),
        "a partial that nothing loads stays unused: {unused:?}"
    );
}

#[test]
fn issue_3820_sass_use_resolves_through_vite_alias() {
    let config = create_config(fixture_path("issue-3820-sass-vite-alias"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_paths(&results);
    assert!(
        !unused
            .iter()
            .any(|path| path.ends_with("src/styles/_tokens.scss")),
        "the aliased partial is reachable: {unused:?}"
    );
    assert_eq!(
        unresolved_specifiers(&results),
        vec!["./src/styles/missing".to_string()],
        "only the aliased file that does not exist stays unresolved"
    );
}

#[test]
fn issue_3821_vuetify_styles_config_file_is_an_entry() {
    let config = create_config(fixture_path("issue-3821-vuetify-config-file"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_paths(&results);
    assert!(
        unused.is_empty(),
        "the configFile and the partial it loads are used: {unused:?}"
    );
}
