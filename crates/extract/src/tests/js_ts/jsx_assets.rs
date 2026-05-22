//! JSX resource attribute ignore tests.
//!
//! Generic JSX and TSX `<script src>` and `<link href>` literals are runtime
//! HTML attributes, not JavaScript module specifiers. They should not create
//! `ImportInfo` records by default. HTML files and the bare `html` tagged
//! template scanner keep their own asset extraction paths.

use fallow_types::extract::ImportedName;

use crate::tests::parse_tsx;

fn import_sources(source: &str) -> Vec<String> {
    parse_tsx(source)
        .imports
        .into_iter()
        .map(|import| import.source)
        .collect()
}

#[test]
fn jsx_script_src_string_literal_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <html>
            <head>
              <script src="./app.js"></script>
            </head>
          </html>
        );"#,
    );

    assert!(!sources.contains(&"./app.js".to_string()));
}

#[test]
fn jsx_link_stylesheet_href_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <html>
            <head>
              <link rel="stylesheet" href="./global.css" />
            </head>
          </html>
        );"#,
    );

    assert!(!sources.contains(&"./global.css".to_string()));
}

#[test]
fn jsx_link_modulepreload_href_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <link rel="modulepreload" href="./vendor.js" />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./vendor.js".to_string()));
}

#[test]
fn jsx_link_reversed_attr_order_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <link href="./style.css" rel="stylesheet" />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./style.css".to_string()));
}

#[test]
fn jsx_root_relative_href_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <link rel="stylesheet" href="/static/style.css" />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"/static/style.css".to_string()));
}

#[test]
fn jsx_bare_script_src_not_normalized_to_import() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <script src="app.js"></script>
          </head>
        );"#,
    );

    assert!(!sources.contains(&"app.js".to_string()));
    assert!(!sources.contains(&"./app.js".to_string()));
}

#[test]
fn jsx_multiple_resource_attributes_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <html>
            <head>
              <link rel="stylesheet" href="/static/global.css" />
              <link rel="modulepreload" href="/static/vendor.js" />
              <script src="/static/app.js"></script>
            </head>
          </html>
        );"#,
    );

    assert!(!sources.contains(&"/static/global.css".to_string()));
    assert!(!sources.contains(&"/static/vendor.js".to_string()));
    assert!(!sources.contains(&"/static/app.js".to_string()));
}

#[test]
fn tsx_static_imports_still_extracted() {
    let info = parse_tsx(
        r#"import "./real.css";
        import { helper } from "./real";

        export const Layout = () => (
          <head>
            <link rel="stylesheet" href="./global.css" />
            <script src="./app.js"></script>
            <span>{helper()}</span>
          </head>
        );"#,
    );

    assert!(
        info.imports
            .iter()
            .any(|import| import.source == "./real.css"
                && matches!(import.imported_name, ImportedName::SideEffect))
    );
    assert!(info.imports.iter().any(|import| import.source == "./real"
        && matches!(&import.imported_name, ImportedName::Named(name) if name == "helper")));
    assert!(
        info.imports
            .iter()
            .all(|import| import.source != "./global.css")
    );
    assert!(
        info.imports
            .iter()
            .all(|import| import.source != "./app.js")
    );
}

#[test]
fn jsx_script_src_remote_http_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <script src="https://cdn.example.com/lib.js"></script>
          </head>
        );"#,
    );

    assert!(
        !sources
            .iter()
            .any(|source| source.contains("cdn.example.com"))
    );
}

#[test]
fn jsx_script_src_protocol_relative_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <script src="//cdn.example.com/lib.js"></script>
          </head>
        );"#,
    );

    assert!(
        !sources
            .iter()
            .any(|source| source.contains("cdn.example.com"))
    );
}

#[test]
fn jsx_link_icon_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <link rel="icon" href="./favicon.ico" />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./favicon.ico".to_string()));
}

#[test]
fn jsx_link_preload_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <link rel="preload" href="./font.woff2" as="font" />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./font.woff2".to_string()));
}

#[test]
fn jsx_script_src_expression_container_ignored() {
    let sources = import_sources(
        r#"const src = "./dynamic.js";
        export const Layout = () => (
          <head>
            <script src={src}></script>
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./dynamic.js".to_string()));
}

#[test]
fn jsx_link_href_expression_container_ignored() {
    let sources = import_sources(
        r#"const css = "./dynamic.css";
        export const Layout = () => (
          <head>
            <link rel="stylesheet" href={css} />
          </head>
        );"#,
    );

    assert!(!sources.contains(&"./dynamic.css".to_string()));
}

#[test]
fn jsx_capitalized_component_props_ignored() {
    let sources = import_sources(
        r#"import { Script, Link } from 'some-lib';
        export const Layout = () => (
          <>
            <Script src="./should-not-be-tracked.js" />
            <Link rel="stylesheet" href="./should-not-be-tracked.css" />
          </>
        );"#,
    );

    assert!(!sources.contains(&"./should-not-be-tracked.js".to_string()));
    assert!(!sources.contains(&"./should-not-be-tracked.css".to_string()));
    assert!(sources.contains(&"some-lib".to_string()));
}

#[test]
fn jsx_script_without_src_ignored() {
    let sources = import_sources(
        "export const Layout = () => (
          <head>
            <script>{`console.log('inline');`}</script>
          </head>
        );",
    );

    assert!(!sources.iter().any(|source| source.contains("inline")));
}

#[test]
fn jsx_empty_src_ignored() {
    let sources = import_sources(
        r#"export const Layout = () => (
          <head>
            <script src=""></script>
          </head>
        );"#,
    );

    assert!(sources.iter().all(|source| !source.is_empty()));
}
