//! `` html`...` `` tagged template literal asset extraction tests.
//!
//! Mirrors the HTML parser and the JSX `<script src>` / `<link href>` override
//! for SSR helpers like `hono/html`, `lit-html`, and `htm`, where layout
//! components emit HTML via a tagged template whose tag is the identifier
//! `html`. See issue #105 (till's follow-up comment).

use fallow_types::extract::ImportedName;

use crate::tests::parse_ts;

#[test]
fn html_tagged_template_script_src_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = ({ title, body }) => html`
  <!doctype html>
  <html>
    <head>
      <title>${title}</title>
      <script defer src="/static/otp-input.js"></script>
    </head>
    <body>${body}</body>
  </html>
`;"#,
    );
    let imp = info
        .imports
        .iter()
        .find(|i| i.source == "/static/otp-input.js")
        .expect("html`` <script src> should produce an ImportInfo");
    assert!(matches!(imp.imported_name, ImportedName::SideEffect));
    assert!(imp.local_name.is_empty());
    assert!(!imp.is_type_only);
}

#[test]
fn html_tagged_template_link_stylesheet_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="stylesheet" href="/static/global.css" />
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/global.css")
    );
}

#[test]
fn html_tagged_template_link_modulepreload_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="modulepreload" href="/static/vendor.js" />
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "/static/vendor.js"));
}

#[test]
fn html_tagged_template_link_reversed_attr_order_extracted() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link href="./style.css" rel="stylesheet" />
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./style.css"));
}

#[test]
fn html_tagged_template_bare_src_normalized() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="app.js"></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().any(|i| i.source == "./app.js"));
}

#[test]
fn html_tagged_template_multiple_assets() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="stylesheet" href="/static/global.css" />
    <link rel="modulepreload" href="/static/vendor.js" />
    <script src="/static/app.js"></script>
  </head>
`;"#,
    );
    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(sources.contains(&"/static/global.css"));
    assert!(sources.contains(&"/static/vendor.js"));
    assert!(sources.contains(&"/static/app.js"));
}

#[test]
fn html_tagged_template_remote_urls_skipped() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="https://cdn.example.com/lib.js"></script>
    <link rel="stylesheet" href="//cdn.example.com/style.css" />
    <script src="http://example.com/legacy.js"></script>
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("example.com"))
    );
}

#[test]
fn html_tagged_template_multi_line_attributes() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <link
    rel="stylesheet"
    href="/static/multi-line.css"
  />
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/multi-line.css")
    );
}

#[test]
fn html_tagged_template_comments_stripped() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <!-- <script src="/static/old.js"></script> -->
    <script src="/static/new.js"></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().all(|i| i.source != "/static/old.js"));
    assert!(info.imports.iter().any(|i| i.source == "/static/new.js"));
}

#[test]
fn html_tagged_template_interpolated_asset_across_boundary_skipped() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
const base = "/static";
export const Layout = () => html`
  <script src="${base}/app.js"></script>
`;"#,
    );
    assert!(info.imports.iter().all(|i| !i.source.ends_with("/app.js")));
}

#[test]
fn html_tagged_template_rel_icon_ignored() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <link rel="icon" href="/static/favicon.ico" />
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("favicon.ico"))
    );
}

#[test]
fn non_html_tag_ignored() {
    let info = parse_ts(
        r#"const css = (strings: TemplateStringsArray, ...values: unknown[]) => "";
const style = css`
  <script src="./should-not-track.js"></script>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .all(|i| !i.source.contains("should-not-track"))
    );
}

#[test]
fn html_tagged_template_in_jsx_file_also_works() {
    let info = crate::tests::parse_tsx(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src="/static/from-tsx.js"></script>
  </head>
`;"#,
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "/static/from-tsx.js")
    );
}

#[test]
fn html_tagged_template_empty_src_ignored() {
    let info = parse_ts(
        r#"import { html } from "hono/html";
export const Layout = () => html`
  <head>
    <script src=""></script>
  </head>
`;"#,
    );
    assert!(info.imports.iter().all(|i| !i.source.is_empty()));
}

#[test]
fn lit_custom_element_decorator_records_registered_tag() {
    let info = parse_ts(
        r#"import { LitElement, html } from "lit";
import { customElement } from "lit/decorators.js";
@customElement("my-element")
export class MyElement extends LitElement {
  render() {
    return html`<div></div>`;
  }
}"#,
    );
    let reg = info
        .registered_custom_elements
        .iter()
        .find(|r| r.tag == "my-element")
        .expect("my-element registered");
    assert_eq!(reg.class_local_name, "MyElement");
}

#[test]
fn custom_elements_define_records_registered_tag() {
    let info = parse_ts(
        r#"class XFoo extends HTMLElement {}
customElements.define("x-foo", XFoo);"#,
    );
    let tags: Vec<&str> = info
        .registered_custom_elements
        .iter()
        .map(|r| r.tag.as_str())
        .collect();
    assert!(tags.contains(&"x-foo"), "registered: {tags:?}");
}

#[test]
fn html_template_records_used_custom_element_tags_excluding_native() {
    let info = parse_ts(
        r#"import { html } from "lit";
export const tpl = () => html`<my-card><span>x</span><other-el></other-el></my-card>`;"#,
    );
    assert!(
        info.used_custom_element_tags
            .contains(&"my-card".to_string()),
        "{:?}",
        info.used_custom_element_tags
    );
    assert!(
        info.used_custom_element_tags
            .contains(&"other-el".to_string()),
        "{:?}",
        info.used_custom_element_tags
    );
    assert!(
        !info.used_custom_element_tags.contains(&"span".to_string()),
        "a native (non-hyphenated) tag must not be recorded: {:?}",
        info.used_custom_element_tags
    );
}

#[test]
fn document_create_element_credits_custom_element_tag() {
    let info = parse_ts(r#"document.body.appendChild(document.createElement("x-foo"));"#);
    assert!(
        info.used_custom_element_tags.contains(&"x-foo".to_string()),
        "createElement should credit the tag as rendered: {:?}",
        info.used_custom_element_tags
    );
    // A native (non-hyphenated) createElement is not a custom element.
    let native = parse_ts(r#"document.createElement("div");"#);
    assert!(native.used_custom_element_tags.is_empty());
}

#[test]
fn dynamic_html_tag_records_dynamic_sentinel() {
    let info = parse_ts(
        r#"import { html } from "lit";
export const render = (tag) => html`<${tag}></${tag}>`;"#,
    );
    assert!(
        info.used_custom_element_tags
            .contains(&"<dynamic>".to_string()),
        "a `<${{tag}}>` dynamic render must record the dynamic sentinel: {:?}",
        info.used_custom_element_tags
    );
}
