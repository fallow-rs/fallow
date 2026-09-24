//! Issue #2851: a Nuxt module that a package layer registers is active for
//! the app that extends the layer. With `extends: ['docus']`, `@nuxt/content`
//! is active, so the `components/content/` files and `content.config.ts` of
//! the app are entry points with `autoImports` on and off.
//!
//! - An installed layer: fallow reads the `modules` of its `nuxt.config`.
//! - A layer that is not installed: a small table of known content layers
//!   (`docus`) implies `@nuxt/content`.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

const COMPONENT: &str = "<template><div><slot /></div></template>\n";

fn layer_app(root: &Path, layer: &str) {
    write(
        root,
        "package.json",
        &format!(
            r#"{{ "name": "docs", "private": true, "dependencies": {{ "nuxt": "^4.0.0", "{layer}": "^5.0.0" }} }}"#
        ),
    );
    write(
        root,
        "nuxt.config.ts",
        &format!("export default defineNuxtConfig({{ extends: ['{layer}'] }})\n"),
    );
    write(root, "content/index.md", "::callout\nHello\n::\n");
    write(
        root,
        "content.config.ts",
        "export default defineContentConfig({ collections: {} })\n",
    );
    write(root, "components/content/Callout.vue", COMPONENT);
    write(root, "components/Dead.vue", COMPONENT);
}

fn install_layer(root: &Path, layer: &str, modules: &str) {
    write(
        root,
        &format!("node_modules/{layer}/package.json"),
        &format!(r#"{{ "name": "{layer}", "version": "5.0.0", "main": "./nuxt.config.ts" }}"#),
    );
    write(
        root,
        &format!("node_modules/{layer}/nuxt.config.ts"),
        &format!("export default defineNuxtConfig({{ modules: [{modules}] }})\n"),
    );
}

fn unused_files(root: &Path, auto_imports: bool) -> Vec<String> {
    let mut config = create_config(root.to_path_buf());
    config.auto_imports = auto_imports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused.sort();
    unused
}

const CONTENT_FILES: &[&str] = &["components/content/Callout.vue", "content.config.ts"];

fn assert_content_active(unused: &[String]) {
    for kept in CONTENT_FILES {
        assert!(
            !unused.contains(&(*kept).to_string()),
            "{kept} must not report while a layer registers @nuxt/content, got: {unused:?}"
        );
    }
}

#[test]
fn an_installed_package_layer_that_registers_content_activates_it() {
    for auto_imports in [true, false] {
        let dir = tempfile::tempdir().expect("temp dir");
        layer_app(dir.path(), "@acme/docs-layer");
        install_layer(
            dir.path(),
            "@acme/docs-layer",
            "'@nuxt/ui', ['@nuxt/content', { build: {} }]",
        );
        let unused = unused_files(dir.path(), auto_imports);
        assert_content_active(&unused);
        if auto_imports {
            assert!(
                unused.contains(&"components/Dead.vue".to_string()),
                "an ordinary unreferenced component must still report, got: {unused:?}"
            );
        }
    }
}

#[test]
fn docus_implies_content_when_it_is_not_installed() {
    for auto_imports in [true, false] {
        let dir = tempfile::tempdir().expect("temp dir");
        layer_app(dir.path(), "docus");
        let unused = unused_files(dir.path(), auto_imports);
        assert_content_active(&unused);
    }
}

#[test]
fn a_package_layer_without_content_keeps_the_findings() {
    let dir = tempfile::tempdir().expect("temp dir");
    layer_app(dir.path(), "@acme/plain-layer");
    install_layer(dir.path(), "@acme/plain-layer", "'@nuxt/ui'");
    let unused = unused_files(dir.path(), true);
    for dead in CONTENT_FILES {
        assert!(
            unused.contains(&(*dead).to_string()),
            "{dead} must report when no layer registers @nuxt/content, got: {unused:?}"
        );
    }
}
