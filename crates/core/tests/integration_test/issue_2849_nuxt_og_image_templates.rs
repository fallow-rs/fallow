//! Issue #2849: `nuxt-og-image` renders a template component that a
//! `defineOgImage('Name')` or `defineOgImageComponent('Name')` call names
//! with a string. With `autoImports` on, that string credits the template
//! file. A template that no call names still reports. With `autoImports`
//! off, every component file is an entry point, templates included.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

const TEMPLATE: &str = "<script setup lang=\"ts\">\ndefineProps<{ title?: string }>()\n</script>\n<template><div>{{ title }}</div></template>\n";

fn og_image_project(root: &Path) {
    write(
        root,
        "package.json",
        r#"{ "name": "og", "private": true, "dependencies": { "nuxt": "^4.0.0", "nuxt-og-image": "^6.0.0" } }"#,
    );
    write(
        root,
        "nuxt.config.ts",
        "export default defineNuxtConfig({ modules: ['nuxt-og-image'] })\n",
    );
    write(root, "app/app.vue", "<template><NuxtPage /></template>\n");
    write(
        root,
        "app/pages/index.vue",
        "<script setup lang=\"ts\">\ndefineOgImage('Docs.takumi', { title: 'Home' })\n</script>\n<template><div /></template>\n",
    );
    write(
        root,
        "app/pages/blog.vue",
        "<script setup lang=\"ts\">\ndefineOgImageComponent('BlogPost', { title: 'Blog' })\nuseShareImage()\n</script>\n<template><div /></template>\n",
    );
    write(
        root,
        "app/composables/useShareImage.ts",
        "export function useShareImage() {\n  return defineOgImage('OgImageHome')\n}\n",
    );
    write(root, "app/components/og-image/Docs.takumi.vue", TEMPLATE);
    write(root, "app/components/OgImage/Home.satori.vue", TEMPLATE);
    write(root, "app/components/og-image/blog/Post.vue", TEMPLATE);
    write(root, "app/components/og-image/Unused.takumi.vue", TEMPLATE);
    write(root, "app/components/Dead.vue", TEMPLATE);
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

const USED_TEMPLATES: &[&str] = &[
    "app/components/og-image/Docs.takumi.vue",
    "app/components/OgImage/Home.satori.vue",
    "app/components/og-image/blog/Post.vue",
];

#[test]
fn a_template_named_by_a_string_is_used_with_auto_imports_on() {
    let dir = tempfile::tempdir().expect("temp dir");
    og_image_project(dir.path());
    let unused = unused_files(dir.path(), true);
    for used in USED_TEMPLATES {
        assert!(
            !unused.contains(&(*used).to_string()),
            "{used} is named by a defineOgImage string and must not report, got: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"app/components/og-image/Unused.takumi.vue".to_string()),
        "a template that no call names must still report, got: {unused:?}"
    );
    assert!(
        unused.contains(&"app/components/Dead.vue".to_string()),
        "an ordinary unreferenced component must still report, got: {unused:?}"
    );
}

#[test]
fn templates_are_entry_points_with_auto_imports_off() {
    let dir = tempfile::tempdir().expect("temp dir");
    og_image_project(dir.path());
    let unused = unused_files(dir.path(), false);
    for used in USED_TEMPLATES {
        assert!(
            !unused.contains(&(*used).to_string()),
            "{used} must not report with autoImports off, got: {unused:?}"
        );
    }
}
