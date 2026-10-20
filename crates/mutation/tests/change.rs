//! The public change of a mutant: tokens from a closed vocabulary, never
//! project text.

mod common;

use common::run;
use fallow_mutation::{MutantChange, Operator, SourceKind};

const JS: SourceKind = SourceKind::Js;

/// Every value that a change side can have.
const VOCABULARY: &[&str] = &[
    "",
    "+",
    "-",
    "*",
    "/",
    "%",
    "+=",
    "-=",
    "<",
    "<=",
    ">",
    ">=",
    "==",
    "!=",
    "===",
    "!==",
    "&&",
    "||",
    "??",
    "!",
    "++",
    "--",
    "?.",
    ".",
    "true",
    "false",
    "undefined",
    "void 0",
    "\"\"",
    "''",
    "\"fallow\"",
    "'fallow'",
    "^",
    "$",
    "[",
    "[^",
    "\\d",
    "\\D",
    "\\w",
    "\\W",
    "\\s",
    "\\S",
    "<condition>",
    "<call>",
    "<expression>",
    "<string>",
];

/// A source, an operator, and the expected changes of that operator.
type Case = (
    &'static str,
    Operator,
    &'static [(&'static str, &'static str)],
);

fn changes(source: &str, operator: Operator) -> Vec<(&'static str, &'static str)> {
    run(source, JS)
        .mutants
        .iter()
        .filter(|mutant| mutant.operator == operator)
        .map(|mutant| (mutant.change.from, mutant.change.to))
        .collect()
}

#[test]
fn nullish_coalescing_reports_the_operator_tokens() {
    let source = "export function label(preset) { return preset ?? \"compact\"; }";
    assert_eq!(
        changes(source, Operator::LogicalOperator),
        [("??", "&&")],
        "{source}"
    );
}

#[test]
fn each_operator_reports_its_tokens() {
    let cases: &[Case] = &[
        ("x = a+-b;", Operator::ArithmeticOperator, &[("+", "-")]),
        ("x += 1;", Operator::ArithmeticOperator, &[("+=", "-=")]),
        (
            "x = a < b;",
            Operator::RelationalOperator,
            &[("<expression>", "false"), ("<", "<="), ("<", "!=")],
        ),
        (
            "x = a < b == c;",
            Operator::RelationalOperator,
            &[
                ("<expression>", "false"),
                ("<", "<="),
                ("<", "!="),
                ("<expression>", "false"),
                ("==", "!="),
            ],
        ),
        (
            "x = a ?? b ?? c;",
            Operator::LogicalOperator,
            &[("??", "&&"), ("??", "&&")],
        ),
        (
            "if (ready) go();",
            Operator::NegateCondition,
            &[("<condition>", "true"), ("<condition>", "false")],
        ),
        ("x = !a;", Operator::UnaryOperator, &[("!", "")]),
        ("!function () {}();", Operator::UnaryOperator, &[("!", "")]),
        ("i++;", Operator::UpdateOperator, &[("++", "--")]),
        ("save(x);", Operator::RemovedCall, &[("<call>", "void 0")]),
        (
            "const s = z.number().int();",
            Operator::RemovedChainLink,
            &[("<call>", "")],
        ),
        (
            "function f() { return a + b; }",
            Operator::ReturnValue,
            &[("<expression>", "undefined")],
        ),
        ("x = true;", Operator::BooleanLiteral, &[("true", "false")]),
        (
            "x = 'secret';",
            Operator::StringLiteral,
            &[("<string>", "''")],
        ),
        (
            "x = \"\";",
            Operator::StringLiteral,
            &[("\"\"", "\"fallow\"")],
        ),
        ("x = a?.b;", Operator::OptionalChaining, &[("?.", ".")]),
        ("x = 1?.b;", Operator::OptionalChaining, &[("?.", ".")]),
        ("x = a?.[b];", Operator::OptionalChaining, &[("?.", "")]),
        (
            "x = /^[a-z]\\d$/;",
            Operator::RegexLiteral,
            &[("^", ""), ("[", "[^"), ("\\d", "\\D"), ("$", "")],
        ),
        ("x = /[^a]/;", Operator::RegexLiteral, &[("[^", "[")]),
    ];
    for (source, operator, want) in cases {
        let mut got = changes(source, *operator);
        let mut want = want.to_vec();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "{source}");
    }
}

#[test]
fn a_change_never_contains_project_text() {
    let source = r#"
export function check(items, limit, secretName) {
  let count = 0;
  for (let i = 0; i < items.length; i++) {
    if ((items[i]?.active && !items[i].hidden) || (secretName ?? limit)) {
      count += secretName.length * 2;
    }
  }
  notify("secret message", /secret[0-9]+\w$/);
  if (secretName) return secretName === "secret" ? true : -limit;
  return (count > limit) == (count <= 3);
}
"#;
    let result = run(source, JS);
    assert!(result.mutants.len() > 20, "{}", result.mutants.len());
    for mutant in &result.mutants {
        let MutantChange { from, to } = mutant.change;
        for side in [from, to] {
            assert!(
                VOCABULARY.contains(&side),
                "{} gives {side:?}",
                mutant.operator
            );
        }
    }
}
