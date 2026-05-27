use std::fs;

use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

use super::common::{create_config, fixture_path};

// ── Vitest __mocks__ virtual specifiers ───────────────────────

#[test]
fn vitest_mocks_specifiers_not_flagged_as_unlisted_dep() {
    let root = fixture_path("vitest-mocks-virtual");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unlisted_names.contains(&"@aws-sdk/__mocks__"),
        "@aws-sdk/__mocks__ should not be flagged as an unlisted dependency, got: {unlisted_names:?}"
    );
}

// ── Vitest __mocks__ virtual specifiers in monorepo workspace ─────

#[test]
fn vitest_mocks_scoped_specifiers_not_flagged_in_workspace_monorepo() {
    // Vitest is only in apps/mrv/package.json (workspace), not root package.json.
    // The virtual_package_suffixes contributed by VitestPlugin must be merged from
    // the workspace plugin result into the root aggregated result, otherwise
    // @scope/__mocks__ specifiers are still flagged as unlisted dependencies.
    let root = fixture_path("vitest-mocks-workspace");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    for specifier in &[
        "@aws-sdk/__mocks__",
        "@supabase/__mocks__",
        "@sentry/__mocks__",
    ] {
        assert!(
            !unlisted_names.contains(specifier),
            "{specifier} should not be flagged as an unlisted dependency in workspace monorepo, got: {unlisted_names:?}"
        );
    }
}

// ── Unlisted dependencies integration ──────────────────────────

#[test]
fn unlisted_dependencies_detected() {
    let root = fixture_path("unlisted-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unlisted_names.contains(&"some-pkg"),
        "some-pkg should be detected as unlisted dependency, found: {unlisted_names:?}"
    );
}

#[test]
fn unlisted_re_export_dependency_reports_re_export_line() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "unlisted-re-export",
  "main": "src/index.ts"
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const local = 1;\nexport { default as pad } from 'left-pad';\n",
    )
    .expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let finding = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "left-pad")
        .expect("left-pad re-export should be reported as unlisted");

    assert_eq!(finding.dep.imported_from.len(), 1);
    assert_eq!(finding.dep.imported_from[0].line, 2);
}

// ── Unresolved imports integration ─────────────────────────────

#[test]
fn unresolved_imports_detected() {
    let root = fixture_path("unresolved-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        unresolved_specifiers.contains(&"./nonexistent"),
        "\"./nonexistent\" should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"./missing-re-export"),
        "named re-export source should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"./missing-star-re-export"),
        "star re-export source should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
}

#[test]
fn ignore_unresolved_imports_config_suppresses_matching_specifiers() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("create src dir");
    fs::create_dir_all(root.join("node_modules/@example/icons")).expect("create package dir");
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "ignore-unresolved-imports-config",
  "main": "src/index.ts",
  "dependencies": {
    "@example/icons": "1.0.0"
  }
}"#,
    )
    .expect("write package.json");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "ignoreUnresolvedImports": [
    "@example/icons",
    "@example/icons/**",
    "../generated/**"
  ]
}"#,
    )
    .expect("write fallow config");
    fs::write(
        root.join("node_modules/@example/icons/package.json"),
        r#"{
  "name": "@example/icons",
  "version": "1.0.0",
  "exports": {
    ".": "./dist/index.js",
    "./metadata": "./dist/metadata.js"
  }
}"#,
    )
    .expect("write package manifest");
    fs::write(
        root.join("src/index.ts"),
        r#"import { Icon } from "@example/icons";
import { metadata } from "@example/icons/metadata";
import { generated } from "../generated/client";
import { local } from "./still-missing";

export const main = () => [Icon, metadata, generated, local];
"#,
    )
    .expect("write source");

    let (loaded, _) = FallowConfig::find_and_load(root)
        .expect("config discovery should succeed")
        .expect("fixture config should be discovered");
    let config = loaded.resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert_eq!(
        unresolved_specifiers,
        vec!["./still-missing"],
        "config-loaded ignoreUnresolvedImports should suppress bare package, package subpath, and parent-relative generated specifiers"
    );
}

// ── Unused devDependencies ─────────────────────────────────────

#[test]
fn unused_dev_dependency_detected() {
    let root = fixture_path("unused-dev-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unused_dev_dep_names.contains(&"my-custom-dev-tool"),
        "my-custom-dev-tool should be detected as unused dev dependency, found: {unused_dev_dep_names:?}"
    );
}

// ── Unused optionalDependencies ───────────────────────────────

#[test]
fn unused_optional_dependency_detected() {
    let root = fixture_path("optional-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_optional_dep_names: Vec<&str> = results
        .unused_optional_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unused_optional_dep_names.contains(&"unused-optional-pkg"),
        "unused-optional-pkg should be detected as unused optional dependency, found: {unused_optional_dep_names:?}"
    );
}

#[test]
fn unused_workspace_dependency_reports_other_workspace_usage() {
    let root = fixture_path("cross-workspace-dependency-context");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dep = results
        .unused_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "lodash-es")
        .expect("lodash-es should be unused in the shared workspace");

    assert!(
        dep.dep.path.ends_with("packages/shared/package.json"),
        "finding should point at the workspace that declares lodash-es, got {}",
        dep.dep.path.display()
    );
    assert_eq!(
        dep.dep.used_in_workspaces,
        vec![root.join("packages/consumer")],
        "unused dependency should identify the sibling workspace importing it"
    );

    let unlisted = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "lodash-es")
        .expect("lodash-es should be unlisted in the consumer workspace");
    assert_eq!(
        unlisted.dep.imported_from.len(),
        1,
        "lodash-es should have one unlisted import site"
    );
    assert!(
        unlisted.dep.imported_from[0]
            .path
            .ends_with("packages/consumer/src/index.ts"),
        "finding should point at the importing consumer file, got {}",
        unlisted.dep.imported_from[0].path.display()
    );
}

// ── Peer dependencies ─────────────────────────────────────────

#[test]
fn peer_dependency_of_used_installed_package_is_not_unused() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(root.join("node_modules/react-dom")).expect("create react-dom dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "peer-dep-repro",
  "private": true,
  "dependencies": {
    "react": "18.3.1",
    "react-dom": "18.3.1",
    "left-pad": "1.3.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.tsx"),
        "import { createRoot } from 'react-dom/client';\ncreateRoot(document.body).render('hello');\n",
    )
    .expect("write source");
    std::fs::write(
        root.join("node_modules/react-dom/package.json"),
        r#"{"name":"react-dom","peerDependencies":{"react":"^18.3.1"}}"#,
    )
    .expect("write react-dom package");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"react"),
        "react is required as react-dom's peer dependency and must not be reported: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "unrelated unused dependencies should still be reported: {unused_dep_names:?}"
    );
}

#[test]
fn peer_dependency_of_parent_installed_package_is_not_unused() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let parent = tmp.path().join("monorepo");
    let root = parent.join("packages/app");
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(parent.join("node_modules/react-dom"))
        .expect("create parent react-dom dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "peer-dep-hoisted-repro",
  "private": true,
  "dependencies": {
    "react": "18.3.1",
    "react-dom": "18.3.1",
    "left-pad": "1.3.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.tsx"),
        "import { createRoot } from 'react-dom/client';\ncreateRoot(document.body).render('hello');\n",
    )
    .expect("write source");
    std::fs::write(
        parent.join("node_modules/react-dom/package.json"),
        r#"{
  "name": "react-dom",
  "peerDependencies": {"react": "^18.3.1"},
  "exports": {"./client": "./client.js"}
}"#,
    )
    .expect("write react-dom package");
    std::fs::write(
        parent.join("node_modules/react-dom/client.js"),
        "export function createRoot() { return { render() {} }; }\n",
    )
    .expect("write react-dom client");

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"react"),
        "react is required as parent-installed react-dom's peer dependency and must not be reported: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "unrelated unused dependencies should still be reported: {unused_dep_names:?}"
    );
}

// ── Package.json `imports` field (#subpath imports) ─────────

#[test]
fn subpath_imports_resolve_correctly() {
    let root = fixture_path("subpath-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // #utils and #components/Button should resolve — no unresolved imports
    assert!(
        results.unresolved_imports.is_empty(),
        "# imports should resolve via package.json imports field, got unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|u| u.import.specifier.as_str())
            .collect::<Vec<_>>()
    );

    // #utils and #components/Button should not be unlisted deps
    assert!(
        results.unlisted_dependencies.is_empty(),
        "# imports should not be reported as unlisted deps, got: {:?}",
        results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );

    // The `unused` export in utils/index.ts should still be detected
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unused"),
        "unused export should still be detected, got: {unused_export_names:?}"
    );
}

#[test]
fn package_imports_missing_dist_resolve_to_source() {
    let root = fixture_path("package-imports-missing-dist");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#nitro/runtime/task"),
        "manifest-mapped runtime import should resolve, got: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"#nitro/virtual/polyfills"),
        "manifest-mapped virtual import should resolve, got: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"#nitro/runtime/missing"),
        "manifest match without a source target should stay unresolved: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"#other/alias"),
        "unmatched hash alias should stay unresolved: {unresolved_specifiers:?}"
    );

    assert!(
        results.unlisted_dependencies.is_empty(),
        "root self import and package imports should not become unlisted deps: {:?}",
        results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/internal/orphan.ts")),
        "unrelated source files should still be reported as unused"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/internal/task.ts")),
        "runtime task source should be reachable through imports fallback"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/virtual/polyfills.ts")),
        "virtual polyfills source should be reachable through imports fallback"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/self.ts")),
        "root self package export should resolve back to source"
    );
}

#[test]
fn package_imports_external_targets_credit_dependency_usage() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r##"{
  "name": "imports-external-target",
  "main": "src/index.ts",
  "imports": {
    "#pad": "left-pad"
  },
  "dependencies": {
    "left-pad": "1.3.0",
    "unused": "1.0.0"
  }
}"##,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import pad from '#pad';\nexport const value = pad('x', 2);\n",
    )
    .expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#pad"),
        "package imports external target should resolve: {unresolved_specifiers:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"left-pad"),
        "external target dependency should be credited as used: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"unused"),
        "unrelated dependency should still be reported unused: {unused_dep_names:?}"
    );
}

#[test]
fn package_imports_array_fallback_resolves_reachable_target() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r##"{
  "name": "imports-array-fallback",
  "main": "src/index.ts",
  "imports": {
    "#public/feature": ["./dist/missing.js", "./src/feature.ts"]
  }
}"##,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { feature } from '#public/feature';\nexport const value = feature();\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/feature.ts"),
        "export function feature() { return 'ok'; }\n",
    )
    .expect("write feature");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#public/feature"),
        "array fallback should resolve to the reachable target: {unresolved_specifiers:?}"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/feature.ts")),
        "array fallback target should be reachable"
    );
}

#[test]
fn package_exports_array_fallback_resolves_self_package_source() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "self-array-fallback",
  "main": "src/index.ts",
  "exports": {
    "./public-feature": ["./dist/missing.js", "./src/feature.ts"]
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { feature } from 'self-array-fallback/public-feature';\nexport const value = feature();\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/feature.ts"),
        "export function feature() { return 'ok'; }\n",
    )
    .expect("write feature");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"self-array-fallback/public-feature"),
        "self-package exports array fallback should resolve: {unresolved_specifiers:?}"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/feature.ts")),
        "self-package exports array fallback target should be reachable"
    );
}

// ── Issue #124: ignorePatterns applied to workspace package.json discovery ──

#[test]
fn ignore_patterns_applied_to_workspace_package_json_for_unused_deps() {
    // Issue #124: when `.fallowrc.json` excludes `**/dist/**`, fallow must also
    // skip `packages/*/dist/package.json` (build artifacts from ng-packagr, tsc,
    // Rollup, etc.) during unused-dependency scanning. Without the fix, every
    // dep listed in the build-artifact package.json is reported as unused.
    let root = fixture_path("ignore-patterns-workspace-package-json");
    let config = FallowConfig {
        schema: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec!["**/dist/**".to_string()],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        duplicates: fallow_config::DuplicatesConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Every unused-dep finding whose path leads through a `dist/` directory is
    // a false positive — that package.json should have been ignored entirely.
    let dist_findings: Vec<String> = results
        .unused_dependencies
        .iter()
        .filter(|d| {
            d.dep
                .path
                .components()
                .any(|c| matches!(c, std::path::Component::Normal(s) if s == "dist"))
        })
        .map(|d| format!("{} -> {}", d.dep.package_name, d.dep.path.display()))
        .collect();
    assert!(
        dist_findings.is_empty(),
        "deps from dist/package.json must not be reported when dist/ is ignored: {dist_findings:?}"
    );

    // `is-odd` is only declared in `packages/my-lib/package.json` and never
    // imported — it should still be reported. This guards against a regression
    // where the ignore check accidentally skips real workspace package.json files.
    let reported: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        reported.contains(&"is-odd"),
        "real unused dep `is-odd` should still be reported, got: {reported:?}"
    );
}
