//! One positive fixture per operator, with near inputs that must not mutate.

mod common;

use common::{expected, pairs, run};
use fallow_mutation::{Operator, SourceKind};

const JS: SourceKind = SourceKind::Js;

#[test]
fn arithmetic_operator_swaps_and_skips_string_concatenation() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = a + b;", &[("+", "-")]),
        ("x = a - b;", &[("-", "+")]),
        ("x = a * b;", &[("*", "/")]),
        ("x = a / b;", &[("/", "*")]),
        ("x = a % b;", &[("%", "*")]),
        ("x += 1;", &[("+=", "-=")]),
        ("x -= 1;", &[("-=", "+=")]),
        ("x = a+-b;", &[("+", "- ")]),
        ("x = \"a\" + b;", &[]),
        ("x = a + `t${b}`;", &[]),
        ("x = a ** b;", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::ArithmeticOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn relational_operator_uses_the_non_redundant_subset() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "x = a < b;",
            &[("<", "<="), ("<", "!="), ("a < b", "false")],
        ),
        (
            "x = a <= b;",
            &[("<=", "<"), ("<=", "=="), ("a <= b", "true")],
        ),
        (
            "x = a > b;",
            &[(">", ">="), (">", "!="), ("a > b", "false")],
        ),
        (
            "x = a >= b;",
            &[(">=", ">"), (">=", "=="), ("a >= b", "true")],
        ),
        ("x = a === b;", &[("===", "!=="), ("a === b", "false")]),
        ("x = a !== b;", &[("!==", "==="), ("a !== b", "true")]),
        ("x = a == b;", &[("==", "!="), ("a == b", "false")]),
        ("x = a != b;", &[("!=", "=="), ("a != b", "true")]),
        ("x = a instanceof B;", &[]),
        ("x = k in o;", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::RelationalOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn logical_operator_swaps_and_keeps_precedence() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = a && b;", &[("&&", "||")]),
        ("x = a || b;", &[("||", "&&")]),
        ("x = a ?? b;", &[("??", "&&")]),
        (
            "x = a || b || c;",
            &[("a || b", "(a && b)"), ("a || b || c", "(a || b) && c")],
        ),
        (
            "x = a ?? b ?? c;",
            &[("a ?? b", "(a && b)"), ("a ?? b ?? c", "(a ?? b) && c")],
        ),
        (
            "x = a || b && c;",
            &[("b && c", "(b || c)"), ("a || b && c", "a && (b && c)")],
        ),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::LogicalOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn negate_condition_covers_plain_tests_only() {
    let both: &[(&str, &str)] = &[("x", "false"), ("x", "true")];
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("if (x) {}", both),
        ("while (x) {}", both),
        ("do {} while (x);", both),
        ("for (; x; ) {}", both),
        ("y = x ? 1 : 2;", both),
        ("if (a < b) {}", &[]),
        ("if (a === b) {}", &[]),
        ("if (a && b) {}", &[]),
        ("for (;;) { break; }", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::NegateCondition),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn negate_condition_yields_to_boolean_literal_on_a_literal_test() {
    let result = run("if (true) { go(); }", JS);
    assert_eq!(pairs(&result, Operator::NegateCondition), expected(&[]));
    assert_eq!(
        pairs(&result, Operator::BooleanLiteral),
        expected(&[("true", "false")])
    );
}

#[test]
fn unary_operator_removes_not_and_negation() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = !a;", &[("!", "")]),
        ("x = -a;", &[("-", "")]),
        ("x = typeof a;", &[]),
        ("x = +a;", &[]),
        ("x = ~a;", &[]),
        (
            "!function () {}();",
            &[("!function () {}()", "void (function () {}())")],
        ),
        ("-{}.x;", &[("-{}.x", "({}.x)")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::UnaryOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn unary_operator_keeps_export_default_and_arrow_bodies_valid() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "export default !function () { return 1; }();",
            &[(
                "!function () { return 1; }()",
                "(function () { return 1; }())",
            )],
        ),
        ("export default -class {};", &[("-class {}", "(class {})")]),
        ("const g = () => !{}.x;", &[("!{}.x", "({}.x)")]),
        ("const h = () => !a;", &[("!a", "(a)")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::UnaryOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn relational_operator_keeps_the_grouping_of_a_nested_comparison() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "x = c == a < b;",
            &[
                ("c == a < b", "false"),
                ("==", "!="),
                ("a < b", "false"),
                ("<", "<="),
                ("a < b", "(a != b)"),
            ],
        ),
        (
            "x = a < b < c;",
            &[
                ("a < b < c", "false"),
                ("<", "<="),
                ("<", "!="),
                ("a < b", "false"),
                ("<", "<="),
                ("a < b", "(a != b)"),
            ],
        ),
        (
            "x = a >= b in o;",
            &[("a >= b", "true"), (">=", ">"), ("a >= b", "(a == b)")],
        ),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::RelationalOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn unary_operator_mutates_a_double_negation_once() {
    let source = "x = !!a;";
    let result = run(source, JS);
    let unary: Vec<_> = result
        .mutants
        .iter()
        .filter(|m| m.operator == Operator::UnaryOperator)
        .collect();
    assert_eq!(unary.len(), 1);
    assert_eq!(unary[0].span, (5, 6));
}

#[test]
fn update_operator_swaps_prefix_and_postfix() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("i++;", &[("++", "--")]),
        ("i--;", &[("--", "++")]),
        ("++i;", &[("++", "--")]),
        ("--i;", &[("--", "++")]),
        ("do++e;while(x);", &[("++", "--")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::UpdateOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn removed_call_turns_call_statements_into_no_ops() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("save(1);", &[("save(1)", "void 0")]),
        ("store.save();", &[("store.save()", "void 0")]),
        ("a?.b();", &[("a?.b()", "void 0")]),
        (
            "async function f() { await save(); }",
            &[("await save()", "void 0")],
        ),
        ("const y = save();", &[]),
        ("console.log(x);", &[]),
        ("console.error(x);", &[]),
        ("debug(x);", &[]),
        ("class A extends B { constructor() { super(); } }", &[]),
        ("x = 1;", &[]),
        (
            "do(function(){})();while(x);",
            &[("(function(){})()", " void 0")],
        ),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::RemovedCall),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn return_value_returns_undefined_outside_constructors_and_generators() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("function f() { return a; }", &[("a", "undefined")]),
        (
            "const f = async () => { return await g(); };",
            &[("await g()", "undefined")],
        ),
        ("function f() { return; }", &[]),
        ("function f() { return undefined; }", &[]),
        ("function f() { return void 0; }", &[]),
        ("function f() { return true; }", &[]),
        ("class A { constructor() { return a; } }", &[]),
        ("function* g() { return a; }", &[]),
        ("const f = () => a;", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::ReturnValue),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn boolean_literal_swaps() {
    let result = run("const t = true, f = false;", JS);
    assert_eq!(
        pairs(&result, Operator::BooleanLiteral),
        expected(&[("false", "true"), ("true", "false")]),
    );
}

#[test]
fn string_literal_empties_and_fills() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("const s = \"abc\";", &[("\"abc\"", "\"\"")]),
        ("const s = 'abc';", &[("'abc'", "''")]),
        ("const s = '';", &[("''", "'fallow'")]),
        ("throw new Error(\"bad\");", &[("\"bad\"", "\"\"")]),
        ("console.warn(\"quiet\");", &[]),
        ("debug(\"quiet\");", &[]),
        ("const m = require(\"mod\");", &[]),
        ("const m = import(\"mod\");", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::StringLiteral),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn optional_chaining_removes_each_optional_link() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = a?.b;", &[("?.", ".")]),
        ("x = a?.[k];", &[("?.", "")]),
        ("x = f?.();", &[("?.", "")]),
        ("x = a?.b?.c;", &[("?.", "."), ("?.", ".")]),
        ("x = a.b;", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::OptionalChaining),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn arithmetic_operator_finds_the_token_after_a_comment() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = total // items\n  / count;", &[("/", "*")]),
        ("x = total /* items */ / count;", &[("/", "*")]),
        ("x = total /** a / b */ / count;", &[("/", "*")]),
        ("x = a // c\n  + b;", &[("+", "-")]),
        ("x = a /* c */ * b;", &[("*", "/")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::ArithmeticOperator),
            expected(want),
            "{source}"
        );
        for mutant in &result.mutants {
            let region = &source[mutant.span.0 as usize..mutant.span.1 as usize];
            assert_eq!(region, mutant.original, "{source}");
        }
    }
}

#[test]
fn arithmetic_operator_ends_a_line_comment_at_each_line_terminator() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = total // items\n  / count;", &[("/", "*")]),
        ("x = total // items\r  / count;", &[("/", "*")]),
        ("x = total // items\r\n  / count;", &[("/", "*")]),
        ("x = total // items\u{2028}  / count;", &[("/", "*")]),
        ("x = total // items\u{2029}  / count;", &[("/", "*")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::ArithmeticOperator),
            expected(want),
            "{source:?}"
        );
    }
}

#[test]
fn arithmetic_operator_separates_division_from_a_regex() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = a / /re/.source;", &[("/", "*")]),
        ("x = a / /re/g.source.length;", &[("/", "*")]),
        ("x = /re/.source / b;", &[("/", "*")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::ArithmeticOperator),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn optional_chaining_on_other_number_literals() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = 1e3?.toFixed;", &[("?.", " .")]),
        ("x = 0x1?.toString;", &[("?.", " .")]),
        ("x = 1n?.toString;", &[("?.", ".")]),
        ("x = 1.5?.toFixed;", &[("?.", " .")]),
        ("x = 1e3?.[k];", &[("?.", "")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::OptionalChaining),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn optional_chaining_on_a_number_literal_stays_a_member_access() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = 1?.toString;", &[("?.", " .")]),
        ("x = 1 ?.toString;", &[("?.", ".")]),
        ("x = 1.5?.toFixed;", &[("?.", " .")]),
        ("x = (1)?.toString;", &[("?.", ".")]),
        ("x = 1?.[k];", &[("?.", "")]),
        ("x = a1?.b;", &[("?.", ".")]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::OptionalChaining),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn optional_chaining_skips_a_callee_behind_type_wrappers() {
    let cases = [
        ("(o?.m as any)(); x = c?.d;", SourceKind::Ts),
        ("(o?.m)!(); x = c?.d;", SourceKind::Ts),
        ("(o?.m!)(); x = c?.d;", SourceKind::Ts),
        ("(<any>o?.m)(); x = c?.d;", SourceKind::Ts),
        ("(o?.m satisfies any)(); x = c?.d;", SourceKind::Ts),
        ("((o?.m) as any)!(); x = c?.d;", SourceKind::Ts),
        ("(o?.m<string>)(); x = c?.d;", SourceKind::Ts),
        (
            "(((o?.m as any)) satisfies any)(); x = c?.d;",
            SourceKind::Tsx,
        ),
    ];
    for (source, kind) in cases {
        let result = run(source, kind);
        let control = source.find("c?.d").map_or(0, |offset| offset as u32 + 1);
        let spans: Vec<_> = result
            .mutants
            .iter()
            .filter(|m| m.operator == Operator::OptionalChaining)
            .map(|m| m.span)
            .collect();
        assert_eq!(spans, vec![(control, control + 2)], "{source}");
        let callee_end = source.find("();").unwrap_or(0);
        assert!(
            result.code.contains(&source[..callee_end + 2]),
            "the callee was wrapped: {}",
            result.code
        );
    }
}

#[test]
fn optional_chaining_skips_a_parenthesized_callee() {
    let result = run("(a?.b)(); x = c?.d;", JS);
    let spans: Vec<_> = result
        .mutants
        .iter()
        .filter(|m| m.operator == Operator::OptionalChaining)
        .map(|m| m.span)
        .collect();
    assert_eq!(spans, vec![(15, 17)]);
}

#[test]
fn regex_literal_changes_one_pattern_token() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        ("x = /^a+$/;", &[("/^a+$/", "/a+$/"), ("/^a+$/", "/^a+/")]),
        (
            "x = /[a-z]\\d/gi;",
            &[
                ("/[a-z]\\d/gi", "/[^a-z]\\d/gi"),
                ("/[a-z]\\d/gi", "/[a-z]\\D/gi"),
            ],
        ),
        (
            "x = /[^\\s]/;",
            &[("/[^\\s]/", "/[\\s]/"), ("/[^\\s]/", "/[^\\S]/")],
        ),
        ("x = /\\W/u;", &[("/\\W/u", "/\\w/u")]),
        ("x = /^/;", &[("/^/", "/(?:)/")]),
        ("x = /a\\$/;", &[]),
        ("x = /[$]/;", &[("/[$]/", "/[^$]/")]),
        ("x = /a[b]c/;", &[("/a[b]c/", "/a[^b]c/")]),
        ("x = /[\\d--x]/v;", &[("/[\\d--x]/v", "/[\\D--x]/v")]),
        ("x = /abc/;", &[]),
        ("console.log(/^a$/);", &[]),
    ];
    for (source, want) in cases {
        let result = run(source, JS);
        assert_eq!(
            pairs(&result, Operator::RegexLiteral),
            expected(want),
            "{source}"
        );
    }
}

#[test]
fn regex_literal_at_a_statement_start_stays_valid() {
    let result = run("a();\n/^b/.test(c);", JS);
    assert_eq!(pairs(&result, Operator::RegexLiteral).len(), 1);
}

#[test]
fn operator_filter_keeps_only_allowed_operators() {
    let options = fallow_mutation::MutationOptions {
        operators: vec![Operator::BooleanLiteral],
        ranges: None,
    };
    let result = common::run_with("x = a + b; y = true;", JS, &options);
    let operators: Vec<_> = result.mutants.iter().map(|m| m.operator).collect();
    assert_eq!(operators, vec![Operator::BooleanLiteral]);
}

#[test]
fn operator_names_are_kebab_case() {
    let names: Vec<_> = Operator::ALL.iter().map(|op| op.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "arithmetic-operator",
            "relational-operator",
            "logical-operator",
            "negate-condition",
            "unary-operator",
            "update-operator",
            "removed-call",
            "return-value",
            "boolean-literal",
            "string-literal",
            "optional-chaining",
            "regex-literal",
        ],
    );
    assert_eq!(
        Operator::from_name("return-value"),
        Some(Operator::ReturnValue)
    );
}
