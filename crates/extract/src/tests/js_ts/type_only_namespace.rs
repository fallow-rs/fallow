//! A TypeScript namespace that holds only types emits no JavaScript. Its
//! export is type-only even without `declare` or `export type`. One value
//! statement anywhere in the body makes the namespace a runtime value.

use fallow_types::extract::{ExportName, ModuleInfo};

use crate::tests::parse_ts as parse_source;

fn type_only_flags(info: &ModuleInfo, name: &str) -> Vec<bool> {
    info.exports
        .iter()
        .filter(|export| export.name == ExportName::Named(name.to_string()))
        .map(|export| export.is_type_only)
        .collect()
}

fn assert_type_only(source: &str, name: &str) {
    let info = parse_source(source);
    let flags = type_only_flags(&info, name);
    assert!(
        !flags.is_empty() && flags.iter().all(|flag| *flag),
        "`{name}` must be a type-only export for:\n{source}\nflags: {flags:?}"
    );
}

fn assert_value(source: &str, name: &str) {
    let info = parse_source(source);
    let flags = type_only_flags(&info, name);
    assert!(
        flags.iter().any(|flag| !*flag),
        "`{name}` must be a value export for:\n{source}\nflags: {flags:?}"
    );
}

#[test]
fn namespace_with_only_types_is_type_only() {
    assert_type_only(
        "export namespace JSX {
            export interface Element { tag: string }
            export type Props = Record<string, unknown>;
            interface Local { a: number }
        }",
        "JSX",
    );
}

#[test]
fn module_keyword_with_only_types_is_type_only() {
    assert_type_only(
        "export module Shapes { export interface Circle { r: number } }",
        "Shapes",
    );
}

#[test]
fn empty_namespace_is_type_only() {
    assert_type_only("export namespace Empty {}", "Empty");
}

#[test]
fn namespace_with_ambient_members_is_type_only() {
    assert_type_only(
        "export namespace Ambient {
            export declare const version: string;
            declare function helper(): void;
            export declare class Box {}
            export declare enum Mode { A }
            export declare namespace Inner { const x: number }
        }",
        "Ambient",
    );
}

#[test]
fn namespace_with_type_import_alias_is_type_only() {
    assert_type_only(
        "export namespace Aliases {
            import type Other = Elsewhere.Thing;
            export type Kept = Other;
        }",
        "Aliases",
    );
}

#[test]
fn nested_type_only_namespace_is_type_only() {
    assert_type_only(
        "export namespace Outer {
            export namespace Inner { export interface Leaf { a: number } }
            namespace Hidden { type T = string }
        }",
        "Outer",
    );
}

#[test]
fn dotted_type_only_namespace_is_type_only() {
    assert_type_only(
        "export namespace A.B.C { export interface Leaf { a: number } }",
        "A",
    );
}

#[test]
fn namespace_with_exported_value_is_value() {
    assert_value(
        "export namespace Runtime {
            export interface Shape { a: number }
            export const version = '1';
        }",
        "Runtime",
    );
}

#[test]
fn namespace_with_local_value_is_value() {
    assert_value(
        "export namespace Hidden {
            export type T = string;
            const secret = 1;
        }",
        "Hidden",
    );
}

#[test]
fn namespace_with_function_class_or_enum_is_value() {
    for body in [
        "function f() {}",
        "export function f() {}",
        "class C {}",
        "enum E { A }",
        "const enum E { A }",
        "f();",
    ] {
        assert_value(&format!("export namespace Mixed {{ {body} }}"), "Mixed");
    }
}

#[test]
fn namespace_with_value_import_alias_is_value() {
    assert_value(
        "export namespace Aliases { export import Other = Elsewhere.Thing; }",
        "Aliases",
    );
    assert_value(
        "export namespace Aliases { import Other = Elsewhere.Thing; }",
        "Aliases",
    );
}

#[test]
fn nested_instantiated_namespace_is_value() {
    assert_value(
        "export namespace Outer {
            export interface Shape { a: number }
            export namespace Inner { export const x = 1; }
        }",
        "Outer",
    );
}

#[test]
fn dotted_instantiated_namespace_is_value() {
    assert_value("export namespace A.B.C { export const x = 1; }", "A");
}

/// A namespace that holds only types merges with a value of the same name into
/// one runtime binding. Each declaration keeps its own export entry, and both
/// entries stay values, so a value import credits the whole merged symbol.
#[test]
fn function_merged_with_type_only_namespace_stays_value() {
    let info = parse_source(
        "export function merged(): number { return 1; }
        export namespace merged { export interface Opts { x: number } }",
    );
    assert_eq!(type_only_flags(&info, "merged"), vec![false, false]);
}

#[test]
fn class_merged_with_type_only_namespace_stays_value() {
    let info = parse_source(
        "export class Merged {}
        export namespace Merged { export type Opts = { x: number } }",
    );
    assert_eq!(type_only_flags(&info, "Merged"), vec![false, false]);
}

#[test]
fn class_merged_with_instantiated_namespace_stays_value() {
    let info = parse_source(
        "export class Merged {}
        export namespace Merged { export const version = 1; }",
    );
    assert_eq!(type_only_flags(&info, "Merged"), vec![false, false]);
}

#[test]
fn interface_merged_with_type_only_namespace_is_type_only() {
    assert_type_only(
        "export interface Shape { a: number }
        export namespace Shape { export type Kind = 'a' }",
        "Shape",
    );
}

#[test]
fn type_only_namespace_merged_with_later_const_stays_value() {
    let info = parse_source(
        "export namespace Config { export type Mode = 'a' | 'b' }
        export const Config = { mode: 'a' };",
    );
    assert_eq!(type_only_flags(&info, "Config"), vec![false, false]);
}
