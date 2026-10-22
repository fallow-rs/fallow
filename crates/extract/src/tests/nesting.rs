use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module;

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
