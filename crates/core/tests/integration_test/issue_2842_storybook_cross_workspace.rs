//! A Storybook app can load stories from a sibling workspace, and it can
//! name them with the `{ directory, files }` object form (issue #2842).

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn unused_files(root: &Path) -> Vec<String> {
    let config = create_config(root.to_path_buf());
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

/// A monorepo with a central docs app that loads stories from `packages/ui`.
fn central_docs_monorepo(root: &Path, stories: &str) {
    write(
        root,
        "package.json",
        r#"{ "name": "mono", "private": true, "workspaces": ["apps/*", "packages/*"] }"#,
    );
    write(
        root,
        "apps/docs/package.json",
        r#"{ "name": "docs", "private": true, "devDependencies": { "storybook": "^9.0.0" } }"#,
    );
    write(
        root,
        "apps/docs/.storybook/main.ts",
        &format!("export default {{ stories: {stories} }};\n"),
    );
    write(
        root,
        "packages/ui/package.json",
        r#"{ "name": "ui", "private": true, "main": "src/index.ts" }"#,
    );
    write(root, "packages/ui/src/index.ts", "export const ui = 1;\n");
    write(
        root,
        "packages/ui/src/a.docs.tsx",
        "export const Docs = {};\n",
    );
    write(
        root,
        "packages/ui/src/b.stories.tsx",
        "export default {};\n",
    );
    write(root, "packages/ui/src/c.mdx", "# Intro\n");
    write(
        root,
        "packages/ui/src/orphan.ts",
        "export const orphan = 1;\n",
    );
}

#[test]
fn a_central_docs_app_credits_stories_in_a_sibling_workspace() {
    let dir = tempfile::tempdir().expect("temp dir");
    central_docs_monorepo(dir.path(), r#"["../../../packages/ui/src/**/*.docs.tsx"]"#);
    let unused = unused_files(dir.path());
    assert!(
        !unused.contains(&"packages/ui/src/a.docs.tsx".to_string()),
        "a story in a sibling workspace must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/orphan.ts".to_string()),
        "a file outside the stories globs must still be reported, found {unused:?}"
    );
}

#[test]
fn a_stories_pattern_outside_the_project_credits_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().join("repo");
    central_docs_monorepo(&root, r#"["../../../../packages/ui/src/**/*.docs.tsx"]"#);
    let unused = unused_files(&root);
    assert!(
        unused.contains(&"packages/ui/src/a.docs.tsx".to_string()),
        "a pattern that leaves the project root must not credit a file, found {unused:?}"
    );
}

#[test]
fn the_object_form_applies_its_files_glob_under_the_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    central_docs_monorepo(
        dir.path(),
        r#"[{ directory: "../../../packages/ui/src", files: "**/*.docs.tsx", titlePrefix: "UI" }]"#,
    );
    let unused = unused_files(dir.path());
    assert!(
        !unused.contains(&"packages/ui/src/a.docs.tsx".to_string()),
        "a file that `files` matches under `directory` must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/b.stories.tsx".to_string()),
        "a file that `files` does not match must still be reported, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/orphan.ts".to_string()),
        "a file outside every matched set must still be reported, found {unused:?}"
    );
}

#[test]
fn the_object_form_without_files_uses_the_storybook_default() {
    let dir = tempfile::tempdir().expect("temp dir");
    central_docs_monorepo(
        dir.path(),
        r#"[{ directory: "../../../packages/ui/src", titlePrefix: "UI" }]"#,
    );
    let unused = unused_files(dir.path());
    assert!(
        !unused.contains(&"packages/ui/src/b.stories.tsx".to_string()),
        "a `*.stories.tsx` file under `directory` must be credited, found {unused:?}"
    );
    assert!(
        !unused.contains(&"packages/ui/src/c.mdx".to_string()),
        "an `*.mdx` file under `directory` must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/a.docs.tsx".to_string()),
        "a file that the default `files` glob does not match must still be reported, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/orphan.ts".to_string()),
        "a file outside every matched set must still be reported, found {unused:?}"
    );
}

#[test]
fn the_object_form_in_a_single_project_resolves_against_the_config_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "sb-object", "private": true, "main": "src/index.ts", "devDependencies": { "storybook": "^9.0.0" } }"#,
    );
    write(
        root,
        ".storybook/main.ts",
        r#"import type { StorybookConfig } from "@storybook/react-vite";
const config: StorybookConfig = {
  stories: ["../src/**/*.case.tsx", { directory: "../src/docs", files: "*.page.@(ts|tsx)" }],
};
export default config;
"#,
    );
    write(root, "src/index.ts", "export const x = 1;\n");
    write(root, "src/button.case.tsx", "export const Case = {};\n");
    write(root, "src/docs/intro.page.tsx", "export const Page = {};\n");
    write(
        root,
        "src/docs/nested/deep.page.tsx",
        "export const Page = {};\n",
    );
    write(root, "src/orphan.ts", "export const orphan = 1;\n");
    let unused = unused_files(root);
    assert!(
        !unused.contains(&"src/button.case.tsx".to_string()),
        "a string pattern next to an object entry must still be credited, found {unused:?}"
    );
    assert!(
        !unused.contains(&"src/docs/intro.page.tsx".to_string()),
        "a file that `files` matches under `directory` must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"src/docs/nested/deep.page.tsx".to_string()),
        "a file that the `files` glob does not reach must still be reported, found {unused:?}"
    );
    assert!(
        unused.contains(&"src/orphan.ts".to_string()),
        "a file outside every matched set must still be reported, found {unused:?}"
    );
}
