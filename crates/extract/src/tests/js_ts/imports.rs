use fallow_types::extract::{ExportName, ImportedName, MemberKind};

use crate::tests::parse_ts as parse_source;

#[test]
fn extracts_named_exports() {
    let info = parse_source("export const foo = 1; export function bar() {}");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("foo".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("bar".to_string()));
}

#[test]
fn extracts_default_export() {
    let info = parse_source("export default function main() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn extracts_named_imports() {
    let info = parse_source("import { foo, bar } from './utils';");
    assert_eq!(info.imports.len(), 2);
    assert_eq!(
        info.imports[0].imported_name,
        ImportedName::Named("foo".to_string())
    );
    assert_eq!(info.imports[0].source, "./utils");
}

#[test]
fn extracts_namespace_import() {
    let info = parse_source("import * as utils from './utils';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Namespace);
}

#[test]
fn extracts_side_effect_import() {
    let info = parse_source("import './styles.css';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::SideEffect);
}

#[test]
fn empty_specifier_list_import_is_side_effect() {
    let info = parse_source("import {} from './setup';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./setup");
    assert_eq!(info.imports[0].imported_name, ImportedName::SideEffect);
    assert!(info.imports[0].local_name.is_empty());
    assert!(!info.imports[0].is_type_only);
}

#[test]
fn empty_specifier_list_type_import_is_type_only_side_effect() {
    let info = parse_source("import type {} from '@scope/types';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "@scope/types");
    assert_eq!(info.imports[0].imported_name, ImportedName::SideEffect);
    assert!(info.imports[0].is_type_only);
}

#[test]
fn extracts_re_exports() {
    let info = parse_source("export { foo, bar as baz } from './module';");
    assert_eq!(info.re_exports.len(), 2);
    assert_eq!(info.re_exports[0].imported_name, "foo");
    assert_eq!(info.re_exports[0].exported_name, "foo");
    assert_eq!(info.re_exports[1].imported_name, "bar");
    assert_eq!(info.re_exports[1].exported_name, "baz");
}

#[test]
fn extracts_star_re_export() {
    let info = parse_source("export * from './module';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "*");
    assert_eq!(info.re_exports[0].exported_name, "*");
}

#[test]
fn extracts_dynamic_import() {
    let info = parse_source("const mod = import('./lazy');");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lazy");
}

#[test]
fn extracts_require_call() {
    let info = parse_source("const fs = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "fs");
}

#[test]
fn extracts_type_exports() {
    let info = parse_source("export type Foo = string; export interface Bar { x: number; }");
    assert_eq!(info.exports.len(), 2);
    assert!(info.exports[0].is_type_only);
    assert!(info.exports[1].is_type_only);
}

#[test]
fn extracts_type_only_imports() {
    let info = parse_source("import type { Foo } from './types';");
    assert_eq!(info.imports.len(), 1);
    assert!(info.imports[0].is_type_only);
}

#[test]
fn detects_cjs_module_exports() {
    let info = parse_source("module.exports = { foo: 1 };");
    assert!(info.has_cjs_exports);
}

#[test]
fn detects_cjs_exports_property() {
    let info = parse_source("exports.foo = 42;");
    assert!(info.has_cjs_exports);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("foo".to_string()));
}

#[test]
fn detects_cjs_module_exports_dot_property() {
    let info = parse_source("module.exports.myFunc = function() {};\nmodule.exports.myConst = 42;");
    assert!(info.has_cjs_exports);
    assert_eq!(info.exports.len(), 2);
}

#[test]
fn extracts_static_member_accesses() {
    let info = parse_source(
        "import { Status, MyClass } from './types';\nconsole.log(Status.Active);\nMyClass.create();",
    );
    assert!(info.member_accesses.len() >= 2);
    let has_status_active = info
        .member_accesses
        .iter()
        .any(|a| a.object == "Status" && a.member == "Active");
    let has_myclass_create = info
        .member_accesses
        .iter()
        .any(|a| a.object == "MyClass" && a.member == "create");
    assert!(has_status_active, "Should capture Status.Active");
    assert!(has_myclass_create, "Should capture MyClass.create");
}

#[test]
fn extracts_default_import() {
    let info = parse_source("import React from 'react';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
    assert_eq!(info.imports[0].local_name, "React");
    assert_eq!(info.imports[0].source, "react");
}

#[test]
fn extracts_mixed_import_default_and_named() {
    let info = parse_source("import React, { useState, useEffect } from 'react';");
    assert_eq!(info.imports.len(), 3);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
    assert_eq!(info.imports[0].local_name, "React");
    assert_eq!(
        info.imports[1].imported_name,
        ImportedName::Named("useState".to_string())
    );
    assert_eq!(
        info.imports[2].imported_name,
        ImportedName::Named("useEffect".to_string())
    );
}

#[test]
fn extracts_import_with_alias() {
    let info = parse_source("import { foo as bar } from './utils';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(
        info.imports[0].imported_name,
        ImportedName::Named("foo".to_string())
    );
    assert_eq!(info.imports[0].local_name, "bar");
}

#[test]
fn extracts_export_specifier_list() {
    let info = parse_source("const foo = 1; const bar = 2; export { foo, bar };");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("foo".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("bar".to_string()));
}

#[test]
fn extracts_export_with_alias() {
    let info = parse_source("const foo = 1; export { foo as myFoo };");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("myFoo".to_string()));
}

#[test]
fn extracts_star_re_export_with_alias() {
    let info = parse_source("export * as utils from './utils';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "*");
    assert_eq!(info.re_exports[0].exported_name, "utils");
}

#[test]
fn extracts_export_class_declaration() {
    let info = parse_source("export class MyService { name: string = ''; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("MyService".to_string())
    );
}

#[test]
fn class_constructor_is_excluded() {
    let info = parse_source("export class Foo { constructor() {} greet() {} }");
    assert_eq!(info.exports.len(), 1);
    let members: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(
        !members.contains(&"constructor"),
        "constructor should be excluded from members"
    );
    assert!(members.contains(&"greet"), "greet should be included");
}

#[test]
fn extracts_ts_enum_declaration() {
    let info = parse_source("export enum Direction { Up, Down, Left, Right }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("Direction".to_string())
    );
    assert_eq!(info.exports[0].members.len(), 4);
    assert_eq!(info.exports[0].members[0].kind, MemberKind::EnumMember);
}

#[test]
fn extracts_ts_module_declaration() {
    let info = parse_source("export declare module 'my-module' {}");
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].is_type_only);
}

#[test]
fn extracts_type_only_named_import() {
    let info = parse_source("import { type Foo, Bar } from './types';");
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports[0].is_type_only);
    assert!(!info.imports[1].is_type_only);
}

#[test]
fn extracts_type_re_export() {
    let info = parse_source("export type { Foo } from './types';");
    assert_eq!(info.re_exports.len(), 1);
    assert!(info.re_exports[0].is_type_only);
}

#[test]
fn extracts_destructured_array_export() {
    let info = parse_source("export const [first, second] = [1, 2];");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("first".to_string()));
    assert_eq!(
        info.exports[1].name,
        ExportName::Named("second".to_string())
    );
}

#[test]
fn extracts_nested_destructured_export() {
    let info = parse_source("export const { a, b: { c } } = obj;");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("a".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("c".to_string()));
}

#[test]
fn extracts_default_export_function_expression() {
    let info = parse_source("export default function() { return 42; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_name_display() {
    assert_eq!(ExportName::Named("foo".to_string()).to_string(), "foo");
    assert_eq!(ExportName::Default.to_string(), "default");
}

#[test]
fn no_exports_no_imports() {
    let info = parse_source("const x = 1; console.log(x);");
    assert!(info.exports.is_empty());
    assert!(info.imports.is_empty());
    assert!(info.re_exports.is_empty());
    assert!(!info.has_cjs_exports);
}

#[test]
fn dynamic_import_non_string_ignored() {
    let info = parse_source("const mod = import(variable);");
    assert_eq!(info.dynamic_imports.len(), 0);
}

#[test]
fn multiple_require_calls() {
    let info =
        parse_source("const a = require('a'); const b = require('b'); const c = require('c');");
    assert_eq!(info.require_calls.len(), 3);
}

#[test]
fn extracts_ts_interface() {
    let info = parse_source("export interface Props { name: string; age: number; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Props".to_string()));
    assert!(info.exports[0].is_type_only);
}

#[test]
fn extracts_ts_type_alias() {
    let info = parse_source("export type ID = string | number;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("ID".to_string()));
    assert!(info.exports[0].is_type_only);
}

#[test]
fn extracts_member_accesses_inside_exported_functions() {
    let info = parse_source(
        "import { Color } from './types';\nexport const isRed = (c: Color) => c === Color.Red;",
    );
    let has_color_red = info
        .member_accesses
        .iter()
        .any(|a| a.object == "Color" && a.member == "Red");
    assert!(
        has_color_red,
        "Should capture Color.Red inside exported function body"
    );
}
