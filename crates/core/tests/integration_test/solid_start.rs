use std::fs;
use std::path::Path;

use super::common::create_config;
use fallow_types::results::AnalysisResults;

#[test]
fn solid_start_entries_and_routes_are_reachable() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_solid_start_app(root, "src", "routes");
    write_file(
        root,
        "vite.config.ts",
        r#"
            import { defineConfig } from "vite";
            import { solidStart } from "@solidjs/start/config";
            export default defineConfig({ plugins: [solidStart()] });
        "#,
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);
    let unused_exports = unused_export_names(&results, root);

    for reachable in [
        "src/app.tsx",
        "src/entry-client.tsx",
        "src/entry-server.tsx",
        "src/routes/index.tsx",
        "src/routes/blog/[slug].tsx",
        "src/routes/api/health.ts",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} should be reachable, unused: {unused_files:?}"
        );
    }
    for unused in ["src/orphan.ts", "src/middleware.ts"] {
        assert!(
            unused_files.contains(&unused.to_string()),
            "SolidStart loads middleware only through the `middleware` option, \
             so {unused} stays unused: {unused_files:?}"
        );
    }

    for credited in [
        "src/app.tsx:default",
        "src/entry-server.tsx:default",
        "src/routes/index.tsx:default",
        "src/routes/index.tsx:route",
        "src/routes/api/health.ts:GET",
        "src/routes/api/health.ts:POST",
    ] {
        assert!(
            !unused_exports.contains(&credited.to_string()),
            "{credited} is read by SolidStart, unused: {unused_exports:?}"
        );
    }
    assert!(
        unused_exports.contains(&"src/routes/index.tsx:helper".to_string()),
        "a route export that SolidStart does not read stays reportable: {unused_exports:?}"
    );
}

#[test]
fn solid_start_options_move_the_app_root_and_route_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_solid_start_app(root, "app", "pages");
    write_file(
        root,
        "vite.config.ts",
        r#"
            import { defineConfig } from "vite";
            import { solidStart } from "@solidjs/start/config";
            export default defineConfig({
                plugins: [
                    solidStart({
                        appRoot: "./app",
                        routeDir: "./pages",
                        middleware: "./server/mw.ts",
                    }),
                ],
            });
        "#,
    );
    write_file(root, "server/mw.ts", "export default { onRequest: [] };\n");
    write_file(
        root,
        "src/routes/stale.tsx",
        "export default function Stale() { return null; }\n",
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);

    for reachable in [
        "app/app.tsx",
        "app/entry-client.tsx",
        "app/entry-server.tsx",
        "app/pages/index.tsx",
        "app/pages/blog/[slug].tsx",
        "server/mw.ts",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} should be reachable, unused: {unused_files:?}"
        );
    }
    assert!(
        unused_files.contains(&"src/routes/stale.tsx".to_string()),
        "a custom appRoot replaces the src defaults: {unused_files:?}"
    );
}

#[test]
fn solid_start_v1_app_config_sets_the_app_root() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_solid_start_app(root, "web", "routes");
    write_file(
        root,
        "app.config.ts",
        r#"
            import { defineConfig } from "@solidjs/start/config";
            export default defineConfig({ appRoot: "./web" });
        "#,
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);

    for reachable in [
        "web/app.tsx",
        "web/entry-client.tsx",
        "web/entry-server.tsx",
        "web/routes/index.tsx",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} should be reachable, unused: {unused_files:?}"
        );
    }
}

fn write_solid_start_app(root: &Path, app_root: &str, route_dir: &str) {
    write_file(
        root,
        "package.json",
        r#"{
            "private": true,
            "type": "module",
            "dependencies": {
                "@solidjs/start": "latest",
                "solid-js": "latest"
            },
            "devDependencies": { "vite": "latest" }
        }"#,
    );
    let files = [
        (
            "app.tsx".to_string(),
            "export default function App() { return null; }\n",
        ),
        (
            "entry-client.tsx".to_string(),
            "import { mount } from \"@solidjs/start/client\";\nmount(() => null, document.body);\n",
        ),
        (
            "entry-server.tsx".to_string(),
            "import { createHandler } from \"@solidjs/start/server\";\nexport default createHandler(() => null);\n",
        ),
        (
            "middleware.ts".to_string(),
            "export default { onRequest: [] };\n",
        ),
        (
            format!("{route_dir}/index.tsx"),
            "export const route = { preload() {} };\nexport const helper = 1;\nexport default function Home() { return null; }\n",
        ),
        (
            format!("{route_dir}/blog/[slug].tsx"),
            "export default function Post() { return null; }\n",
        ),
        (
            format!("{route_dir}/api/health.ts"),
            "export function GET() { return new Response(\"ok\"); }\nexport function POST() { return new Response(\"ok\"); }\n",
        ),
        ("orphan.ts".to_string(), "export const orphan = 1;\n"),
    ];
    for (relative, contents) in files {
        write_file(root, &format!("{app_root}/{relative}"), contents);
    }
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
