use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ModuleInfo, VisibilityTag};

use crate::parse_source_to_module;

/// Parse as a .js file (not .tsx) to test JSX retry fallback logic.
fn parse_as_js(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("component.js"), source, 0, false)
}

#[test]
fn jsx_retry_extracts_exports_from_js_with_jsx() {
    let source = r#"
export const App = () => <div className="app"><span>Hello World from JSX in a plain JS file</span></div>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract the App export from .js file with JSX"
    );
}

#[test]
fn jsx_retry_extracts_imports_from_js_with_jsx() {
    let source = r#"
export default function Component() {
    return <main><section className="hero"><h1>Title</h1><p>Description paragraph</p></section></main>;
}
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract the default export from .js file with JSX"
    );
}

#[test]
fn jsx_retry_preserves_jsdoc_public_tag() {
    let source = r#"
/** @public */
export const Button = ({ children }) => <button className="btn">{children}</button>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.exports.is_empty(),
        "JSX retry should extract Button export"
    );
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::Public,
        "@public JSDoc tag must be recognized on JSX exports in .js files"
    );
}

#[test]
fn jsx_retry_preserves_suppressions() {
    let source = r#"
// fallow-ignore-next-line unused-export
export const Unused = ({ text }) => <span className="unused-component">{text}</span>;
"#;
    let info = parse_as_js(source);
    assert!(
        !info.suppressions.is_empty(),
        "Suppressions must be parsed from retry parse comments, not the original failed parse"
    );
}

#[test]
fn jsx_in_js_file_retry_extracts_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("component.js"),
        r"import React from 'react';
import { Button } from './Button';

const App = () => <Button>Hello</Button>;
export default App;",
        0,
        false,
    );
    assert!(
        info.imports.iter().any(|i| i.source == "react"),
        "JSX retry should extract imports from JSX in .js file"
    );
    assert!(
        info.imports.iter().any(|i| i.source == "./Button"),
        "JSX retry should extract all imports"
    );
}

#[test]
fn jsx_retry_parses_a_small_js_file_with_jsx() {
    let source = "export default function Page() {\n  return <p>Hello</p>\n}\n";
    assert!(source.len() <= 100, "the fixture must stay a small file");
    let info = parse_source_to_module(FileId(0), Path::new("app/page.js"), source, 0, false);
    assert_eq!(
        info.exports.len(),
        1,
        "the default export must be extracted"
    );
    assert_eq!(
        info.parse_error_count, 0,
        "a small .js file with JSX must parse without errors"
    );
}

#[test]
fn top_level_return_in_commonjs_js_parses_without_errors() {
    let source = "const { app } = require('electron');\nif (!app) {\n  return;\n}\nmodule.exports = { app };\n";
    let info = parse_source_to_module(FileId(0), Path::new("main.js"), source, 0, false);
    assert_eq!(
        info.parse_error_count, 0,
        "a top-level return in a CommonJS .js file is valid"
    );
    assert!(
        info.require_calls
            .iter()
            .any(|call| call.source == "electron"),
        "the require call must be extracted"
    );
}

#[test]
fn jsx_retry_keeps_top_level_return_valid_in_js_file() {
    let source = "const el = <div />;\nif (!el) {\n  return;\n}\nexport const z = el;\n";
    let info = parse_source_to_module(FileId(0), Path::new("main.js"), source, 0, false);
    assert_eq!(
        info.parse_error_count, 0,
        "the JSX retry of a .js file must allow a top-level return"
    );
    assert_eq!(info.exports.len(), 1, "the export must be extracted");
}
