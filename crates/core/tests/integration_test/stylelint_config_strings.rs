//! Stylelint loads the modules that its config names in `extends`, `plugins`
//! and `customSyntax`: a name is a package, which counts as used, and a
//! relative path is a local module, which is reachable. An `extends` target
//! is read as a config: a local file, or the `main` file of a workspace
//! package. This holds for every config form, including the `stylelint` key
//! of `package.json`.

use std::path::Path;

use super::common::{create_config, fixture_path};

struct Findings {
    /// `(package, manifest)` pairs, with project-relative manifest paths.
    unused_deps: Vec<(String, String)>,
    /// Project-relative paths.
    unused_files: Vec<String>,
}

fn analyze(fixture: &str) -> Findings {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let relative = |path: &Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let unused_deps = results
        .unused_dependencies
        .iter()
        .map(|finding| &finding.dep)
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|finding| &finding.dep),
        )
        .map(|dep| (dep.package_name.clone(), relative(&dep.path)))
        .collect();
    let unused_files = results
        .unused_files
        .iter()
        .map(|file| relative(&file.file.path))
        .collect();
    Findings {
        unused_deps,
        unused_files,
    }
}

fn dep(name: &str, manifest: &str) -> (String, String) {
    (name.to_string(), manifest.to_string())
}

#[test]
fn config_files_credit_named_packages_and_keep_local_modules() {
    let findings = analyze("stylelint-config-strings");
    let unused = &findings.unused_deps;

    for credited in [
        dep(
            "stylelint-config-example",
            "packages/config-stylelint/package.json",
        ),
        dep(
            "stylelint-config-example-order",
            "packages/config-stylelint/package.json",
        ),
        dep(
            "stylelint-plugin-example",
            "packages/config-stylelint/package.json",
        ),
        dep(
            "@acme/stylelint-plugin-tokens",
            "packages/config-stylelint/package.json",
        ),
        dep(
            "postcss-example-html",
            "packages/config-stylelint/package.json",
        ),
        dep(
            "postcss-example-base",
            "packages/config-stylelint/package.json",
        ),
        dep("@acme/config-stylelint", "apps/web/package.json"),
        dep("@acme/config-stylelint", "apps/docs/package.json"),
        dep("postcss-example-markdown", "apps/docs/package.json"),
        dep("@acme/config-stylelint", "apps/admin/package.json"),
        dep("@acme/config-stylelint-main", "apps/admin/package.json"),
        dep(
            "@acme/stylelint-plugin-main",
            "packages/config-stylelint-main/package.json",
        ),
    ] {
        assert!(
            !unused.contains(&credited),
            "{credited:?} is named by a Stylelint config and must be credited, got {unused:?}"
        );
    }
    for control in [
        dep("unused-control", "packages/config-stylelint/package.json"),
        dep("unused-web-control", "apps/web/package.json"),
        dep("unused-docs-control", "apps/docs/package.json"),
        dep("unused-admin-control", "apps/admin/package.json"),
        dep(
            "unused-main-control",
            "packages/config-stylelint-main/package.json",
        ),
    ] {
        assert!(
            unused.contains(&control),
            "{control:?} should still be reported, got {unused:?}"
        );
    }

    let files = &findings.unused_files;
    for local in [
        "packages/config-stylelint/local-plugin.js",
        "packages/config-stylelint/base.cjs",
        "apps/admin/rules/admin-rule.js",
        "packages/config-stylelint-main/main-plugin.js",
    ] {
        assert!(
            !files.iter().any(|file| file == local),
            "{local} is a local module of a Stylelint config, got {files:?}"
        );
    }
    assert!(
        files
            .iter()
            .any(|file| file == "packages/config-stylelint/unused-helper.js"),
        "unused-helper.js should still be reported, got {files:?}"
    );
}

#[test]
fn workspace_package_json_key_credits_named_packages_and_keeps_local_modules() {
    let findings = analyze("stylelint-config-strings");
    let unused = &findings.unused_deps;

    for credited in [
        dep("@acme/config-stylelint-inline", "apps/inline/package.json"),
        dep("postcss-example-inline", "apps/inline/package.json"),
        // The shared config names this plugin, and only the inline key of
        // apps/inline extends the shared config.
        dep(
            "@acme/stylelint-plugin-inline",
            "packages/config-stylelint-inline/package.json",
        ),
    ] {
        assert!(
            !unused.contains(&credited),
            "{credited:?} is named through apps/inline/package.json#stylelint, got {unused:?}"
        );
    }
    for control in [
        dep("unused-inline-control", "apps/inline/package.json"),
        dep(
            "unused-inline-config-control",
            "packages/config-stylelint-inline/package.json",
        ),
    ] {
        assert!(
            unused.contains(&control),
            "{control:?} should still be reported, got {unused:?}"
        );
    }

    let files = &findings.unused_files;
    assert!(
        !files
            .iter()
            .any(|file| file == "apps/inline/rules/inline-rule.js"),
        "inline-rule.js is a plugin of apps/inline/package.json#stylelint, got {files:?}"
    );
}

#[test]
fn package_json_key_credits_named_packages_and_keeps_local_modules() {
    let findings = analyze("stylelint-package-json-config");
    let unused = &findings.unused_deps;
    assert!(
        !unused.contains(&dep("@acme/config-stylelint", "package.json")),
        "@acme/config-stylelint is the `extends` of package.json#stylelint, got {unused:?}"
    );
    assert!(
        unused.contains(&dep("unused-control", "package.json")),
        "unused-control should still be reported, got {unused:?}"
    );

    let files = &findings.unused_files;
    assert!(
        !files.iter().any(|file| file == "local-plugin.js"),
        "local-plugin.js is a plugin of package.json#stylelint, got {files:?}"
    );
    assert!(
        files.iter().any(|file| file == "unused-helper.js"),
        "unused-helper.js should still be reported, got {files:?}"
    );
}
