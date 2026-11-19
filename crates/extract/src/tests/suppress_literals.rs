//! A suppression marker inside a string or template literal of an inline
//! script has no effect (issue #4751).

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::suppress::SuppressionScope;

use crate::parse_source_to_module;

/// The `(line, comment_line)` of each suppression of the file. A file
/// suppression has line 0.
fn suppressions(filename: &str, source: &str) -> Vec<(u32, u32)> {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, false)
        .suppressions
        .iter()
        .filter(|suppression| !matches!(suppression.scope, SuppressionScope::Range { .. }))
        .map(|suppression| (suppression.line, suppression.comment_line))
        .collect()
}

const IN_TEMPLATE: &str = "export const docs = `
// fallow-ignore-file unused-export
// fallow-ignore-next-line unused-export
`;
";

const REAL_MARKER: &str = "// fallow-ignore-next-line unused-type
export type Kept = string;
";

#[test]
fn a_marker_in_a_vue_template_literal_has_no_effect() {
    let source = format!("<script setup lang=\"ts\">\n{IN_TEMPLATE}{REAL_MARKER}</script>\n");
    assert_eq!(suppressions("Docs.vue", &source), vec![(7, 6)]);
}

#[test]
fn a_marker_in_a_svelte_string_has_no_effect() {
    let source = format!(
        "<script lang=\"ts\">\nexport const a = \"\\\n// fallow-ignore-file unused-export\";\n{IN_TEMPLATE}{REAL_MARKER}</script>\n<p>{{a}}</p>\n"
    );
    assert_eq!(suppressions("Docs.svelte", &source), vec![(9, 8)]);
}

#[test]
fn a_marker_in_an_astro_script_literal_has_no_effect() {
    let source = format!(
        "---\n{IN_TEMPLATE}---\n<script>\n{IN_TEMPLATE}</script>\n<!-- fallow-ignore-file unused-file -->\n"
    );
    assert_eq!(suppressions("Page.astro", &source), vec![(0, 13)]);
}

#[test]
fn a_marker_in_a_typed_astro_script_literal_has_no_effect() {
    let source = format!(
        "---\nconst t = 1;\n---\n<p>{{t}}</p>\n<script>\nconst n: number = 1;\n{IN_TEMPLATE}</script>\n"
    );
    assert_eq!(suppressions("Page.astro", &source), Vec::new());
    let parsed = crate::parse_suppressions_for_file(Path::new("src/Page.astro"), &source);
    assert!(parsed.suppressions.is_empty(), "{:?}", parsed.suppressions);
}

#[test]
fn a_marker_in_an_html_script_literal_has_no_effect() {
    let source = format!(
        "<html><body>\n<script type=\"module\">\n{IN_TEMPLATE}</script>\n<!-- fallow-ignore-file unused-file -->\n</body></html>\n"
    );
    assert_eq!(suppressions("index.html", &source), vec![(0, 8)]);
}

#[test]
fn a_marker_in_a_classic_html_script_literal_has_no_effect_and_keeps_its_imports() {
    let source = "<script>\nconst docs = `\n// fallow-ignore-file unused-file\n`;\nimport(\"./lazy.js\");\n</script>\n";
    let info = parse_source_to_module(FileId(0), Path::new("index.html"), source, 0, false);
    assert!(info.suppressions.is_empty());
    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|import| import.source.as_str())
        .collect();
    assert_eq!(sources, vec!["./lazy.js"]);
}

#[test]
fn a_marker_in_an_mdx_export_literal_has_no_effect() {
    let source = format!("{IN_TEMPLATE}\n# Title\n");
    assert_eq!(suppressions("page.mdx", &source), Vec::new());
}

#[test]
fn a_marker_outside_a_script_still_counts() {
    let source = "<script setup lang=\"ts\">\nexport const a = 1;\n</script>\n<!-- fallow-ignore-file unused-file -->\n";
    assert_eq!(suppressions("Plain.vue", source), vec![(0, 4)]);
}

#[test]
fn the_text_scan_of_a_script_file_drops_a_marker_in_a_literal() {
    let source = format!("{IN_TEMPLATE}{REAL_MARKER}");
    let parsed = crate::parse_suppressions_for_file(Path::new("src/docs.ts"), &source);
    let lines: Vec<u32> = parsed.suppressions.iter().map(|s| s.line).collect();
    assert_eq!(lines, vec![6]);
}

#[test]
fn the_text_scan_of_a_css_file_keeps_its_markers() {
    let source = "/* fallow-ignore-file */\n.a { color: red; }\n";
    let parsed = crate::parse_suppressions_for_file(Path::new("src/a.css"), source);
    assert_eq!(parsed.suppressions.len(), 1);
}
