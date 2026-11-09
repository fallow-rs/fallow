use std::fs;
use std::path::Path;

use super::common::create_config;
use fallow_types::results::AnalysisResults;

#[test]
fn dumi_markdown_demos_and_local_theme_are_reachable() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_dumi_site(root);

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);
    let unused_exports = unused_export_names(&results, root);
    let unused_dev_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();

    for reachable in [
        ".dumirc.ts",
        "src/button/demo/basic.tsx",
        "src/button/demo/helper.ts",
        "src/button/demo/size.tsx",
        "docs/shared/block.tsx",
        ".dumi/theme/builtins/Previewer/index.tsx",
        ".dumi/theme/common/Common.tsx",
        ".dumi/global.ts",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} should be reachable, unused: {unused_files:?}"
        );
    }
    for unused in [
        "src/button/demo/unlisted.tsx",
        ".dumi/theme/common/Orphan.tsx",
    ] {
        assert!(
            unused_files.contains(&unused.to_string()),
            "dumi does not load {unused}, unused: {unused_files:?}"
        );
    }

    for credited in [
        "src/button/demo/basic.tsx:default",
        "src/button/demo/size.tsx:default",
    ] {
        assert!(
            !unused_exports.contains(&credited.to_string()),
            "dumi renders the default export of {credited}, unused: {unused_exports:?}"
        );
    }

    for used in ["dumi-plugin-extra", "theme-only-dep"] {
        assert!(
            !unused_dev_deps.contains(&used),
            "{used} is used, unused dev dependencies: {unused_dev_deps:?}"
        );
    }
    assert!(
        unused_dev_deps.contains(&"really-unused"),
        "unused dev dependencies: {unused_dev_deps:?}"
    );
}

fn write_dumi_site(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{
            "name": "dumi-site",
            "private": true,
            "main": "src/index.ts",
            "scripts": { "start": "dumi dev" },
            "dependencies": { "react": "latest" },
            "devDependencies": {
                "dumi": "latest",
                "dumi-plugin-extra": "latest",
                "theme-only-dep": "latest",
                "really-unused": "latest"
            }
        }"#,
    );
    write_file(
        root,
        ".dumirc.ts",
        r#"
            import { defineConfig } from "dumi";
            export default defineConfig({ plugins: ["dumi-plugin-extra"] });
        "#,
    );
    write_file(root, "src/index.ts", "export const lib = 1;\n");
    write_file(
        root,
        "src/button/index.en-US.md",
        r#"# Button

<code src="./demo/basic.tsx">Basic</code>
<code src='./demo/size' debug>Size</code>
<code src="/docs/shared/block.tsx"></code>
<code src="../../../outside.tsx"></code>
"#,
    );
    write_file(
        root,
        "src/button/demo/basic.tsx",
        "import { helper } from \"./helper\";\nexport default () => helper;\n",
    );
    write_file(
        root,
        "src/button/demo/helper.ts",
        "export const helper = 1;\n",
    );
    write_file(
        root,
        "src/button/demo/size.tsx",
        "export default () => null;\n",
    );
    write_file(
        root,
        "src/button/demo/unlisted.tsx",
        "export default () => null;\n",
    );
    write_file(
        root,
        "docs/shared/block.tsx",
        "export default () => null;\n",
    );
    write_file(
        root,
        ".dumi/theme/builtins/Previewer/index.tsx",
        "import useSWR from \"theme-only-dep\";\nimport { Common } from \"../../common/Common\";\nexport default () => [useSWR, Common];\n",
    );
    write_file(
        root,
        ".dumi/theme/common/Common.tsx",
        "export const Common = 1;\n",
    );
    write_file(
        root,
        ".dumi/theme/common/Orphan.tsx",
        "export const Orphan = 1;\n",
    );
    write_file(root, ".dumi/global.ts", "export {};\n");
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| relative_path(&finding.file.path, root))
        .collect()
}

fn unused_export_names(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative_path(&finding.export.path, root),
                finding.export.export_name
            )
        })
        .collect()
}
