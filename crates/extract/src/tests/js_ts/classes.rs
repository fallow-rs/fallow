use fallow_types::extract::{ExportName, MemberKind};

use crate::tests::parse_ts as parse_source;

// ── Declaration extraction edge cases ────────────────────────────

#[test]
fn enum_with_string_values_extracts_members() {
    let info = parse_source(
        "export enum Status { Active = \"ACTIVE\", Inactive = \"INACTIVE\", Pending = \"PENDING\" }",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("Status".to_string())
    );
    assert_eq!(info.exports[0].members.len(), 3);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, vec!["Active", "Inactive", "Pending"]);
    assert!(
        info.exports[0]
            .members
            .iter()
            .all(|m| m.kind == MemberKind::EnumMember)
    );
}

#[test]
fn enum_with_numeric_values_extracts_members() {
    let info = parse_source("export enum HttpCode { OK = 200, NotFound = 404, ServerError = 500 }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 3);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, vec!["OK", "NotFound", "ServerError"]);
}

#[test]
fn enum_not_type_only() {
    // Enums are runtime values, not type-only
    let info = parse_source("export enum Color { Red, Green, Blue }");
    assert_eq!(info.exports.len(), 1);
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn const_enum_not_type_only() {
    let info = parse_source("export const enum Direction { Up, Down }");
    assert_eq!(info.exports.len(), 1);
    // const enums are still exported as values (unless isolated modules)
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn abstract_class_export_single_export() {
    let info = parse_source("export abstract class Base { abstract doWork(): void; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Base".to_string()));
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn abstract_class_with_concrete_members() {
    let info = parse_source(
        r"export abstract class Base {
            abstract doWork(): void;
            getName() { return 'base'; }
            label: string = 'base';
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let members: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    // Abstract methods and concrete methods/properties are all tracked
    assert!(members.contains(&"doWork"));
    assert!(members.contains(&"getName"));
    assert!(members.contains(&"label"));
}

#[test]
fn class_private_members_excluded() {
    let info = parse_source(
        r"export class Svc {
            private secret: string = '';
            private doSecret() {}
            public visible() {}
            name: string = '';
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(
        !names.contains(&"secret"),
        "Private property should be excluded"
    );
    assert!(
        !names.contains(&"doSecret"),
        "Private method should be excluded"
    );
    assert!(
        names.contains(&"visible"),
        "Public method should be included"
    );
    assert!(
        names.contains(&"name"),
        "Unadorned property should be included"
    );
}

#[test]
fn class_protected_members_excluded() {
    let info = parse_source(
        r"export class Base {
            protected internalMethod() {}
            protected internalProp: number = 0;
            publicMethod() {}
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(
        !names.contains(&"internalMethod"),
        "Protected method should be excluded"
    );
    assert!(
        !names.contains(&"internalProp"),
        "Protected property should be excluded"
    );
    assert!(
        names.contains(&"publicMethod"),
        "Public method should be included"
    );
}

#[test]
fn class_decorated_members_tracked() {
    let info = parse_source(
        r"export class Controller {
            @Get('/users')
            getUsers() { return []; }
            @Post('/users')
            createUser() {}
            plain() {}
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let get_users = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "getUsers")
        .expect("getUsers should be in members");
    assert!(
        get_users.has_decorator,
        "getUsers should have has_decorator = true"
    );
    let create_user = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "createUser")
        .expect("createUser should be in members");
    assert!(
        create_user.has_decorator,
        "createUser should have has_decorator = true"
    );
    let plain = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "plain")
        .expect("plain should be in members");
    assert!(
        !plain.has_decorator,
        "plain should have has_decorator = false"
    );
    assert_eq!(
        get_users.decorator_names,
        vec!["Get".to_string()],
        "getUsers decorator path should be captured as 'Get'"
    );
    assert_eq!(
        create_user.decorator_names,
        vec!["Post".to_string()],
        "createUser decorator path should be captured as 'Post'"
    );
    assert!(
        plain.decorator_names.is_empty(),
        "plain has no decorators; decorator_names should be empty"
    );
}

#[test]
fn class_decorator_path_shapes() {
    // Exercise the five decorator AST shapes the visitor must handle: bare
    // identifier, call expression, namespaced static-member, namespaced
    // static-member call, and multi-level static-member chain. Issue #471.
    let info = parse_source(
        r"export class Demo {
            @Bare
            bare() {}
            @Call('arg')
            call() {}
            @ns.namespaced
            namespaced() {}
            @ns.namespaced_call('arg')
            namespacedCall() {}
            @a.b.c
            multiLevel() {}
        }",
    );
    let demo = &info.exports[0];
    let by_name = |name: &str| {
        demo.members
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} should be in members"))
    };
    assert_eq!(by_name("bare").decorator_names, vec!["Bare".to_string()]);
    assert_eq!(by_name("call").decorator_names, vec!["Call".to_string()]);
    assert_eq!(
        by_name("namespaced").decorator_names,
        vec!["ns.namespaced".to_string()]
    );
    assert_eq!(
        by_name("namespacedCall").decorator_names,
        vec!["ns.namespaced_call".to_string()]
    );
    assert_eq!(
        by_name("multiLevel").decorator_names,
        vec!["a.b.c".to_string()]
    );
}

#[test]
fn class_decorated_properties_tracked() {
    let info = parse_source(
        r"export class Entity {
            @Column()
            name: string = '';
            @Column()
            age: number = 0;
            undecorated: boolean = false;
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let name_prop = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "name")
        .expect("name should be in members");
    assert!(name_prop.has_decorator);
    assert_eq!(name_prop.kind, MemberKind::ClassProperty);
    let undecorated = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "undecorated")
        .expect("undecorated should be in members");
    assert!(!undecorated.has_decorator);
    assert!(undecorated.decorator_names.is_empty());
    assert_eq!(
        name_prop.decorator_names,
        vec!["Column".to_string()],
        "Column-decorated property should capture the decorator name"
    );
}

#[test]
fn class_member_kinds_correct() {
    let info = parse_source(
        r"export class MyClass {
            method() {}
            prop: string = '';
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let method = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "method")
        .unwrap();
    assert_eq!(method.kind, MemberKind::ClassMethod);
    let prop = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "prop")
        .unwrap();
    assert_eq!(prop.kind, MemberKind::ClassProperty);
}

#[test]
fn function_overloads_different_names_not_deduplicated() {
    let info = parse_source("export function foo(): void {}\nexport function bar(): void {}");
    assert_eq!(
        info.exports.len(),
        2,
        "Different function names should produce separate exports"
    );
    assert_eq!(info.exports[0].name, ExportName::Named("foo".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("bar".to_string()));
}

#[test]
fn function_overloads_many_signatures_single_export() {
    let info = parse_source(
        r"export function create(): void;
export function create(name: string): void;
export function create(name: string, age: number): void;
export function create(name?: string, age?: number): void {}",
    );
    assert_eq!(
        info.exports.len(),
        1,
        "Four overload signatures should deduplicate to 1 export"
    );
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("create".to_string())
    );
}

#[test]
fn multiple_variable_declarations_in_one_export() {
    let info = parse_source("export const a = 1, b = 'two', c = true;");
    assert_eq!(info.exports.len(), 3);
    assert_eq!(info.exports[0].name, ExportName::Named("a".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("b".to_string()));
    assert_eq!(info.exports[2].name, ExportName::Named("c".to_string()));
}

#[test]
fn destructured_export_with_defaults() {
    let info = parse_source("export const { a = 1, b = 2 } = obj;");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("a".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("b".to_string()));
}

#[test]
fn deeply_nested_array_destructured_export() {
    let info = parse_source("export const [[a], [b, c]] = nested;");
    assert_eq!(info.exports.len(), 3);
    assert_eq!(info.exports[0].name, ExportName::Named("a".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("b".to_string()));
    assert_eq!(info.exports[2].name, ExportName::Named("c".to_string()));
}

#[test]
fn mixed_object_array_destructured_export() {
    let info = parse_source("export const { items: [first, second] } = config;");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("first".to_string()));
    assert_eq!(
        info.exports[1].name,
        ExportName::Named("second".to_string())
    );
}

#[test]
fn destructured_export_with_rename() {
    let info = parse_source("export const { original: renamed } = obj;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("renamed".to_string())
    );
}

#[test]
fn require_namespace_binding_captures_local_name() {
    let info = parse_source("const fs = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "fs");
    assert_eq!(
        info.require_calls[0].local_name,
        Some("fs".to_string()),
        "Namespace require should capture the local binding name"
    );
    assert!(info.require_calls[0].destructured_names.is_empty());
}

#[test]
fn require_destructured_no_local_name() {
    let info = parse_source("const { readFile, writeFile } = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert!(
        info.require_calls[0].local_name.is_none(),
        "Destructured require should have no local_name"
    );
    assert_eq!(
        info.require_calls[0].destructured_names,
        vec!["readFile", "writeFile"]
    );
}

#[test]
fn ts_module_declaration_identifier() {
    let info = parse_source("export declare module MyModule {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("MyModule".to_string())
    );
    assert!(info.exports[0].is_type_only);
}

#[test]
fn ts_namespace_declaration() {
    let info = parse_source("export namespace Utils { export function helper() {} }");
    // Only the namespace itself is a top-level export; inner exports become members
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Utils".to_string()));
    // Runtime namespace (no `declare`) is NOT type-only
    assert!(!info.exports[0].is_type_only);
    // Inner function extracted as namespace member
    assert_eq!(info.exports[0].members.len(), 1);
    assert_eq!(info.exports[0].members[0].name, "helper");
    assert_eq!(info.exports[0].members[0].kind, MemberKind::NamespaceMember);
}

#[test]
fn ts_declare_namespace_is_type_only() {
    let info = parse_source("export declare namespace Types { export type Foo = string; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Types".to_string()));
    assert!(info.exports[0].is_type_only);
}

#[test]
fn ts_namespace_multiple_members() {
    let info = parse_source(
        "export namespace BusinessHelper {
            export async function inviteSupplier() {}
            export async function toggleSuspension() {}
            export const API_URL = 'https://example.com';
        }",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("BusinessHelper".to_string())
    );
    assert!(!info.exports[0].is_type_only);
    assert_eq!(info.exports[0].members.len(), 3);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(names.contains(&"inviteSupplier"));
    assert!(names.contains(&"toggleSuspension"));
    assert!(names.contains(&"API_URL"));
    assert!(
        info.exports[0]
            .members
            .iter()
            .all(|m| m.kind == MemberKind::NamespaceMember)
    );
}

#[test]
fn ts_namespace_inner_exports_not_top_level() {
    let info = parse_source(
        "export namespace Ns { export function a() {} export class B {} export enum C {} }",
    );
    // Only the namespace should be a top-level export
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Ns".to_string()));
    // All inner declarations should be namespace members
    assert_eq!(info.exports[0].members.len(), 3);
}

#[test]
fn ts_nested_namespace() {
    let info = parse_source(
        "export namespace Outer { export namespace Inner { export function deep() {} } }",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Outer".to_string()));
    // Inner namespace and its contents are flattened into Outer's members
    assert_eq!(info.exports[0].members.len(), 2);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(names.contains(&"Inner"));
    assert!(names.contains(&"deep"));
}

#[test]
fn export_let_declaration() {
    let info = parse_source("export let mutable = 42;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("mutable".to_string())
    );
}

#[test]
fn export_var_declaration() {
    let info = parse_source("export var legacy = 'old';");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("legacy".to_string())
    );
}

#[test]
fn export_async_function() {
    let info = parse_source("export async function fetchData() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("fetchData".to_string())
    );
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn export_generator_function() {
    let info = parse_source("export function* generateItems() { yield 1; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("generateItems".to_string())
    );
}

#[test]
fn type_alias_always_type_only() {
    let info = parse_source(
        "export type Result<T> = { ok: true; data: T } | { ok: false; error: string };",
    );
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].is_type_only);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("Result".to_string())
    );
}

#[test]
fn interface_always_type_only() {
    let info = parse_source(
        "export interface Config { debug: boolean; verbose: boolean; output: string; }",
    );
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].is_type_only);
}

#[test]
fn interface_extending_another_type_only() {
    let info =
        parse_source("export interface ExtendedConfig extends BaseConfig { extra: boolean; }");
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].is_type_only);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("ExtendedConfig".to_string())
    );
}

#[test]
fn dynamic_import_then_destructuring_captures_member_accesses() {
    let info = parse_source(
        r"async function load() {
            const mod = await import('./service');
            const { handler, middleware } = mod;
        }",
    );
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
    let has_handler = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "handler");
    let has_middleware = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "middleware");
    assert!(
        has_handler,
        "Should capture 'handler' from namespace destructuring"
    );
    assert!(
        has_middleware,
        "Should capture 'middleware' from namespace destructuring"
    );
}

#[test]
fn namespace_destructuring_rest_marks_whole_object_for_require() {
    let info = parse_source("const mod = require('./mod');\nconst { a, ...rest } = mod;");
    assert!(
        info.whole_object_uses.contains(&"mod".to_string()),
        "Rest pattern in require namespace destructuring should mark whole-object use"
    );
}

#[test]
fn export_default_class() {
    let info = parse_source("export default class MyClass {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_anonymous_class() {
    let info = parse_source("export default class {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_expression_literal() {
    let info = parse_source("export default 'hello';");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_object_expression() {
    let info = parse_source("export default { key: 'value' };");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn class_static_method_tracked() {
    let info = parse_source(
        r"export class Factory {
            static create() { return new Factory(); }
            instance() {}
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(names.contains(&"create"), "Static method should be tracked");
    assert!(
        names.contains(&"instance"),
        "Instance method should be tracked"
    );
}

#[test]
fn class_getter_setter_tracked() {
    let info = parse_source(
        r"export class Config {
            get value() { return this._value; }
            set value(v: string) { this._value = v; }
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let has_value = info.exports[0].members.iter().any(|m| m.name == "value");
    assert!(has_value, "Getter/setter should be tracked as member");
}

#[test]
fn enum_single_member() {
    let info = parse_source("export enum Single { Only }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 1);
    assert_eq!(info.exports[0].members[0].name, "Only");
}

#[test]
fn enum_empty() {
    let info = parse_source("export enum Empty {}");
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].members.is_empty());
}

#[test]
fn enum_string_literal_member_name() {
    // Enum members can use string literal keys
    let info = parse_source("export enum Weird { 'hello-world' = 1 }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 1);
    assert_eq!(info.exports[0].members[0].name, "hello-world");
}

#[test]
fn multiple_type_exports_all_type_only() {
    let info = parse_source(
        "export type A = string;\nexport type B = number;\nexport interface C { x: boolean; }",
    );
    assert_eq!(info.exports.len(), 3);
    assert!(info.exports.iter().all(|e| e.is_type_only));
}

#[test]
fn mixed_value_and_type_exports() {
    let info = parse_source(
        "export const value = 1;\nexport type TypeAlias = string;\nexport function fn() {}",
    );
    assert_eq!(info.exports.len(), 3);
    assert!(
        !info.exports[0].is_type_only,
        "const should not be type-only"
    );
    assert!(
        info.exports[1].is_type_only,
        "type alias should be type-only"
    );
    assert!(
        !info.exports[2].is_type_only,
        "function should not be type-only"
    );
}

#[test]
fn array_destructured_export_with_skip() {
    // Skipping elements in array destructuring with holes
    let info = parse_source("export const [, second, , fourth] = arr;");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("second".to_string())
    );
    assert_eq!(
        info.exports[1].name,
        ExportName::Named("fourth".to_string())
    );
}

#[test]
fn object_destructured_export_with_rest() {
    let info = parse_source("export const { a, b, ...rest } = obj;");
    assert_eq!(info.exports.len(), 3);
    assert_eq!(info.exports[0].name, ExportName::Named("a".to_string()));
    assert_eq!(info.exports[1].name, ExportName::Named("b".to_string()));
    assert_eq!(info.exports[2].name, ExportName::Named("rest".to_string()));
}

#[test]
fn array_destructured_export_with_rest() {
    let info = parse_source("export const [first, ...remaining] = arr;");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].name, ExportName::Named("first".to_string()));
    assert_eq!(
        info.exports[1].name,
        ExportName::Named("remaining".to_string())
    );
}

#[test]
fn export_local_name_matches_for_simple_declarations() {
    let info = parse_source("export const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].local_name,
        Some("foo".to_string()),
        "local_name should match the binding name"
    );
}

#[test]
fn export_specifier_with_as_default() {
    // `export { foo as default }` uses a named specifier with "default" as the exported name
    let info = parse_source("const foo = 1;\nexport { foo as default };");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("default".to_string())
    );
}

// ── Class member extraction: static properties ──────────────

#[test]
fn class_static_property_tracked() {
    let info = parse_source(
        r"export class Foo {
            static count = 0;
            static label: string = 'default';
            regular: number = 1;
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let names: Vec<&str> = info.exports[0]
        .members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert!(
        names.contains(&"count"),
        "Static property 'count' should be tracked"
    );
    assert!(
        names.contains(&"label"),
        "Static property 'label' should be tracked"
    );
    assert!(
        names.contains(&"regular"),
        "Regular property should also be tracked"
    );
}

// ── Class member extraction: getter/setter kinds ────────────

#[test]
fn class_getter_setter_are_class_method_kind() {
    let info = parse_source(
        r"export class Config {
            get value() { return this._value; }
            set value(v: string) { this._value = v; }
            normal() {}
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let value_members: Vec<_> = info.exports[0]
        .members
        .iter()
        .filter(|m| m.name == "value")
        .collect();
    assert!(
        !value_members.is_empty(),
        "Getter/setter 'value' should be present"
    );
    assert!(
        value_members
            .iter()
            .all(|m| m.kind == MemberKind::ClassMethod),
        "Getter/setter should have ClassMethod kind"
    );
    let normal = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "normal")
        .unwrap();
    assert_eq!(normal.kind, MemberKind::ClassMethod);
}

// ── Class member extraction: decorated property ─────────────

#[test]
fn class_decorated_property_with_column_decorator() {
    let info = parse_source(
        r"export class Entity {
            @Column()
            name: string = '';
            age: number = 0;
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let name_member = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "name")
        .expect("name should be in members");
    assert!(
        name_member.has_decorator,
        "@Column() decorated member should have has_decorator = true"
    );
    assert_eq!(name_member.kind, MemberKind::ClassProperty);
    let age_member = info.exports[0]
        .members
        .iter()
        .find(|m| m.name == "age")
        .expect("age should be in members");
    assert!(
        !age_member.has_decorator,
        "Undecorated member should have has_decorator = false"
    );
}

// ── Instance member tracking via new expression ─────────────

#[test]
fn instance_member_access_via_new_expression() {
    let info = parse_source(
        r"import { MyService } from './service';
        const svc = new MyService();
        svc.greet();
        svc.initialize();",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "greet"),
        "svc.greet() should be mapped to MyService.greet, found: {:?}",
        info.member_accesses
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "initialize"),
        "svc.initialize() should be mapped to MyService.initialize, found: {:?}",
        info.member_accesses
    );
}

// ── Builtin constructor not tracked ─────────────────────────

#[test]
fn builtin_constructor_instance_not_tracked() {
    let info = parse_source(
        r"const arr = new Array();
        arr.push(1);
        const url = new URL('https://example.com');
        url.hostname;",
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Array"),
        "new Array() should not create instance binding for member tracking"
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "URL"),
        "new URL() should not create instance binding for member tracking"
    );
}

// ── Class with mixed accessibility and decorators ───────────

#[test]
fn class_mixed_members_comprehensive() {
    let info = parse_source(
        r"export class Service {
            static version = '1.0';
            @Inject()
            private db: Database;
            protected logger: Logger;
            public name: string = '';
            constructor(db: Database) { this.db = db; }
            private connect() {}
            protected log() {}
            handle() {}
            @Get('/health')
            healthCheck() {}
        }",
    );
    assert_eq!(info.exports.len(), 1);
    let members = &info.exports[0].members;
    let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();

    // Public and static members included
    assert!(
        names.contains(&"version"),
        "Static property should be included"
    );
    assert!(
        names.contains(&"name"),
        "Public property should be included"
    );
    assert!(
        names.contains(&"handle"),
        "Public method should be included"
    );
    assert!(
        names.contains(&"healthCheck"),
        "Decorated public method should be included"
    );

    // Private, protected, and constructor excluded
    assert!(
        !names.contains(&"db"),
        "Private property should be excluded"
    );
    assert!(
        !names.contains(&"logger"),
        "Protected property should be excluded"
    );
    assert!(
        !names.contains(&"constructor"),
        "Constructor should be excluded"
    );
    assert!(
        !names.contains(&"connect"),
        "Private method should be excluded"
    );
    assert!(
        !names.contains(&"log"),
        "Protected method should be excluded"
    );

    // Decorator tracking
    let health_check = members.iter().find(|m| m.name == "healthCheck").unwrap();
    assert!(
        health_check.has_decorator,
        "healthCheck should have has_decorator = true"
    );
    let handle = members.iter().find(|m| m.name == "handle").unwrap();
    assert!(
        !handle.has_decorator,
        "handle should have has_decorator = false"
    );
}

// ── super.member access tracking ────────────────────────────

#[test]
fn super_method_access_attributed_to_parent_class() {
    // `super.speak()` in a subclass is a use of the parent's `speak`.
    // Must be recorded with `object` = the local `extends` identifier, so
    // `local_to_imported` in unused-member analysis can map it to the
    // parent's export name. See issue #130.
    let info = parse_source(
        r"import { Animal } from './animal';
        export class Dog extends Animal {
            bark(): string { return super.speak() + ' Woof!'; }
        }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Animal" && a.member == "speak"),
        "super.speak() should be recorded as Animal.speak, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn super_property_access_attributed_to_parent_class() {
    // Property access through `super` should also be credited to the parent.
    let info = parse_source(
        r"import { Base } from './base';
        export class Child extends Base {
            read(): string { return super.name; }
        }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Base" && a.member == "name"),
        "super.name should be recorded as Base.name, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn super_access_same_file_parent_uses_local_name() {
    // Parent class defined in the same file: the `extends` identifier is
    // a local name, not an import. `find_unused_members` falls back to
    // the raw object name when no import mapping exists.
    let info = parse_source(
        r"class Animal { speak(): string { return 'base'; } }
        export class Dog extends Animal {
            bark(): string { return super.speak() + '!'; }
        }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Animal" && a.member == "speak"),
        "super.speak() should resolve to Animal.speak even when Animal is local, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn super_access_in_object_literal_method_is_dropped() {
    // `super.X` inside an object literal method is syntactically valid JS
    // (refers to the prototype chain). We cannot attribute it to a class,
    // so it must be silently dropped. This exercises the `None` branch of
    // the `class_super_stack.last()` guard: the object method is visited
    // without any class frame on the stack.
    let info = parse_source(
        r"export const obj = {
            greet() { return super.toString(); }
        };",
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.member == "toString"),
        "super.toString() in an object method should not emit an access, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn super_access_respects_nested_class_boundary() {
    // When a class expression is nested inside an outer class's method,
    // `super` inside the INNER class must resolve to the INNER's parent,
    // not the outer's. Tests the push/pop stack discipline.
    let info = parse_source(
        r"import { Outer } from './outer';
        import { Inner } from './inner';
        export class ChildOfOuter extends Outer {
            make(): unknown {
                return new (class extends Inner {
                    call() { return super.run(); }
                })();
            }
        }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Inner" && a.member == "run"),
        "super.run() inside inner class should resolve to Inner.run, found: {:?}",
        info.member_accesses
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Outer" && a.member == "run"),
        "super.run() must not leak to Outer: {:?}",
        info.member_accesses
    );
}

// ── Static factory methods (issue #346) ─────────────────────

#[test]
fn static_factory_method_returning_new_this_is_flagged() {
    let info = parse_source(
        r"export class MyClass {
            static getInstance() { return new this(); }
            getData() { return [1, 2, 3]; }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"))
        .expect("MyClass export should be present");
    let get_instance = class_export
        .members
        .iter()
        .find(|m| m.name == "getInstance")
        .expect("getInstance member should be present");
    assert!(
        get_instance.is_instance_returning_static,
        "static getInstance() {{ return new this(); }} should be flagged as instance-returning"
    );
    let get_data = class_export
        .members
        .iter()
        .find(|m| m.name == "getData")
        .expect("getData member should be present");
    assert!(
        !get_data.is_instance_returning_static,
        "instance method getData must not be flagged as instance-returning static"
    );
}

#[test]
fn static_factory_method_returning_new_same_class_name_is_flagged() {
    let info = parse_source(
        r"export class Service {
            static create() { return new Service(); }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Service"))
        .expect("Service export should be present");
    let create = class_export
        .members
        .iter()
        .find(|m| m.name == "create")
        .expect("create member should be present");
    assert!(
        create.is_instance_returning_static,
        "static create() {{ return new Service(); }} should be flagged as instance-returning"
    );
}

#[test]
fn static_factory_method_returning_other_class_not_flagged() {
    // `new SomeOther()` is not an instance of MyClass, so the static method
    // must NOT be flagged as instance-returning. Conservative: only
    // `new this()` or `new <SameClassName>()` qualifies.
    let info = parse_source(
        r"export class MyClass {
            static getBuilder() { return new Builder(); }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"))
        .expect("MyClass export should be present");
    let get_builder = class_export
        .members
        .iter()
        .find(|m| m.name == "getBuilder")
        .expect("getBuilder member should be present");
    assert!(
        !get_builder.is_instance_returning_static,
        "factory returning a different class must not be flagged"
    );
}

#[test]
fn instance_method_with_new_this_return_not_flagged() {
    // The flag applies only to STATIC methods. An instance method returning
    // `new this.constructor()` or similar is a different pattern out of scope.
    let info = parse_source(
        r"export class MyClass {
            clone() { return new this.constructor(); }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"))
        .expect("MyClass export should be present");
    let clone = class_export
        .members
        .iter()
        .find(|m| m.name == "clone")
        .expect("clone member should be present");
    assert!(
        !clone.is_instance_returning_static,
        "instance method must not be flagged regardless of return shape"
    );
}

#[test]
fn static_factory_binding_emits_sentinel_member_access() {
    // Cross-file case: `MyClass` comes from an import, so `resolve_factory_call_candidates`
    // takes the import-match branch and emits a sentinel binding the analyze
    // layer decodes. The visitor surface check is that the sentinel access
    // appears on `myInstance.getData()`.
    let info = parse_source(
        r"import { MyClass } from './my-class';
        const myInstance = MyClass.getInstance();
        myInstance.getData();",
    );
    let sentinel_access = info
        .member_accesses
        .iter()
        .find(|a| a.object.starts_with(crate::FACTORY_CALL_SENTINEL))
        .expect("sentinel-prefixed access should be emitted for the factory call result");
    assert_eq!(sentinel_access.member, "getData");
    assert!(
        sentinel_access.object.ends_with("MyClass:getInstance"),
        "sentinel should encode the call shape, got: {}",
        sentinel_access.object
    );
}

#[test]
fn static_factory_binding_same_file_emits_direct_access() {
    // Same-file case: `MyClass` is a locally declared class with an
    // instance-returning static method. `resolve_factory_call_candidates`
    // takes the local-class branch and binds directly to `MyClass` instead
    // of the sentinel. The visitor surface check is the direct
    // `MemberAccess { object: "MyClass", member: "getData" }`.
    let info = parse_source(
        r"export class MyClass {
            static getInstance() { return new this(); }
            getData() { return [1, 2, 3]; }
        }
        const myInstance = MyClass.getInstance();
        myInstance.getData();",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyClass" && a.member == "getData"),
        "same-file factory call should expand `myInstance.getData` to `MyClass.getData`, found: {:?}",
        info.member_accesses
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object.starts_with(crate::FACTORY_CALL_SENTINEL)),
        "same-file case must not emit a sentinel access: {:?}",
        info.member_accesses
    );
}

#[test]
fn static_factory_with_early_guard_return_is_flagged() {
    // `if (cond) return null;` is a top-level `IfStatement` (the nested
    // `return null` lives inside the consequent block) so it does not count
    // as a top-level qualifying return. The last top-level statement is
    // `return new this()`, which IS the canonical factory shape, so the
    // method must stay flagged as instance-returning.
    let info = parse_source(
        r"export class MyClass {
            static getInstance(cond: boolean) {
                if (cond) return null;
                return new this();
            }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"))
        .expect("MyClass export should be present");
    let get_instance = class_export
        .members
        .iter()
        .find(|m| m.name == "getInstance")
        .expect("getInstance member should be present");
    assert!(
        get_instance.is_instance_returning_static,
        "factory with an early guard return must still be flagged as instance-returning"
    );
}

#[test]
fn static_method_returning_conditional_expression_is_not_flagged() {
    // `return cond ? new this() : null;` returns a ConditionalExpression,
    // not a bare NewExpression. The conservative detection requires the
    // return argument to be a `new ...` directly, so this shape must NOT
    // be flagged.
    let info = parse_source(
        r"export class MyClass {
            static maybe(cond: boolean) {
                return cond ? new this() : null;
            }
        }",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"))
        .expect("MyClass export should be present");
    let maybe = class_export
        .members
        .iter()
        .find(|m| m.name == "maybe")
        .expect("maybe member should be present");
    assert!(
        !maybe.is_instance_returning_static,
        "ConditionalExpression in the return argument must not flag the method"
    );
}

#[test]
fn factory_call_candidate_with_unknown_object_is_dropped() {
    // `Math.floor()` is not a known import and not a local class. The
    // visitor must not emit a sentinel-prefixed access for it.
    let info = parse_source(
        r"const n = Math.floor(1.5);
        n.toString();",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object.starts_with(crate::FACTORY_CALL_SENTINEL)),
        "calls on globals must not produce a sentinel binding: {:?}",
        info.member_accesses
    );
}
