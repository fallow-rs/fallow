//! Positions that must never produce a mutant. Each fixture has a control
//! expression that must still produce mutants, so the tests fail when the
//! collector is off.

mod common;

use common::{assert_only_in_control, run};
use fallow_mutation::{Operator, SourceKind};

const TS: SourceKind = SourceKind::Ts;

#[test]
fn type_positions() {
    let source = r#"
let v: "lit" | true = cast as unknown as "x";
function f<T extends "a" = "b">(p: T, q: false): "r" { throw p; }
const g = (x: "y"): x is "y" => keep - 1 > 0;
"#;
    assert_only_in_control(source, TS, "keep - 1 > 0");
}

#[test]
fn interfaces_type_aliases_and_declarations() {
    let source = r#"
interface I { a: "x"; b: true }
type T = "y" | false;
declare const c: "z";
declare function d(x: number): "w";
declare class K { m(): boolean }
declare module "m" { const q: "v"; }
let keep = a + b;
"#;
    assert_only_in_control(source, TS, "a + b");
}

#[test]
fn overload_signatures() {
    let source = r#"
function over(a: "s"): true;
function over(a: number): false;
function over(a: unknown) { return keep * 2; }
"#;
    assert_only_in_control(source, TS, "keep * 2");
}

#[test]
fn enum_and_const_enum_members() {
    let source = r#"
enum E { A = "a", B = 1 + 2, C = -1 }
const enum C { X = "x", Y = 2 * 3 }
let keep = a + b;
"#;
    assert_only_in_control(source, TS, "a + b");
}

#[test]
fn decorators_and_their_arguments() {
    let source = r#"
@dec("arg", 1 + 2, true)
class A {
  @prop(!x) field = keep + 1;
  method(@inject("t") p: string) {}
}
"#;
    assert_only_in_control(source, TS, "keep + 1");
}

#[test]
fn import_and_export_declarations() {
    let source = r#"
import a from "a";
import { "s" as s } from "m";
import json from "./x.json" with { type: "json" };
import type { T } from "t";
export { a as "b" };
export * from "x";
export * as ns from "y";
export { s } from "z";
export const keep = a + 1;
"#;
    assert_only_in_control(source, TS, "a + 1");
}

#[test]
fn directives() {
    let source = r#""use strict";
function f() {
  "use client";
  return keep + 1;
}
"#;
    assert_only_in_control(source, TS, "keep + 1");
}

#[test]
fn assignment_targets() {
    let source = r#"
obj[i + 1] = keep + 1;
[x = "d", y = true] = arr;
({ p: q = !z } = o);
for (const [k = "n"] of list) {}
"#;
    assert_only_in_control(source, TS, "keep + 1");
}

#[test]
fn update_and_delete_operands() {
    let source = "arr[i + 1]++;\ndelete obj[k + 1];\n";
    let result = run(source, TS);
    let operators: Vec<_> = result.mutants.iter().map(|m| m.operator).collect();
    assert_eq!(operators, vec![Operator::UpdateOperator]);
}

#[test]
fn tagged_template_tags() {
    let source = "(cond ? tagA : tagB)`x`;\nString.raw`y${keep + 1}`;\n";
    let result = run(source, TS);
    let inner: Vec<_> = result
        .mutants
        .iter()
        .filter(|m| m.operator != Operator::RemovedCall)
        .map(|m| (m.operator, m.original.as_str()))
        .collect();
    assert_eq!(inner, vec![(Operator::ArithmeticOperator, "+")]);
}

#[test]
fn new_target_and_import_meta() {
    let source = r"
function F() {
  if (new.target) {}
  return import.meta;
}
let keep = a + b;
";
    assert_only_in_control(source, TS, "a + b");
}

#[test]
fn super_member_targets() {
    let source = r#"
class B extends A {
  m() {
    super.x = keep + 1;
    super["y" + z] = 2;
  }
}
"#;
    assert_only_in_control(source, TS, "keep + 1");
}

#[test]
fn jsx_text_and_attribute_strings() {
    let source = r#"const el = <div title="t" data-x='u'>text {"keep"}</div>;"#;
    assert_only_in_control(source, SourceKind::Tsx, r#""keep""#);
}

#[test]
fn property_keys() {
    let source = r#"
const o = { "k": 1, ["c" + d]: 2, e: true };
class C { "m"() {} ["n" + 1] = 3; }
const { "k": v } = o;
"#;
    assert_only_in_control(source, TS, "e: true");
}

/// The accepted rule skips each mutant in a logging call, also logic in the
/// arguments, and keeps the call statement.
#[test]
fn logging_calls_and_all_their_arguments() {
    let source = r#"
console.log(total(a + b), items.map((x) => { if (x) return x + 1; }));
console.error?.(`n=${n * 2}`, !ok);
debug(i++, "msg", c < d);
keep = e - 1;
"#;
    assert_only_in_control(source, TS, "keep = e - 1");
}

/// The logging and `require` rules see through parentheses and TypeScript
/// wrappers on the callee.
#[test]
fn logging_and_require_callees_behind_type_wrappers() {
    let source = r#"
(console.log as any)(a + b, "msg");
(console.log satisfies any)(c < d);
(console.log)!(i++);
(<any>debug)(e - f, true);
(require as any)("left-pad" + g);
keep = e - 1;
"#;
    assert_only_in_control(source, TS, "keep = e - 1");
}

/// A pure condition whose branches hold only logging calls gives no mutant
/// (issue #3719). A changed test can only change which message is logged.
#[test]
fn pure_logging_only_conditions() {
    let source = r#"
export const countItems = (count: 0 | 1 | 2): 0 | 1 | 2 => {
  if (count > 0) {
    console.info("Items", count);
  }
  if (items.length >= limit && !quiet) console.warn("full");
  if (state?.mode === "debug") {
    console.debug("mode", state.mode);
    debug("state");
  } else if (count < 2) {
    console.log("low");
  } else {
    console.error?.("high");
  }
  if (count === 2) console["log"](`n=${count}`);
  return keep - 1;
};
"#;
    assert_only_in_control(source, TS, "keep - 1");
}

/// True when a `relational-operator` mutant replaces the `>` of `marker`, the
/// first occurrence of a `<name> > 0` comparison in `source`.
fn comparison_keeps_mutants(source: &str, marker: &str) -> bool {
    let Some(start) = source.find(marker) else {
        panic!("the marker {marker:?} is not in the source");
    };
    let Some(offset) = marker.find('>') else {
        panic!("the marker {marker:?} has no comparison");
    };
    let operator = (start + offset) as u32;
    common::run(source, TS).mutants.iter().any(|mutant| {
        mutant.operator == Operator::RelationalOperator && mutant.span == (operator, operator + 1)
    })
}

/// A condition keeps its mutants when the test can have an effect, when a
/// branch does more than logging, or when no branch logs.
#[test]
fn conditions_with_effects_or_other_statements_keep_mutants() {
    let source = r"
if (c01 > 0 && check()) console.info(c01);
if (c02 > 0) { console.info(c02); total += 1; }
if (c03 > 0) console.info(c03); else total = 0;
if (c04 > 0 && (seen = d)) console.info(seen);
if (c05 > 0 && e++) console.info(e);
if (c06 > 0 && await ready()) console.info(c06);
if (c07 > 0 && new Gate().open) console.info(c07);
if (c08 > 0) {}
if (c09 > 0) { console.info(c09); if (j) total -= 1; }
if (c10 > 0) console.log(1); else if (b) console.log(2); else total++;
function r() { if (c11 > 0) return console.log(1); }
function t() { if (c12 > 0) throw console.log(1); }
if (c13 > 0) console.log(...items);
if (c14 > 0 && tag`x`) console.log(1);
if (c15 > 0) console.log(tag`x`);
if ((check(), c16 > 0)) console.log(1);
if (c17 > 0 && a?.b()) console.log(1);
if (c18 > 0) console[pick()]('x');
";
    for marker in [
        "c01 > 0", "c02 > 0", "c03 > 0", "c04 > 0", "c05 > 0", "c06 > 0", "c07 > 0", "c08 > 0",
        "c09 > 0", "c10 > 0", "c11 > 0", "c12 > 0", "c13 > 0", "c14 > 0", "c15 > 0", "c16 > 0",
        "c17 > 0", "c18 > 0",
    ] {
        assert!(
            comparison_keeps_mutants(source, marker),
            "the condition {marker:?} lost its mutants"
        );
    }
}

/// A local or imported `console` or `debug` can have effects, so a condition
/// that calls it keeps its mutants. The other name stays a global.
#[test]
fn shadowed_logging_names_keep_their_conditions() {
    let source = r"
import debug from 'debug';
const log = debug('app');
if (c01 > 0) debug('x');
if (c02 > 0) console.log('y');
";
    assert!(comparison_keeps_mutants(source, "c01 > 0"));
    assert!(!comparison_keeps_mutants(source, "c02 > 0"));

    let source = r"
function report(console: { log(message: string): void }) {
  if (c03 > 0) console.log('z');
}
";
    assert!(comparison_keeps_mutants(source, "c03 > 0"));
}
