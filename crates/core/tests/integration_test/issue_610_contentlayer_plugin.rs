use super::common::{create_config, fixture_path};

#[test]
fn contentlayer_plugin_marks_config_content_generated_output_and_processors_used() {
    let root = fixture_path("issue-610-contentlayer-plugin");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.file.path.to_string_lossy().into_owned())
        .collect();

    for path in [
        "contentlayer.config.ts",
        "data/blog/post.mdx",
        "data/authors/bart.mdx",
        ".contentlayer/generated/index.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused.ends_with(path)),
            "{path} should be credited by the Contentlayer plugin, got {unused_files:?}"
        );
    }

    assert!(
        unused_files
            .iter()
            .any(|unused| unused.ends_with("src/orphan.ts")),
        "unrelated orphan source should still be reported, got {unused_files:?}"
    );

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|dep| dep.dep.package_name.as_str()),
        )
        .collect();

    for dep in [
        "contentlayer2",
        "next-contentlayer2",
        "remark-gfm",
        "remark-math",
        "rehype-slug",
        "rehype-katex",
    ] {
        assert!(
            !unused_dependencies.contains(&dep),
            "{dep} should be credited by the Contentlayer plugin, got {unused_dependencies:?}"
        );
    }

    assert!(
        unused_dependencies.contains(&"unused-control"),
        "unreferenced control dependency should still be reported, got {unused_dependencies:?}"
    );
}
