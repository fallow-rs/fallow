//! A workspace package can configure a tool under a key of its own
//! `package.json`, such as `"prettier": "@acme/config-prettier"`. The packages
//! that this inline config names count as used by that workspace and by the
//! root manifest, not by a sibling workspace. Other values of the config
//! apply to the workspace too, such as the Jest `setupFiles`.

use super::common::{create_config, fixture_path};

struct Findings {
    /// Unused dependencies and devDependencies as (package, manifest) pairs.
    unused_deps: Vec<(String, String)>,
    unused_files: Vec<String>,
}

fn analyze_fixture() -> Findings {
    let root = fixture_path("workspace-package-json-inline-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let relative = |path: &std::path::Path| {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let prod = results
        .unused_dependencies
        .iter()
        .map(|dep| (dep.dep.package_name.clone(), relative(&dep.dep.path)));
    let dev = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| (dep.dep.package_name.clone(), relative(&dep.dep.path)));
    Findings {
        unused_deps: prod.chain(dev).collect(),
        unused_files: results
            .unused_files
            .iter()
            .map(|file| relative(&file.file.path))
            .collect(),
    }
}

fn pair(name: &str, manifest: &str) -> (String, String) {
    (name.to_string(), manifest.to_string())
}

#[test]
fn workspace_inline_prettier_string_credits_config_package() {
    // The workspace has no `prettier` dependency of its own: a script runs the
    // hoisted root `prettier`, which activates the plugin for the workspace.
    let unused = analyze_fixture().unused_deps;

    assert!(
        !unused.contains(&pair("@acme/config-prettier", "apps/web/package.json")),
        "apps/web/package.json#prettier names @acme/config-prettier, got {unused:?}"
    );
    assert!(
        unused.contains(&pair("unused-web-control", "apps/web/package.json")),
        "unused-web-control should still be reported, got {unused:?}"
    );
}

#[test]
fn workspace_inline_prettier_plugins_credit_own_and_root_manifest_only() {
    let unused = analyze_fixture().unused_deps;

    assert!(
        !unused.contains(&pair("prettier-plugin-example", "packages/ui/package.json")),
        "packages/ui/package.json#prettier.plugins names prettier-plugin-example, got {unused:?}"
    );
    assert!(
        unused.contains(&pair("unused-ui-control", "packages/ui/package.json")),
        "unused-ui-control should still be reported, got {unused:?}"
    );
    // Prettier resolves the plugin from the workspace directory up to the
    // project root, so the root declaration counts as used.
    assert!(
        !unused.contains(&pair("prettier-plugin-hoisted", "package.json")),
        "packages/ui/package.json#prettier.plugins names the root prettier-plugin-hoisted, got {unused:?}"
    );
    assert!(
        unused.contains(&pair("unused-root-control", "package.json")),
        "unused-root-control should still be reported, got {unused:?}"
    );
    // A sibling workspace that declares the same plugin and names it nowhere
    // gets no credit from packages/ui.
    assert!(
        unused.contains(&pair(
            "prettier-plugin-example",
            "packages/docs/package.json"
        )),
        "packages/docs declares prettier-plugin-example without using it, got {unused:?}"
    );
}

#[test]
fn workspace_inline_jest_setup_files_are_entries() {
    let unused = analyze_fixture().unused_files;

    assert!(
        !unused.contains(&"apps/api/setup.js".to_string()),
        "apps/api/package.json#jest.setupFiles names setup.js, got {unused:?}"
    );
    assert!(
        unused.contains(&"apps/api/unused.js".to_string()),
        "apps/api/unused.js should still be reported, got {unused:?}"
    );
}
