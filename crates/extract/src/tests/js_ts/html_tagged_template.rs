//! `` html`...` `` tagged template literal asset extraction tests.
//!
//! Mirrors the HTML parser and the JSX `<script src>` / `<link href>` override
//! for SSR helpers like `hono/html`, `lit-html`, and `htm`, where layout
//! components emit HTML via a tagged template whose tag is the identifier
//! `html`. See issue #105 (till's follow-up comment).

use fallow_types::extract::{ImportedName, SemanticFact};

use crate::tests::parse_ts;

const DYNAMIC_CUSTOM_ELEMENT_TAG: &str = "<dynamic>";

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
fn dynamic_html_tag_records_typed_dynamic_render_fact() {
    let info = parse_ts(
        r#"import { html } from "lit";
export const render = (tag) => html`<${tag}></${tag}>`;"#,
    );
    assert!(
        info.semantic_facts
            .iter()
            .any(|fact| matches!(fact, SemanticFact::DynamicCustomElementRender(_))),
        "a `<${{tag}}>` dynamic render must record a typed semantic fact: {:?}",
        info.semantic_facts
    );
    assert!(
        !info
            .used_custom_element_tags
            .contains(&DYNAMIC_CUSTOM_ELEMENT_TAG.to_string()),
        "new extraction must not persist the legacy dynamic sentinel: {:?}",
        info.used_custom_element_tags
    );
}

const PROJECT_TAG_FIXTURE: &str = r#"export const fixture = {
  'index.html': html`
    <script type="module" src="./src/main.js"></script>
    <link rel="stylesheet" href="./src/index.css" />
  `,
}"#;

fn asset_sources(info: &fallow_types::extract::ModuleInfo) -> Vec<&str> {
    info.imports
        .iter()
        .map(|import| import.source.as_str())
        .filter(|source| source.starts_with("./src/"))
        .collect()
}

#[test]
fn html_tag_imported_from_project_file_is_plain_text() {
    let info = parse_ts(&format!(
        "import {{ html }} from './utils'\n{PROJECT_TAG_FIXTURE}"
    ));
    assert!(
        asset_sources(&info).is_empty(),
        "{:?}",
        asset_sources(&info)
    );
}

#[test]
fn html_tag_declared_in_module_is_plain_text() {
    for declaration in [
        "const html = String.raw",
        "export const html = String.raw",
        "function html(strings) { return strings.join('') }",
        "const html = require('./utils').html",
        "const { html } = require('../utils')",
    ] {
        let info = parse_ts(&format!("{declaration}\n{PROJECT_TAG_FIXTURE}"));
        assert!(
            asset_sources(&info).is_empty(),
            "{declaration}: {:?}",
            asset_sources(&info)
        );
    }
}

#[test]
fn html_tag_from_package_is_markup() {
    for declaration in [
        "import { html } from 'lit'",
        "import { html } from 'hono/html'",
        "const { html } = require('lit-html')",
    ] {
        let info = parse_ts(&format!("{declaration}\n{PROJECT_TAG_FIXTURE}"));
        assert_eq!(
            asset_sources(&info),
            vec!["./src/main.js", "./src/index.css"],
            "{declaration}"
        );
    }
}

#[test]
fn html_tagged_template_asset_span_is_the_quoted_attribute_value() {
    let source = r#"import { html } from 'hono/html'

export const layout = () => html`
  <!-- <script src="./old.js"></script> -->
  <script type="module" src="./src/main.js"></script>
  <link rel="stylesheet" href="./src/index.css" />
`"#;
    let info = parse_ts(source);
    for specifier in ["./src/main.js", "./src/index.css"] {
        let import = info
            .imports
            .iter()
            .find(|import| import.source == specifier)
            .expect("asset import");
        let quoted = format!("\"{specifier}\"");
        let span = import.source_span;
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            quoted,
            "{specifier}"
        );
        assert_eq!(import.span, span, "{specifier}");
    }
}

#[test]
fn html_tagged_template_with_escape_anchors_asset_at_template_text() {
    let source = "import { html } from 'hono/html'\nexport const layout = () => html`\\u0020<script src=\"./app.js\"></script>`";
    let info = parse_ts(source);
    let import = info
        .imports
        .iter()
        .find(|import| import.source == "./app.js")
        .expect("asset import");
    let template_text = source.find("\\u0020").expect("template text") as u32;
    assert_eq!(import.span.start, template_text);
    assert!(import.source_span.is_empty());
}

#[test]
fn html_tagged_template_with_crlf_anchors_asset_at_template_text() {
    // The parser turns CRLF into LF in the raw template text, so the offsets
    // of that text do not match the source after the first CRLF.
    let source = "import { html } from 'hono/html'\r\nexport const layout = () => html`\r\n\r\n\r\n\r\n<script src=\"./a.js\"></script>`";
    let info = parse_ts(source);
    let import = info
        .imports
        .iter()
        .find(|import| import.source == "./a.js")
        .expect("asset import");
    let template_text = (source.find('`').expect("template") + 1) as u32;
    assert_eq!(import.span.start, template_text);
    assert!(import.source_span.is_empty());
}
