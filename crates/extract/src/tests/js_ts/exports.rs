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
fn local_export_of_a_default_class_name_keeps_its_value() {
    let info = parse_source(
        "interface Foo { a: number }\nexport default class Foo {}\nexport { Foo as Bar };",
    );
    assert!(!is_type_only_export(
        &info,
        &ExportName::Named("Bar".to_string())
    ));
}

#[test]
fn default_export_of_an_interface_is_type_only() {
    let info = parse_source("export default interface Options { enabled: boolean }");
    assert!(is_type_only_export(&info, &ExportName::Default));
    let class = parse_source("export default class Options {}");
    assert!(!is_type_only_export(&class, &ExportName::Default));
}

/// The decorator metadata names of `source`, sorted, with `?` after a name
/// that TypeScript emits only without `strictNullChecks`.
fn metadata_names(source: &str) -> Vec<String> {
    let mut names: Vec<String> = parse_source(source)
        .semantic_facts
        .iter()
        .filter_map(|fact| match fact {
            fallow_types::extract::SemanticFact::DecoratorMetadataReference(fact) => {
                Some(if fact.without_strict_null_checks {
                    format!("{}?", fact.local)
                } else {
                    fact.local.clone()
                })
            }
            _ => None,
        })
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn decorated_members_emit_metadata_names() {
    let source = "
        @C() export class A {
            @P() property: Property;
            @P() accessor auto: Auto;
            constructor(ctor: Ctor) {}
            @M() method(this: This, param: Param, ...rest: Rest[]): Return { return x; }
            @M() generic(...items: Array<Item>): Promise<Hidden> { return x; }
            // A parameter decorator also emits the return type of its method.
            withParam(@P() decorated: Decorated, other: Other): ParamReturn { return x; }
            @P() get getter(): Getter { return x; }
            @P() set setter(value: Setter) {}
            @P() set paired(value) {}
            get paired(): Paired { return x; }
            @P() qualified: ns.Inner;
        }
    ";
    assert_eq!(
        metadata_names(source),
        [
            "Auto",
            "Ctor",
            "Decorated",
            "Getter",
            "Item",
            "Other",
            "Paired",
            "Param",
            "ParamReturn",
            "Promise",
            "Property",
            "Rest",
            "Return",
            "Setter",
            "This",
            "ns",
        ]
    );
}

#[test]
fn undecorated_and_non_emitted_positions_emit_no_metadata_names() {
    // A generic type emits only its outer name, so `Promise<Hidden[]>`
    // emits `Promise` and never `Hidden`.
    let source = "
        export class Plain {
            constructor(ctor: Ctor) {}
            method(param: Param): Return { return x; }
            property: Property;
        }
        export class Members {
            constructor(ctor: Ctor) {}
            @M() list(): Promise<Hidden[]> { return x; }
            @M() array(items: Hidden[], fn: () => Hidden, text: string): Hidden[] { return x; }
            @M() overload(a: Hidden): void;
            @P() mixed: First | Second;
            @P() qualifiedUnion: ns.Inner | ns.Inner;
            @P() withKeyword: Hidden | string;
            @P() unknown: unknown;
        }
        const Expression = @C() class { constructor(ctor: Ctor) {} };
        declare class Ambient { @P() property: Property; }
    ";
    assert_eq!(metadata_names(source), ["Promise"]);
}

#[test]
fn unions_emit_one_shared_name() {
    let source = "
        export class A {
            @P() same: Same | Same;
            @P() never: Never | never;
            @P() nullable: Nullable | null;
            @P() optional: (Optional | undefined);
            @P() nested: (Nested | null) | Nested;
            @P() conditional: T extends U ? Branch : Branch;
            @P() qualifiedNullable: ns.Inner | null;
            @P() both: Both | null;
            @P() strictBoth: Both;
        }
    ";
    assert_eq!(
        metadata_names(source),
        [
            "Both",
            "Branch",
            "Nested?",
            "Never",
            "Nullable?",
            "Optional?",
            "Same",
            "ns?",
        ]
    );
}
