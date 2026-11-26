use fallow_types::extract::ExportName;

use crate::tests::{parse_ts as parse_source, parse_ts_with_complexity};

#[test]
fn function_overloads_deduplicated_to_single_export() {
    let info = parse_source(
        "export function parse(): void;\nexport function parse(input: string): void;\nexport function parse(input?: string): void {}",
    );
    assert_eq!(
        info.exports.len(),
        1,
        "Function overloads should produce exactly 1 export, got {}",
        info.exports.len()
    );
    assert_eq!(info.exports[0].name, ExportName::Named("parse".to_string()));
}

#[test]
fn line_offsets_populated_for_ts_file() {
    let info = parse_source("const a = 1;\nconst b = 2;\nconst c = 3;\n");
    assert!(
        !info.line_offsets.is_empty(),
        "Line offsets should be populated after parsing"
    );
    assert_eq!(info.line_offsets[0], 0, "First line starts at byte 0");
}

#[test]
fn complexity_metrics_populated_for_functions() {
    let info = parse_ts_with_complexity(
        r"export function complex(x: number) {
            if (x > 0) {
                for (let i = 0; i < x; i++) {
                    if (x > 5) { return true; }
                }
            }
            return false;
        }",
    );
    assert!(
        !info.complexity.is_empty(),
        "Complexity metrics should be populated"
    );
    let f = info.complexity.iter().find(|c| c.name == "complex");
    assert!(f.is_some());
    assert!(f.unwrap().cyclomatic > 1);
}

#[test]
fn function_overload_deduplication() {
    let info = parse_source(
        r"export function foo(x: string): string;
export function foo(x: number): number;
export function foo(x: string | number): string | number {
    return x;
}",
    );
    let foo_count = info
        .exports
        .iter()
        .filter(|e| matches!(&e.name, ExportName::Named(n) if n == "foo"))
        .count();
    assert_eq!(
        foo_count, 1,
        "Overloaded function should produce a single export entry"
    );
}

fn is_type_only_export(info: &fallow_types::extract::ModuleInfo, name: &ExportName) -> bool {
    info.exports
        .iter()
        .find(|export| &export.name == name)
        .unwrap_or_else(|| panic!("no export {name:?}"))
        .is_type_only
}

#[test]
fn local_export_of_a_type_is_type_only() {
    let info = parse_source(
        "type Shape = { w: number };\ninterface Box { h: number }\nexport { Shape, Box as Container };",
    );
    assert!(is_type_only_export(
        &info,
        &ExportName::Named("Shape".to_string())
    ));
    assert!(is_type_only_export(
        &info,
        &ExportName::Named("Container".to_string())
    ));
}

#[test]
fn local_export_of_a_value_or_merged_name_keeps_its_value() {
    let info = parse_source(
        "interface Token { id: string }\nconst Token = {};\nclass Engine {}\nenum Mode { A }\nexport { Token, Engine, Mode };",
    );
    for name in ["Token", "Engine", "Mode"] {
        assert!(
            !is_type_only_export(&info, &ExportName::Named(name.to_string())),
            "{name} has a value meaning"
        );
    }
}

#[test]
fn default_export_of_an_interface_is_type_only() {
    let info = parse_source("export default interface Options { enabled: boolean }");
    assert!(is_type_only_export(&info, &ExportName::Default));
    let class = parse_source("export default class Options {}");
    assert!(!is_type_only_export(&class, &ExportName::Default));
}
