use super::common::{create_config, create_production_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

fn assert_used_and_unused(fixture: &str, used: &[&str], unused: &[&str]) {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for path in used {
        assert!(
            !unused_files.iter().any(|file| file == path),
            "the site config lists {path}, unused files: {unused_files:?}"
        );
    }
    for path in unused {
        assert!(
            unused_files.iter().any(|file| file == path),
            "{path} is not in the site config and must stay unused, unused files: {unused_files:?}"
        );
    }
}

#[test]
fn mkdocs_extra_javascript_and_extra_css_files_are_entry_points() {
    assert_used_and_unused(
        "mkdocs-extra-assets",
        &[
            "docs/js/extra.js",
            "docs/js/helper.js",
            "docs/js/module.mjs",
            "docs/css/extra.css",
        ],
        &["docs/js/orphan.js", "docs/css/orphan.css"],
    );
}

#[test]
fn zensical_config_in_a_plain_subdirectory_resolves_against_its_docs_dir() {
    assert_used_and_unused(
        "zensical-extra-assets",
        &[
            "docs/chapters/javascripts/nav.mjs",
            "docs/chapters/javascripts/module.mjs",
            "docs/chapters/stylesheets/extra.css",
        ],
        &["docs/chapters/javascripts/orphan.mjs"],
    );
}

#[test]
fn workspace_package_site_configs_resolve_against_their_package() {
    assert_used_and_unused(
        "mkdocs-workspace-sites",
        &["docs/docs/js/docs.js", "packages/site/docs/js/site.js"],
        &["packages/site/docs/js/orphan.js"],
    );
}

/// In production mode only a root config activates the root pass, so the
/// workspace pass reads the config of the package named `docs` alone.
#[test]
fn production_workspace_site_config_resolves_against_its_package() {
    let root = fixture_path("mkdocs-workspace-sites");
    let config = create_production_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    assert!(
        !unused_files
            .iter()
            .any(|file| file == "docs/docs/js/docs.js"),
        "the docs package config lists js/docs.js, unused files: {unused_files:?}"
    );
}
