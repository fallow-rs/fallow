use fallow_types::extract::ImportedName;

use crate::tests::parse_ts as parse_source;

#[test]
fn detects_object_values_whole_use() {
    let info = parse_source("import { Status } from './types';\nObject.values(Status);");
    assert!(info.whole_object_uses.contains(&"Status".to_string()));
}

#[test]
fn detects_object_keys_whole_use() {
    let info = parse_source("import { Dir } from './types';\nObject.keys(Dir);");
    assert!(info.whole_object_uses.contains(&"Dir".to_string()));
}

#[test]
fn detects_object_entries_whole_use() {
    let info = parse_source("import { E } from './types';\nObject.entries(E);");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn detects_for_in_whole_use() {
    let info = parse_source("import { Color } from './types';\nfor (const k in Color) {}");
    assert!(info.whole_object_uses.contains(&"Color".to_string()));
}

#[test]
fn detects_spread_whole_use() {
    let info = parse_source("import { X } from './types';\nconst y = { ...X };");
    assert!(info.whole_object_uses.contains(&"X".to_string()));
}

#[test]
fn computed_member_string_literal_resolves() {
    let info = parse_source("import { Status } from './types';\nStatus[\"Active\"];");
    let has_access = info
        .member_accesses
        .iter()
        .any(|a| a.object == "Status" && a.member == "Active");
    assert!(
        has_access,
        "Status[\"Active\"] should resolve to a static member access"
    );
}

#[test]
fn computed_member_variable_marks_whole_use() {
    let info = parse_source("import { Status } from './types';\nconst k = 'foo';\nStatus[k];");
    assert!(info.whole_object_uses.contains(&"Status".to_string()));
}

#[test]
fn namespace_destructuring_generates_member_accesses() {
    let info = parse_source("import * as utils from './utils';\nconst { foo, bar } = utils;");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Namespace);
    let has_foo = info
        .member_accesses
        .iter()
        .any(|a| a.object == "utils" && a.member == "foo");
    let has_bar = info
        .member_accesses
        .iter()
        .any(|a| a.object == "utils" && a.member == "bar");
    assert!(
        has_foo,
        "Should capture destructured 'foo' as member access"
    );
    assert!(
        has_bar,
        "Should capture destructured 'bar' as member access"
    );
}

#[test]
fn namespace_destructuring_with_rest_marks_whole_object() {
    let info = parse_source("import * as utils from './utils';\nconst { foo, ...rest } = utils;");
    assert!(
        info.whole_object_uses.contains(&"utils".to_string()),
        "Rest pattern should mark namespace as whole-object use"
    );
}

#[test]
fn namespace_destructuring_from_dynamic_import() {
    let info = parse_source(
        "async function f() {\n  const mod = await import('./mod');\n  const { a, b } = mod;\n}",
    );
    let has_a = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "a");
    let has_b = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "b");
    assert!(
        has_a,
        "Should capture destructured 'a' from dynamic import namespace"
    );
    assert!(
        has_b,
        "Should capture destructured 'b' from dynamic import namespace"
    );
}

#[test]
fn namespace_destructuring_from_require() {
    let info = parse_source("const mod = require('./mod');\nconst { x, y } = mod;");
    let has_x = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "x");
    let has_y = info
        .member_accesses
        .iter()
        .any(|a| a.object == "mod" && a.member == "y");
    assert!(
        has_x,
        "Should capture destructured 'x' from require namespace"
    );
    assert!(
        has_y,
        "Should capture destructured 'y' from require namespace"
    );
}

#[test]
fn non_namespace_destructuring_not_captured() {
    let info =
        parse_source("import { foo } from './utils';\nconst obj = { a: 1 };\nconst { a } = obj;");
    let has_obj_a = info
        .member_accesses
        .iter()
        .any(|a| a.object == "obj" && a.member == "a");
    assert!(
        !has_obj_a,
        "Should not capture destructuring of non-namespace variables"
    );
}

/// Regression test for issue #845: a method call on a value narrowed by
/// `if (x instanceof ClassName)` must be credited as a use of
/// `ClassName.method`, preventing a false `unused-class-member` finding.
#[test]
fn instanceof_narrowed_method_call_is_credited_as_class_member_use() {
    let info = parse_source(
        r"
import { BaseException } from './exceptions';
function handle(e) {
    if (e instanceof BaseException) {
        e.getMessage();
    }
}
",
    );
    let has_access = info
        .member_accesses
        .iter()
        .any(|a| a.object == "BaseException" && a.member == "getMessage");
    assert!(
        has_access,
        "e.getMessage() inside `if (e instanceof BaseException)` must be \
         credited as BaseException.getMessage; got member_accesses = {:?}",
        info.member_accesses,
    );
}

/// Regression test for issue #845: `&&`-chained instanceof guards must all
/// contribute narrowings so each narrowed local's method calls are credited.
#[test]
fn instanceof_narrowing_through_logical_and_chain() {
    let info = parse_source(
        r"
import { FooError } from './foo';
import { BarError } from './bar';
function handle(a, b) {
    if (a instanceof FooError && b instanceof BarError) {
        a.getFooMessage();
        b.getBarMessage();
    }
}
",
    );
    let has_foo = info
        .member_accesses
        .iter()
        .any(|a| a.object == "FooError" && a.member == "getFooMessage");
    let has_bar = info
        .member_accesses
        .iter()
        .any(|a| a.object == "BarError" && a.member == "getBarMessage");
    assert!(
        has_foo,
        "a.getFooMessage() inside `if (a instanceof FooError && ...)` must be \
         credited as FooError.getFooMessage; got member_accesses = {:?}",
        info.member_accesses,
    );
    assert!(
        has_bar,
        "b.getBarMessage() inside `if (... && b instanceof BarError)` must be \
         credited as BarError.getBarMessage; got member_accesses = {:?}",
        info.member_accesses,
    );
}
