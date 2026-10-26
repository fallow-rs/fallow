use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse_source_to_module;

/// Far past the depth at which the Oxc parser overflows a 16 MiB stack.
const DEEP: usize = 20_000;

fn parse(path: &str, source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(path), source, 0, true)
}

fn assert_degraded(info: &ModuleInfo) {
    assert!(
        info.parse_panicked,
        "a too-deep file must be a failed parse"
    );
    assert!(info.parse_error_count > 0);
    assert!(info.exports.is_empty());
    assert!(info.imports.is_empty());
}

#[test]
fn deeply_nested_parens_degrade_instead_of_overflowing() {
    let source = format!(
        "import a from './a';\nexport const x = {}1{};\n",
        "(".repeat(DEEP),
        ")".repeat(DEEP)
    );
    let info = parse("src/index.mjs", &source);
    assert_degraded(&info);
    assert_eq!(
        info.line_offsets.len(),
        3,
        "line offsets still describe the source"
    );
}

#[test]
fn deeply_nested_arrays_in_a_glimmer_file_degrade() {
    let source = format!("export default {};\n", "[".repeat(DEEP));
    assert_degraded(&parse("src/App.gts", &source));
}

#[test]
fn deep_nesting_inside_a_vue_script_degrades() {
    let source = format!(
        "<script setup>\nconst x = {}{};\n</script>\n",
        "{a:".repeat(DEEP),
        "}".repeat(DEEP)
    );
    assert_degraded(&parse("src/App.vue", &source));
}

#[test]
fn closers_in_literals_do_not_hide_real_nesting() {
    // Each level opens a real paren and hides a closer in a literal. A plain
    // byte count sees depth 0 or 1 here.
    for unit in ["(')',", "(\")\",", "(`)`,", "(/)/,", "(/*)*/", "(//)\n"] {
        let source = format!("f{};\n", unit.repeat(DEEP));
        assert_degraded(&parse("src/index.js", &source));
    }
}

#[test]
fn template_substitutions_count_as_nesting() {
    let source = format!("const s = {}{};\n", "`${".repeat(DEEP), "}`".repeat(DEEP));
    assert_degraded(&parse("src/index.js", &source));
}

#[test]
fn ordinary_nesting_still_parses() {
    let depth = 200;
    let source = format!(
        "import a from './a';\nexport const x = {}a{};\n",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let info = parse("src/index.ts", &source);
    assert!(!info.parse_panicked);
    assert_eq!(info.parse_error_count, 0);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn brackets_inside_literals_do_not_count_as_nesting() {
    // Serialized parser tables and regexes carry many unbalanced openers in
    // strings. They are not nesting and must not degrade the file.
    let openers = "(".repeat(DEEP);
    let source = format!(
        "export const a = '{openers}';\nexport const b = \"{openers}\";\n\
         export const c = `{openers}`;\nexport const d = /\\({{3}}[(]/;\n\
         // {openers}\n/* {openers} */\nexport const e = 1 / (2) / 3;\n"
    );
    let info = parse("src/index.js", &source);
    assert!(!info.parse_panicked);
    assert_eq!(info.exports.len(), 5);
}

fn deep_custom_property() -> String {
    format!(".a{{--x:{}1{};}}\n", "(".repeat(DEEP), ")".repeat(DEEP))
}

#[test]
fn deep_css_after_a_protocol_relative_url_degrades() {
    // Minified CSS is one line. A `//` in `url()` is not a comment in CSS, so
    // the nesting after it must still count.
    let source = format!(
        ".b{{background:url(//cdn.example.com/a.png)}}{}",
        deep_custom_property()
    );
    assert_degraded(&parse("src/styles.css", &source));
}

#[test]
fn css_module_exports_skip_a_too_deep_stylesheet() {
    let source = format!(".keep{{color:red}}\n{}", deep_custom_property());
    let exports = crate::css::extract_css_module_exports(&source, false);
    assert!(
        exports
            .iter()
            .any(|export| export.name.to_string() == "keep")
    );
}

#[test]
fn css_analytics_skip_a_too_deep_stylesheet() {
    assert!(crate::css_metrics::compute_css_analytics(&deep_custom_property()).is_none());
}

#[test]
fn a_hash_or_at_url_prefix_does_not_hide_nesting() {
    for prefix in ["#url(", "@url(", "url("] {
        let source = format!(".a{{--x:{prefix}{}}}", "(".repeat(DEEP));
        assert!(
            crate::parse_style_bounded(&source, || ()).is_none(),
            "{prefix} hid the nesting"
        );
    }
}

/// Depth of the nested destructure defaults. Work that doubles at each level
/// does not end in the time limit at this depth.
const DEFAULT_DEPTH: usize = 40;

/// Extract `source` on another thread and fail when it does not end in time.
fn assert_extracts_in_time(source: String) {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let info = parse("src/index.ts", &source);
        let _ = done.send(info.parse_error_count);
    });
    let errors = finished
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("extraction must end in time");
    assert_eq!(errors, 0);
}

#[test]
fn nested_array_destructure_defaults_cost_linear_work() {
    let mut pattern = String::from("{ m }");
    for _ in 0..DEFAULT_DEPTH {
        pattern = format!("[{pattern} = [s]]");
    }
    assert_extracts_in_time(format!(
        "declare const s: any;\nconst {pattern} = [s];\nfor (const {pattern} of [[s]]) {{}}\n"
    ));
}

#[test]
fn nested_object_destructure_assignment_defaults_cost_linear_work() {
    let mut pattern = String::from("{ m }");
    for _ in 0..DEFAULT_DEPTH {
        pattern = format!("{{ k: {pattern} = s }}");
    }
    assert_extracts_in_time(format!(
        "declare const s: any;\nlet m: unknown;\n({pattern} = s);\n"
    ));
}
