//! The opt-in operators: one positive fixture per kind, with near inputs
//! that must not mutate, and the default set that leaves them out.

mod common;

use common::{assert_runs_like_each_mutant, expected, pairs, run, run_with};
use fallow_mutation::{Instrumented, MutationOptions, Operator, SourceKind};

const JS: SourceKind = SourceKind::Js;
const TS: SourceKind = SourceKind::Ts;

/// Instruments `source` with only `operator`.
fn only(source: &str, kind: SourceKind, operator: Operator) -> Instrumented {
    let options = MutationOptions {
        operators: vec![operator],
        ranges: None,
    };
    run_with(source, kind, &options)
}

/// Asserts the pairs of `operator` for each `(source, want)` case.
fn assert_cases(operator: Operator, kind: SourceKind, cases: &[(&str, &[(&str, &str)])]) {
    for (source, want) in cases {
        let result = only(source, kind, operator);
        assert_eq!(pairs(&result, operator), expected(want), "{source}");
    }
}

/// The mutated programs of `operator` for `source`.
fn mutated(source: &str, operator: Operator) -> Vec<String> {
    let result = only(source, JS, operator);
    let mut programs: Vec<String> = result
        .mutants
        .iter()
        .map(|mutant| common::apply(source, mutant))
        .collect();
    programs.sort();
    programs
}

#[test]
fn the_default_set_leaves_out_each_opt_in_operator() {
    let source = "function f(s, o) {\n  const a = [1];\n  const b = { k: 1 };\n  switch (s) { case 1: g(); break; }\n  if (!s) { throw new Error(\"x\"); }\n  const r = new RegExp(\"^a+$\");\n  return s.trim() + h(a, b, r, /(?=a)\\b/);\n}\n";
    let result = run(source, JS);
    let opt_in: Vec<_> = result
        .mutants
        .iter()
        .filter(|mutant| !mutant.operator.is_default())
        .map(|mutant| mutant.operator)
        .collect();
    assert_eq!(opt_in, Vec::new());
    let all = run_with(
        source,
        JS,
        &MutationOptions {
            operators: Operator::ALL.to_vec(),
            ranges: None,
        },
    );
    for operator in Operator::OPT_IN {
        assert!(
            all.mutants.iter().any(|mutant| mutant.operator == operator),
            "{operator} gave no mutant"
        );
    }
}

#[test]
fn operator_sets_partition_every_operator() {
    let mut joined: Vec<Operator> = Operator::DEFAULT.to_vec();
    joined.extend(Operator::OPT_IN);
    assert_eq!(joined, Operator::ALL.to_vec());
    assert_eq!(
        Operator::from_name("block-statement"),
        Some(Operator::BlockStatement)
    );
    assert!(!Operator::BlockStatement.is_default());
    assert!(Operator::RegexLiteral.is_default());
    assert_eq!(
        MutationOptions::default().operators,
        Operator::DEFAULT.to_vec()
    );
}

#[test]
fn block_statement_empties_non_empty_blocks() {
    assert_eq!(
        mutated("function f() { a(); b(); }", Operator::BlockStatement),
        vec!["function f() {}"]
    );
    assert_eq!(
        mutated("if (x) { a(); } else { b(); }", Operator::BlockStatement),
        vec!["if (x) { a(); } else {}", "if (x) {} else { b(); }"]
    );
    assert_eq!(
        mutated("const f = () => { return 1; };", Operator::BlockStatement),
        vec!["const f = () => {};"]
    );
    assert_eq!(
        mutated(
            "function f() {\n  \"use strict\";\n  a();\n}",
            Operator::BlockStatement
        ),
        vec!["function f() {\n  \"use strict\";\n  }"]
    );
    assert_eq!(
        mutated("try { a(); } catch { b(); }", Operator::BlockStatement),
        vec!["try { a(); } catch {}", "try {} catch { b(); }"]
    );
    assert_cases(
        Operator::BlockStatement,
        JS,
        &[
            ("function f() {}", &[]),
            ("if (x) {}", &[]),
            ("function f() { \"use strict\"; }", &[]),
            ("const f = () => 1;", &[]),
            (
                "class A extends B { constructor() { super(); this.x = 1; } }",
                &[],
            ),
            ("class A { static { a(); } }", &[]),
        ],
    );
    // A constructor without a `super` call keeps the mutant.
    assert_eq!(
        mutated(
            "class A { constructor() { this.x = 1; } }",
            Operator::BlockStatement
        ),
        vec!["class A { constructor() {} }"]
    );
}

#[test]
fn block_statement_keeps_nested_mutants_and_declarations_valid() {
    let source = "function f(x) {\n  const y = x + 1;\n  function g() { return y; }\n  return g();\n}\nf(1);\n";
    let result = run_with(
        source,
        JS,
        &MutationOptions {
            operators: vec![
                Operator::BlockStatement,
                Operator::ArithmeticOperator,
                Operator::ReturnValue,
            ],
            ranges: None,
        },
    );
    // The body of `f` declares the function `g`, so only the body of `g`
    // gives a mutant.
    assert_eq!(
        pairs(&result, Operator::BlockStatement),
        expected(&[(" return y; ", "")])
    );
    assert_eq!(pairs(&result, Operator::ArithmeticOperator).len(), 1);
}

#[test]
fn array_literal_empties_and_fills() {
    assert_cases(
        Operator::ArrayLiteral,
        JS,
        &[
            ("x = [a, b];", &[("[a, b]", "[]")]),
            ("x = [];", &[("[]", "[\"fallow\"]")]),
            ("[a].forEach(f);", &[("[a]", "[]")]),
            ("const [a] = b;", &[]),
            ("[a] = b;", &[]),
            ("console.log([a]);", &[]),
        ],
    );
    assert_cases(
        Operator::ArrayLiteral,
        TS,
        &[("let x: [number] = [1];", &[("[1]", "[]")])],
    );
}

#[test]
fn object_literal_empties() {
    assert_cases(
        Operator::ObjectLiteral,
        JS,
        &[
            ("x = { a: 1 };", &[("{ a: 1 }", "{}")]),
            ("x = { ...a };", &[("{ ...a }", "{}")]),
            ("f(() => ({ a: 1 }));", &[("{ a: 1 }", "{}")]),
            ("export default { a: 1 };", &[("{ a: 1 }", "{}")]),
            ("x = {};", &[]),
            ("const { a } = b;", &[]),
            ("({ a } = b);", &[]),
            ("console.log({ a });", &[]),
        ],
    );
}

#[test]
fn method_expression_swaps_pairs_and_removes_calls() {
    assert_cases(
        Operator::MethodExpression,
        JS,
        &[
            ("x = s.startsWith(p);", &[("startsWith", "endsWith")]),
            ("x = s.endsWith(p);", &[("endsWith", "startsWith")]),
            ("x = a.some(f);", &[("some", "every")]),
            ("x = a.every(f);", &[("every", "some")]),
            ("x = Math.min(a, b);", &[("min", "max")]),
            ("x = Math.max(a, b);", &[("max", "min")]),
            ("x = s.toLowerCase();", &[("toLowerCase", "toUpperCase")]),
            ("x = s.trimEnd();", &[("trimEnd", "trimStart")]),
            ("d.setHours(1);", &[("setHours", "setMinutes")]),
            ("d.setUTCDate(1);", &[("setUTCDate", "setTime")]),
            ("d.setTime(1);", &[("setTime", "setDate")]),
            ("x = s.trim();", &[(".trim()", "")]),
            ("x = a.filter(f).map(g);", &[(".filter(f)", "")]),
            ("x = a.slice(1);", &[(".slice(1)", "")]),
            ("x = a\n  .sort();", &[(".sort()", "")]),
            ("x = s.trim\n();", &[(".trim\n()", "")]),
            ("x = s?.trim();", &[]),
            ("x = s.trim?.();", &[]),
            ("x = a?.b.trim();", &[]),
            ("x = s[\"trim\"]();", &[]),
            ("x = (s.trim)();", &[]),
            ("x = trim(s);", &[]),
            ("console.log(s.trim());", &[]),
            ("class A extends B { m() { return super.trim(); } }", &[]),
        ],
    );
}

#[test]
fn switch_case_empties_a_clause() {
    assert_eq!(
        mutated(
            "switch (x) { case 1: a(); break; default: b(); }",
            Operator::SwitchCase
        ),
        vec![
            "switch (x) { case 1:  default: b(); }",
            "switch (x) { case 1: a(); break; default:  }",
        ]
    );
    assert_cases(
        Operator::SwitchCase,
        JS,
        &[
            ("switch (x) { case 1: case 2: a(); }", &[("a();", "")]),
            ("switch (x) { case 1: let y = 1; f(y); }", &[]),
            ("switch (x) { case 1: function g() {} g(); }", &[]),
            ("switch (x) { case 1: }", &[]),
        ],
    );
}

#[test]
fn call_expression_replaces_calls_in_expression_positions() {
    assert_cases(
        Operator::CallExpression,
        JS,
        &[
            ("x = f();", &[("f()", "(void 0)")]),
            ("x = a.b(c) + 1;", &[("a.b(c)", "(void 0)")]),
            ("x = f() /* c */ ** 2;", &[("f()", "(void 0)")]),
            ("x = f().y;", &[("f()", "(void 0)")]),
            (
                "x = f(g());",
                &[("f(g())", "(void 0)"), ("g()", "(void 0)")],
            ),
            ("x = await f();", &[("f()", "(void 0)")]),
            ("f();", &[]),
            ("async function h() { await f(); }", &[]),
            ("function h() { return f(); }", &[]),
            ("const h = () => f();", &[]),
            ("x = a?.b();", &[]),
            ("x = a?.b.c();", &[]),
            ("x = require(\"m\");", &[]),
            ("console.log(f());", &[]),
            ("class A extends B { constructor() { super(); } }", &[]),
        ],
    );
}

#[test]
fn throw_statement_removes_a_thrown_instance() {
    assert_eq!(
        mutated(
            "function f(x) { if (!x) throw new Error(\"no\"); return x; }",
            Operator::ThrowStatement
        ),
        vec!["function f(x) { if (!x) {} return x; }"]
    );
    assert_eq!(
        mutated("if (x) throw new E(); else y();", Operator::ThrowStatement),
        vec!["if (x) {} else y();"]
    );
    assert_cases(
        Operator::ThrowStatement,
        JS,
        &[
            ("function f() { throw error; }", &[]),
            ("function f() { throw make(); }", &[]),
        ],
    );
}

#[test]
fn regex_assertion_negates_lookarounds_and_word_boundaries() {
    assert_cases(
        Operator::RegexAssertion,
        JS,
        &[
            ("x = /a(?=b)/;", &[("/a(?=b)/", "/a(?!b)/")]),
            ("x = /a(?!b)/;", &[("/a(?!b)/", "/a(?=b)/")]),
            ("x = /(?<=a)b/;", &[("/(?<=a)b/", "/(?<!a)b/")]),
            ("x = /(?<!a)b/;", &[("/(?<!a)b/", "/(?<=a)b/")]),
            (
                "x = /\\ba\\B/;",
                &[("/\\ba\\B/", "/\\Ba\\B/"), ("/\\ba\\B/", "/\\ba\\b/")],
            ),
            ("x = /[\\b]/;", &[]),
            ("x = /(?:a)(?<n>b)/;", &[]),
            ("x = /\\\\b/;", &[]),
        ],
    );
    let default = run("x = /a(?=b)\\b/;", JS);
    assert_eq!(pairs(&default, Operator::RegexAssertion), expected(&[]));
}

#[test]
fn regex_constructor_mutates_a_string_pattern() {
    assert_cases(
        Operator::RegexConstructor,
        JS,
        &[
            (
                "x = new RegExp(\"^a+$\");",
                &[
                    ("\"^a+$\"", "\"a+$\""),
                    ("\"^a+$\"", "\"^a$\""),
                    ("\"^a+$\"", "\"^a+\""),
                ],
            ),
            ("x = RegExp('\\\\d', 'g');", &[("'\\\\d'", "'\\\\D'")]),
            ("x = new RegExp(\"a/b+\");", &[("\"a/b+\"", "\"a/b\"")]),
            (
                "x = new RegExp(\"a(?=b)\");",
                &[("\"a(?=b)\"", "\"a(?!b)\"")],
            ),
            ("x = new RegExp(p);", &[]),
            ("x = new RegExp(\"a+\", flags);", &[]),
            ("x = new RegExp(`a+`);", &[]),
            ("x = new Foo(\"a+\");", &[]),
            (
                "x = new RegExp(\"(a)\\\\1?2\", \"u\");",
                &[("\"(a)\\\\1?2\"", "\"(a)\\\\1(?:)2\"")],
            ),
        ],
    );
}

#[test]
fn regex_constructor_keeps_string_escapes() {
    assert_cases(
        Operator::RegexConstructor,
        JS,
        &[
            (
                "x = new RegExp(\"a\\nb+\");",
                &[("\"a\\nb+\"", "\"a\\nb\"")],
            ),
            (
                "x = new RegExp(\"\\\"a+\");",
                &[("\"\\\"a+\"", "\"\\\"a\"")],
            ),
        ],
    );
}

#[test]
fn statement_sites_keep_each_line_and_parse() {
    let source = "function f(x) {\n  \"use strict\";\n  if (!x) throw new Error(\"no\"); // c\n  switch (x) { case 1: g(); break; default: h(); // d\n  }\n  { a(); // e\n  }\n  return x;\n}\n";
    let result = run_with(
        source,
        JS,
        &MutationOptions {
            operators: Operator::ALL.to_vec(),
            ranges: None,
        },
    );
    assert_eq!(result.code.lines().count(), source.lines().count() + 2);
    assert!(pairs(&result, Operator::BlockStatement).len() >= 2);
    assert_eq!(pairs(&result, Operator::SwitchCase).len(), 2);
    assert_eq!(pairs(&result, Operator::ThrowStatement).len(), 1);
}

#[test]
fn block_statement_skips_ranges_with_lexical_declarations() {
    assert_cases(
        Operator::BlockStatement,
        JS,
        &[
            ("function f() { const a = 1; g(a); }", &[]),
            ("if (x) { let y = 1; g(y); }", &[]),
            ("if (x) { class A {} g(A); }", &[]),
            ("function f() { var g = 1; function g() {} return g; }", &[]),
            ("function f() { var a = 1; g(a); }", &[]),
            ("function f() { if (x) { var a = 1; } g(a); }", &[]),
            ("function f() { if (x) { function h() {} } g(); }", &[]),
            (
                "function f() { g(() => { var a = 1; return a; }); }",
                &[(" g(() => { var a = 1; return a; }); ", "")],
            ),
            (
                "function f() { if (x) { let a = 1; g(a); } h(); }",
                &[(" if (x) { let a = 1; g(a); } h(); ", "")],
            ),
            (
                "class A extends B { constructor(x) { if (x) { super(1); } else { super(2); } this.y = 1; } }",
                &[],
            ),
        ],
    );
}

#[test]
fn block_statement_keeps_a_directive_without_semicolon() {
    let cases: &[(&str, &str)] = &[
        (
            "function f() {\n  \"use strict\"\n  a();\n}",
            "function f() {\n  \"use strict\"\n  }",
        ),
        (
            "function f() {\n  \"use strict\" // c\n  a();\n}",
            "function f() {\n  \"use strict\" // c\n  }",
        ),
        (
            "function f() {\n  \"use strict\"\n  \"other\"\n  a();\n}",
            "function f() {\n  \"use strict\"\n  \"other\"\n  }",
        ),
        (
            "const f = () => {\n  \"use strict\";\n  a();\n};",
            "const f = () => {\n  \"use strict\";\n  };",
        ),
    ];
    for (source, want) in cases {
        assert_eq!(
            mutated(source, Operator::BlockStatement),
            vec![*want],
            "{source}"
        );
    }
}

/// Sloppy-mode code where moving statements into a nested block would
/// change a declaration, with each opt-in statement operator.
const SLOPPY_FIXTURE: &str = r#"const out = [];
function f1() { var g = 1; function g() {} return g; }
out.push(f1());
function f2(g) { function g() {} return arguments[0] === g; }
out.push(f2(1));
function f3(x) { { var y = x + 1; } if (x) { out.push(y); } else { out.push(0); } return y; }
out.push(f3(2), f3(0));
function f4(x) { switch (x) { case 1: var z = 1; out.push("one"); case 2: out.push("two"); break; default: out.push("d"); } return z; }
out.push(f4(1), f4(2), f4(3));
function f5(x) { try { if (!x) throw new Error("no"); return x; } catch (e) { return e.message; } }
out.push(f5(0), f5(4));
function* f6(n) { for (var i = 0; i < n; i++) { yield i; } }
out.push([...f6(2)].length);
class B { constructor(v) { this.v = v; } }
class C extends B { constructor(x) { if (x) { super(x); } else { super(0); } this.w = { a: x }; } }
out.push(new C(3).v, new C(0).w.a);
async function f7(x) { if (x) { await null; return x * 2; } return -1; }
f7(2).then((v) => { out.push(v); console.log(JSON.stringify(out)); });
"#;

/// Strict code with directives without semicolons, and calls in operator
/// positions.
const STRICT_FIXTURE: &str = r#""use strict"
const out = [];
function h1(a) {
  "use strict"
  var b = a + 1;
  out.push(b);
  return b;
}
out.push(h1(1));
function h2() {
  "use strict" // c
  "other"
  out.push(2);
}
h2();
function k() { return 3; }
out.push(String(k() + 1), String(k() ** 2), k().toString(), [k()][0] === 3);
const s = "  Abc ";
out.push(s.trim().toLowerCase().startsWith("a"), [3, 1, 2].sort().slice(1).join(""), Math.min(2, 5));
out.push(/a(?=b)\b/.test("ab"), new RegExp("^a+$").test("aa"));
console.log(JSON.stringify(out));
"#;

#[test]
fn opt_in_mutants_run_like_the_applied_mutant() {
    let options = MutationOptions {
        operators: Operator::ALL.to_vec(),
        ranges: None,
    };
    for fixture in [SLOPPY_FIXTURE, STRICT_FIXTURE] {
        let count = assert_runs_like_each_mutant(fixture, &options);
        assert!(count == 0 || count > 10, "{count}");
    }
}
