//! End-to-end smoke test for the Ember.js / Glimmer / Embroider plugin
//! against the `tests/fixtures/ember-classic/` fixture.
//!
//! Covers the plugin's two suppression mechanisms plus one HTML-scanner
//! integration at the level of `AnalysisResults`, not the per-shape unit
//! tests in `crates/core/src/plugins/ember.rs`:
//!
//! 1. `tooling_dependencies`: `ember-source`, `ember-cli-htmlbars`, and
//!    other runtime-resolved packages are not flagged as `unused-dependency`
//!    even though no source file imports them.
//! 2. `virtual_module_prefixes`: `@ember/object`,
//!    `@ember/routing/router-service`, and `@ember/service` (AMD-loader /
//!    Embroider-rewritten specifiers; not real npm packages) are not
//!    flagged as `unresolved-import` or `unlisted-dependency`.
//! 3. `<script src="{{rootURL}}...">` / `<script src="{{config.assetsPath}}...">`
//!    placeholders in `app/index.html` are skipped by the generic
//!    template-placeholder filter in `crate::extract::html`; they never
//!    enter the import graph in the first place, so they don't surface as
//!    `unresolved-import`. (This is a framework-agnostic HTML-scanner fix,
//!    not Ember-specific plugin code; the witness lives here because
//!    Ember is the most common consumer.)
//!
//! Also fences:
//!
//! - `used_class_member_rules`: `Service::init` / `Service::willDestroy`
//!   and `Route::model` / `Route::setupController` are not surfaced as
//!   `unused-class-member` on the convention subclasses.
//! - Template-only imports survive: `on` in `app/components/counter.gts`
//!   is referenced only inside the `<template>` block.

use super::common::{create_config, fixture_path};

#[test]
fn ember_classic_fixture_recognises_plugin_suppressions() {
    let root = fixture_path("ember-classic");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    let unlisted_deps: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    let unused_members: Vec<(String, String)> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            (
                finding.member.parent_name.clone(),
                finding.member.member_name.clone(),
            )
        })
        .collect();

    // 1. Tooling-only dependencies stay credited.
    for tool in [
        "ember-source",
        "ember-cli",
        "ember-cli-htmlbars",
        "ember-cli-babel",
        "loader.js",
    ] {
        assert!(
            !unused_deps.contains(&tool),
            "{tool} should not surface as unused-dependency; unused_deps = {unused_deps:?}"
        );
    }

    // 2. `@ember/*` virtual specifiers consumed by `app/controllers/application.ts`
    // must not surface as unresolved or unlisted.
    for virtual_spec in [
        "@ember/object",
        "@ember/service",
        "@ember/routing/router-service",
        "@ember/controller",
    ] {
        assert!(
            !unresolved.contains(&virtual_spec),
            "{virtual_spec} should be silenced by virtual_module_prefixes; \
             unresolved_imports = {unresolved:?}"
        );
        let pkg = virtual_spec
            .split('/')
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
        assert!(
            !unlisted_deps.contains(&pkg.as_str()),
            "{pkg} should be silenced by virtual_module_prefixes; \
             unlisted_dependencies = {unlisted_deps:?}"
        );
    }

    // 3. `{{rootURL}}` / `{{config.assetsPath}}` placeholders in
    //    `app/index.html` are filtered out at HTML asset extraction
    //    (`crate::extract::html::is_template_placeholder`), so they never
    //    enter the import graph and cannot surface as `unresolved-import`.
    for placeholder_fragment in ["{{rootURL}}", "{{config.assetsPath}}"] {
        assert!(
            !unresolved
                .iter()
                .any(|spec| spec.contains(placeholder_fragment)),
            "{placeholder_fragment} must be filtered out by the HTML asset \
             scanner's template-placeholder check; unresolved_imports = {unresolved:?}"
        );
    }

    // 4. Framework-invoked lifecycle members on convention subclasses
    //    survive (scoped used-class-member rules).
    let lifecycle_must_survive = [
        ("SessionService", "init"),
        ("SessionService", "willDestroy"),
        ("ApplicationRoute", "model"),
        ("ApplicationRoute", "setupController"),
    ];
    for (parent, member) in lifecycle_must_survive {
        assert!(
            !unused_members
                .iter()
                .any(|(p, m)| p == parent && m == member),
            "{parent}.{member} is framework-invoked and must not surface as \
             unused-class-member; unused_class_members = {unused_members:?}"
        );
    }
}
