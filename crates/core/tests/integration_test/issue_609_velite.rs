//! Issue #609: Velite plugin keeps config, content roots, and generated
//! `.velite` output reachable, while files outside the content root stay
//! reported. Modeled on the `BlakePetersen/petersen-pack` project shape.

use super::common::create_config;

fn write(path: std::path::PathBuf, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create dir");
    }
    std::fs::write(path, contents).expect("write file");
}

#[test]
fn velite_config_content_roots_and_generated_output_are_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    write(
        root.join("package.json"),
        r#"{
            "name": "velite-fixture",
            "private": true,
            "main": "src/app.ts",
            "devDependencies": {
                "velite": "0.2.0",
                "@shikijs/rehype": "1.0.0",
                "left-pad": "1.0.0"
            }
        }"#,
    );
    write(
        root.join("velite.config.ts"),
        r"
            import { defineConfig, defineCollection, s } from 'velite';
            import rehypeShiki from '@shikijs/rehype';

            const posts = defineCollection({
                name: 'Post',
                pattern: 'blog/**/*.mdx',
                schema: s.object({ title: s.string() }),
            });

            export default defineConfig({
                root: 'content',
                output: { data: '.velite' },
                collections: { posts },
                mdx: { rehypePlugins: [rehypeShiki] },
            });
        ",
    );
    // App entry consumes the generated collection output.
    write(
        root.join("src/app.ts"),
        "import { posts } from '../.velite';\nexport const app = posts;\n",
    );
    // Generated output (produced by Velite at build time).
    write(
        root.join(".velite/index.ts"),
        "export const posts: unknown[] = [];\n",
    );
    // Velite-managed content under the configured root.
    write(root.join("content/blog/post.mdx"), "# Post\n");
    // Control 1: an MDX file OUTSIDE the content root stays flagged.
    write(root.join("notes/stray.mdx"), "# Stray\n");
    // Control 2: a plain orphan source file stays flagged.
    write(root.join("orphan.ts"), "export const orphan = true;\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|unresolved| unresolved.import.specifier.as_str())
        .collect();
    let unused_dev_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for path in [
        "velite.config.ts",
        ".velite/index.ts",
        "content/blog/post.mdx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be treated as used by the Velite plugin: {unused_files:?}"
        );
    }

    // Controls: scoping must not credit files outside the content root.
    assert!(
        unused_files
            .iter()
            .any(|unused| unused == "notes/stray.mdx"),
        "content outside the Velite root should still be reported: {unused_files:?}"
    );
    assert!(
        unused_files.iter().any(|unused| unused == "orphan.ts"),
        "plain orphan files should still be reported: {unused_files:?}"
    );

    assert!(
        !unresolved_specs.contains(&"../.velite"),
        "generated .velite import should resolve: {unresolved_specs:?}"
    );
    // velite.config.ts import credits @shikijs/rehype; velite is tooling.
    assert!(
        !unused_dev_deps.contains(&"@shikijs/rehype"),
        "config import should credit @shikijs/rehype: {unused_dev_deps:?}"
    );
    assert!(
        !unused_dev_deps.contains(&"velite"),
        "velite is a tooling dependency and must not be flagged: {unused_dev_deps:?}"
    );
    // Control: an unrelated dep stays reported.
    assert!(
        unused_dev_deps.contains(&"left-pad"),
        "genuinely unused dev deps should still be reported: {unused_dev_deps:?}"
    );
}
