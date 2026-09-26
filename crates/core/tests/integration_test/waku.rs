use std::fs;
use std::path::Path;

use super::common::create_config;
use fallow_types::results::AnalysisResults;

#[test]
fn waku_routes_are_entries_and_only_framework_exports_are_credited() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_waku_app(root, "src");

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);
    let unused_exports = unused_export_names(&results, root);

    for reachable in [
        "src/pages/_root.tsx",
        "src/pages/_layout.tsx",
        "src/pages/index.tsx",
        "src/pages/blog/[slug].tsx",
        "src/pages/_api/hello.ts",
        "src/pages/_slices/sidebar.tsx",
        "src/middleware/cookie.ts",
        "src/pages/_components/header.tsx",
        "src/pages.gen.ts",
        "waku.config.ts",
    ] {
        assert!(
            !unused_files.contains(&reachable.to_string()),
            "{reachable} should be reachable, unused: {unused_files:?}"
        );
    }
    for skipped in [
        "src/pages/_components/orphan.tsx",
        "src/pages/_hooks/orphan.ts",
        "src/pages/_actions/orphan.ts",
    ] {
        assert!(
            unused_files.contains(&skipped.to_string()),
            "the router skips {skipped}, so it stays unused when not imported, unused: {unused_files:?}"
        );
    }

    for credited in [
        "src/pages/index.tsx:default",
        "src/pages/index.tsx:getConfig",
        "src/pages/_api/hello.ts:GET",
        "src/pages/_api/hello.ts:POST",
        "src/middleware/cookie.ts:default",
        "waku.config.ts:default",
    ] {
        assert!(
            !unused_exports.contains(&credited.to_string()),
            "{credited} is read by Waku, unused: {unused_exports:?}"
        );
    }
    for flagged in [
        "src/pages/index.tsx:getconfig",
        "src/pages/index.tsx:GET",
        "src/pages/_api/hello.ts:helper",
        "src/pages/_components/header.tsx:unusedHelper",
        "src/pages/_components/header.tsx:default",
    ] {
        assert!(
            unused_exports.contains(&flagged.to_string()),
            "{flagged} is not a Waku export, unused: {unused_exports:?}"
        );
    }
}

#[test]
fn waku_src_dir_from_config_moves_the_route_root() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_waku_app(root, "app");
    write_file(
        root,
        "waku.config.ts",
        r#"
            import { defineConfig } from "waku/config";
            export default defineConfig({ srcDir: "app" });
        "#,
    );
    write_file(root, "src/pages.gen.ts", "export {};\n");
    write_file(
        root,
        "src/pages/stale.tsx",
        "export default function Stale() { return null; }\n",
    );

    let mut config = create_config(root.to_path_buf());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);
    let unused_exports = unused_export_names(&results, root);

    assert!(
        !unused_files.contains(&"app/pages/index.tsx".to_string()),
        "custom srcDir pages should be reachable, unused: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"app/pages.gen.ts".to_string()),
        "custom srcDir route types should be kept, unused: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/pages.gen.ts".to_string()),
        "a stale default route-types file should be reported, unused: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/pages/stale.tsx".to_string()),
        "the default src/pages root no longer applies, unused: {unused_files:?}"
    );
    assert!(
        !unused_exports.contains(&"app/pages/index.tsx:getConfig".to_string()),
        "custom srcDir route exports should be credited, unused: {unused_exports:?}"
    );
    assert!(
        unused_exports.contains(&"app/pages/index.tsx:getconfig".to_string()),
        "custom srcDir route typos should be flagged, unused: {unused_exports:?}"
    );
}

#[test]
fn waku_pages_need_the_waku_dependency() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_file(root, "package.json", r#"{ "private": true }"#);
    write_file(
        root,
        "src/pages/index.tsx",
        "export default function Home() { return null; }\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_files = unused_file_paths(&results, root);

    assert!(
        unused_files.contains(&"src/pages/index.tsx".to_string()),
        "pages should stay unused without waku, unused: {unused_files:?}"
    );
}

fn write_waku_app(root: &Path, src_dir: &str) {
    write_file(
        root,
        "package.json",
        r#"{
            "private": true,
            "type": "module",
            "dependencies": {
                "react": "latest",
                "waku": "latest"
            }
        }"#,
    );
    write_file(
        root,
        "waku.config.ts",
        r#"
            import { defineConfig } from "waku/config";
            export default defineConfig({});
        "#,
    );
    let files = [
        (
            "pages/_root.tsx",
            "export default function Root({ children }) { return children; }\n",
        ),
        (
            "pages/_layout.tsx",
            r#"
                import { Header } from "./_components/header";
                export default function Layout({ children }) { return [Header(), children]; }
                export const getConfig = async () => ({ render: "static" });
            "#,
        ),
        (
            "pages/index.tsx",
            r#"
                export default function Home() { return null; }
                export const getConfig = async () => ({ render: "dynamic" });
                export const getconfig = 1;
                export const GET = () => null;
            "#,
        ),
        (
            "pages/blog/[slug].tsx",
            "export default function Post() { return null; }\n",
        ),
        (
            "pages/_api/hello.ts",
            r#"
                export const GET = () => new Response("hi");
                export const POST = () => new Response("hi");
                export const helper = 1;
            "#,
        ),
        (
            "pages/_slices/sidebar.tsx",
            "export default function Sidebar() { return null; }\n",
        ),
        (
            "pages/_components/header.tsx",
            r"
                export function Header() { return null; }
                export const unusedHelper = 1;
                export default function HeaderDefault() { return null; }
            ",
        ),
        (
            "pages/_components/orphan.tsx",
            "export function Orphan() { return null; }\n",
        ),
        (
            "pages/_hooks/orphan.ts",
            "export default function useOrphan() { return null; }\n",
        ),
        (
            "pages/_actions/orphan.ts",
            "export default async function orphanAction() { return null; }\n",
        ),
        (
            "middleware/cookie.ts",
            "export default () => async (_ctx, next) => next();\n",
        ),
        ("pages.gen.ts", "export {};\n"),
    ];
    for (relative, contents) in files {
        write_file(root, &format!("{src_dir}/{relative}"), contents);
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
