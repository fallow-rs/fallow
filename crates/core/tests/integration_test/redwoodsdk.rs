use std::fs;
use std::path::Path;

use super::common::create_config;
use fallow_types::results::AnalysisResults;

#[test]
fn redwoodsdk_worker_entrypoint_is_scoped_to_rwsdk_workspace() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_redwoodsdk_monorepo(root);

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_paths = unused_file_paths(&results, root);

    assert!(
        !unused_paths.contains(&"apps/website/src/worker.tsx".to_string()),
        "RedwoodSDK worker should be reachable, unused: {unused_paths:?}"
    );
    assert!(
        !unused_paths.contains(&"apps/website/src/pages/Home.tsx".to_string()),
        "worker-imported page should be reachable, unused: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"apps/website/src/orphan.ts".to_string()),
        "same-workspace orphan should still report as unused, unused: {unused_paths:?}"
    );
    assert!(
        unused_paths.contains(&"apps/admin/src/worker.tsx".to_string()),
        "plain Vite sibling worker should not inherit RedwoodSDK reachability, unused: {unused_paths:?}"
    );
}

#[test]
fn plain_vite_worker_does_not_activate_redwoodsdk() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(
        root,
        "package.json",
        r#"{
            "private": true,
            "devDependencies": {
                "vite": "latest"
            }
        }"#,
    );
    write_file(root, "src/worker.tsx", "export default {};\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_paths = unused_file_paths(&results, root);

    assert!(
        unused_paths.contains(&"src/worker.tsx".to_string()),
        "plain Vite worker should remain unused without rwsdk, unused: {unused_paths:?}"
    );
}

fn write_redwoodsdk_monorepo(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{
            "private": true,
            "workspaces": ["apps/*"]
        }"#,
    );
    write_file(
        root,
        "apps/website/package.json",
        r#"{
            "name": "@repo/website",
            "private": true,
            "dependencies": {
                "rwsdk": "latest"
            },
            "devDependencies": {
                "@cloudflare/vite-plugin": "latest",
                "vite": "latest"
            }
        }"#,
    );
    write_file(
        root,
        "apps/website/vite.config.mts",
        r#"
            import { cloudflare } from "@cloudflare/vite-plugin";
            import { defineConfig } from "vite";
            import { redwood } from "rwsdk/vite";

            export default defineConfig({
                plugins: [
                    cloudflare({ viteEnvironment: { name: "worker" } }),
                    redwood(),
                ],
            });
        "#,
    );
    write_file(
        root,
        "apps/website/src/worker.tsx",
        r#"
            import { defineApp } from "rwsdk/worker";
            import { home } from "./pages/Home";

            export default defineApp([
                home,
            ]);
        "#,
    );
    write_file(
        root,
        "apps/website/src/pages/Home.tsx",
        r#"
            export function home() {
                return "home";
            }
        "#,
    );
    write_file(
        root,
        "apps/website/src/orphan.ts",
        "export const orphan = true;\n",
    );
    write_file(
        root,
        "apps/admin/package.json",
        r#"{
            "name": "@repo/admin",
            "private": true,
            "devDependencies": {
                "vite": "latest"
            }
        }"#,
    );
    write_file(root, "apps/admin/src/worker.tsx", "export default {};\n");
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}
