//! The shape of the instrumented output: prelude placement, switches, scope
//! semantics, language preservation and the splice map.

mod common;

use common::{run, run_with};
use fallow_mutation::{MutationOptions, Operator, SourceKind};
use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_syntax::scope::ScopeFlags;

const PRELUDE: &str = "function __fallow_m_a(id) { const m = globalThis.__fallowMutation; return m ? m.a(id) : false; } function __fallow_m_c(id) { const m = globalThis.__fallowMutation; if (m) m.c(id); }";

fn with_program<R>(code: &str, kind: SourceKind, read: impl FnOnce(&Program<'_>) -> R) -> R {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, kind.source_type()).parse();
    read(&parsed.program)
}

#[derive(Default)]
struct FunctionCount(usize);

impl<'a> Visit<'a> for FunctionCount {
    fn visit_function(&mut self, it: &oxc_ast::ast::Function<'a>, flags: ScopeFlags) {
        self.0 += 1;
        oxc_ast_visit::walk::walk_function(self, it, flags);
    }

    fn visit_arrow_function_expression(&mut self, it: &oxc_ast::ast::ArrowFunctionExpression<'a>) {
        self.0 += 1;
        oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
    }
}

fn function_count(code: &str, kind: SourceKind) -> usize {
    with_program(code, kind, |program| {
        let mut count = FunctionCount::default();
        count.visit_program(program);
        count.0
    })
}

#[test]
fn each_mutant_has_one_activation_and_one_hit_switch() {
    let source = "export function f(a, b) {\n  if (a < b) { log(a); }\n  return a + b;\n}\n";
    let result = run(source, SourceKind::Js);
    assert!(result.mutants.len() >= 5);
    for mutant in &result.mutants {
        let activation = format!("__fallow_m_a(\"{}\")", mutant.id);
        let hit = format!("__fallow_m_c(\"{}\")", mutant.id);
        assert_eq!(result.code.matches(&activation).count(), 1, "{}", mutant.id);
        assert_eq!(result.code.matches(&hit).count(), 1, "{}", mutant.id);
    }
}

#[test]
fn prelude_follows_hashbang_and_directives() {
    let source = "#!/usr/bin/env node\n\"use strict\";\n'use client'\nx = a + 1;\n";
    let result = run(source, SourceKind::Js);
    let want = format!(
        "#!/usr/bin/env node\n// @ts-nocheck\n\"use strict\";\n'use client';{PRELUDE}\nx = "
    );
    assert!(result.code.starts_with(&want), "{}", result.code);
    let directives = with_program(&result.code, SourceKind::Js, |program| {
        program
            .directives
            .iter()
            .map(|d| d.directive.as_str().to_owned())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        directives,
        vec!["use strict".to_owned(), "use client".to_owned()]
    );
}

#[test]
fn prelude_without_hashbang_or_directives_comes_first() {
    let result = run("x = a + 1;\n", SourceKind::Ts);
    assert!(
        result
            .code
            .starts_with(&format!("// @ts-nocheck\n{PRELUDE}\nx = ")),
        "{}",
        result.code
    );
}

#[test]
fn header_follows_the_leading_comments() {
    let source = "/** @jsxImportSource preact */\n// license\nf(a + b);\n";
    let result = run(source, SourceKind::Jsx);
    let want =
        format!("/** @jsxImportSource preact */\n// license\n// @ts-nocheck\n{PRELUDE}\nvoid ");
    assert!(result.code.starts_with(&want), "{}", result.code);
}

#[test]
fn header_follows_hashbang_and_comments_before_directives() {
    let source = "#!/usr/bin/env node\n/* c */\n\"use strict\";\nx = a + 1;\n";
    let result = run(source, SourceKind::Js);
    let want =
        format!("#!/usr/bin/env node\n/* c */\n// @ts-nocheck\n\"use strict\";{PRELUDE}\nx = ");
    assert!(result.code.starts_with(&want), "{}", result.code);
}

/// The directive texts and the name that the first body statement declares.
fn directives_and_first_binding(code: &str, kind: SourceKind) -> (Vec<String>, Option<String>) {
    with_program(code, kind, |program| {
        let directives = program
            .directives
            .iter()
            .map(|d| d.directive.as_str().to_owned())
            .collect();
        let binding = match program.body.first() {
            Some(Statement::FunctionDeclaration(function)) => {
                function.id.as_ref().map(|id| id.name.as_str().to_owned())
            }
            _ => None,
        };
        (directives, binding)
    })
}

#[test]
fn prelude_stays_code_when_a_comment_shares_the_directive_line() {
    let cases = [
        (
            "/* c */ 'use client'\nexport const a = 1 + 2;\n",
            SourceKind::Js,
            vec!["use client"],
        ),
        (
            "/** @jsxImportSource x */ 'use client';\nexport const a = <b>{1 + 2}</b>;\n",
            SourceKind::Jsx,
            vec!["use client"],
        ),
        (
            "#!/usr/bin/env node\n/* a */ /* b */ \"use strict\"\nexports.v = 1 + 2;\n",
            SourceKind::Js,
            vec!["use strict"],
        ),
        (
            "/* c */ \"use strict\"; 'use client'\nx = a + 1;\n",
            SourceKind::Ts,
            vec!["use strict", "use client"],
        ),
    ];
    for (source, kind, want) in cases {
        let result = run(source, kind);
        assert!(!result.mutants.is_empty(), "{source}");
        let (directives, binding) = directives_and_first_binding(&result.code, kind);
        assert_eq!(directives, want, "{}", result.code);
        assert_eq!(binding.as_deref(), Some("__fallow_m_a"), "{}", result.code);
    }
}

#[test]
fn prelude_stays_code_when_a_comment_shares_the_first_statement_line() {
    let source = "/* c */ x = a + 1;\n";
    let result = run(source, SourceKind::Js);
    let (directives, binding) = directives_and_first_binding(&result.code, SourceKind::Js);
    assert!(directives.is_empty(), "{}", result.code);
    assert_eq!(binding.as_deref(), Some("__fallow_m_a"), "{}", result.code);
}

/// In a module cycle, another module can call a hoisted function of this
/// module before the prelude line runs. The switch functions must then
/// already exist, so the prelude declares only hoisted functions and no
/// lexical binding (a `const` would throw a `ReferenceError` there).
#[test]
fn prelude_declares_only_hoisted_switch_functions() {
    let source = "export function early(a, b) {\n  return a + b;\n}\n";
    let result = run(source, SourceKind::Ts);
    assert!(!result.mutants.is_empty());
    let header: Vec<(String, Option<String>)> =
        with_program(&result.code, SourceKind::Ts, |program| {
            program
                .body
                .iter()
                .map(|statement| match statement {
                    Statement::FunctionDeclaration(function) => (
                        "function".to_owned(),
                        function.id.as_ref().map(|id| id.name.as_str().to_owned()),
                    ),
                    _ => ("other".to_owned(), None),
                })
                .collect()
        });
    assert_eq!(
        header,
        vec![
            ("function".to_owned(), Some("__fallow_m_a".to_owned())),
            ("function".to_owned(), Some("__fallow_m_c".to_owned())),
            ("other".to_owned(), None),
        ],
        "{}",
        result.code
    );
    for mutant in &result.mutants {
        assert!(
            result
                .code
                .contains(&format!("__fallow_m_a(\"{}\") ?", mutant.id)),
            "{}",
            result.code
        );
    }
}

#[test]
fn file_without_mutants_still_gets_the_prelude() {
    let result = run("import a from \"a\";\n", SourceKind::Js);
    assert!(result.mutants.is_empty());
    assert_eq!(
        result.code,
        format!("// @ts-nocheck\n{PRELUDE}\nimport a from \"a\";\n")
    );
}

#[test]
fn function_declarations_stay_hoistable_statements() {
    let source = "run();\nfunction run() { return 1 + 2; }\n";
    let result = run(source, SourceKind::Js);
    assert!(!result.mutants.is_empty());
    let names = with_program(&result.code, SourceKind::Js, |program| {
        program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => {
                    function.id.as_ref().map(|id| id.name.as_str().to_owned())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(
        names,
        vec![
            "__fallow_m_a".to_owned(),
            "__fallow_m_c".to_owned(),
            "run".to_owned()
        ]
    );
}

#[test]
fn instrumentation_adds_no_function_boundary() {
    let source = r"
class Counter {
  total() { return this.a + this.b + arguments.length; }
  static make() { return new.target === undefined && super.constructor; }
}
";
    let result = run(source, SourceKind::Js);
    assert!(
        result
            .mutants
            .iter()
            .any(|m| m.operator == Operator::ArithmeticOperator)
    );
    // The prelude adds two arrows; the mutated branches copy no function.
    assert_eq!(
        function_count(&result.code, SourceKind::Js),
        function_count(source, SourceKind::Js) + 2
    );
}

#[test]
fn async_and_generator_bodies_keep_await_and_yield_valid() {
    let source = r"
async function load(a) {
  await save(a);
  return (await fetch(a)) + 1;
}
function* gen(a) {
  yield a - 1;
  return a;
}
async function* both(a) {
  for await (const x of a) { yield x * 2; }
}
";
    let result = run(source, SourceKind::Js);
    let operators: Vec<_> = result
        .mutants
        .iter()
        .map(|m| (m.operator, m.original.as_str()))
        .collect();
    assert!(operators.contains(&(Operator::RemovedCall, "await save(a)")));
    assert!(operators.contains(&(Operator::ArithmeticOperator, "-")));
    assert!(operators.contains(&(Operator::ArithmeticOperator, "*")));
    assert!(!operators.contains(&(Operator::ReturnValue, "a")));
}

#[test]
fn expression_statements_are_safe_against_automatic_semicolon_insertion() {
    let source = "const a = b\ni++\nfoo()\n[1, 2].map(n => n)\n";
    let result = run(source, SourceKind::Js);
    let count = |code: &str| with_program(code, SourceKind::Js, |program| program.body.len());
    assert!(
        result
            .mutants
            .iter()
            .any(|m| m.operator == Operator::UpdateOperator)
    );
    assert!(
        result
            .mutants
            .iter()
            .any(|m| m.operator == Operator::RemovedCall)
    );
    // The prelude adds two statements. A wrapper that merges with the line
    // before it would remove one.
    assert_eq!(count(&result.code), count(source) + 2, "{}", result.code);
}

#[test]
fn statement_guard_does_not_join_the_keyword_before_it() {
    let source = "do++e;while(x);";
    let result = run(source, SourceKind::Js);
    assert!(
        result
            .mutants
            .iter()
            .any(|m| m.operator == Operator::UpdateOperator)
    );
    let is_do_while = with_program(&result.code, SourceKind::Js, |program| {
        matches!(program.body.last(), Some(Statement::DoWhileStatement(_)))
    });
    assert!(is_do_while, "{}", result.code);
}

#[test]
fn jsx_output_stays_jsx() {
    let source = "const el = <div>{open && <Panel title=\"t\" />}</div>;\n";
    let result = run(source, SourceKind::Jsx);
    assert_eq!(
        common::pairs(&result, Operator::LogicalOperator),
        common::expected(&[("&&", "||")])
    );
    assert!(result.code.contains("<Panel title=\"t\" />"));
}

#[test]
fn typescript_output_keeps_generics_and_satisfies() {
    let source = "const v = id<number>(a + 1);\nconst o = { n: b - 1 } satisfies Rec;\nconst p = <T,>(x: T): T => x;\n";
    let result = run(source, SourceKind::Tsx);
    assert_eq!(
        common::pairs(&result, Operator::ArithmeticOperator),
        common::expected(&[("+", "-"), ("-", "+")]),
    );
    assert!(result.code.contains("id<number>("));
    assert!(result.code.contains("satisfies Rec"));
    assert!(result.code.contains("<T,>(x: T): T => x"));
}

#[test]
fn ranges_limit_mutants_to_overlapping_spans() {
    let source = "function a() { return 1 + 2; }\nfunction b() { return 3 - 4; }\n";
    let second = source.find("function b").map_or(0, |offset| offset as u32);
    let options = MutationOptions {
        ranges: Some(vec![(second, source.len() as u32)]),
        ..MutationOptions::default()
    };
    let limited = run_with(source, SourceKind::Js, &options);
    let all = run(source, SourceKind::Js);
    assert!(all.mutants.iter().any(|m| m.span.0 < second));
    assert!(!limited.mutants.is_empty());
    assert!(limited.mutants.iter().all(|m| m.span.0 >= second));
    assert!(!limited.code.contains("1 - 2"));
    assert!(limited.code.contains("3 + 4"));
}

#[test]
fn inner_mutants_live_only_in_the_original_branch() {
    let source = "x = (a + b) * c;\n";
    let result = run(source, SourceKind::Js);
    // The outer mutant copies the raw operand; the instrumented operand
    // appears only once, inside the original branch.
    assert!(result.code.contains("? ((a + b) / c) :"), "{}", result.code);
    assert_eq!(
        result.code.matches("? (a - b) :").count(),
        1,
        "{}",
        result.code
    );
}

#[test]
fn mutant_positions_use_one_based_lines_and_zero_based_columns() {
    let source = "x = 1;\n  y = a < b;\n";
    let result = run(source, SourceKind::Js);
    let mutant = result.mutants.iter().find(|m| m.original == "<");
    assert_eq!(mutant.map(|m| (m.line, m.column)), Some((2, 8)));
}

#[test]
fn enclosing_function_names_come_from_the_syntax() {
    let source = r"
function named() { return a + 1; }
const arrow = () => b + 1;
class K { method() { return c + 1; } }
const obj = { prop() { return d + 1; } };
x = e + 1;
";
    let result = run(source, SourceKind::Js);
    let names: Vec<_> = result
        .mutants
        .iter()
        .filter(|m| m.operator == Operator::ArithmeticOperator)
        .map(|m| m.enclosing_function.as_ref().and_then(|f| f.name.clone()))
        .collect();
    assert_eq!(
        names,
        vec![
            Some("named".to_owned()),
            Some("arrow".to_owned()),
            Some("method".to_owned()),
            Some("prop".to_owned()),
            None,
        ],
    );
    let function = result.mutants[0]
        .enclosing_function
        .as_ref()
        .map(|f| f.span);
    let start = source
        .find("function named")
        .map_or(0, |offset| offset as u32);
    assert_eq!(function.map(|span| span.0), Some(start));
}

#[test]
fn splice_map_points_copied_text_back_to_the_original() {
    let source = "const total = price * count;\n";
    let result = run(source, SourceKind::Js);
    let original = source.find("price").map_or(0, |offset| offset as u32);
    let generated = result.code.rfind("price").map_or(0, |offset| offset as u32);
    assert_eq!(result.map.original_offset(generated), Some(original));
    let prelude = result
        .code
        .find("__fallow_m_a")
        .map_or(0, |offset| offset as u32);
    assert_eq!(result.map.original_offset(prelude), None);
}

#[test]
fn a_binding_with_a_reserved_prelude_name_is_an_error() {
    let cases = [
        ("const __fallow_m_a = 1; x = a + b;", "__fallow_m_a"),
        ("function __fallow_m_c() {} x = a + b;", "__fallow_m_c"),
        (
            "import { y as __fallow_m_a } from 'm'; x = a + b;",
            "__fallow_m_a",
        ),
        ("let { __fallow_m_c } = o; x = a + b;", "__fallow_m_c"),
        ("class __fallow_m_a {} x = a + b;", "__fallow_m_a"),
    ];
    for (source, name) in cases {
        let result = fallow_mutation::instrument(
            source,
            "src/reserved.js",
            SourceKind::Js,
            &MutationOptions::default(),
        );
        assert_eq!(
            result.err(),
            Some(fallow_mutation::MutationError::ReservedName {
                name: name.to_owned()
            }),
            "{source}"
        );
    }
    // Control: a reference without a binding, and a near name, still work.
    let control = run("__fallow_m_a; const __fallow_m_b = a + b;", SourceKind::Js);
    assert_eq!(control.mutants.len(), 1);
}

#[test]
fn parse_errors_are_reported() {
    let error = fallow_mutation::instrument(
        "const = ;",
        "src/bad.js",
        SourceKind::Js,
        &MutationOptions::default(),
    );
    assert!(matches!(
        error,
        Err(fallow_mutation::MutationError::Parse { .. })
    ));
}

#[test]
fn snapshot_javascript_mix() {
    let source = r#"#!/usr/bin/env node
"use strict";
export function check(items, limit) {
  let count = 0;
  for (let i = 0; i < items.length; i++) {
    if (items[i]?.active && !items[i].hidden) {
      count += 1;
    }
  }
  notify("done");
  return count > limit;
}
"#;
    let result = run(source, SourceKind::Js);
    insta::assert_snapshot!(result.code);
}

#[test]
fn snapshot_typescript_class() {
    let source = r"
interface Opts { retries: number }
export class Client {
  constructor(private readonly opts: Opts) {}
  async send(body: string): Promise<boolean> {
    const ok = await this.post(body ?? '');
    return ok satisfies boolean;
  }
}
";
    let result = run(source, SourceKind::Ts);
    insta::assert_snapshot!(result.code);
}

const RUNTIME_FIXTURE: &str = r#"const out = [];
const obj = {
  base: 10,
  add(n) { return this.base + n + arguments.length; },
  get double() { return this.base * 2; },
};
out.push(obj.add(5), obj.double);
const maybe = { fn() { return this === maybe; } };
out.push(maybe?.fn?.());
function* gen(a) { const b = yield a + 1; return b - a; }
const it = gen(3);
out.push(it.next().value, it.next(7).value);
class K { static size = 2 + 3; check(x) { return x >= K.size ? "big" : "small"; } }
out.push(new K().check(4), new K().check(5));
const flag = !(obj.base === 10) || maybe.missing?.x;
out.push(flag ?? null, -obj.base);
const run = async (v) => (await v) * 2;
run(4).then((v) => { out.push(v); console.log(JSON.stringify(out)); });
"#;

/// Runs a CommonJS script with `node`. Returns `None` when `node` is not
/// available.
fn run_node(code: &str) -> Option<(String, Option<i32>)> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("node")
        .args(["--input-type=commonjs", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(code.as_bytes()).ok()?;
    let output = child.wait_with_output().ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code(),
    ))
}

fn activate(id: &str, code: &str) -> String {
    format!(
        "globalThis.__fallowMutation = {{ a: (id) => id === \"{id}\", c: () => {{}} }};\n{code}"
    )
}

#[test]
fn instrumented_code_runs_like_the_original_and_like_each_mutant() {
    // A machine without `node` skips the check.
    let Some(original) = run_node(RUNTIME_FIXTURE) else {
        return;
    };
    assert_eq!(original.1, Some(0), "the fixture fails: {original:?}");
    let result = run(RUNTIME_FIXTURE, SourceKind::Js);
    assert!(result.mutants.len() > 20, "{}", result.mutants.len());
    assert_eq!(run_node(&result.code), Some(original.clone()));
    let mut differs = 0;
    for mutant in &result.mutants {
        let mutated = run_node(&common::apply(RUNTIME_FIXTURE, mutant));
        let switched = run_node(&activate(mutant.id.as_str(), &result.code));
        assert_eq!(
            switched, mutated,
            "{} {:?} -> {:?}",
            mutant.operator, mutant.original, mutant.replacement
        );
        if mutated.as_ref() != Some(&original) {
            differs += 1;
        }
    }
    assert!(differs > 0, "no mutant changed the output");
}

#[test]
fn applied_replacement_never_joins_the_previous_statement() {
    let sources = [
        "log(1)\n!function () { g() }()\n",
        "log(1)\na && b || c\n",
        "log(1)\na < b === c\n",
        "do!f()\nwhile (0)\n",
    ];
    for source in sources {
        let statements = with_program(source, SourceKind::Js, |program| program.body.len());
        let result = run(source, SourceKind::Js);
        assert!(!result.mutants.is_empty(), "{source}");
        for mutant in &result.mutants {
            let mutated = common::apply(source, mutant);
            common::assert_valid(&mutated, SourceKind::Js);
            let count = with_program(&mutated, SourceKind::Js, |program| program.body.len());
            assert_eq!(
                count, statements,
                "{:?}\n---\n{mutated}",
                mutant.replacement
            );
        }
    }
}
