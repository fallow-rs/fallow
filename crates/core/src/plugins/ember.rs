//! Ember.js / Glimmer / Embroider plugin.
//!
//! Activates on `ember-source`, `ember-cli`, `@embroider/core`,
//! `@embroider/compat`, or `@glimmer/component` dependencies. Tracks Ember's
//! build, test, and runtime tooling deps (so they are not flagged as unused),
//! whitelists the lifecycle and reflectively-invoked members on Ember's class
//! hierarchy, exposes Ember's filesystem-resolved conventions (the classic
//! `app/` and `tests/` layouts) as entry-point globs since those files are
//! loaded by the Ember resolver rather than by static `import`, and declares
//! Ember's `@ember/*` namespace as a virtual module prefix so
//! Embroider-rewritten specifiers like `@ember/object` and
//! `@ember/routing/router-service` don't surface as `unresolved-import`.
//!
//! Scoped to strict-mode Ember apps and v2 addons. Classic v1 addon layouts
//! (`addon/`, `addon-test-support/`) are intentionally out of scope:
//! they predate strict-mode `.gts` / `.gjs`, so the plugin's value-adds
//! (template scanner, virtual `@ember/*` prefixes, `.gts` parsing) don't
//! apply. v1-addon maintainers can list those paths via `entry` in their
//! fallow config; v2 addons follow the standard `package.json#main` /
//! `exports` entry shape and don't need framework-specific globs.
//!
//! Template-block import tracking (`<template>...</template>`, `.gjs`/`.gts`
//! single-file components, and `.hbs` references) is handled separately by the
//! Glimmer-aware extractor in `crates/extract/src/glimmer.rs`. **Co-located
//! `.hbs` templates remain a known limitation** of that extractor: imports
//! consumed only by a sibling `.hbs` file still surface as `unused-import` on
//! the sibling `.js`/`.ts`; see the module-level note in
//! `crates/extract/src/sfc_template/glimmer.rs`. `ENTRY_PATTERNS` below
//! includes `*.hbs` paths so the templates themselves stay reachable as
//! files; binding-level usage tracking inside them is out of scope until the
//! scanner gains a Handlebars front-end. Migrating a component to `.gts`
//! removes the limitation entirely. Decorator-form
//! component, service, helper, and modifier registration (`@classic`,
//! `@service`, `@tracked`, `@action`) flows through the visitor and is not
//! re-implemented here. This plugin only handles the lifecycle and convention
//! members that the framework calls reflectively at runtime.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &[
    "ember-source",
    "ember-cli",
    "@embroider/core",
    "@embroider/compat",
    "@glimmer/component",
];

/// Packages required by an Ember project but never statically imported from
/// source. Anything an `import` statement can reach in a modern Ember app
/// (`@glimmer/component`, `@ember/test-helpers`, `@ember-data/*`, ...) is
/// intentionally omitted: the normal import graph already credits those, so
/// listing them here would only mask real removals when a user genuinely
/// drops a dependency.
///
/// Note: a package may legitimately appear in BOTH this list and `ENABLERS`.
/// `ember-source` is both the activation signal (its presence in
/// `package.json` is how we detect an Ember project) AND a runtime-resolved
/// dependency that no source file imports directly. The two roles are
/// independent: enablers gate plugin activation; tooling deps suppress
/// `unused-dependency` for build-/CLI-/runtime-resolved packages. Don't
/// dedupe.
const TOOLING_DEPENDENCIES: &[&str] = &[
    // Core Ember runtime / build pipeline
    //
    // `ember-source` is the meta-package: source code imports through the
    // `@ember/*` namespace (e.g. `@ember/application`, `@ember/routing/route`)
    // and never references the `ember-source` specifier directly.
    "ember-source",
    "ember-cli",
    "ember-cli-htmlbars",
    "ember-cli-babel",
    "ember-auto-import",
    // Embroider runtime core. The compat / webpack / vite halves are
    // `require()`'d from `ember-cli-build.js` (which is an entry pattern),
    // and `@embroider/addon-shim` is `require()`'d from each v2 addon's
    // `index.js` (reached via `package.json#main`); they're credited through
    // the normal import graph and don't need an allowlist entry. The macros /
    // router / test-setup halves are imported from source and likewise rely
    // on the import graph.
    "@embroider/core",
    // Glint type-checker CLI + tsconfig environment shims (`@glint/template`
    // IS imported as type-only and so is omitted here).
    "@glint/core",
    "@glint/environment-ember-loose",
    "@glint/environment-ember-template-imports",
    // Test infrastructure invoked by the runner, not imported from source
    // (`ember-qunit`, `qunit`, `qunit-dom`, `@ember/test-helpers` are imported
    // and so are omitted here).
    "ember-cli-test-loader",
    "ember-exam",
    // Common addons that act through ember-cli config, package.json keys, or
    // the build server rather than via source imports.
    "ember-template-lint",
    "ember-template-imports",
    "ember-source-channel-url",
    "@ember/optional-features",
    "ember-cli-dependency-checker",
    "ember-cli-inject-live-reload",
    "ember-cli-sri",
    "ember-cli-terser",
    "loader.js",
    // ember-cli build / runtime addons that register through
    // `ember-cli-build.js`, initializers, or `package.json#ember` rather
    // than via source imports. All ship as devDeps in the default
    // `ember new` blueprint.
    "broccoli-asset-rev",
    "ember-cli-app-version",
    "ember-export-application-global",
    // TypeScript ecosystem deps that the typescript plugin normally credits
    // via tsconfig `extends` / `compilerOptions.plugins`. Listed here as a
    // safety net because the default Ember TS blueprint ships them in
    // devDeps before the tsconfig is fully wired up, and a fresh Ember user
    // shouldn't have to debug a blueprint dep showing as unused.
    "@tsconfig/ember",
    "@glint/tsserver-plugin",
];

/// Glimmer / classic Ember component lifecycle members called by the framework
/// at runtime. Covers both `@glimmer/component` and the legacy
/// `@ember/component` class hierarchy.
const COMPONENT_MEMBERS: &[&str] = &[
    "willDestroy",
    "didInsertElement",
    "didRender",
    "didUpdate",
    "didReceiveAttrs",
    "willRender",
    "willUpdate",
    "willClearRender",
    "willDestroyElement",
    "didDestroyElement",
];

// `EmberObject` teardown lifecycle that the framework invokes reflectively
// on every subclass: `init` runs during instance construction, `willDestroy`
// is the documented teardown hook, and `destroy` is its less-common
// lower-level companion. The three names appear at the tail of each
// class-specific member list below so a user override on (say) a `Service`
// subclass is not flagged as unused.
//
// Intentionally NOT allowlisted: inherited utility methods that the user
// *calls* on the instance (`get`, `set`, `getProperties`, `setProperties`,
// `incrementProperty`, `decrementProperty`, `toggleProperty`,
// `notifyPropertyChange`, `cacheFor`, `addObserver`, `removeObserver`,
// `toString`, `send`, `controllerFor`, `paramsFor`, `modelFor`,
// `intermediateTransitionTo`, `refresh`, `has`, `off`, `on`, `one`,
// `trigger`, `register`, `reset`, `boot`, `visit`, `buildInstance`,
// `advanceReadiness`, `deferReadiness`). Users almost never override
// them, so an allowlist entry would silence the "you defined this and
// nothing calls it" check without preventing realistic false positives.
// Same reasoning excludes `isDestroyed` / `isDestroying` /
// `concatenatedProperties` / `mergedProperties` (set by the framework,
// never user-defined as a field).

/// Route hooks called by the Ember router during transitions, plus the
/// convention properties (`actions`, `queryParams`, `templateName`,
/// `controllerName`) that the resolver reads reflectively.
const ROUTE_MEMBERS: &[&str] = &[
    "model",
    "beforeModel",
    "afterModel",
    "setupController",
    "resetController",
    "redirect",
    "serialize",
    "deserialize",
    "activate",
    "deactivate",
    "buildRouteInfoMetadata",
    "actions",
    "queryParams",
    "templateName",
    "controllerName",
    // EmberObject lifecycle (init / willDestroy / destroy).
    "init",
    "willDestroy",
    "destroy",
];

/// Controller convention properties the Ember resolver reads reflectively
/// plus the `EmberObject` teardown lifecycle. `actions` and `queryParams`
/// are the common reflective cases; `templateName` and `controllerName`
/// are documented Ember APIs that the route's resolver honors when looking
/// up the paired template / controller for a route.
const CONTROLLER_MEMBERS: &[&str] = &[
    "actions",
    "queryParams",
    "templateName",
    "controllerName",
    // EmberObject lifecycle.
    "init",
    "willDestroy",
    "destroy",
];

const SERVICE_MEMBERS: &[&str] = &["init", "willDestroy", "destroy"];

const HELPER_MEMBERS: &[&str] = &[
    "compute",
    "recompute",
    // EmberObject lifecycle (class-based helpers extend `Helper` which
    // extends `EmberObject`; function-based helpers don't have lifecycle).
    "init",
    "willDestroy",
    "destroy",
];

const MODIFIER_MEMBERS: &[&str] = &[
    "modify",
    "willDestroy",
    "didReceiveArguments",
    "didInstall",
    "didUpdateArguments",
    "willRemove",
];

/// Application convention properties Ember reads at boot, plus the
/// `EmberObject` teardown lifecycle. `ready` is the framework-invoked
/// post-boot hook. `customEvents` / `eventDispatcher` / `resolver` /
/// `rootElement` are class fields that the framework consults during
/// initialization (e.g. `rootElement = '#my-app'`); without the
/// allowlist they would surface as unused class members on a user's
/// `Application` subclass.
const APPLICATION_MEMBERS: &[&str] = &[
    "ready",
    "customEvents",
    "eventDispatcher",
    "resolver",
    "rootElement",
    // EmberObject lifecycle.
    "init",
    "willDestroy",
    "destroy",
];

const ROUTER_MEMBERS: &[&str] = &[
    "map",
    "location",
    "rootURL",
    "willTransition",
    "didTransition",
];

/// Import-specifier prefixes that `ember-source` exposes through the AMD
/// loader (classic) or the Embroider rewriter (Embroider) rather than as
/// separate npm packages. Anything matching one of these prefixes is
/// suppressed from `unresolved-import` and `unlisted-dependency` reporting.
///
/// The list is **enumerated, not a blanket `@ember/`**, because parts of the
/// `@ember/*` namespace ARE real npm packages users install explicitly and
/// must keep listed in `package.json`:
///
/// - `@ember/test-helpers`
/// - `@ember/render-modifiers`
/// - `@ember/test-waiters`
/// - `@ember/string`
/// - `@ember/jquery`
/// - `@ember/legacy-built-in-components`
/// - `@ember/optional-features`
///
/// Silencing those with a blanket prefix would mask real missing-dep bugs.
///
/// Source of truth for the virtual list: `ember-source`'s `package.json`
/// `exports` field. Keep this in sync (it changes slowly; most additions
/// land as new subpaths under existing roots like `@ember/object/...` which
/// the prefix-match already covers).
///
/// Known gaps NOT covered (documented; users can `ignoreDependencies` or
/// add an inline `fallow-ignore-next-line unresolved-import`):
///
/// - Bare `import Ember from 'ember'`: a legacy Embroider-rewritten
///   specifier. A `"ember"` prefix would also catch `ember-cli`,
///   `ember-data`, `ember-source` and silence legitimate missing-dep
///   reports, so we leave it.
/// - v1 Ember addon subpaths (`ember-in-viewport/modifiers/in-viewport`,
///   `ember-power-select/components/...`): the v1 addon `addon/` tree
///   convention is not Node `exports`. A proper fix is addon-shape probing
///   in the resolver; the escape hatch today is `ignoreDependencies` per
///   addon (or migrating to its v2 build).
const VIRTUAL_MODULE_PREFIXES: &[&str] = &[
    // Bare `ember` (legacy `import Ember from 'ember'`). The trailing slash
    // is load-bearing: it makes the entry exact-match `ember` (via the
    // `strip_suffix('/')` shortcut in the suppression logic) AND match any
    // future `ember/<subpath>` shape without also covering `ember-cli`,
    // `ember-data`, or `ember-source`. A no-slash entry would prefix-match
    // every `ember-*` real npm package and mask legitimate missing-dep
    // reports.
    "ember/",
    // ember-source modules exposed via the AMD loader / Embroider rewriter.
    // Each entry covers its bare specifier plus every subpath
    // (`@ember/object` also covers `@ember/object/computed`,
    // `@ember/object/proxy`, etc.) via `starts_with`.
    "@ember/application",
    "@ember/array",
    "@ember/canary-features",
    "@ember/component",
    "@ember/controller",
    "@ember/debug",
    "@ember/destroyable",
    "@ember/engine",
    "@ember/enumerable",
    "@ember/error",
    "@ember/helper",
    "@ember/instrumentation",
    "@ember/modifier",
    "@ember/object",
    "@ember/owner",
    "@ember/renderer",
    "@ember/routing",
    "@ember/runloop",
    "@ember/service",
    // `@ember/template` covers `template-compilation`, `template-compiler`,
    // and `template-factory` via prefix-match.
    "@ember/template",
    "@ember/utils",
    "@ember/version",
];

// Build-time template placeholders (`{{rootURL}}` Handlebars expressions,
// `###APPNAME###` ember-cli blueprint scaffolds) that leak into `<script
// src>` / `<link href>` extractions from `app/index.html` are filtered out
// at extraction time by `crate::extract::html::is_template_placeholder`
// (see `crates/extract/src/html.rs`). The filter is generic across template
// engines, not Ember-specific, so the Ember plugin doesn't carry a
// per-framework substring list here.

const ENTRY_PATTERNS: &[&str] = &[
    // Classic app/ layout
    "app/app.{js,ts,gjs,gts}",
    "app/router.{js,ts}",
    "app/index.html",
    "app/components/**/*.{js,ts,gjs,gts,hbs}",
    "app/routes/**/*.{js,ts,gjs,gts}",
    "app/controllers/**/*.{js,ts}",
    "app/templates/**/*.{hbs,gjs,gts}",
    "app/models/**/*.{js,ts}",
    "app/services/**/*.{js,ts}",
    "app/helpers/**/*.{js,ts,gjs,gts}",
    "app/modifiers/**/*.{js,ts}",
    "app/initializers/**/*.{js,ts}",
    "app/instance-initializers/**/*.{js,ts}",
    "app/adapters/**/*.{js,ts}",
    "app/serializers/**/*.{js,ts}",
    "app/transforms/**/*.{js,ts}",
    // The classic v1 addon `addon/` and `addon-test-support/` layouts are
    // intentionally NOT exposed as entry-point globs. v1 addons predate
    // strict-mode `.gts` / `.gjs` (they ship the legacy AMD module
    // shape via ember-cli-babel and the classic resolver); the Ember
    // plugin's template scanner, `.gts` parser, and `@ember/*` virtual
    // prefixes only deliver value for strict-mode code. Maintainers of v1
    // addons can declare those paths explicitly via `entry` in their fallow
    // config; v2 addons follow the standard `package.json#main` /
    // `exports` entry shape and don't need framework-specific globs.

    // Tests
    "tests/test-helper.{js,ts}",
    "tests/index.html",
    "tests/**/*-test.{js,ts,gjs,gts}",
    // Build / config
    "config/environment.js",
    "config/targets.js",
    "config/optional-features.json",
    "config/deprecation-workflow.js",
    "ember-cli-build.js",
    "testem.js",
];

fn scoped_rule(extends: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some(extends.to_string()),
        implements: None,
        members: members.iter().map(|s| (*s).to_string()).collect(),
    })
}

pub struct EmberPlugin;

impl Plugin for EmberPlugin {
    fn name(&self) -> &'static str {
        "ember"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![
            scoped_rule("Component", COMPONENT_MEMBERS),
            scoped_rule("Route", ROUTE_MEMBERS),
            scoped_rule("Controller", CONTROLLER_MEMBERS),
            scoped_rule("Service", SERVICE_MEMBERS),
            scoped_rule("Helper", HELPER_MEMBERS),
            scoped_rule("Modifier", MODIFIER_MEMBERS),
            scoped_rule("Application", APPLICATION_MEMBERS),
            scoped_rule("Router", ROUTER_MEMBERS),
        ]
    }

    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        VIRTUAL_MODULE_PREFIXES
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_classic_embroider_and_glimmer() {
        let plugin = EmberPlugin;
        assert!(plugin.enablers().contains(&"ember-source"));
        assert!(plugin.enablers().contains(&"@embroider/core"));
        assert!(plugin.enablers().contains(&"@glimmer/component"));
    }

    #[test]
    fn tooling_dependencies_cover_runtime_only_packages() {
        let plugin = EmberPlugin;
        let deps = plugin.tooling_dependencies();
        // Build / CLI / config-only packages that no source file imports must
        // be credited via the tooling list.
        assert!(deps.contains(&"ember-source"));
        assert!(deps.contains(&"ember-cli-htmlbars"));
        assert!(deps.contains(&"@embroider/core"));
        assert!(deps.contains(&"@glint/core"));
        assert!(deps.contains(&"ember-exam"));
        assert!(deps.contains(&"loader.js"));
        // ember-cli build / runtime addons that ship in the default blueprint.
        assert!(deps.contains(&"broccoli-asset-rev"));
        assert!(deps.contains(&"ember-cli-app-version"));
        assert!(deps.contains(&"ember-export-application-global"));
        // TS-ecosystem safety nets (also covered by the typescript plugin when
        // tsconfig is wired up; listed here for the fresh-blueprint case).
        assert!(deps.contains(&"@tsconfig/ember"));
        assert!(deps.contains(&"@glint/tsserver-plugin"));
    }

    #[test]
    fn tooling_dependencies_omits_source_imported_packages() {
        // Packages a modern Ember app imports directly (`import Component from
        // '@glimmer/component'`, `import { tracked } from '@glimmer/tracking'`,
        // `import { module, test } from 'qunit'`, etc.) MUST NOT appear in
        // the tooling list. The normal import graph already credits them, and
        // listing them here would mask a real removal when a user genuinely
        // drops the dependency.
        let plugin = EmberPlugin;
        let deps = plugin.tooling_dependencies();
        for name in [
            "@glimmer/component",
            "@glimmer/tracking",
            "@glimmer/env",
            "@glint/template",
            "@ember/test-helpers",
            "ember-qunit",
            "qunit",
            "qunit-dom",
            "ember-data",
            "@ember-data/store",
            "@ember-data/model",
            "@embroider/macros",
            "@embroider/router",
            "@embroider/test-setup",
            // Reached via the normal import graph through `ember-cli-build.js`
            // (an entry pattern) which `require()`s the build half, and via
            // each v2 addon's `package.json#main` index.js for the shim.
            "@embroider/webpack",
            "@embroider/vite",
            "@embroider/addon-shim",
            "ember-load-initializers",
            "ember-resolver",
        ] {
            assert!(
                !deps.contains(&name),
                "{name} is imported from source in modern Ember; remove from tooling_dependencies"
            );
        }
    }

    #[test]
    fn lifecycle_rules_scope_component_members_to_glimmer_component() {
        let rules = EmberPlugin.used_class_member_rules();
        let component_rule = rules.iter().find_map(|r| match r {
            UsedClassMemberRule::Scoped(s) if s.extends.as_deref() == Some("Component") => Some(s),
            _ => None,
        });
        let component_rule = component_rule.expect("Component-scoped rule missing");
        assert!(component_rule.members.iter().any(|m| m == "willDestroy"));
        assert!(
            component_rule
                .members
                .iter()
                .any(|m| m == "didInsertElement")
        );
    }

    #[test]
    fn lifecycle_rules_scope_route_members_to_route_class() {
        let rules = EmberPlugin.used_class_member_rules();
        let route_rule = rules.iter().find_map(|r| match r {
            UsedClassMemberRule::Scoped(s) if s.extends.as_deref() == Some("Route") => Some(s),
            _ => None,
        });
        let route_rule = route_rule.expect("Route-scoped rule missing");
        assert!(route_rule.members.iter().any(|m| m == "model"));
        assert!(route_rule.members.iter().any(|m| m == "beforeModel"));
        assert!(route_rule.members.iter().any(|m| m == "setupController"));
    }

    #[test]
    fn unrelated_classes_get_no_lifecycle_rule_match() {
        let rules = EmberPlugin.used_class_member_rules();
        for r in &rules {
            let UsedClassMemberRule::Scoped(s) = r else {
                continue;
            };
            assert!(!s.matches_heritage(Some("UserService"), &[]));
        }
    }

    #[test]
    fn entry_patterns_cover_classic_layout() {
        let plugin = EmberPlugin;
        let patterns = plugin.entry_patterns();
        assert!(patterns.contains(&"app/components/**/*.{js,ts,gjs,gts,hbs}"));
        assert!(patterns.contains(&"tests/**/*-test.{js,ts,gjs,gts}"));
    }

    #[test]
    fn entry_patterns_do_not_include_v1_addon_layout() {
        // v1 addon `addon/` / `addon-test-support/` layouts are out of scope
        // because they predate strict-mode `.gts` / `.gjs` and gain nothing from
        // the plugin's value-adds. Re-introducing them as automatic entry
        // patterns would credit unused files in apps that happen to have an
        // `addon/` directory for unrelated reasons; v1-addon maintainers
        // should declare the paths via `entry` in their fallow config
        // instead. Regression fence against re-introducing the v1 globs.
        let plugin = EmberPlugin;
        let patterns = plugin.entry_patterns();
        for v1_only in [
            "addon/**/*.{js,ts,gjs,gts,hbs}",
            "addon-test-support/**/*.{js,ts,gjs,gts}",
        ] {
            assert!(
                !patterns.contains(&v1_only),
                "{v1_only} must not be in entry_patterns (v1 addons are out \
                 of scope); current patterns = {patterns:?}"
            );
        }
    }

    /// Check that an import specifier `spec` would be silenced by the
    /// plugin's virtual-module prefix list. Delegates to the production
    /// `matches_virtual_prefix` matcher so this test cannot drift from the
    /// `unresolved-import` / `unlisted-dependency` suppression sites in
    /// `crates/core/src/analyze/unused_deps.rs`.
    fn is_covered(prefixes: &[&str], spec: &str) -> bool {
        prefixes
            .iter()
            .any(|prefix| crate::analyze::matches_virtual_prefix(prefix, spec))
    }

    #[test]
    fn virtual_module_prefixes_cover_ember_source_runtime() {
        // Every specifier the user actually encounters in a strict-mode
        // Ember app and that `ember-source` rewrites at build time must be
        // covered. Includes top-level paths, subpaths under those roots,
        // the `template-*` family covered by the bare `@ember/template`
        // prefix via starts_with, and the bare `ember` legacy specifier
        // covered exactly via the `ember/` trailing-slash entry.
        let prefixes = EmberPlugin.virtual_module_prefixes();
        for spec in [
            // The exact specifiers from the original bug report.
            "@ember/object",
            "@ember/object/computed",
            "@ember/template",
            "@ember/service",
            "@ember/runloop",
            "@ember/utils",
            "@ember/routing/router-service",
            "@ember/helper",
            "@ember/modifier",
            // Other common runtime entries.
            "@ember/application",
            "@ember/component",
            "@ember/component/helper",
            "@ember/controller",
            "@ember/debug",
            "@ember/destroyable",
            "@ember/object/proxy",
            "@ember/routing/route",
            "@ember/template-compilation",
            "@ember/template-factory",
            "@ember/owner",
            // Bare `ember` (legacy `import Ember from 'ember'`).
            "ember",
        ] {
            assert!(
                is_covered(prefixes, spec),
                "expected `{spec}` to be silenced by the virtual-module \
                 prefix list (it is rewritten by Embroider / the AMD loader \
                 and not resolvable through node_modules); prefixes = \
                 {prefixes:?}",
            );
        }
    }

    #[test]
    fn virtual_module_prefixes_do_not_swallow_real_ember_npm_packages() {
        // Parts of the `@ember/*` namespace ARE real npm packages users
        // install explicitly. Silencing them with a blanket prefix would
        // mask legitimate `unlisted-dependency` reports when a user removes
        // one from `package.json`. This test is the regression fence
        // against re-introducing a blanket `@ember/` prefix.
        let prefixes = EmberPlugin.virtual_module_prefixes();
        for real in [
            "@ember/test-helpers",
            "@ember/render-modifiers",
            "@ember/test-waiters",
            "@ember/string",
            "@ember/jquery",
            "@ember/legacy-built-in-components",
            "@ember/optional-features",
        ] {
            assert!(
                !is_covered(prefixes, real),
                "`{real}` is a real npm package and must NOT be covered by \
                 the virtual-module prefix list; prefixes = {prefixes:?}",
            );
        }
    }

    #[test]
    fn virtual_module_prefixes_do_not_swallow_ember_dash_packages() {
        // `@ember-data/*`, `@glimmer/*`, and the entire `ember-*` family of
        // real npm packages (`ember-source`, `ember-cli`, `ember-data`,
        // `ember-in-viewport`, ...) resolve through normal node resolution.
        // A missing or misspelled specifier in any of those namespaces must
        // still surface. The `ember/` virtual entry uses a trailing slash
        // precisely so it ONLY matches bare `ember` and `ember/<subpath>`,
        // not `ember-<anything>`.
        let prefixes = EmberPlugin.virtual_module_prefixes();
        for spec in [
            "@ember-data/store",
            "@ember-data/model",
            "@glimmer/component",
            "@glimmer/tracking",
            "ember-source",
            "ember-cli",
            "ember-data",
            "ember-in-viewport",
            "ember-template-lint",
        ] {
            assert!(
                !is_covered(prefixes, spec),
                "`{spec}` must NOT be covered by the virtual-module prefix \
                 list; prefixes = {prefixes:?}",
            );
        }
    }
}
