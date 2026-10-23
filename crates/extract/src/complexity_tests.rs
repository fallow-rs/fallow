use super::*;
use fallow_types::extract::compute_line_offsets;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn analyze(source: &str) -> Vec<FunctionComplexity> {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_return = Parser::new(&allocator, source, source_type).parse();
    let line_offsets = compute_line_offsets(source);
    compute_complexity(&parser_return.program, source, &line_offsets)
}

fn find_fn<'a>(results: &'a [FunctionComplexity], name: &str) -> &'a FunctionComplexity {
    results
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("function '{name}' not found in results: {results:?}"))
}

#[test]
fn empty_function_has_cyclomatic_1() {
    let results = analyze("function foo() {}");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 1);
}

#[test]
fn overload_signatures_do_not_emit_complexity_units() {
    let results = analyze(
        "function parse(value: string): string;\n\
         function parse(value: number): number;\n\
         function parse(value: string | number) {\n\
           if (typeof value === 'string') { return value; }\n\
           return value.toString();\n\
         }",
    );
    let parse_functions: Vec<_> = results
        .iter()
        .filter(|function| function.name == "parse")
        .collect();

    assert_eq!(parse_functions.len(), 1);
    assert_eq!(parse_functions[0].line, 3);
    assert_eq!(parse_functions[0].cyclomatic, 2);
}

#[test]
fn class_method_overload_signatures_do_not_emit_complexity_units() {
    let results = analyze(
        "class Parser {\n\
           method(a: string): void;\n\
           method(a: number): void;\n\
           method(a: any) { if (a) { return; } }\n\
         }",
    );
    let methods: Vec<_> = results
        .iter()
        .filter(|function| function.name == "method")
        .collect();

    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].line, 4);
    assert_eq!(methods[0].cyclomatic, 2);
}

#[test]
fn class_method_extraction_preserves_private_provenance() {
    let results = analyze(
        "class Vault {\n\
           #wipe() {}\n\
           '#archive'() {}\n\
         }",
    );
    let private_method = find_fn(&results, "#wipe");
    let quoted_public_method = find_fn(&results, "#archive");

    assert!(private_method.is_private_member);
    assert!(!quoted_public_method.is_private_member);
}

#[test]
fn abstract_class_methods_do_not_emit_complexity_units() {
    let results = analyze(
        "abstract class Shape {\n\
           abstract area(): number;\n\
           abstract describe(prefix: string): string;\n\
           name() { return 'shape'; }\n\
         }",
    );

    assert!(results.iter().all(|function| function.name != "area"));
    assert!(results.iter().all(|function| function.name != "describe"));
    let name = find_fn(&results, "name");
    assert_eq!(name.line, 4);
    assert_eq!(name.cyclomatic, 1);
}

#[test]
fn declare_function_does_not_emit_complexity_unit() {
    let results = analyze(
        "declare function ambient(value: string): void;\n\
         function real(value: string) { return value; }",
    );

    assert!(results.iter().all(|function| function.name != "ambient"));
    let real = find_fn(&results, "real");
    assert_eq!(real.line, 2);
    assert_eq!(real.cyclomatic, 1);
}

#[test]
fn if_statement_adds_1() {
    let results = analyze("function foo(x) { if (x) { return 1; } return 0; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn if_else_if_else_adds_2() {
    let results = analyze(
        "function foo(x) { if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; } }",
    );
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 3);
}

#[test]
fn for_loop_adds_1() {
    let results = analyze("function foo() { for (let i = 0; i < 10; i++) {} }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn while_loop_adds_1() {
    let results = analyze("function foo() { while (true) { break; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn switch_case_adds_per_case() {
    let results =
        analyze("function foo(x) { switch (x) { case 1: break; case 2: break; default: break; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 3);
}

#[test]
fn catch_adds_1() {
    let results = analyze("function foo() { try { } catch (e) { } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn ternary_adds_1() {
    let results = analyze("function foo(x) { return x ? 1 : 0; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn logical_and_adds_1() {
    let results = analyze("function foo(a, b) { return a && b; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn logical_or_adds_1() {
    let results = analyze("function foo(a, b) { return a || b; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn nullish_coalescing_adds_1() {
    let results = analyze("function foo(a) { return a ?? 'default'; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn logical_assignment_adds_1() {
    let results = analyze("function foo(a) { a &&= true; a ||= false; a ??= null; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 4);
}

#[test]
fn do_while_adds_1() {
    let results = analyze("function foo() { do { } while (true); }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn for_of_adds_1() {
    let results = analyze("function foo(arr) { for (const x of arr) { } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn for_in_adds_1() {
    let results = analyze("function foo(obj) { for (const k in obj) { } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn optional_chaining_adds_1() {
    let results = analyze("function foo(obj) { return obj?.value; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn optional_chaining_computed_member_adds_1() {
    let results = analyze("function foo(obj) { return obj?.[0]; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn optional_chaining_not_cognitive() {
    let results = analyze("function foo(obj) { return obj?.a?.b?.c; }");
    let f = find_fn(&results, "foo");
    assert!(
        f.cyclomatic > 1,
        "optional chaining should increment cyclomatic"
    );
    assert_eq!(
        f.cognitive, 0,
        "optional chaining should NOT increment cognitive"
    );
}

#[test]
fn complex_function_cyclomatic() {
    let results = analyze(
        r"function complex(x, y) {
            if (x > 0) {
                for (let i = 0; i < x; i++) {
                    if (y && i > 5) {
                        return true;
                    }
                }
            } else if (x < 0) {
                while (y) {
                    y--;
                }
            }
            return x ? true : false;
        }",
    );
    let f = find_fn(&results, "complex");
    assert_eq!(f.cyclomatic, 8);
}

#[test]
fn empty_function_has_cognitive_0() {
    let results = analyze("function foo() {}");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 0);
}

#[test]
fn simple_if_cognitive_1() {
    let results = analyze("function foo(x) { if (x) { return 1; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 1);
}

#[test]
fn nested_if_cognitive_with_nesting() {
    let results = analyze("function foo(x, y) { if (x) { if (y) { return 1; } } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn if_else_cognitive() {
    let results = analyze("function foo(x) { if (x) { return 1; } else { return 0; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 2);
}

#[test]
fn if_else_if_else_cognitive() {
    let results = analyze(
        "function foo(x) { if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; } }",
    );
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn boolean_sequence_same_operator() {
    let results = analyze("function foo(a, b, c) { return a && b && c; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 1);
}

#[test]
fn boolean_sequence_mixed_operators() {
    let results = analyze("function foo(a, b, c) { return a && b || c; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 2);
}

#[test]
fn for_loop_increases_nesting() {
    let results = analyze("function foo(arr) { for (const x of arr) { if (x) { return x; } } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn switch_cognitive_1() {
    let results = analyze("function foo(x) { switch (x) { case 1: break; case 2: break; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 1);
}

#[test]
fn nested_function_resets_nesting() {
    let results = analyze(
        r"function outer(x) {
            if (x) {
                const inner = () => {
                    if (x) { return 1; }
                };
            }
        }",
    );
    let outer = find_fn(&results, "outer");
    let inner = find_fn(&results, "inner");
    assert_eq!(outer.cognitive, 1);
    assert_eq!(inner.cognitive, 1);
}

#[test]
fn break_with_label_adds_1() {
    let results = analyze("function foo() { outer: for (;;) { break outer; } }");
    let f = find_fn(&results, "foo");
    assert!(f.cognitive >= 2);
}

#[test]
fn arrow_function_tracked() {
    let results = analyze("const foo = (x) => x > 0 ? 1 : 0;");
    assert!(!results.is_empty());
    let f = &results[0];
    assert_eq!(f.name, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn line_count_computed() {
    let results = analyze("function foo() {\n  const a = 1;\n  const b = 2;\n  return a + b;\n}");
    let f = find_fn(&results, "foo");
    assert_eq!(f.line_count, 5);
}

#[test]
fn deeply_nested_cognitive() {
    let results = analyze(
        r"function deep(a, b, c, d) {
            if (a) {
                for (;;) {
                    if (b) {
                        while (c) {
                            if (d) {}
                        }
                    }
                }
            }
        }",
    );
    let f = find_fn(&results, "deep");
    assert_eq!(f.cognitive, 15);
}

#[test]
fn object_method_shorthand_named() {
    let results = analyze("const obj = { foo(x) { if (x) {} } };");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn object_arrow_property_named() {
    let results = analyze("const obj = { bar: (x) => x ? 1 : 0 };");
    let f = find_fn(&results, "bar");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn class_method_named() {
    let results = analyze("class Foo { parse(x) { if (x) {} } }");
    let f = find_fn(&results, "parse");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn export_default_function_named() {
    let results = analyze("export default function() { if (true) {} }");
    let f = find_fn(&results, "default");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn export_default_named_function_keeps_name() {
    let results = analyze("export default function myFn() { if (true) {} }");
    let f = find_fn(&results, "myFn");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn catch_cognitive_with_nesting() {
    let results = analyze("function foo() { if (true) { try { } catch (e) { } } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn do_while_cognitive_with_nesting() {
    let results = analyze("function foo() { if (true) { do { } while (true); } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn while_cognitive_with_nesting() {
    let results = analyze("function foo() { if (true) { while (true) { break; } } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn ternary_cognitive_with_nesting() {
    let results = analyze("function foo(x) { if (x) { return x ? 1 : 0; } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn continue_with_label_cognitive() {
    let results =
        analyze("function foo() { outer: for (let i = 0; i < 10; i++) { continue outer; } }");
    let f = find_fn(&results, "foo");
    assert!(f.cognitive >= 2);
}

#[test]
fn class_property_arrow_named() {
    let results = analyze("class Foo { bar = (x: number) => x > 0 ? 1 : 0; }");
    let f = find_fn(&results, "bar");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn nested_arrow_functions_independent_complexity() {
    let results = analyze(
        r"const outer = (x) => {
            if (x) {
                const inner = (y) => {
                    if (y) { return 1; }
                    return 0;
                };
                return inner(x);
            }
            return 0;
        };",
    );
    let outer = find_fn(&results, "outer");
    let inner = find_fn(&results, "inner");
    assert_eq!(outer.cyclomatic, 2);
    assert_eq!(inner.cyclomatic, 2);
}

fn names(results: &[FunctionComplexity]) -> Vec<&str> {
    results.iter().map(|r| r.name.as_str()).collect()
}

#[test]
fn returned_effect_generator_takes_the_binding_name() {
    let results = analyze(
        "const fetchIssue = (key: string) =>\n\
           Effect.gen(function* () { if (key) { yield* load(key); } });",
    );
    let named: Vec<_> = results.iter().filter(|r| r.name == "fetchIssue").collect();
    assert_eq!(named.len(), 2, "{:?}", names(&results));
    let generator = named.iter().find(|r| r.cyclomatic == 2).expect("generator");
    assert!(generator.context_named.is_some());
    let wrapper = named.iter().find(|r| r.cyclomatic == 1).expect("wrapper");
    assert!(
        wrapper.context_named.is_none(),
        "the binding name is not a context name"
    );
}

#[test]
fn generator_returned_through_pipe_takes_the_function_name() {
    let results = analyze(
        "function load(id: string) {\n\
           const key = id.trim();\n\
           return Effect.gen(function* () { if (key) {} }).pipe(Effect.withSpan(\"load\"));\n\
         }",
    );
    assert!(
        !names(&results).contains(&"<anonymous>"),
        "{:?}",
        names(&results)
    );
    let generator = results
        .iter()
        .find(|r| r.name == "load" && r.cyclomatic == 2)
        .expect("generator named load");
    assert!(generator.context_named.is_some());
}

#[test]
fn string_keyed_call_names_its_callback_and_generator() {
    let results = analyze(
        "export const setup = Command.make(\"setup\", { force: flag }, (args) =>\n\
           Effect.gen(function* () { if (args.force) {} }),\n\
         );\n\
         group.handle(\"github\", (_args) => Effect.gen(function* () {}));",
    );
    let all = names(&results);
    assert!(!all.contains(&"<anonymous>"), "{all:?}");
    assert!(!all.contains(&"<arrow>"), "{all:?}");
    assert_eq!(
        results
            .iter()
            .filter(|r| r.name == "Command.make(\"setup\")")
            .count(),
        2,
        "{all:?}"
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| r.name == "group.handle(\"github\")")
            .count(),
        2,
        "{all:?}"
    );
}

#[test]
fn context_names_keep_the_placeholder_as_identity_name() {
    let results = analyze(
        "const load = () => Effect.gen(function* () {});\n\
         Command.make(\"setup\", () => 1);",
    );
    let generator = results
        .iter()
        .find(|r| r.name == "load" && r.context_named.is_some())
        .expect("generator");
    assert_eq!(generator.identity_name(), "<anonymous>");
    let callback = find_fn(&results, "Command.make(\"setup\")");
    assert_eq!(
        callback.context_named,
        Some(AnonymousFunctionKind::Arrow),
        "an arrow keeps its own placeholder kind"
    );
    assert_eq!(callback.identity_name(), "<arrow>");
    let wrapper = results
        .iter()
        .find(|r| r.name == "load" && r.context_named.is_none())
        .expect("wrapper");
    assert_eq!(wrapper.identity_name(), "load");
}

#[test]
fn effect_fn_span_names_the_body() {
    let results = analyze(
        "group.handle(\"get\", Effect.fn(function* ({ payload }) { if (payload) {} }));\n\
         register(Effect.fn(\"Users.create\")(function* (input) { if (input) {} }, (effect) => effect));",
    );
    let all = names(&results);
    assert!(all.contains(&"group.handle(\"get\")"), "{all:?}");
    assert!(all.contains(&"Users.create"), "{all:?}");
    assert!(
        all.contains(&"<arrow>"),
        "a pipeline transform after the body keeps its placeholder: {all:?}"
    );
}

#[test]
fn declared_and_bound_names_do_not_change() {
    let results = analyze(
        "const create = Effect.fn(\"Users.create\")(function* () {});\n\
         const doctor = Command.make(\"doctor\", () => 1);\n\
         it(\"works\", function named() {});",
    );
    let all = names(&results);
    assert!(all.contains(&"create"), "{all:?}");
    assert!(all.contains(&"doctor"), "{all:?}");
    assert!(all.contains(&"named"), "{all:?}");
    assert!(results.iter().all(|r| r.context_named.is_none()), "{all:?}");
}

#[test]
fn callbacks_that_are_not_the_body_keep_their_placeholder() {
    let results = analyze(
        "const render = (items) => items.map((item) => item ? 1 : 0);\n\
         Match.when({ type: \"a\" }, (req) => Effect.gen(function* () {}));\n\
         it(\"wraps\", wrap(() => 1));\n\
         yieldAll(function* () {});",
    );
    let all = names(&results);
    assert_eq!(
        all.iter().filter(|n| **n == "<arrow>").count(),
        3,
        "{all:?}"
    );
    assert_eq!(
        all.iter().filter(|n| **n == "<anonymous>").count(),
        2,
        "{all:?}"
    );
    assert!(!all.contains(&"it(\"wraps\")"), "{all:?}");
}

#[test]
fn context_names_do_not_add_react_hook_density() {
    let results = analyze(
        "Command.make(\"Setup\", () => Effect.gen(function* () { useThing(); }));\n\
         const Panel = () => run(function* () { useThing(); });",
    );
    for result in results.iter().filter(|r| r.context_named.is_some()) {
        assert_eq!(result.cognitive, 0, "{}", result.name);
    }
    assert!(
        results
            .iter()
            .any(|r| r.context_named.is_some() && r.name == "Panel")
    );
}

#[test]
fn effect_combinators_add_no_complexity() {
    let results = analyze(
        "const program = pipe(\n\
           load,\n\
           Effect.catchTag(\"NotFound\", () => Effect.succeed(null)),\n\
           Effect.flatMap((value) =>\n\
             Match.value(value).pipe(\n\
               Match.when({ kind: \"a\" }, () => Effect.void),\n\
               Match.orElse(() => Effect.fail(new Unknown())),\n\
             ),\n\
           ),\n\
           Effect.if({ onTrue: () => a, onFalse: () => b }),\n\
         );\n\
         const run = Effect.gen(function* () {\n\
           const a = yield* first;\n\
           const b = yield* second(a);\n\
           return yield* third(b);\n\
         });",
    );
    for result in &results {
        assert_eq!(
            (result.cyclomatic, result.cognitive),
            (1, 0),
            "{} counts a combinator as a decision",
            result.name
        );
    }
}

#[test]
fn nesting_does_not_pass_into_an_effect_callback() {
    let results = analyze(
        "const outer = (flag) => {\n\
           if (flag) {\n\
             return Effect.gen(function* () { if (flag) {} });\n\
           }\n\
           return Effect.void;\n\
         };",
    );
    let generator = results
        .iter()
        .find(|r| r.name == "outer" && r.context_named.is_some())
        .expect("generator");
    assert_eq!(generator.cognitive, 1, "the generator starts at nesting 0");
}

#[test]
fn method_definition_named() {
    let results = analyze("class Foo { doWork(x) { if (x) { return 1; } return 0; } }");
    let f = find_fn(&results, "doWork");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn logical_nullish_cognitive() {
    let results = analyze("function foo(a, b) { return a ?? b; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 1);
}

#[test]
fn mixed_logical_operators_cognitive() {
    let results = analyze("function foo(a, b, c, d) { return a && b || c ?? d; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn saturating_add_prevents_overflow() {
    let mut source = "function foo() {".to_string();
    for _ in 0..20 {
        source.push_str("if (true) {");
    }
    for _ in 0..20 {
        source.push('}');
    }
    source.push('}');
    let results = analyze(&source);
    assert!(!results.is_empty());
}

#[test]
fn empty_source_no_functions() {
    let results = analyze("");
    assert!(results.is_empty());
}

#[test]
fn top_level_code_reported_as_the_module_unit() {
    let results = analyze("if (true) { console.log('hello'); }");
    assert_eq!(results.len(), 1);
    let module = find_fn(&results, "<module>");
    assert_eq!(module.cyclomatic, 2);
    assert_eq!(module.cognitive, 1);
}

#[test]
fn for_in_cognitive_with_nesting() {
    let results = analyze("function foo(obj) { for (const k in obj) { if (k) {} } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn for_of_cognitive_with_nesting() {
    let results = analyze("function foo(arr) { for (const x of arr) { if (x) {} } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn optional_call_expression_cyclomatic() {
    let results = analyze("function foo(obj) { return obj?.method(); }");
    let f = find_fn(&results, "foo");
    assert!(f.cyclomatic >= 1);
    assert_eq!(f.cognitive, 0);
}

#[test]
fn logical_assignment_not_cognitive() {
    let results = analyze("function foo(a) { a &&= true; }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 2);
}

#[test]
fn multiple_switch_cases_cyclomatic() {
    let results = analyze(
        "function foo(x) { switch (x) { case 1: break; case 2: break; case 3: break; default: break; } }",
    );
    let f = find_fn(&results, "foo");
    assert_eq!(f.cyclomatic, 4);
}

#[test]
fn switch_nested_in_if_cognitive() {
    let results = analyze("function foo(x, y) { if (x) { switch (y) { case 1: break; } } }");
    let f = find_fn(&results, "foo");
    assert_eq!(f.cognitive, 3);
}

#[test]
fn line_and_col_computed_correctly() {
    let results = analyze("\n\nfunction foo() {\n  if (true) {}\n}\n");
    let f = find_fn(&results, "foo");
    assert_eq!(f.line, 3);
}

#[test]
fn param_count_zero_for_no_params() {
    let results = analyze("function foo() {}");
    assert_eq!(find_fn(&results, "foo").param_count, 0);
}

#[test]
fn param_count_simple_params() {
    let results = analyze("function foo(a, b, c) {}");
    assert_eq!(find_fn(&results, "foo").param_count, 3);
}

#[test]
fn param_count_arrow_function() {
    let results = analyze("const bar = (a, b, c, d, e) => {}");
    assert_eq!(find_fn(&results, "bar").param_count, 5);
}

#[test]
fn param_count_excludes_ts_this_parameter() {
    let results = analyze("function greet(this: Context, name: string) {}");
    assert_eq!(find_fn(&results, "greet").param_count, 1);
}

#[test]
fn param_count_destructured_counts_as_one() {
    let results = analyze("function foo({ a, b, c }: Options) {}");
    assert_eq!(find_fn(&results, "foo").param_count, 1);
}

#[test]
fn param_count_rest_parameter() {
    let results = analyze("function foo(a: number, ...rest: string[]) {}");
    assert_eq!(find_fn(&results, "foo").param_count, 2);
}

#[test]
fn param_count_method_definition() {
    let results = analyze("class Foo { bar(a: number, b: string) {} }");
    assert_eq!(find_fn(&results, "bar").param_count, 2);
}

// --- Per-decision-point contribution breakdown ---

fn sum_weights(f: &FunctionComplexity, metric: ComplexityMetric) -> u16 {
    f.contributions
        .iter()
        .filter(|c| c.metric == metric)
        .map(|c| c.weight)
        .sum()
}

/// The load-bearing invariant: the recorded breakdown must reconstruct the
/// aggregate metric exactly. `cyclomatic = 1 + sum(cyclomatic weights)`,
/// `cognitive = sum(cognitive weights)`. Asserted over an adversarial corpus
/// that stresses every kind, including nested `else if` inside loops and
/// `try` (the exact path whose arithmetic the else-if refactor rewrote).
#[test]
fn contributions_reconstruct_aggregate_metrics() {
    let corpus = [
        "function f(a,b){ if(a){} else if(b){} else {} }",
        "function f(a){ for(let i=0;i<10;i++){ if(a){ while(a){} } } }",
        "function f(a){ try { if(a){} } catch(e) { if(a){} else if(a){} } }",
        "function f(a,b,c){ return a && b || c ?? a; }",
        "function f(a){ switch(a){ case 1: break; case 2: return; default: return; } }",
        "function f(a){ for(const x of a){ if(x){ if(x){ if(x){} } } } }",
        "function f(a){ return a?.b?.c?.d; }",
        "function f(a){ a ||= 1; a &&= 2; a ??= 3; }",
        "function f(a){ outer: for(;;){ if(a){ break outer; } continue outer; } }",
        "function f(a,b){ if(a){ if(b){} else if(a){} else {} } else if(b){ for(;;){} } }",
        "const f = (a) => a ? (a ? 1 : 2) : 3;",
        "function f(a){ do { if(a){} } while(a); }",
        // React shapes: the JSX-depth, hook-density, and prop-count folds
        // must reconstruct cognitive exactly like every other increment.
        "function App({ a, b, c, d, e, f, g }) { return <div><span><b><i/></b></span></div>; }",
        "function App() { useState(); useEffect(() => {}); return <div/>; }",
        "const App = ({ a, b }) => (<ul><li><a><img/></a></li></ul>);",
        "function App({ x }) { return x ? <div><p><em/></p></div> : <span/>; }",
    ];
    for src in corpus {
        for func in analyze(src) {
            assert_eq!(
                func.cyclomatic,
                1 + sum_weights(&func, ComplexityMetric::Cyclomatic),
                "cyclomatic mismatch for `{src}`: {:?}",
                func.contributions
            );
            assert_eq!(
                func.cognitive,
                sum_weights(&func, ComplexityMetric::Cognitive),
                "cognitive mismatch for `{src}`: {:?}",
                func.contributions
            );
        }
    }
}

#[test]
fn else_if_contribution_is_flat_one() {
    // `else if` adds a flat +1 cognitive (no nesting penalty) and is recorded
    // as the ElseIf kind on the else-if line, not the leading `if` line.
    let results = analyze("function f(a, b) {\n  if (a) {\n  } else if (b) {\n  }\n}");
    let f = find_fn(&results, "f");
    let else_if = f
        .contributions
        .iter()
        .find(|c| {
            c.kind == ComplexityContributionKind::ElseIf && c.metric == ComplexityMetric::Cognitive
        })
        .expect("else-if cognitive contribution");
    assert_eq!(else_if.weight, 1, "else-if cognitive is flat +1");
    assert_eq!(else_if.nesting, 0);
    assert_eq!(else_if.line, 3, "anchored on the `else if` line");
}

#[test]
fn nested_if_carries_nesting_weight() {
    // An `if` nested one level deep inside another `if` gets cognitive +2
    // (+1 base, +1 nesting), recorded with nesting == 1.
    let results = analyze("function f(a, b) { if (a) { if (b) {} } }");
    let f = find_fn(&results, "f");
    let nested = f
        .contributions
        .iter()
        .filter(|c| {
            c.kind == ComplexityContributionKind::If && c.metric == ComplexityMetric::Cognitive
        })
        .max_by_key(|c| c.nesting)
        .expect("a nested if cognitive contribution");
    assert_eq!(nested.nesting, 1);
    assert_eq!(nested.weight, 2, "+1 base, +1 nesting");
}

#[test]
fn cyclomatic_and_cognitive_logical_share_a_line() {
    // The cyclomatic and cognitive contributions for a `&&` are anchored to
    // the same node span, so a consumer grouping by line sees both together.
    let results = analyze("function f(a, b) { return a && b; }");
    let f = find_fn(&results, "f");
    let cyc = f
        .contributions
        .iter()
        .find(|c| {
            c.kind == ComplexityContributionKind::LogicalAnd
                && c.metric == ComplexityMetric::Cyclomatic
        })
        .expect("cyclomatic &&");
    let cog = f
        .contributions
        .iter()
        .find(|c| {
            c.kind == ComplexityContributionKind::LogicalAnd
                && c.metric == ComplexityMetric::Cognitive
        })
        .expect("cognitive &&");
    assert_eq!(cyc.line, cog.line);
}

#[test]
fn simple_function_has_no_contributions() {
    let results = analyze("function f(a) { return a; }");
    assert!(find_fn(&results, "f").contributions.is_empty());
}

#[test]
fn default_case_emits_no_contribution() {
    // A bare `default:` carries no test, so it adds nothing to cyclomatic and
    // produces no Case contribution; only the two real cases do.
    let results = analyze("function f(a) { switch(a) { case 1: break; default: break; } }");
    let f = find_fn(&results, "f");
    let cases = f
        .contributions
        .iter()
        .filter(|c| c.kind == ComplexityContributionKind::Case)
        .count();
    assert_eq!(cases, 1, "only the `case 1` test, not `default`");
}

// --- React-aware complexity folding (Phase 2) ---

fn count_kind(f: &FunctionComplexity, kind: ComplexityContributionKind) -> usize {
    f.contributions.iter().filter(|c| c.kind == kind).count()
}

#[test]
fn shallow_jsx_is_free() {
    // A component with shallow host markup accrues no JSX cognitive penalty:
    // a normal component must not surface as a hotspot.
    let results = analyze("function App() { return <div><span/></div>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.cognitive, 0, "shallow JSX adds no cognitive load");
    assert_eq!(count_kind(f, ComplexityContributionKind::JsxDepth), 0);
    assert_eq!(f.react_jsx_max_depth, 2);
}

#[test]
fn deep_jsx_is_descriptive_only() {
    // A deeply nested JSX tree records max depth for context, but pure layout
    // depth must not score as cognitive control flow.
    let deep = analyze("function App() { return <a><b><c><d><e/></d></c></b></a>; }");
    let f = find_fn(&deep, "App");
    assert_eq!(f.cognitive, 0, "deep JSX adds no cognitive load");
    assert_eq!(count_kind(f, ComplexityContributionKind::JsxDepth), 0);
    assert_eq!(f.react_jsx_max_depth, 5);
}

#[test]
fn deep_jsx_does_not_score_higher_than_flat() {
    // Layout depth alone is not cognitive complexity, so a deeply nested
    // component and a flat one with no control flow score the same.
    let flat = analyze("function Flat() { return <div><span/></div>; }");
    let deep = analyze("function Deep() { return <a><b><c><d><e><f/></e></d></c></b></a>; }");
    let flat_cog = find_fn(&flat, "Flat").cognitive;
    let deep_cog = find_fn(&deep, "Deep").cognitive;
    assert_eq!(deep_cog, flat_cog);
    assert_eq!(find_fn(&deep, "Deep").react_jsx_max_depth, 6);
}

#[test]
fn ternary_inside_deep_jsx_keeps_normal_weight() {
    // A ternary rendered inside JSX keeps normal cognitive weight. Markup
    // depth does not deepen the structural nesting level.
    let shallow = analyze("function A({ x }) { return <div>{x ? <p/> : <span/>}</div>; }");
    let deep =
        analyze("function B({ x }) { return <a><b><c><d>{x ? <p/> : <span/>}</d></c></b></a>; }");
    let shallow_tern = find_fn(&shallow, "A")
        .contributions
        .iter()
        .find(|c| {
            c.kind == ComplexityContributionKind::Ternary && c.metric == ComplexityMetric::Cognitive
        })
        .expect("ternary in shallow")
        .weight;
    let deep_tern = find_fn(&deep, "B")
        .contributions
        .iter()
        .find(|c| {
            c.kind == ComplexityContributionKind::Ternary && c.metric == ComplexityMetric::Cognitive
        })
        .expect("ternary in deep")
        .weight;
    assert_eq!(shallow_tern, 1);
    assert_eq!(deep_tern, shallow_tern);
    assert_eq!(
        count_kind(find_fn(&deep, "B"), ComplexityContributionKind::JsxDepth),
        0
    );
}

#[test]
fn presentational_skeleton_jsx_does_not_emit_complexity() {
    let results = analyze(
        "function SkeletonTable() { return <Card><CardContent><Table><TableBody><TableRow><TableCell><Skeleton/></TableCell></TableRow></TableBody></Table></CardContent></Card>; }",
    );
    let f = find_fn(&results, "SkeletonTable");
    assert_eq!(f.cyclomatic, 1);
    assert_eq!(f.cognitive, 0);
    assert_eq!(f.react_jsx_max_depth, 7);
    assert_eq!(count_kind(f, ComplexityContributionKind::JsxDepth), 0);
    assert!(f.contributions.is_empty());
}

#[test]
fn hook_density_folds_into_cognitive() {
    // Each hook call in a component body adds a flat HookDensity cognitive
    // increment and is counted descriptively.
    let results = analyze(
        "function App() { const [a, setA] = useState(0); useEffect(() => {}, []); const v = useMemo(() => 1, []); return <div/>; }",
    );
    let f = find_fn(&results, "App");
    assert_eq!(f.react_hook_count, 3, "three hook calls counted");
    assert_eq!(count_kind(f, ComplexityContributionKind::HookDensity), 3);
    assert!(f.cognitive >= 3, "three hooks add at least +3 cognitive");
}

#[test]
fn hook_heavy_component_scores_higher_than_flat() {
    // The headline property for hooks: a hook-heavy component scores strictly
    // higher cognitive than one with no hooks.
    let flat = analyze("function Flat() { return <div/>; }");
    let heavy = analyze(
        "function Heavy() { useState(); useState(); useEffect(() => {}); useMemo(() => 1); return <div/>; }",
    );
    let flat_cog = find_fn(&flat, "Flat").cognitive;
    let heavy_cog = find_fn(&heavy, "Heavy").cognitive;
    assert!(
        heavy_cog > flat_cog,
        "hook-heavy ({heavy_cog}) must score higher than flat ({flat_cog})"
    );
}

#[test]
fn custom_hook_callee_counts() {
    // A custom `use*` hook call is folded just like a built-in hook.
    let results = analyze("function App() { const data = useFetchUser(); return <div/>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.react_hook_count, 1);
    assert_eq!(count_kind(f, ComplexityContributionKind::HookDensity), 1);
}

#[test]
fn use_prefixed_non_hook_not_counted() {
    // `use` / `used` (no following uppercase) is not a hook and must not fold.
    let results = analyze("function App() { use(); used(); return <div/>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.react_hook_count, 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::HookDensity), 0);
}

#[test]
fn hook_in_non_react_function_counts_but_does_not_fold() {
    // A `use*` call inside an ordinary non-React function (not capitalized, not
    // hook-named) is counted descriptively but does NOT fold into cognitive,
    // keeping the zero-FP posture for non-React code.
    let results = analyze("function helper() { useState(); return 1; }");
    let f = find_fn(&results, "helper");
    assert_eq!(f.react_hook_count, 1, "still counted for context");
    assert_eq!(
        count_kind(f, ComplexityContributionKind::HookDensity),
        0,
        "but not folded into cognitive"
    );
    assert_eq!(f.cognitive, 0);
}

#[test]
fn prop_count_under_floor_is_free() {
    // A component with a small prop interface accrues no prop penalty.
    let results = analyze("function App({ a, b, c }) { return <div/>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.react_prop_count, 3);
    assert_eq!(count_kind(f, ComplexityContributionKind::PropCount), 0);
    assert_eq!(f.cognitive, 0);
}

#[test]
fn wide_prop_interface_folds_excess_into_cognitive() {
    // A wide prop interface folds the props past the floor into cognitive.
    let results = analyze("function App({ a, b, c, d, e, f, g, h }) { return <div/>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.react_prop_count, 8);
    let prop = f
        .contributions
        .iter()
        .find(|c| c.kind == ComplexityContributionKind::PropCount)
        .expect("a PropCount contribution");
    assert_eq!(prop.weight, 8 - PROP_COUNT_FLOOR, "excess over the floor");
    assert!(f.cognitive >= prop.weight);
}

#[test]
fn prop_count_not_folded_without_jsx() {
    // A plain function destructuring a wide options bag (no JSX) is NOT a
    // component and must not accrue the prop penalty.
    let results = analyze("function configure({ a, b, c, d, e, f, g, h }) { return a + b; }");
    let f = find_fn(&results, "configure");
    assert_eq!(f.react_prop_count, 8, "still counted descriptively");
    assert_eq!(
        count_kind(f, ComplexityContributionKind::PropCount),
        0,
        "but not folded: not a component"
    );
}

#[test]
fn bare_props_identifier_counts_zero_props() {
    // A component taking a bare `props` identifier is not statically
    // prop-countable; prop_count is 0 and nothing folds.
    let results = analyze("function App(props) { return <div>{props.title}</div>; }");
    let f = find_fn(&results, "App");
    assert_eq!(f.react_prop_count, 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::PropCount), 0);
}

#[test]
fn arrow_component_folds_react_signals() {
    // Arrow-bound components keep descriptive JSX depth while hook density
    // and prop count still fold into cognitive complexity.
    let results = analyze(
        "const App = ({ a, b, c, d, e, f }) => { useState(); return <ul><li><span><b/></span></li></ul>; };",
    );
    let f = find_fn(&results, "App");
    assert_eq!(f.react_prop_count, 6);
    assert_eq!(f.react_hook_count, 1);
    assert!(f.react_jsx_max_depth >= 4);
    assert!(count_kind(f, ComplexityContributionKind::PropCount) > 0);
    assert!(count_kind(f, ComplexityContributionKind::HookDensity) > 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::JsxDepth), 0);
}

#[test]
fn non_react_function_unaffected() {
    // A plain function with no JSX/hooks/props is byte-identical to before:
    // no React fields, no React contributions.
    let results = analyze("function add(a, b) { if (a) { return a + b; } return b; }");
    let f = find_fn(&results, "add");
    assert_eq!(f.react_hook_count, 0);
    assert_eq!(f.react_jsx_max_depth, 0);
    assert_eq!(f.react_prop_count, 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::JsxDepth), 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::HookDensity), 0);
    assert_eq!(count_kind(f, ComplexityContributionKind::PropCount), 0);
    assert_eq!(f.cognitive, 1, "just the single if");
}

/// The set-level form of the per-unit identity asserted above. Everything
/// the branching aggregate reports rests on it, so it is checked together
/// with the absence of `u16` saturation: the identity alone would still
/// hold vacuously if a unit had saturated.
fn assert_conservation(source: &str) -> fallow_types::extract::FileBranching {
    let units = analyze(source);
    assert!(
        units.iter().all(|unit| unit.cyclomatic < u16::MAX),
        "a unit saturated u16, the identity is vacuous for `{source}`"
    );
    let direct: u32 = units.iter().map(|unit| u32::from(unit.cyclomatic)).sum();
    let totals = fallow_types::extract::FileBranching::from_units(&units);
    assert_eq!(
        totals.implied_cyclomatic(),
        direct,
        "functions + branch_points must equal the summed cyclomatic for `{source}`"
    );
    totals
}

#[test]
fn branching_is_conserved_when_one_function_becomes_many() {
    // The canonical shape an agent produces under a per-function ceiling.
    let whole = assert_conservation(
        "function price(a, b, c, d) {
          if (a) { return 1; }
          if (b) { return 2; }
          if (c) { return 3; }
          if (d) { return 4; }
          return 0;
        }",
    );
    let split = assert_conservation(
        "function a1(a) { if (a) { return 1; } return 0; }
         function a2(b) { if (b) { return 2; } return 0; }
         function a3(c) { if (c) { return 3; } return 0; }
         function a4(d) { if (d) { return 4; } return 0; }
         function price(a, b, c, d) {
           return a1(a) + a2(b) + a3(c) + a4(d);
         }",
    );

    assert_eq!(whole.functions, 1);
    assert_eq!(whole.branch_points, 4);
    assert_eq!(whole.peak_cyclomatic, 5);
    assert_eq!(split.functions, 5, "four helpers plus the caller");
    assert_eq!(
        split.branch_points, whole.branch_points,
        "the four branches survive the split unchanged"
    );
    assert_eq!(
        split.peak_cyclomatic, 2,
        "the peak falls from 5 to 2 even though nothing was removed"
    );
}

#[test]
fn conservation_holds_across_every_increment_family() {
    for source in [
        "function f(a) { return a ? 1 : 2; }",
        "function f(a, b) { return a && b; }",
        "function f(a) { for (const x of a) { if (x) { return x; } } return null; }",
        "function f(a) { switch (a) { case 1: return 1; case 2: return 2; default: return 0; } }",
        "function f() { try { g(); } catch (e) { return e; } return null; }",
        "function f(a) { return a?.b?.c; }",
        "const f = (a) => (b) => a && b;",
        "function App({ a, b, c, d, e, f, g }) { return <div><span><b/></span></div>; }",
    ] {
        assert_conservation(source);
    }
}

#[test]
fn cognitive_aggregate_excludes_prop_count_and_hook_density() {
    // Both are cognitive-only and move without any branching changing, so
    // the aggregate deliberately diverges from `FunctionComplexity::cognitive`.
    let source = "function App({ a, b, c, d, e, f, g, h }) { return <div/>; }";
    let units = analyze(source);
    let totals = fallow_types::extract::FileBranching::from_units(&units);
    let raw: u32 = units.iter().map(|unit| u32::from(unit.cognitive)).sum();
    assert!(
        units.iter().any(|unit| unit
            .contributions
            .iter()
            .any(|c| c.kind == ComplexityContributionKind::PropCount)),
        "fixture must actually produce a PropCount contribution"
    );
    assert!(
        totals.cognitive < raw,
        "prop-count excess must be excluded: {} vs {raw}",
        totals.cognitive
    );
    assert_eq!(totals.branch_points, 0, "no branching in the fixture");
}

#[test]
fn synthetic_template_units_are_excluded_from_the_aggregate() {
    let mut units = analyze("function f(a) { if (a) { return 1; } return 0; }");
    let mut synthetic = units[0].clone();
    synthetic.name = "<template>".to_string();
    units.push(synthetic);
    let totals = fallow_types::extract::FileBranching::from_units(&units);
    assert_eq!(totals.functions, 1, "the synthetic unit is not counted");
    assert_eq!(totals.branch_points, 1);
}

#[test]
fn hoisting_a_branch_to_module_scope_keeps_it_counted() {
    // The root module frame is what makes this hold: hoisting an `if` out
    // of a function moves the increment into the `<module>` unit instead of
    // deleting it, exactly as moving it into a new function would.
    let inside = assert_conservation("const f = (a) => { if (a) { return 1; } return 0; };");
    let hoisted = assert_conservation(
        "let value = 0;
         if (globalThis.flag) { value = 1; }
         const f = () => value;",
    );
    assert_eq!(inside.branch_points, 1);
    assert_eq!(
        hoisted.branch_points, 1,
        "a branch hoisted to module scope stays in the total"
    );
    assert_eq!(
        hoisted.functions, 2,
        "the module unit is counted alongside the arrow"
    );
}

#[test]
fn module_scope_unit_is_emitted_only_when_it_branches() {
    let plain = analyze("const a = 1;\nexport const b = a;\n");
    assert!(
        !plain.iter().any(|unit| unit.name == "<module>"),
        "a file with no module-scope decision point gets no unit: {plain:?}"
    );

    let branching = analyze("export const b = globalThis.cfg ?? fallback;\n");
    let unit = find_fn(&branching, "<module>");
    assert_eq!(unit.cyclomatic, 2);
    assert_eq!(unit.cognitive, 1);
    assert_eq!(unit.param_count, 0);
    assert!(unit.source_hash.is_none());
}

#[test]
fn module_scope_unit_anchors_and_sizes_on_its_own_contributions() {
    // Line 1 is a comment and lines 5-9 are a function body, so an anchor at
    // line 1 or a whole-file line count would both be wrong.
    let source = "// header\n\
                  \n\
                  const mode = globalThis.MODE ?? 'dev';\n\
                  \n\
                  export function run(items) {\n\
                  \x20 for (const item of items) {\n\
                  \x20   if (item) { return item; }\n\
                  \x20 }\n\
                  \x20 return null;\n\
                  }\n";
    let units = analyze(source);
    let module = find_fn(&units, "<module>");
    assert_eq!(
        module.line, 3,
        "anchored at the first contributing construct"
    );
    assert_eq!(module.line_count, 1, "sized by its own contributions");
}

#[test]
fn a_root_frame_does_not_move_any_per_function_number() {
    // The root frame flips `is_nested` to true for top-level functions,
    // which now calls `inc_nesting` on it. Nothing may accrue to the root
    // while a child frame is on top, and no child number may move.
    let source = "let flag = 0;\n\
                  if (globalThis.debug) { flag = 1; }\n\
                  export function run(items) {\n\
                  \x20 for (const item of items) {\n\
                  \x20   if (item && item.id) { return item; }\n\
                  \x20 }\n\
                  \x20 return null;\n\
                  }\n";
    let with_module_branching = analyze(source);
    let without = analyze(
        "export function run(items) {\n\
                  \x20 for (const item of items) {\n\
                  \x20   if (item && item.id) { return item; }\n\
                  \x20 }\n\
                  \x20 return null;\n\
                  }\n",
    );
    let run_with = find_fn(&with_module_branching, "run");
    let run_without = find_fn(&without, "run");
    assert_eq!(run_with.cyclomatic, run_without.cyclomatic);
    assert_eq!(run_with.cognitive, run_without.cognitive);
    assert_eq!(
        run_with.contributions.len(),
        run_without.contributions.len()
    );
    for (a, b) in run_with
        .contributions
        .iter()
        .zip(run_without.contributions.iter())
    {
        assert_eq!(
            (a.metric, a.kind, a.weight, a.nesting),
            (b.metric, b.kind, b.weight, b.nesting),
            "a per-function contribution moved"
        );
    }
    let module = find_fn(&with_module_branching, "<module>");
    assert_eq!(
        module.cyclomatic, 2,
        "only the module-scope `if` accrues to the root frame"
    );
    assert_eq!(module.cognitive, 1);
}
