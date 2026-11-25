use super::common::create_config;

#[test]
fn nx_project_json_marks_nested_main_as_reachable_without_workspace_package_json() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("apps/query/src")).expect("app source dir should exist");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "query-monorepo",
            "dependencies": {
                "nx": "1.0.0"
            }
        }"#,
    )
    .expect("root package.json should be written");
    std::fs::write(
        root.join("apps/query/project.json"),
        r#"{
            "targets": {
                "build": {
                    "executor": "@nx/js:tsc",
                    "options": {
                        "main": "apps/query/src/main.ts"
                    }
                }
            }
        }"#,
    )
    .expect("project.json should be written");
    std::fs::write(
        root.join("apps/query/src/main.ts"),
        "import { helper } from './helper';\nconsole.log(helper);\n",
    )
    .expect("main.ts should be written");
    std::fs::write(
        root.join("apps/query/src/helper.ts"),
        "export const helper = 1;\nexport const unusedHelper = 2;\n",
    )
    .expect("helper.ts should be written");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|issue| {
            issue
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&issue.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        !unused_files.contains(&"apps/query/src/main.ts".to_string()),
        "main.ts should be reachable via Nx project.json, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"apps/query/src/helper.ts".to_string()),
        "helper.ts should be reachable from main.ts, unused files: {unused_files:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|issue| issue.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.contains(&"unusedHelper".to_string()),
        "unusedHelper should still be reported, unused exports: {unused_exports:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "imports should resolve, found unresolved imports: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|issue| issue.import.specifier.as_str())
            .collect::<Vec<_>>()
    );
}

fn write_files(root: &std::path::Path, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("file has a parent"))
            .expect("parent dir should be created");
        std::fs::write(&path, contents).expect("file should be written");
    }
}

fn unused_file_paths(root: &std::path::Path) -> Vec<String> {
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_files
        .iter()
        .map(|issue| {
            issue
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&issue.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn nx_credits_executors_generators_and_local_plugins() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let root = tmp.path();
    write_files(
        root,
        &[
            (
                "package.json",
                r#"{"name":"root","private":true,"workspaces":["packages/*"],"devDependencies":{"nx":"21.0.0"}}"#,
            ),
            (
                "nx.json",
                r#"{"plugins":[{"plugin":"./tools/local-plugin/index.ts"}]}"#,
            ),
            (
                "tools/local-plugin/index.ts",
                "export const createNodes = [];\n",
            ),
            ("tools/orphan.ts", "export const orphan = 1;\n"),
            (
                "packages/app/package.json",
                r#"{"name":"@mre/app","version":"1.0.0","main":"src/index.ts"}"#,
            ),
            ("packages/app/src/index.ts", "export const app = 1;\n"),
            (
                "packages/app/project.json",
                r#"{"name":"app","targets":{"build":{"executor":"@mre/nx-plugin:compile"}}}"#,
            ),
            (
                "packages/nx-plugin/package.json",
                r#"{"name":"@mre/nx-plugin","version":"1.0.0","main":"src/index.ts","executors":"./executors.json","generators":"./generators.json"}"#,
            ),
            (
                "packages/nx-plugin/src/index.ts",
                "export const pluginName = \"mre\";\n",
            ),
            (
                "packages/nx-plugin/executors.json",
                r#"{"executors":{"compile":{"implementation":"./src/executors/compile/executor","schema":"./src/executors/compile/schema.json"}}}"#,
            ),
            (
                "packages/nx-plugin/generators.json",
                r#"{"generators":{"gen":{"factory":"./src/generators/gen/generator","schema":"./src/generators/gen/schema.json"}}}"#,
            ),
            (
                "packages/nx-plugin/src/executors/compile/executor.ts",
                "import { helper } from \"./helper\"; export default async function executor() { return { success: helper() }; }\n",
            ),
            (
                "packages/nx-plugin/src/executors/compile/helper.ts",
                "export function helper() { return true; }\n",
            ),
            (
                "packages/nx-plugin/src/generators/gen/generator.ts",
                "export default async function generator() {}\n",
            ),
            (
                "packages/nx-plugin/src/executors/compile/schema.json",
                "{}\n",
            ),
            ("packages/nx-plugin/src/generators/gen/schema.json", "{}\n"),
        ],
    );

    let unused = unused_file_paths(root);
    for path in [
        "packages/nx-plugin/src/executors/compile/executor.ts",
        "packages/nx-plugin/src/executors/compile/helper.ts",
        "packages/nx-plugin/src/generators/gen/generator.ts",
        "tools/local-plugin/index.ts",
    ] {
        assert!(
            !unused.contains(&path.to_string()),
            "{path} is loaded by Nx, unused files: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"tools/orphan.ts".to_string()),
        "a file that Nx does not load stays unused: {unused:?}"
    );
}

#[test]
fn nx_follows_local_plugin_directories_and_local_executor_paths() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let root = tmp.path();
    write_files(
        root,
        &[
            (
                "package.json",
                r#"{"name":"root","private":true,"devDependencies":{"nx":"21.0.0"}}"#,
            ),
            ("nx.json", r#"{"plugins":["./tools/dir-plugin"]}"#),
            (
                "tools/dir-plugin/package.json",
                r#"{"name":"dir-plugin","main":"./src/plugin.ts"}"#,
            ),
            (
                "tools/dir-plugin/src/plugin.ts",
                "export const createNodesV2 = [];\n",
            ),
            (
                "apps/web/project.json",
                r#"{"targets":{"e2e":{"executor":"./tools/local-executors:e2e"}}}"#,
            ),
            ("apps/web/src/main.ts", "export const web = 1;\n"),
            (
                "tools/local-executors/package.json",
                r#"{"name":"local-executors","builders":"./custom-builders.json"}"#,
            ),
            (
                "tools/local-executors/custom-builders.json",
                r#"{"builders":{"e2e":{"implementation":"./impl/e2e.ts#runE2e","schema":"./impl/schema.json"}}}"#,
            ),
            (
                "tools/local-executors/impl/e2e.ts",
                "export async function runE2e() { return { success: true }; }\n",
            ),
            ("tools/orphan.ts", "export const orphan = 1;\n"),
        ],
    );

    let unused = unused_file_paths(root);
    for path in [
        "tools/dir-plugin/src/plugin.ts",
        "tools/local-executors/impl/e2e.ts",
    ] {
        assert!(
            !unused.contains(&path.to_string()),
            "{path} is loaded by Nx, unused files: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"tools/orphan.ts".to_string()),
        "a file that Nx does not load stays unused: {unused:?}"
    );
}

#[test]
fn nx_local_executor_in_a_workspace_package_resolves_from_the_workspace_root() {
    for app_manifest in [
        r#"{"name":"web","version":"1.0.0"}"#,
        r#"{"name":"web","version":"1.0.0","devDependencies":{"nx":"21.0.0"}}"#,
    ] {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let root = tmp.path();
        write_files(
            root,
            &[
                (
                    "package.json",
                    r#"{"name":"root","private":true,"workspaces":["apps/*","tools/*"],"devDependencies":{"nx":"21.0.0"}}"#,
                ),
                ("nx.json", "{}"),
                ("apps/web/package.json", app_manifest),
                (
                    "apps/web/project.json",
                    r#"{"targets":{"e2e":{"executor":"./tools/local-executors:e2e"}}}"#,
                ),
                ("apps/web/src/main.ts", "export const web = 1;\n"),
                (
                    "tools/local-executors/package.json",
                    r#"{"name":"local-executors","version":"1.0.0","executors":"./custom.json"}"#,
                ),
                (
                    "tools/local-executors/custom.json",
                    r#"{"executors":{"e2e":{"implementation":"./impl/e2e","schema":"./impl/schema.json"}}}"#,
                ),
                (
                    "tools/local-executors/impl/e2e.ts",
                    "export default async function e2e() { return { success: true }; }\n",
                ),
                (
                    "tools/local-executors/orphan.ts",
                    "export const orphan = 1;\n",
                ),
            ],
        );

        let unused = unused_file_paths(root);
        assert!(
            !unused.contains(&"tools/local-executors/impl/e2e.ts".to_string()),
            "{app_manifest}: the local executor is loaded by Nx, unused files: {unused:?}"
        );
        assert!(
            unused.contains(&"tools/local-executors/orphan.ts".to_string()),
            "{app_manifest}: unused files: {unused:?}"
        );
    }
}
