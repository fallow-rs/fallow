use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ImportedName, ModuleInfo};

use crate::parse::parse_source_to_module;

fn parse_css(source: &str, filename: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, false)
}

fn parse_css_module(source: &str) -> ModuleInfo {
    parse_source_to_module(
        FileId(0),
        Path::new("Component.module.css"),
        source,
        0,
        false,
    )
}

fn parse_css_non_module(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("styles.css"), source, 0, false)
}

#[test]
fn extracts_css_import_quoted() {
    let info = parse_css(r#"@import "./reset.css";"#, "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./reset.css");
    assert_eq!(info.imports[0].imported_name, ImportedName::SideEffect);
}

#[test]
fn css_import_spans_point_at_source_specifier() {
    let source = "\n\n@import \"./reset.css\";\n";
    let info = parse_css(source, "styles.css");
    let import = &info.imports[0];
    let (line, _col) = fallow_types::extract::byte_offset_to_line_col(
        &info.line_offsets,
        import.source_span.start,
    );
    assert_eq!(line, 3);
    assert_eq!(
        &source[import.source_span.start as usize..import.source_span.end as usize],
        "./reset.css"
    );
}

#[test]
fn extracts_css_import_single_quoted() {
    let info = parse_css("@import './variables.css';", "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./variables.css");
}

#[test]
fn extracts_css_import_url() {
    let info = parse_css(r#"@import url("./base.css");"#, "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./base.css");
}

#[test]
fn extracts_css_import_url_single_quoted() {
    let info = parse_css("@import url('./base.css');", "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./base.css");
}

#[test]
fn extracts_css_import_url_unquoted() {
    let info = parse_css("@import url(./base.css);", "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./base.css");
}

#[test]
fn extracts_multiple_css_imports() {
    let info = parse_css(
        r#"
@import "./reset.css";
@import "./variables.css";
@import url("./base.css");
"#,
        "styles.css",
    );
    assert_eq!(info.imports.len(), 3);
    assert_eq!(info.imports[0].source, "./reset.css");
    assert_eq!(info.imports[1].source, "./variables.css");
    assert_eq!(info.imports[2].source, "./base.css");
}

#[test]
fn extracts_css_import_tailwind_package() {
    let info = parse_css(r#"@import "tailwindcss";"#, "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "tailwindcss");
}

#[test]
fn extracts_css_package_subpath_import_as_bare() {
    let info = parse_css(
        r#"@import "tailwindcss/theme.css" layer(theme);"#,
        "styles.css",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "tailwindcss/theme.css");
}

#[test]
fn scss_import_without_dot_slash_normalized() {
    let info = parse_css("@import 'app.scss';", "index.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./app.scss");
}

#[test]
fn scss_import_bare_extensionless_normalized_to_relative() {
    // In SCSS, extensionless imports are partial references (local files),
    // not npm packages. They get ./ prepended so the resolver can try
    // the SCSS partial (_filename) convention. Actual npm packages will
    // fall through the partial fallback to npm classification in the resolver.
    let info = parse_css(r#"@import "some-package";"#, "styles.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./some-package");
}

#[test]
fn scss_builtin_module_stays_bare() {
    // SCSS built-in modules (sass:math, sass:color) should stay bare
    let info = parse_css(r#"@use "sass:math";"#, "styles.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "sass:math");
}

#[test]
fn css_apply_creates_tailwind_dependency() {
    let info = parse_css(
        r"
.btn {
    @apply px-4 py-2 bg-blue-500 text-white;
}
",
        "styles.css",
    );
    assert!(
        info.imports.iter().any(|i| i.source == "tailwindcss"),
        "should create synthetic tailwindcss import"
    );
}

#[test]
fn css_tailwind_directive_creates_dependency() {
    let info = parse_css(
        r"
@tailwind base;
@tailwind components;
@tailwind utilities;
",
        "styles.css",
    );
    assert!(
        info.imports.iter().any(|i| i.source == "tailwindcss"),
        "should create synthetic tailwindcss import"
    );
}

#[test]
fn css_plugin_directive_creates_plugin_dependency() {
    let info = parse_css(
        r#"
@import "tailwindcss";
@plugin "@tailwindcss/typography";
@plugin "daisyui" {
    themes: light --default;
}
"#,
        "styles.css",
    );

    let sources: Vec<&str> = info.imports.iter().map(|i| i.source.as_str()).collect();
    assert!(sources.contains(&"tailwindcss"));
    assert!(sources.contains(&"@tailwindcss/typography"));
    assert!(sources.contains(&"daisyui"));
}

#[test]
fn css_plugin_directive_tracks_relative_plugin_file() {
    let info = parse_css(r#"@plugin "./tailwind-plugin.js";"#, "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./tailwind-plugin.js");
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
}

#[test]
fn scss_plugin_directive_keeps_package_specifier_bare() {
    let info = parse_css(r#"@plugin "daisyui";"#, "styles.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "daisyui");
}

#[test]
fn css_without_apply_no_tailwind_dependency() {
    let info = parse_css(
        r"
.btn {
    padding: 4px;
    color: blue;
}
",
        "styles.css",
    );
    assert!(
        !info.imports.iter().any(|i| i.source == "tailwindcss"),
        "should NOT create tailwindcss import without @apply"
    );
}

#[test]
fn extracts_scss_use() {
    let info = parse_css(r#"@use "./variables";"#, "styles.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./variables");
}

#[test]
fn extracts_scss_forward() {
    let info = parse_css(r#"@forward "./mixins";"#, "styles.scss");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./mixins");
}

#[test]
fn scss_use_not_extracted_from_css() {
    let info = parse_css(r#"@use "./variables";"#, "styles.css");
    assert_eq!(info.imports.len(), 0);
}

#[test]
fn css_apply_with_multiple_classes() {
    let info = parse_css(
        r"
.card {
    @apply shadow-lg rounded-lg p-4;
}
.header {
    @apply text-xl font-bold;
}
",
        "styles.css",
    );
    assert_eq!(
        info.imports
            .iter()
            .filter(|i| i.source == "tailwindcss")
            .count(),
        1
    );
}

#[test]
fn css_file_has_no_exports() {
    let info = parse_css(
        r#"
@import "./reset.css";
.btn { @apply px-4 py-2; }
"#,
        "styles.css",
    );
    assert!(info.exports.is_empty(), "CSS files should not have exports");
    assert!(info.re_exports.is_empty());
}

#[test]
fn scss_combined_imports_and_apply() {
    let info = parse_css(
        r#"
@use "./variables";
@use "./mixins";
@import "./reset.css";

.btn {
    @apply px-4 py-2;
}
"#,
        "app.scss",
    );
    assert_eq!(info.imports.len(), 4);
    assert!(info.imports.iter().any(|i| i.source == "./variables"));
    assert!(info.imports.iter().any(|i| i.source == "./mixins"));
    assert!(info.imports.iter().any(|i| i.source == "./reset.css"));
    assert!(info.imports.iter().any(|i| i.source == "tailwindcss"));
}

#[test]
fn css_import_with_media_query() {
    let info = parse_css(r#"@import "./print.css" print;"#, "styles.css");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./print.css");
}

#[test]
fn css_commented_apply_not_extracted() {
    let info = parse_css(
        r"
/* @apply px-4 py-2; */
.btn {
    padding: 4px;
}
",
        "styles.css",
    );
    assert!(
        !info.imports.iter().any(|i| i.source == "tailwindcss"),
        "commented-out @apply should NOT create tailwindcss import"
    );
}

#[test]
fn css_commented_import_not_extracted() {
    let info = parse_css(
        r#"
/* @import "./old-reset.css"; */
.btn { color: red; }
"#,
        "styles.css",
    );
    assert!(info.imports.is_empty());
}

#[test]
fn css_commented_tailwind_not_extracted() {
    let info = parse_css(
        r"
/*
@tailwind base;
@tailwind components;
@tailwind utilities;
*/
.btn { color: red; }
",
        "styles.css",
    );
    assert!(
        !info.imports.iter().any(|i| i.source == "tailwindcss"),
        "commented-out @tailwind should NOT create tailwindcss import"
    );
}

#[test]
fn css_commented_plugin_not_extracted() {
    let info = parse_css(
        r#"
/* @plugin "daisyui"; */
.btn { color: red; }
"#,
        "styles.css",
    );
    assert!(
        !info.imports.iter().any(|i| i.source == "daisyui"),
        "commented-out @plugin should NOT create an import"
    );
}

#[test]
fn scss_line_comment_not_extracted() {
    let info = parse_css(
        r#"
// @use "./old-variables";
// @apply px-4;
.btn { color: red; }
"#,
        "styles.scss",
    );
    assert!(info.imports.is_empty());
}

#[test]
fn css_url_import_skipped() {
    let info = parse_css(
        r#"
@import "https://fonts.googleapis.com/css?family=Roboto";
@import url("https://cdn.example.com/reset.css");
@import "./local.css";
"#,
        "styles.css",
    );
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./local.css");
}

#[test]
fn css_data_uri_import_skipped() {
    let info = parse_css(
        r#"@import url("data:text/css;base64,Ym9keSB7fQ==");"#,
        "styles.css",
    );
    assert!(info.imports.is_empty());
}

#[test]
fn css_mixed_comments_and_real_directives() {
    let info = parse_css(
        r#"
/* @import "./commented-out.css"; */
@import "./real-import.css";
/* @apply hidden; */
.visible {
    @apply block text-lg;
}
"#,
        "styles.css",
    );
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports.iter().any(|i| i.source == "./real-import.css"));
    assert!(info.imports.iter().any(|i| i.source == "tailwindcss"));
}

// -- CSS Module extraction --

#[test]
fn css_module_extracts_class_names_as_exports() {
    let info = parse_css_module(".header { color: red; } .footer { color: blue; }");
    let export_names: Vec<&ExportName> = info.exports.iter().map(|e| &e.name).collect();
    assert!(export_names.contains(&&ExportName::Named("header".to_string())));
    assert!(export_names.contains(&&ExportName::Named("footer".to_string())));
    assert!(!export_names.contains(&&ExportName::Default));
}

#[test]
fn css_module_extracts_kebab_case_class_names() {
    let info = parse_css_module(".nav-bar { display: flex; } .main-content { padding: 10px; }");
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert!(named.contains(&"nav-bar".to_string()));
    assert!(named.contains(&"main-content".to_string()));
}

#[test]
fn css_module_deduplicates_class_names() {
    let info = parse_css_module(".btn { color: red; } .btn { font-size: 14px; }");
    let named_count = info
        .exports
        .iter()
        .filter(|e| matches!(&e.name, ExportName::Named(n) if n == "btn"))
        .count();
    assert_eq!(
        named_count, 1,
        "Duplicate class names should be deduplicated"
    );
}

#[test]
fn css_module_no_default_export() {
    let info = parse_css_module(".foo { color: red; }");
    assert!(
        !info.exports.iter().any(|e| e.name == ExportName::Default),
        "CSS modules should not emit a default export (handled at graph level)"
    );
}

#[test]
fn non_module_css_has_no_exports() {
    let info = parse_css_non_module(".header { color: red; }");
    assert!(
        info.exports.is_empty(),
        "Non-module CSS should have no exports"
    );
}

#[test]
fn css_module_ignores_classes_in_comments() {
    let info = parse_css_module("/* .commented { color: red; } */ .active { color: green; }");
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert!(
        !named.contains(&"commented".to_string()),
        "Classes in comments should be ignored"
    );
    assert!(named.contains(&"active".to_string()));
}

#[test]
fn scss_module_extracts_class_names() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("Component.module.scss"),
        ".wrapper { .inner { color: red; } }",
        0,
        false,
    );
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert!(named.contains(&"wrapper".to_string()));
    assert!(named.contains(&"inner".to_string()));
}

#[test]
fn css_module_with_complex_selectors() {
    let info =
        parse_css_module(".btn:hover { color: red; } .btn.active { } .container > .child { }");
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert!(named.contains(&"btn".to_string()));
    assert!(named.contains(&"active".to_string()));
    assert!(named.contains(&"container".to_string()));
    assert!(named.contains(&"child".to_string()));
}

#[test]
fn css_module_ignores_classes_in_strings_and_urls() {
    let info = parse_css_module(
        r#".real { content: ".fake"; background: url(./img/hero.png); } .also-real { color: red; }"#,
    );
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert!(named.contains(&"real".to_string()));
    assert!(named.contains(&"also-real".to_string()));
    assert!(
        !named.contains(&"fake".to_string()),
        "Classes inside quoted strings should be ignored"
    );
    assert!(
        !named.contains(&"png".to_string()),
        "File extensions inside url() should be ignored"
    );
}

#[test]
fn issue_540_nested_cascade_layers_do_not_export_sub_names() {
    // Verbatim reproduction from issue #540. The CSS Modules parser previously
    // tokenised the dot in `@layer foo.bar` as the start of a class selector
    // and emitted phantom `bar` / `baz` exports. After the fix, only the
    // actual `.root` and `.pressed` selectors inside the block bodies are
    // extracted.
    let info = parse_css_module(
        "@layer foo;
@layer foo.bar, foo.baz;

@layer foo.bar {
  .root { color: red; }
}

@layer foo.baz {
  .pressed { color: blue; }
}",
    );
    let named: Vec<String> = info
        .exports
        .iter()
        .filter_map(|e| match &e.name {
            ExportName::Named(n) => Some(n.clone()),
            ExportName::Default => None,
        })
        .collect();
    assert_eq!(named, vec!["root", "pressed"], "phantom exports leaked");
}
