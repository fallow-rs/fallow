//! A `css-broken-reference` finding for an undefined `@keyframes` and a
//! `css-dead-surface` finding for an unused scoped class report the line of
//! the code that causes them. A `fallow-ignore-next-line` comment above that
//! line suppresses the finding.

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write fixture file");
}

/// A project with `src/main.ts` as its entry point, which imports
/// `stylesheet`, and the file `path` with `contents`.
fn project(stylesheet: &str, path: &str, contents: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"shop","version":"1.0.0","private":true,"main":"src/main.ts"}"#,
    );
    write(root, "src/main.ts", &format!("import \"{stylesheet}\";\n"));
    write(root, path, contents);
    dir
}

/// The `(sub_kind, path, line)` of each styling finding with `code`.
fn styling_findings(root: &Path, code: &str) -> Vec<(String, String, u64)> {
    let output = run_fallow_in_root("health", root, &["--css", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    json["styling_findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|finding| finding["code"].as_str() == Some(code))
        .map(|finding| {
            (
                finding["sub_kind"].as_str().unwrap_or_default().to_string(),
                finding["path"].as_str().unwrap_or_default().to_string(),
                finding["line"].as_u64().unwrap_or_default(),
            )
        })
        .collect()
}

fn keyframes_stylesheet(comment: &str) -> String {
    format!(".page {{\n  margin: 0;\n}}\n\n{comment}.toast {{\n  animation: fade-in 1s;\n}}\n")
}

#[test]
fn undefined_keyframes_reports_the_line_of_the_referencing_rule() {
    let dir = project("./app.css", "src/app.css", &keyframes_stylesheet(""));
    assert_eq!(
        styling_findings(dir.path(), "css-broken-reference"),
        vec![(
            "undefined-keyframes".to_string(),
            "src/app.css".to_string(),
            5
        )]
    );
}

#[test]
fn a_comment_above_the_referencing_rule_suppresses_undefined_keyframes() {
    let stylesheet = keyframes_stylesheet("/* fallow-ignore-next-line css-broken-reference */\n");
    let dir = project("./app.css", "src/app.css", &stylesheet);
    assert!(styling_findings(dir.path(), "css-broken-reference").is_empty());
}

fn component(comment: &str) -> String {
    format!(
        "<template>\n  <div class=\"card\"></div>\n</template>\n\n<style scoped>\n\
         .card {{\n  padding: 16px;\n}}\n{comment}.card__badge {{\n  color: green;\n}}\n</style>\n"
    )
}

#[test]
fn scoped_unused_class_reports_the_line_of_the_class() {
    let dir = project("./Card.vue", "src/Card.vue", &component(""));
    assert_eq!(
        styling_findings(dir.path(), "css-dead-surface"),
        vec![(
            "scoped-unused-class".to_string(),
            "src/Card.vue".to_string(),
            9
        )]
    );
}

#[test]
fn a_comment_above_the_class_suppresses_the_scoped_unused_class() {
    let source = component("/* fallow-ignore-next-line css-dead-surface */\n");
    let dir = project("./Card.vue", "src/Card.vue", &source);
    assert!(styling_findings(dir.path(), "css-dead-surface").is_empty());
}
