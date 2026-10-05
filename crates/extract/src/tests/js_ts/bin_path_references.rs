//! A string that holds a `node_modules/.bin/<name>` path names an installed
//! binary. Code hands that path to a consumer that fallow cannot follow, such
//! as a child process. Extraction records the binary name, and the analysis
//! maps it to the package that declares the binary.

use crate::tests::parse_ts as parse_source;

fn bin_references(source: &str) -> Vec<String> {
    parse_source(source).bin_path_references.to_vec()
}

#[test]
fn string_literal_paths_record_the_binary_name() {
    let source = "export const a = 'node_modules/.bin/cli-tool';\n\
         export const b = \"./node_modules/.bin/runner\";\n\
         export const c = '/abs/app/node_modules/.bin/vite';\n";
    assert_eq!(bin_references(source), ["cli-tool", "runner", "vite"]);
}

#[test]
fn template_quasis_record_the_binary_name() {
    let source = "const root = process.cwd();\n\
         export const a = `${root}/node_modules/.bin/tsx`;\n\
         export const b = `node_modules/.bin/lint-tool --fix ${root}`;\n";
    assert_eq!(bin_references(source), ["tsx", "lint-tool"]);
}

#[test]
fn the_name_stops_at_a_separator_and_repeats_are_recorded_once() {
    let source = "export const a = 'node_modules/.bin/tool-a && node_modules/.bin/tool-b';\n\
         export const b = 'node_modules/.bin/tool-a/extra';\n";
    assert_eq!(bin_references(source), ["tool-a", "tool-b"]);
}

#[test]
fn a_path_without_a_binary_name_records_nothing() {
    let source = "const name = 'x';\n\
         export const a = 'node_modules/.bin/';\n\
         export const b = `node_modules/.bin/${name}`;\n\
         export const c = 'other_node_modules/.bin/tool';\n\
         export const d = 'node_modules/pkg/bin/tool';\n";
    assert!(
        bin_references(source).is_empty(),
        "only a complete `node_modules/.bin/<name>` path segment names a binary"
    );
}
