//! Server-side template files (Liquid, Nunjucks, Handlebars, Twig, Blade, and
//! the extensions in `markupExtensions`) count as class references for
//! `unreferenced-css-class`, but they never join source discovery (issue #3917).

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

/// A theme where `index.html` applies one class of `assets/base.css` and a
/// template file applies two more. `.never-used` has no reference anywhere.
fn theme_project(template_path: &str, template: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"theme","version":"1.0.0","private":true}"#,
    );
    write(
        root,
        "assets/base.css",
        ".page-shell { color: red; }\n\
         .product-card { display: grid; }\n\
         .product-card__title { font-weight: 700; }\n\
         .never-used { color: blue; }\n",
    );
    write(
        root,
        "index.html",
        "<!doctype html>\n<html><head><link rel=\"stylesheet\" href=\"assets/base.css\"></head>\
         <body><div class=\"page-shell\"></div></body></html>\n",
    );
    write(root, template_path, template);
    dir
}

const LIQUID_SECTION: &str = "<div class=\"product-card\">\n  \
    <h2 class=\"product-card__title\">{{ product.title }}</h2>\n</div>\n";

fn unreferenced_classes(root: &Path) -> (Vec<String>, serde_json::Value) {
    unreferenced_classes_with(root, &[])
}

fn unreferenced_classes_with(root: &Path, extra: &[&str]) -> (Vec<String>, serde_json::Value) {
    let mut args = vec!["--css", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("health", root, &args);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let rows = json["css_analytics"]["unreferenced_css_classes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let classes = rows
        .iter()
        .filter_map(|row| row["class"].as_str().map(str::to_owned))
        .collect();
    (classes, json)
}

#[test]
fn liquid_template_classes_count_as_references_without_config() {
    let dir = theme_project("sections/product.liquid", LIQUID_SECTION);

    let (classes, json) = unreferenced_classes(dir.path());

    assert_eq!(classes, vec!["never-used".to_owned()], "{json}");
    let action = &json["css_analytics"]["unreferenced_css_classes"][0]["actions"][0];
    assert!(
        action["description"]
            .as_str()
            .is_some_and(|text| text.contains(".liquid")),
        "the verify text names the template extension the scan read: {action}"
    );
    assert!(
        action["command"]
            .as_str()
            .is_some_and(|command| command.contains("--include='*.liquid'")),
        "the verify command searches the template extension: {action}"
    );
}

#[test]
fn template_text_is_a_dynamic_reference() {
    // Blade applies classes through directives, not a static `class="..."`.
    let dir = theme_project(
        "resources/views/product.blade.php",
        "<div @class(['product-card', 'product-card__title' => $open])></div>\n",
    );

    let (classes, json) = unreferenced_classes(dir.path());

    assert_eq!(classes, vec!["never-used".to_owned()], "{json}");
}

#[test]
fn configured_markup_extension_counts_as_reference() {
    let dir = theme_project("templates/product.tpl", LIQUID_SECTION);

    let (classes, json) = unreferenced_classes(dir.path());
    assert!(
        classes.contains(&"product-card".to_owned()),
        "a .tpl file is not markup by default: {json}"
    );

    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"markupExtensions": [".tpl"]}"#,
    );
    let (classes, json) = unreferenced_classes(dir.path());
    assert_eq!(classes, vec!["never-used".to_owned()], "{json}");
}

#[test]
fn template_files_do_not_join_discovery() {
    let dir = theme_project("sections/product.liquid", LIQUID_SECTION);

    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--unused-files"],
    );
    let json = parse_json(&output);
    let unused = json["unused_files"].as_array().cloned().unwrap_or_default();
    assert!(
        unused.iter().all(|row| !row["path"]
            .as_str()
            .is_some_and(|path| path.ends_with(".liquid"))),
        "a template file is never an unused file: {json}"
    );
}

#[test]
fn ignored_template_is_not_a_reference() {
    let dir = theme_project("vendor/theme/product.liquid", LIQUID_SECTION);

    let (classes, json) = unreferenced_classes(dir.path());

    assert!(
        classes.contains(&"product-card".to_owned()),
        "templates under vendor/ are third-party code: {json}"
    );

    let dir = theme_project("generated/product.liquid", LIQUID_SECTION);
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"ignorePatterns": ["generated/**"]}"#,
    );
    let (classes, json) = unreferenced_classes(dir.path());
    assert!(
        classes.contains(&"product-card".to_owned()),
        "ignorePatterns removes the template from the reference surface: {json}"
    );
}

#[test]
fn script_or_stylesheet_markup_extension_is_a_config_error() {
    for value in [
        "css",
        ".ts",
        "",
        "a/b",
        "*",
        "module.css",
        "min.js",
        "d.ts",
        "icons.svg",
    ] {
        let dir = theme_project("sections/product.liquid", LIQUID_SECTION);
        write(
            dir.path(),
            ".fallowrc.json",
            &format!(r#"{{"markupExtensions": ["{value}"]}}"#),
        );

        let output = run_fallow_in_root(
            "health",
            dir.path(),
            &["--css", "--format", "json", "--quiet"],
        );

        assert_eq!(output.code, 2, "value {value:?}: {}", output.stderr);
        assert!(
            output.stdout.contains("markupExtensions entry")
                || output.stderr.contains("markupExtensions entry"),
            "the error names the key for {value:?}: {} {}",
            output.stdout,
            output.stderr
        );
    }
}

#[test]
fn echo_prefix_and_unquoted_words_count_as_references() {
    let dir = theme_project(
        "sections/banner.liquid",
        "<div class=\"product-card--{{ size }}\"><h2 class=product-card__title></h2></div>\n",
    );
    write(
        dir.path(),
        "assets/base.css",
        ".page-shell { color: red; }\n\
         .product-card--large { display: grid; }\n\
         .product-card__title { font-weight: 700; }\n\
         .never-used { color: blue; }\n",
    );

    let (classes, json) = unreferenced_classes(dir.path());

    assert_eq!(classes, vec!["never-used".to_owned()], "{json}");
}

#[test]
fn template_over_the_file_size_limit_is_not_read() {
    let mut template = String::from(LIQUID_SECTION);
    template.push_str(&" ".repeat(1_100_000));
    let dir = theme_project("sections/product.liquid", &template);

    let (classes, json) = unreferenced_classes_with(dir.path(), &["--max-file-size", "1"]);
    assert!(
        classes.contains(&"product-card".to_owned()),
        "a template over the limit is skipped: {json}"
    );

    let (classes, json) = unreferenced_classes(dir.path());
    assert_eq!(classes, vec!["never-used".to_owned()], "{json}");
}

#[test]
fn extends_child_list_replaces_the_parent_list() {
    let dir = theme_project("templates/product.tpl", LIQUID_SECTION);
    write(dir.path(), "base.json", r#"{"markupExtensions": ["tpl"]}"#);
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"extends": "./base.json"}"#,
    );
    let (classes, json) = unreferenced_classes(dir.path());
    assert_eq!(
        classes,
        vec!["never-used".to_owned()],
        "the parent list applies: {json}"
    );

    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"extends": "./base.json", "markupExtensions": ["erb"]}"#,
    );
    let (classes, json) = unreferenced_classes(dir.path());
    assert!(
        classes.contains(&"product-card".to_owned()),
        "the child list replaces the parent list: {json}"
    );
}
