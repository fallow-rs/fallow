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

#[test]
fn template_literal_new_class_credits_to_string() {
    let info =
        parse_source("import { Money } from './money';\nconst label = `Total: ${new Money(5)}`;");
    let has_access = info
        .member_accesses
        .iter()
        .any(|a| a.object == "Money" && a.member == "toString");
    assert!(
        has_access,
        "`${{new Money()}}` in a template literal must credit Money.toString; \
         got member_accesses = {:?}",
        info.member_accesses,
    );
}

#[test]
fn string_call_new_class_credits_to_string() {
    let info = parse_source("import { Money } from './money';\nconst s = String(new Money(1));");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "String(new Money()) must credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn string_concat_new_class_credits_to_string() {
    let info = parse_source("import { Money } from './money';\nconst s = '' + new Money(1);");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "'' + new Money() must credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn string_concat_prefix_new_class_credits_to_string() {
    let info =
        parse_source("import { Money } from './money';\nconst s = new Money(1) + ' suffix';");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "new Money() + ' suffix' must credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn numeric_plus_new_class_does_not_credit_to_string() {
    // Sibling operand is a numeric literal, not a string: no coercion proof.
    let info = parse_source("import { Num } from './num';\nconst n = new Num() + 5;");
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Num" && a.member == "toString"),
        "new Num() + 5 must NOT credit Num.toString (numeric context); got {:?}",
        info.member_accesses,
    );
}

#[test]
fn bare_new_class_not_in_coercion_does_not_credit_to_string() {
    // A constructed instance NOT in a coercion position must not be credited.
    let info = parse_source("import { Money } from './money';\nconst m = new Money(1);");
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "a bare `new Money()` must NOT credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn tagged_template_new_class_does_not_credit_to_string() {
    // A tagged template's tag function receives the raw values and does NOT
    // coerce interpolations via toString (Lit `html`, styled-components, gql),
    // so a direct `new Money()` interpolation must NOT credit Money.toString.
    // Regression for #1638 (tagged-template over-credit).
    let info = parse_source(
        "import { Money } from './money';\nconst t = html`<span>${new Money(1)}</span>`;",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "a tagged-template interpolation must NOT credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn nested_plain_template_in_tagged_credits_to_string() {
    // A plain template literal nested inside a tagged-template interpolation
    // still coerces, so the inner `${new Money()}` credits Money.toString.
    let info = parse_source(
        "import { Money } from './money';\nconst t = html`<span>${`x ${new Money(1)}`}</span>`;",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Money" && a.member == "toString"),
        "a plain template nested in a tagged interpolation must credit Money.toString; got {:?}",
        info.member_accesses,
    );
}

// Issue #1793: array-typed formal parameter iteration binding. The TOP-LEVEL
// function shape is load-bearing: recording the element type from the function
// declaration scope (not `visit_formal_parameter`) is required because the
// function-body scoped frame is not yet pushed at param-visit time, so a
// top-level function's params would otherwise be silently dropped.

#[test]
fn top_level_fn_array_param_for_of_credits_element_class_member() {
    let info = parse_source(
        "class Item { used() {} }\n\
         function run(items: Item[]) {\n\
           for (const item of items) {\n\
             item.used();\n\
           }\n\
         }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Item" && a.member == "used"),
        "a for-of over an array-typed top-level fn param must credit Item.used; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn top_level_fn_array_param_map_callback_credits_element_class_member() {
    let info = parse_source(
        "class Item { used() {} }\n\
         function run(items: Item[]) {\n\
           items.forEach((item) => item.used());\n\
         }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Item" && a.member == "used"),
        "a .forEach callback over an array-typed top-level fn param must credit Item.used; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn top_level_fn_builtin_array_param_records_no_class_binding() {
    // `number[]` has a builtin element, so the loop variable must NOT bind to
    // any class; `Item.used` must not appear.
    let info = parse_source(
        "class Item { used() {} }\n\
         function run(items: number[]) {\n\
           for (const item of items) {\n\
             item.used();\n\
           }\n\
         }",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Item" && a.member == "used"),
        "a number[] element must not bind the loop variable to a class; got {:?}",
        info.member_accesses,
    );
}

// Issue #1793: `Promise.all(arr.map(cb))` element inference.

#[test]
fn promise_all_map_credits_element_from_return_type_declared_after_consumer() {
    // `makeThing` is declared AFTER `consume`, proving the pre-pass makes the
    // inference declaration-order independent.
    let info = parse_source(
        "class Thing { greet() {} }\n\
         async function consume() {\n\
           const xs = await Promise.all(['a', 'b'].map(async (n) => makeThing(n)));\n\
           xs.forEach((x) => x.greet());\n\
         }\n\
         async function makeThing(n: string): Promise<Thing> {\n\
           return new Thing();\n\
         }",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Thing" && a.member == "greet"),
        "Promise.all(arr.map(async n => makeThing(n))) must type xs as Thing[] via makeThing's Promise<Thing> return; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn promise_all_settled_map_records_no_element_binding() {
    let info = parse_source(
        "class Thing { greet() {} }\n\
         async function consume() {\n\
           const xs = await Promise.allSettled(['a'].map(async (n) => makeThing(n)));\n\
           xs.forEach((x) => x.greet());\n\
         }\n\
         async function makeThing(n: string): Promise<Thing> {\n\
           return new Thing();\n\
         }",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Thing" && a.member == "greet"),
        "Promise.allSettled must not type xs (only `all` yields the resolved element array); got {:?}",
        info.member_accesses,
    );
}

#[test]
fn promise_all_map_imported_callee_records_no_element_binding() {
    // An imported callee has no local return type in the pre-pass map.
    let info = parse_source(
        "import { makeThing } from './factory';\n\
         class Thing { greet() {} }\n\
         async function consume() {\n\
           const xs = await Promise.all(['a'].map(async (n) => makeThing(n)));\n\
           xs.forEach((x) => x.greet());\n\
         }",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Thing" && a.member == "greet"),
        "an imported map-callback callee must not credit an element class; got {:?}",
        info.member_accesses,
    );
}

#[test]
fn promise_all_map_multi_statement_callback_records_no_element_binding() {
    let info = parse_source(
        "class Thing { greet() {} }\n\
         async function consume() {\n\
           const xs = await Promise.all(['a'].map(async (n) => {\n\
             if (n) {\n\
               return makeThing(n);\n\
             }\n\
             return makeThing(n);\n\
           }));\n\
           xs.forEach((x) => x.greet());\n\
         }\n\
         async function makeThing(n: string): Promise<Thing> {\n\
           return new Thing();\n\
         }",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Thing" && a.member == "greet"),
        "a multi-statement conditional-return callback must not credit an element class; got {:?}",
        info.member_accesses,
    );
}
