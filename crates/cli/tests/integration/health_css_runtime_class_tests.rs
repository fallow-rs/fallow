//! Classes that code adds at runtime count as references for
//! `unreferenced-css-class` (issue #5272): a string constant or literal in
//! project code, the static head of a template literal, and the classes that a
//! declared dependency or framework theme renders.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

fn unreferenced_classes(root: &Path) -> Vec<(String, String)> {
    let output = run_fallow_in_root("health", root, &["--css", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let mut rows: Vec<(String, String)> = json["css_analytics"]["unreferenced_css_classes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|row| {
            (
                row["path"].as_str().unwrap_or_default().replace('\\', "/"),
                row["class"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    rows.sort();
    rows
}

#[test]
fn classes_from_code_constants_literals_and_template_heads_are_credited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"table-editor","private":true}"#,
    );
    write(
        root,
        "src/table.css",
        ".table-shell { display: grid; }\n\
         .table-row-drag-marker { opacity: 0.5; }\n\
         .selectedCell-border-top { border-top: 1px solid; }\n\
         .cell-edge-left { border-left: 1px solid; }\n\
         .never-applied { color: red; }\n\
         .user-card { color: blue; }\n",
    );
    write(
        root,
        "src/markers.ts",
        "export const ROW_DRAG_MARKER_CLASS = \"table-row-drag-marker\";\n\
         // The element ids below are not classes.\n\
         export const CARD_ID = 'user-card';\n",
    );
    write(
        root,
        "src/plugin.ts",
        "import { ROW_DRAG_MARKER_CLASS } from './markers';\n\
         export const createMarker = (): HTMLElement => {\n\
         \x20 const el = document.createElement('div');\n\
         \x20 el.className = `${ROW_DRAG_MARKER_CLASS} hidden`;\n\
         \x20 return el;\n\
         };\n\
         export const borderClasses = (top: boolean): string[] => {\n\
         \x20 const classes: string[] = [];\n\
         \x20 if (top) classes.push(\"selectedCell-border-top\");\n\
         \x20 return classes;\n\
         };\n\
         export const edgeClass = (side: string): string => `cell-edge-${side}`;\n",
    );
    write(
        root,
        "src/App.tsx",
        "import './table.css';\n\
         export const App = () => <div className=\"table-shell\" />;\n",
    );

    assert_eq!(
        unreferenced_classes(root),
        vec![
            ("src/table.css".to_owned(), "never-applied".to_owned()),
            ("src/table.css".to_owned(), "user-card".to_owned()),
        ],
        "a class from a class constant, a pushed literal, or a template head is credited; \
         a literal in a statement that names no class is not"
    );
}

#[test]
fn classes_that_a_declared_dependency_or_theme_renders_are_credited() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"mono","private":true,"workspaces":["packages/*"]}"#,
    );

    write(
        root,
        "packages/editor/package.json",
        r#"{"name":"editor","dependencies":{"@tiptap/pm":"^3.0.0","@tiptap/react":"^3.0.0","@tiptap/extensions":"^3.0.0"}}"#,
    );
    write(
        root,
        "packages/editor/src/editor.css",
        ".editor-shell { display: block; }\n\
         .ProseMirror-selectednode { outline: 1px solid; }\n\
         .selectedCell { background: blue; }\n\
         .column-resize-handle { width: 2px; }\n\
         .is-editor-empty::before { content: attr(data-placeholder); }\n\
         .node-image { display: inline; }\n\
         .editor-unused { color: red; }\n",
    );
    write(
        root,
        "packages/editor/src/Editor.tsx",
        "import './editor.css';\n\
         export const Editor = () => <div className=\"editor-shell\" />;\n",
    );

    write(
        root,
        "packages/docs/package.json",
        r#"{"name":"docs","dependencies":{"@docusaurus/core":"^3.0.0","@docusaurus/preset-classic":"^3.0.0"}}"#,
    );
    write(
        root,
        "packages/docs/src/css/custom.css",
        ".docs-hero { padding: 2rem; }\n\
         .menu__link { font-weight: 600; }\n\
         .breadcrumbs__link { color: gray; }\n\
         .table-of-contents__link { color: gray; }\n\
         .main-wrapper { min-height: 100vh; }\n\
         .docs-unused { color: red; }\n",
    );
    write(
        root,
        "packages/docs/src/pages/index.tsx",
        "import '../css/custom.css';\n\
         export default function Home() { return <header className=\"docs-hero\" />; }\n",
    );

    write(
        root,
        "packages/web/package.json",
        r#"{"name":"web","dependencies":{"maplibre-gl":"^5.0.0","@bprogress/core":"^1.0.0"}}"#,
    );
    write(
        root,
        "packages/web/src/app.css",
        ".web-shell { display: flex; }\n\
         .maplibregl-popup-tip { display: none; }\n\
         .bprogress { z-index: 10; }\n\
         .menu__link { color: red; }\n\
         .web-unused { color: red; }\n",
    );
    write(
        root,
        "packages/web/src/App.tsx",
        "import './app.css';\n\
         export const App = () => <main className=\"web-shell\" />;\n",
    );

    assert_eq!(
        unreferenced_classes(root),
        vec![
            (
                "packages/docs/src/css/custom.css".to_owned(),
                "docs-unused".to_owned()
            ),
            (
                "packages/editor/src/editor.css".to_owned(),
                "editor-unused".to_owned()
            ),
            (
                "packages/web/src/app.css".to_owned(),
                "menu__link".to_owned()
            ),
            (
                "packages/web/src/app.css".to_owned(),
                "web-unused".to_owned()
            ),
        ],
        "a dependency or theme class is credited only in the workspace that declares it"
    );
}
