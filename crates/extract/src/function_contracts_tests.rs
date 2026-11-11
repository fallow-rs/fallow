use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{
    ArgFact, BindingArg, BindingRoot, CallSite, CallTarget, ContractIncomplete, FunctionContract,
    FunctionContractFacts, ImportBindingReferenceKind, KeyArg, KeyValue, LiteralValue, ModuleInfo,
    ParamKind,
};

fn parse_at(path: &str, source: &str) -> ModuleInfo {
    crate::parse_from_content(FileId(0), Path::new(path), source)
}

fn parse(source: &str) -> ModuleInfo {
    parse_at("module.ts", source)
}

fn facts(info: &ModuleInfo) -> &FunctionContractFacts {
    info.function_contracts
        .as_deref()
        .expect("the module has call evidence")
}

fn contract<'a>(info: &'a ModuleInfo, name: &str) -> &'a FunctionContract {
    facts(info)
        .declarations
        .iter()
        .find(|contract| &*contract.binding_name == name)
        .unwrap_or_else(|| panic!("no contract for {name}"))
}

fn local_calls<'a>(info: &'a ModuleInfo, name: &str) -> Vec<&'a CallSite> {
    let facts = facts(info);
    let index = facts
        .declarations
        .iter()
        .position(|contract| &*contract.binding_name == name)
        .and_then(|index| u32::try_from(index).ok())
        .expect("contract exists");
    facts
        .call_sites()
        .filter(|(target, _, _)| *target == CallTarget::Local(index))
        .map(|(_, _, site)| site)
        .collect()
}

fn incomplete(source: &str, name: &str) -> Option<ContractIncomplete> {
    contract(&parse(source), name).incomplete
}

#[test]
fn records_the_contract_of_each_module_scope_function_binding() {
    let info = parse(
        "export function declared(a, b?: number, c = 1, ...rest) { return a; }
         export const arrow = ({ x }, [y]) => x + y;
         const expression = function (flag: boolean) { if (flag) {} };
         export default function named() {}
         function outer() { function inner(z) {} const nested = (w) => w; }",
    );
    let names: Vec<_> = facts(&info)
        .declarations
        .iter()
        .map(|contract| &*contract.binding_name)
        .collect();
    assert_eq!(
        names,
        vec!["declared", "arrow", "expression", "named", "outer"]
    );

    let declared = contract(&info, "declared");
    assert!(declared.returns_value);
    assert_eq!(declared.incomplete, None);
    let params: Vec<_> = declared
        .params
        .iter()
        .map(|param| {
            (
                param.name.as_deref(),
                param.kind,
                param.optional,
                param.has_default,
                param.is_rest,
            )
        })
        .collect();
    assert_eq!(
        params,
        vec![
            (Some("a"), ParamKind::Identifier, false, false, false),
            (Some("b"), ParamKind::Identifier, true, false, false),
            (Some("c"), ParamKind::Identifier, false, true, false),
            (Some("rest"), ParamKind::Identifier, false, false, true),
        ]
    );

    let arrow = contract(&info, "arrow");
    assert!(arrow.returns_value);
    let kinds: Vec<_> = arrow
        .params
        .iter()
        .map(|param| (param.name.as_deref(), param.kind))
        .collect();
    assert_eq!(
        kinds,
        vec![(None, ParamKind::Object), (None, ParamKind::Array)]
    );

    assert!(!contract(&info, "expression").returns_value);
    assert!(!contract(&info, "named").returns_value);
}

#[test]
fn contract_span_points_at_the_binding_identifier() {
    let source = "export const format = (value) => value;\nfunction shout(text) {}";
    let info = parse(source);
    let format = contract(&info, "format");
    assert_eq!(format.span_start as usize, source.find("format").unwrap());
    let shout = contract(&info, "shout");
    assert_eq!(shout.span_start as usize, source.find("shout").unwrap());
}

#[test]
fn parameter_span_points_at_the_parameter() {
    let source = "function pick(first, second?: number, { x } = {}, ...rest) {}";
    let info = parse(source);
    let starts: Vec<_> = contract(&info, "pick")
        .params
        .iter()
        .map(|param| param.span_start as usize)
        .collect();
    assert_eq!(
        starts,
        vec![
            source.find("first").unwrap(),
            source.find("second").unwrap(),
            source.find("{ x }").unwrap(),
            source.find("...rest").unwrap(),
        ]
    );
}

#[test]
fn returns_value_ignores_returns_of_nested_functions() {
    let info = parse(
        "function outer() { const inner = () => { return 1; }; function deep() { return 2; } return; }
         const arrow = () => { [1].map((x) => { return x; }); };",
    );
    assert!(!contract(&info, "outer").returns_value);
    assert!(!contract(&info, "arrow").returns_value);
}

#[test]
fn counts_condition_reads_of_each_parameter() {
    let info = parse(
        "export function format(value, compact, locale) {
           if (compact) {}
           if (!compact) {}
           const label = !!compact ? 'a' : 'b';
           while (locale) {}
           for (; value;) {}
           do {} while ((value));
           if (compact && locale) {}
           [1].forEach(() => { if (locale) {} });
           function shadow(compact) { if (compact) {} }
         }",
    );
    let reads: Vec<_> = contract(&info, "format")
        .params
        .iter()
        .map(|param| param.condition_reads)
        .collect();
    assert_eq!(reads, vec![2, 3, 2]);
}

#[test]
fn parameter_conditions_give_the_span_and_the_negations_of_each_counted_test() {
    let source = "export function format(value, compact) {
           if (!compact) {}
           const label = !!compact ? 'a' : 'b';
           while (value) {}
           if (compact && value) {}
           function shadow(compact) { if (compact) {} }
         }";
    let conditions = crate::parameter_conditions(Path::new("module.ts"), source);
    let found: Vec<_> = conditions
        .iter()
        .map(|condition| {
            let (start, end) = condition.span;
            (
                &*condition.function,
                condition.parameter_index,
                &source[start as usize..end as usize],
                condition.negations,
            )
        })
        .collect();
    assert_eq!(
        found,
        vec![
            ("format", 1, "!compact", 1),
            ("format", 1, "!!compact", 2),
            ("format", 0, "value", 0),
        ]
    );
}

#[test]
fn parameter_conditions_leave_out_the_tests_of_an_option_key() {
    let source = "export function load({ strict }, flag) { if (strict) {} if (flag) {} }";
    let conditions = crate::parameter_conditions(Path::new("module.ts"), source);
    let found: Vec<_> = conditions
        .iter()
        .map(|condition| (condition.parameter_index, condition.negations))
        .collect();
    assert_eq!(found, vec![(1, 0)]);
}

#[test]
fn parameter_conditions_of_a_source_with_a_syntax_error_are_empty() {
    let source = "export function f(flag) { if (flag) {} ";
    assert!(crate::parameter_conditions(Path::new("module.ts"), source).is_empty());
}

#[test]
fn each_incomplete_reason_opens_the_contract() {
    assert_eq!(
        incomplete(
            "function over(a: string): void; function over(a: number): void; function over(a: unknown) {}",
            "over"
        ),
        Some(ContractIncomplete::Overload)
    );
    assert_eq!(
        incomplete("function reads() { return arguments.length; }", "reads"),
        Some(ContractIncomplete::ArgumentsRead)
    );
    assert_eq!(
        incomplete("let mutable = () => {};", "mutable"),
        Some(ContractIncomplete::MutableBinding)
    );
    assert_eq!(
        incomplete("var old = function () {};", "old"),
        Some(ContractIncomplete::MutableBinding)
    );
    assert_eq!(
        incomplete("function swap() {}\nswap = () => {};", "swap"),
        Some(ContractIncomplete::Reassigned)
    );
    assert_eq!(
        incomplete("function decorated(@inject() a) {}", "decorated"),
        Some(ContractIncomplete::Decorator)
    );
    assert_eq!(
        incomplete("function* gen(a) { yield a; }", "gen"),
        Some(ContractIncomplete::Generator)
    );
    let duplicate = parse_at("module.js", "function twice(a) {}\nfunction twice(a, b) {}");
    assert_eq!(
        contract(&duplicate, "twice").incomplete,
        Some(ContractIncomplete::DuplicateDeclaration)
    );
}

#[test]
fn arguments_in_a_nested_arrow_opens_the_function_but_not_a_nested_function() {
    assert_eq!(
        incomplete(
            "function viaArrow() { const read = () => arguments[0]; }",
            "viaArrow"
        ),
        Some(ContractIncomplete::ArgumentsRead)
    );
    assert_eq!(
        incomplete(
            "function viaInner() { function inner() { return arguments; } }",
            "viaInner"
        ),
        None
    );
    assert_eq!(
        incomplete("const arrow = () => arguments.length;", "arrow"),
        None
    );
}

#[test]
fn arguments_in_a_class_key_or_heritage_opens_the_function() {
    for source in [
        "function key(a) { class C { [arguments[0]]() {} } }",
        "function key(a) { class C { static [arguments[0]] = 1; } }",
        "function key(a) { class C { accessor [arguments[0]] = 1; } }",
        "function key(a) { return class extends arguments[0] {}; }",
        "function key(a) { @tag(arguments[0]) class C {} }",
        "function key(a) { class C { @tag(arguments[0]) run() {} } }",
        "function key(a) { class C { run(@tag(arguments[0]) p) {} } }",
        "function key(a) { class C { constructor(@tag(arguments[0]) p) {} } }",
    ] {
        assert_eq!(
            incomplete(source, "key"),
            Some(ContractIncomplete::ArgumentsRead),
            "{source}"
        );
    }
    for source in [
        "function key(a) { class C { run() { return arguments; } } }",
        "function key(a) { class C { [`k`]() { return arguments; } } }",
    ] {
        assert_eq!(incomplete(source, "key"), None, "{source}");
    }
}

#[test]
fn a_class_key_does_not_set_returns_value_or_hide_a_return() {
    let info = parse(
        "function quiet(a) { class C { [(() => { return 1; })()]() {} } }\nfunction loud(a) { class C {} return a; }",
    );
    assert!(!contract(&info, "quiet").returns_value);
    assert!(contract(&info, "loud").returns_value);
}

#[test]
fn a_typescript_this_parameter_is_not_a_formal_parameter() {
    let source = "function bound(this: Window, flag: boolean) { if (flag) {} }\nbound.call(window, true);\nbound(false);";
    let info = parse(source);
    let bound = contract(&info, "bound");
    let names: Vec<_> = bound
        .params
        .iter()
        .map(|param| param.name.as_deref())
        .collect();
    assert_eq!(names, vec![Some("flag")]);
    assert_eq!(bound.params[0].condition_reads, 1);
    let calls = local_calls(&info, "bound");
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].args,
        vec![ArgFact::Literal(LiteralValue::Boolean(false))]
    );
}

#[test]
fn records_local_calls_with_literal_argument_facts() {
    let info = parse(
        "function format(value, compact, locale) { return value; }
         format(1, true);
         format(-0, false, 'nl');
         format(1.0, null, `en`);
         format(2, undefined);
         const out = format(n, { a: 1 }, `x${y}`);",
    );
    let calls = local_calls(&info, "format");
    let args: Vec<_> = calls.iter().map(|call| call.args.clone()).collect();
    let number = |value: f64| ArgFact::Literal(LiteralValue::Number(value.to_bits()));
    assert_eq!(
        args,
        vec![
            vec![number(1.0), ArgFact::Literal(LiteralValue::Boolean(true))],
            vec![
                number(-0.0),
                ArgFact::Literal(LiteralValue::Boolean(false)),
                ArgFact::Literal(LiteralValue::String("nl".into())),
            ],
            vec![
                number(1.0),
                ArgFact::Literal(LiteralValue::Null),
                ArgFact::Literal(LiteralValue::String("en".into())),
            ],
            vec![number(2.0), ArgFact::Literal(LiteralValue::Undefined)],
            vec![
                ArgFact::Unknown,
                ArgFact::Object(vec![key(
                    "a",
                    KeyValue::Literal(LiteralValue::Number(1f64.to_bits()))
                )]),
                ArgFact::Unknown,
            ],
        ]
    );
    let counts: Vec<_> = calls.iter().map(|call| call.arg_count).collect();
    assert_eq!(counts, vec![2, 3, 3, 2, 3]);
    let used: Vec<_> = calls.iter().map(|call| call.result_used).collect();
    assert_eq!(used, vec![false, false, false, false, true]);
}

#[test]
fn negative_zero_and_zero_are_different_values() {
    let info = parse("function scale(n) {}\nscale(0);\nscale(-0);\nscale(-(0));\nscale(0.0);");
    let args: Vec<_> = local_calls(&info, "scale")
        .iter()
        .map(|call| call.args.clone())
        .collect();
    let zero = ArgFact::Literal(LiteralValue::Number(0.0f64.to_bits()));
    let negative = ArgFact::Literal(LiteralValue::Number((-0.0f64).to_bits()));
    assert_ne!(zero, negative);
    assert_eq!(
        args,
        vec![
            vec![zero.clone()],
            vec![negative.clone()],
            vec![negative],
            vec![zero]
        ]
    );
}

#[test]
fn a_vue_script_src_file_is_a_plain_module_that_the_host_imports() {
    let host = parse_at(
        "view.vue",
        "<template><p>{{ label(1) }}</p></template>\n<script src=\"./view-logic.ts\" lang=\"ts\"></script>",
    );
    assert!(host.imports.iter().any(|import| {
        import.source == "./view-logic.ts"
            && matches!(
                import.imported_name,
                fallow_types::extract::ImportedName::SideEffect
            )
    }));
    let logic = parse_at(
        "view-logic.ts",
        "function label(n) { return n; }\nlabel(2);\nexport default { methods: { label } };",
    );
    let facts = facts(&logic);
    assert!(!facts.template_callers);
    assert_eq!(facts.escapes.len(), 1);
}

#[test]
fn a_classic_script_records_its_global_functions() {
    let info = parse_at("legacy.js", "function greet(name) {}\ngreet('a');");
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
    let facts = facts(&info);
    assert!(!facts.template_callers);
    assert_eq!(facts.declarations.len(), 1);
    assert_eq!(local_calls(&info, "greet").len(), 1);
}

#[test]
fn shadowed_undefined_is_unknown() {
    let info = parse("function f(a) {}\nfunction g(undefined) { f(undefined); }\nf(undefined);");
    let args: Vec<_> = local_calls(&info, "f")
        .iter()
        .map(|call| call.args.clone())
        .collect();
    assert_eq!(
        args,
        vec![vec![], vec![ArgFact::Literal(LiteralValue::Undefined)]]
    );
}

#[test]
fn records_spread_and_result_use_of_a_call() {
    let source = "async function save(a, b) { return a; }
         save(1, ...rest);
         await save(1);
         void save(2);
         (save(3));
         const x = await save(4);
         export const y = () => save(5);";
    let info = parse(source);
    let calls = local_calls(&info, "save");
    let shape: Vec<_> = calls
        .iter()
        .map(|call| (call.spread_from, call.arg_count, call.result_used))
        .collect();
    assert_eq!(
        shape,
        vec![
            (Some(1), 2, false),
            (None, 1, false),
            (None, 1, false),
            (None, 1, false),
            (None, 1, true),
            (None, 1, true),
        ]
    );
    assert_eq!(
        calls[0].span_start as usize,
        source.find("save(1, ...rest)").unwrap()
    );
}

#[test]
fn caps_argument_facts_and_keeps_the_count() {
    let args = vec!["1"; 40].join(", ");
    let info = parse(&format!("function many() {{}}\nmany({args});"));
    let calls = local_calls(&info, "many");
    assert_eq!(calls[0].arg_count, 40);
    assert_eq!(calls[0].args.len(), super::MAX_ARG_FACTS);
}

#[test]
fn records_calls_of_admitted_import_bindings_only() {
    let info = parse(
        "import { format } from './format';
         import * as ns from './ns';
         import type { Typed } from './typed';
         import pkg from 'pkg';
         format('a', 1);
         ns.inner.call(true);
         format?.('optional');
         ns['computed']();
         pkg(1);
         console.log(1);
         unknownGlobal(1);",
    );
    let imports: Vec<_> = info
        .imports
        .iter()
        .map(|import| import.local_name.as_str())
        .collect();
    let calls: Vec<_> = facts(&info)
        .call_sites()
        .map(|(target, member_path, site)| {
            let CallTarget::Import(index) = target else {
                panic!("only import calls exist");
            };
            (imports[index as usize], member_path, site.args.clone())
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            (
                "format",
                "",
                vec![
                    ArgFact::Literal(LiteralValue::String("a".into())),
                    ArgFact::Literal(LiteralValue::Number(1f64.to_bits())),
                ]
            ),
            (
                "ns",
                "inner.call",
                vec![ArgFact::Literal(LiteralValue::Boolean(true))]
            ),
            (
                "pkg",
                "",
                vec![ArgFact::Literal(LiteralValue::Number(1f64.to_bits()))]
            ),
        ]
    );
}

#[test]
fn shadowed_import_and_local_names_are_not_calls() {
    let info = parse(
        "import { format } from './format';
         function local(a) {}
         function inner(format, local) { format(1); local(2); }",
    );
    let calls = &facts(&info).calls;
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn records_escapes_of_local_function_bindings() {
    let source = "export function format(v) {}
         items.map(format);
         const alias = format;
         format.bind(null);
         const object = { format };
         const view = <Format />;
         function Format() {}
         format`tagged`;
         new format();
         format?.(1);
         export { format as renamed };
         export default format;
         type T = typeof format;
         format(1);";
    let info = parse_at("module.tsx", source);
    let facts = facts(&info);
    let format = facts
        .declarations
        .iter()
        .position(|contract| &*contract.binding_name == "format")
        .unwrap() as u32;
    let escapes: Vec<_> = facts
        .escapes
        .iter()
        .filter(|escape| escape.contract == format)
        .map(|escape| escape.span_start as usize)
        .collect();
    let expected: Vec<_> = [
        "format);",
        "format;\n",
        "format.bind",
        "format };",
        "format`tagged`",
        "format();",
        "format?.(1)",
    ]
    .iter()
    .map(|needle| source.find(needle).unwrap())
    .collect();
    assert_eq!(escapes, expected);

    let component = facts
        .declarations
        .iter()
        .position(|contract| &*contract.binding_name == "Format")
        .unwrap() as u32;
    let jsx: Vec<_> = facts
        .escapes
        .iter()
        .filter(|escape| escape.contract == component)
        .map(|escape| escape.span_start as usize)
        .collect();
    assert_eq!(jsx, vec![source.find("Format />").unwrap()]);
    assert_eq!(local_calls(&info, "format").len(), 1);
}

#[test]
fn import_binding_references_record_jsx_tagged_template_and_namespace_reads() {
    let source = "import { Format } from './format';
         import { tag } from './tag';
         import * as ns from './ns';
         import { pass } from './pass';
         const view = <Format />;
         tag`text`;
         pass(ns);";
    let info = parse_at("module.tsx", source);
    let kinds: Vec<_> = info
        .import_binding_references
        .iter()
        .map(|reference| {
            (
                info.imports[reference.import_index as usize]
                    .local_name
                    .as_str(),
                reference.kind,
                reference.span_start as usize,
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            (
                "Format",
                ImportBindingReferenceKind::JsxElement,
                source.find("Format />").unwrap()
            ),
            (
                "tag",
                ImportBindingReferenceKind::Other,
                source.find("tag`text`").unwrap()
            ),
            (
                "ns",
                ImportBindingReferenceKind::Other,
                source.find("ns);").unwrap()
            ),
        ]
    );
}

#[test]
fn a_module_without_functions_or_admitted_calls_stores_nothing() {
    let info = parse("import x from 'pkg';\nconsole.log(1);\nconst value = 1;");
    assert!(info.function_contracts.is_none());
}

#[test]
fn embedded_scripts_remap_offsets_and_mark_template_callers() {
    let body =
        "\nimport { format } from './format';\nfunction local(a) {}\nlocal(1);\nformat(2);\n";
    let cases = [
        (
            "view.vue",
            format!(
                "<template><p>{{{{ local(3) }}}}</p></template>\n<script setup lang=\"ts\">{body}</script>"
            ),
        ),
        (
            "view.svelte",
            format!("<p>{{local(3)}}</p>\n<script lang=\"ts\">{body}</script>"),
        ),
        ("view.astro", format!("---{body}---\n<p>{{local(3)}}</p>")),
        (
            "view.gts",
            format!("<template><p>{{{{local 3}}}}</p></template>\n{body}"),
        ),
    ];
    for (path, source) in cases {
        let info = parse_at(path, &source);
        let facts = facts(&info);
        assert!(facts.template_callers, "{path}");
        let local = &facts.declarations[0];
        assert_eq!(
            local.span_start as usize,
            source.find("local(a)").unwrap(),
            "{path}"
        );
        assert_eq!(
            local.params[0].span_start as usize,
            source.find("local(a)").unwrap() + "local(".len(),
            "{path}"
        );
        let calls: Vec<_> = facts
            .call_sites()
            .map(|(target, _, site)| {
                let target = match target {
                    CallTarget::Local(index) => &*facts.declarations[index as usize].binding_name,
                    CallTarget::Import(index) => info.imports[index as usize].local_name.as_str(),
                };
                (target, site.span_start as usize)
            })
            .collect();
        assert_eq!(
            calls,
            vec![
                ("local", source.find("local(1)").unwrap()),
                ("format", source.find("format(2)").unwrap()),
            ],
            "{path}"
        );
    }
}

#[test]
fn a_plain_script_has_no_template_callers() {
    let info = parse("function local() {}\nlocal();");
    assert!(!facts(&info).template_callers);
}

#[test]
fn two_vue_script_blocks_keep_their_own_targets() {
    let source = "<script lang=\"ts\">\nimport { a } from './a';\nexport function first() {}\na(1);\n</script>\n<script setup lang=\"ts\">\nimport { b } from './b';\nfunction second() {}\nb(2);\nsecond();\n</script>\n<template><p /></template>";
    let info = parse_at("view.vue", source);
    let facts = facts(&info);
    let targets: Vec<_> = facts
        .call_sites()
        .map(|(target, _, _)| match target {
            CallTarget::Local(index) => facts.declarations[index as usize].binding_name.to_string(),
            CallTarget::Import(index) => info.imports[index as usize].local_name.clone(),
        })
        .collect();
    assert_eq!(targets, vec!["a", "b", "second"]);
}

#[test]
fn mdx_marks_template_callers() {
    let source = "import { format } from './format';\n\n# Title\n\n{format(1)}\n";
    let info = parse_at("page.mdx", source);
    assert!(facts(&info).template_callers);
}

#[test]
fn the_name_of_a_named_function_expression_is_the_same_contract() {
    let source = "const walk = function visit(node, deep) { visit(node, true); later(visit); };\nwalk(root, false);";
    let info = parse(source);
    let facts = facts(&info);
    assert_eq!(facts.declarations.len(), 1);
    let calls: Vec<_> = local_calls(&info, "walk")
        .iter()
        .map(|call| call.span_start as usize)
        .collect();
    assert_eq!(
        calls,
        vec![
            source.find("visit(node, true)").unwrap(),
            source.find("walk(root").unwrap(),
        ]
    );
    let escapes: Vec<_> = facts
        .escapes
        .iter()
        .map(|escape| escape.span_start as usize)
        .collect();
    assert_eq!(escapes, vec![source.find("visit);").unwrap()]);
}

#[test]
fn a_default_value_counts_for_arguments_and_condition_reads() {
    let info = parse(
        "function count(a, b = arguments.length) {}\nfunction pick(flag, label = flag ? 'a' : 'b') {}",
    );
    assert_eq!(
        contract(&info, "count").incomplete,
        Some(ContractIncomplete::ArgumentsRead)
    );
    assert_eq!(contract(&info, "pick").params[0].condition_reads, 1);
}

#[test]
fn a_direct_eval_opens_the_contract() {
    assert_eq!(
        incomplete("function run(a) { return eval('arguments')[0]; }", "run"),
        Some(ContractIncomplete::DirectEval)
    );
    assert_eq!(
        incomplete(
            "const run = (a) => { [1].map(() => eval('a = 2')); };",
            "run"
        ),
        Some(ContractIncomplete::DirectEval)
    );
    assert_eq!(incomplete("function run(eval) { eval('x'); }", "run"), None);
}

#[test]
fn a_direct_eval_anywhere_in_the_module_marks_the_module() {
    for source in [
        "function f(a) {}\nf(1);\nfunction g() { eval('f(2)'); }",
        "function f(a) {}\nf(1);\neval('f(2)');",
        "function f(a) {}\nf(1);\n(eval)('f(2)');",
        "import { h } from './h';\nh(1);\nconst run = () => [0].map(() => eval('h(2)'));",
    ] {
        let info = parse(source);
        assert!(facts(&info).direct_eval, "{source}");
    }
    for source in [
        "function f(a) {}\nf(1);",
        "function f(a) {}\nf(1);\nfunction g(eval) { eval('f(2)'); }",
        "function f(a) {}\nf(1);\nconst text = 'eval';\nobj.eval('f(2)');",
    ] {
        let info = parse(source);
        assert!(!facts(&info).direct_eval, "{source}");
    }
}

#[test]
fn a_module_with_only_a_direct_eval_stores_the_flag() {
    let info = parse("eval(code);");
    assert!(facts(&info).direct_eval);
}

#[test]
fn argument_facts_stop_at_the_first_spread() {
    let info = parse("function f(a, b, c) {}\nf(true, ...rest, 1);");
    let calls = local_calls(&info, "f");
    assert_eq!(
        calls[0].args,
        vec![ArgFact::Literal(LiteralValue::Boolean(true))]
    );
    assert_eq!(calls[0].spread_from, Some(1));
    assert_eq!(calls[0].arg_count, 3);
}

#[test]
fn sites_of_one_callee_share_one_group_in_source_order() {
    let source = "import { format } from './format';\nfunction local() {}\nformat(1);\nlocal();\nformat(2);\nformat.inner(3);";
    let info = parse(source);
    let groups: Vec<_> = facts(&info)
        .calls
        .iter()
        .map(|calls| {
            let starts: Vec<_> = calls
                .sites
                .iter()
                .map(|site| site.span_start as usize)
                .collect();
            (calls.target, &*calls.member_path, starts)
        })
        .collect();
    assert_eq!(
        groups,
        vec![
            (
                CallTarget::Import(0),
                "",
                vec![
                    source.find("format(1)").unwrap(),
                    source.find("format(2)").unwrap()
                ]
            ),
            (
                CallTarget::Local(0),
                "",
                vec![source.find("local();").unwrap()]
            ),
            (
                CallTarget::Import(0),
                "inner",
                vec![source.find("format.inner").unwrap()]
            ),
        ]
    );
}

#[test]
fn a_long_string_literal_is_unknown() {
    let long = "x".repeat(super::MAX_LITERAL_STRING_BYTES + 1);
    let edge = "y".repeat(super::MAX_LITERAL_STRING_BYTES);
    let info = parse(&format!("function f(a, b) {{}}\nf('{long}', `{edge}`);"));
    assert_eq!(
        local_calls(&info, "f")[0].args,
        vec![
            ArgFact::Unknown,
            ArgFact::Literal(LiteralValue::String(edge.into()))
        ]
    );
}

fn written(info: &ModuleInfo, name: &str) -> Vec<bool> {
    contract(info, name)
        .params
        .iter()
        .map(|param| param.written)
        .collect()
}

#[test]
fn records_each_write_of_a_parameter() {
    let info = parse(
        "function toggle(compact) { compact = !compact; }
         function nested(compact) { const set = () => { compact = 1; }; }
         function nullish(compact) { compact ??= true; }
         function pattern(compact, xs) { [compact] = xs; }
         function update(compact) { compact++; }
         function inDefault(compact, other = (compact = 2)) {}
         function object({ x, y }) { y = 1; }
         function array([x]) { x += 1; }
         function rest(...items) { items = []; }
         function closed(compact, { x }, [y], ...rest) { if (compact) {} return x + y + rest.length; }",
    );
    assert_eq!(written(&info, "toggle"), vec![true]);
    assert_eq!(written(&info, "nested"), vec![true]);
    assert_eq!(written(&info, "nullish"), vec![true]);
    assert_eq!(written(&info, "pattern"), vec![true, false]);
    assert_eq!(written(&info, "update"), vec![true]);
    assert_eq!(written(&info, "inDefault"), vec![true, false]);
    assert_eq!(written(&info, "object"), vec![true]);
    assert_eq!(written(&info, "array"), vec![true]);
    assert_eq!(written(&info, "rest"), vec![true]);
    assert_eq!(written(&info, "closed"), vec![false, false, false, false]);
}

#[test]
fn records_whether_the_function_is_async() {
    let info = parse(
        "async function declared() {}
         const expression = async function () {};
         const arrow = async () => {};
         function plain() {}
         const plainArrow = () => {};",
    );
    let flags: Vec<_> = facts(&info)
        .declarations
        .iter()
        .map(|contract| (&*contract.binding_name, contract.is_async))
        .collect();
    assert_eq!(
        flags,
        vec![
            ("declared", true),
            ("expression", true),
            ("arrow", true),
            ("plain", false),
            ("plainArrow", false),
        ]
    );
}

#[test]
fn a_self_call_records_a_parameter_that_it_passes_through() {
    let info = parse(
        "function walk(node, depth) {
           walk(node as any, 1);
           node.kids.forEach((kid) => walk(kid, depth!));
           walk(depth, node);
         }
         walk(node, 1);",
    );
    let args: Vec<_> = local_calls(&info, "walk")
        .iter()
        .map(|call| call.args.clone())
        .collect();
    let one = ArgFact::Literal(LiteralValue::Number(1.0_f64.to_bits()));
    assert_eq!(
        args,
        vec![
            vec![ArgFact::Param(0), one.clone()],
            vec![ArgFact::Unknown, ArgFact::Param(1)],
            vec![ArgFact::Param(1), ArgFact::Param(0)],
            vec![ArgFact::Unknown, one],
        ]
    );
}

#[test]
fn a_self_call_of_a_named_function_expression_records_pass_through() {
    let info = parse("const outer = function inner(a) { inner(a); };");
    assert_eq!(local_calls(&info, "outer")[0].args, vec![ArgFact::Param(0)]);
}

#[test]
fn a_written_pattern_or_shadowed_parameter_is_not_pass_through() {
    let info = parse(
        "function count(n, step) { step = step + 1; count(n, step); }
         function pick({ a }, b) { pick({ a }, b); }
         function shadow(a) { function inner(a) { shadow(a); } }",
    );
    let args = |name: &str| -> Vec<Vec<ArgFact>> {
        local_calls(&info, name)
            .iter()
            .map(|call| call.args.clone())
            .collect()
    };
    assert_eq!(
        args("count"),
        vec![vec![ArgFact::Param(0), ArgFact::Unknown]]
    );
    assert_eq!(
        args("pick"),
        vec![vec![
            ArgFact::Object(vec![key("a", KeyValue::PassThrough)]),
            ArgFact::Param(1)
        ]]
    );
    assert_eq!(args("shadow"), vec![vec![]]);
}

#[test]
fn a_call_of_another_function_keeps_a_parameter_unknown() {
    let info = parse(
        "function walk(node, depth) {}
         function other(x) { walk(x, 1); }
         function relay(a, b) { walk(a, b); }",
    );
    let args: Vec<_> = local_calls(&info, "walk")
        .iter()
        .map(|call| call.args.clone())
        .collect();
    assert_eq!(
        args,
        vec![
            vec![
                ArgFact::Unknown,
                ArgFact::Literal(LiteralValue::Number(1.0_f64.to_bits()))
            ],
            vec![],
        ]
    );
}

#[test]
fn a_type_assertion_argument_is_transparent() {
    let info = parse("function f(a) {}\nf(<any>true);");
    assert_eq!(
        local_calls(&info, "f")[0].args,
        vec![ArgFact::Literal(LiteralValue::Boolean(true))]
    );
}

#[test]
fn a_rest_parameter_is_not_pass_through() {
    let info = parse("function f(...r) { f(r); }\nf(1);");
    assert_eq!(local_calls(&info, "f")[0].args, vec![]);
}

#[test]
fn a_redeclaration_in_the_body_writes_the_parameter() {
    let info = parse(
        "function varInit(a) { var a = 5; varInit(a); }
         function varBare(a) { var a; varBare(a); }
         function forVar(a, xs) { for (var a of xs) {} forVar(a, xs); }
         function fnDecl(a) { function a() {} fnDecl(a); }
         function shadowLet(a) { { let a = 1; } shadowLet(a); }",
    );
    assert_eq!(written(&info, "varInit"), vec![true]);
    assert_eq!(written(&info, "varBare"), vec![true]);
    assert_eq!(written(&info, "forVar"), vec![true, false]);
    assert_eq!(written(&info, "fnDecl"), vec![true]);
    assert_eq!(written(&info, "shadowLet"), vec![false]);
    assert_eq!(local_calls(&info, "varInit")[0].args, vec![]);
    assert_eq!(
        local_calls(&info, "forVar")[0].args,
        vec![ArgFact::Unknown, ArgFact::Param(1)]
    );
    assert_eq!(local_calls(&info, "fnDecl")[0].args, vec![]);
    assert_eq!(
        local_calls(&info, "shadowLet")[0].args,
        vec![ArgFact::Param(0)]
    );
}

#[test]
fn a_write_through_arguments_opens_the_contract() {
    let info = parse_at(
        "script.cjs",
        "function sloppy(a) { arguments[0] = false; if (a) {} sloppy(a); }\nsloppy(true);",
    );
    assert_eq!(
        contract(&info, "sloppy").incomplete,
        Some(ContractIncomplete::ArgumentsRead)
    );
}

fn import_call_args(info: &ModuleInfo) -> Vec<Vec<ArgFact>> {
    facts(info)
        .call_sites()
        .filter(|(target, _, _)| matches!(target, CallTarget::Import(_)))
        .map(|(_, _, site)| site.args.clone())
        .collect()
}

fn local_binding(path: &str) -> ArgFact {
    ArgFact::Binding(BindingArg {
        root: BindingRoot::Local,
        path: path.into(),
    })
}

fn import_binding(index: u32, path: &str) -> ArgFact {
    ArgFact::Binding(BindingArg {
        root: BindingRoot::Import(index),
        path: path.into(),
    })
}

fn key(key: &str, value: KeyValue) -> KeyArg {
    KeyArg {
        key: key.into(),
        value,
    }
}

#[test]
fn an_argument_that_names_an_immutable_binding_has_an_identity() {
    let info = parse(
        "import { LIMIT, Mode } from './constants';
         import * as constants from './constants';
         import fallback from './fallback';
         const LOCAL = 'a';
         const TYPED = 3 as const;
         enum Kind { Grid, 'List' }
         f(LOCAL, TYPED, Kind.Grid, Kind.List);
         f(LIMIT, Mode.Grid, constants.LIMIT, constants.Mode.Grid, fallback);
         function f(...args) {}",
    );
    let calls = local_calls(&info, "f");
    assert_eq!(
        calls[0].args,
        vec![
            local_binding("LOCAL"),
            local_binding("TYPED"),
            local_binding("Kind.Grid"),
            local_binding("Kind.List"),
        ]
    );
    assert_eq!(
        calls[1].args,
        vec![
            import_binding(0, ""),
            import_binding(1, "Grid"),
            import_binding(2, "LIMIT"),
            import_binding(2, "Mode.Grid"),
            import_binding(3, ""),
        ]
    );
}

#[test]
fn a_mutable_folded_or_shadowed_binding_is_unknown() {
    let info = parse(
        "import { Mode } from './constants';
         import * as constants from './constants';
         let MUTABLE = 1;
         const FOLDED = 1 + 1;
         const OBJECT = { a: 1 };
         const LIST = [1];
         const LOCAL = 'a';
         enum Kind { Grid }
         namespace Space { export const A = 1; }
         f(MUTABLE, FOLDED, OBJECT, LIST, Kind.Missing, Space.A, Mode.Grid.value, constants, Kind?.Grid);
         function g(LOCAL) { f(LOCAL); }
         function f(...args) {}",
    );
    let calls = local_calls(&info, "f");
    assert_eq!(calls[0].args, vec![]);
    assert_eq!(calls[0].arg_count, 9);
    assert_eq!(calls[1].args, vec![]);
}

#[test]
fn only_exported_literal_constants_are_listed() {
    let info = parse(
        "export const A = 1;
         const B = 'b';
         const C = true;
         const D = 1, E = compute();
         export { B, C as Renamed };
         export type { D };
         export default D;
         export let F = 1;",
    );
    let names: Vec<_> = facts(&info)
        .literal_consts
        .iter()
        .map(|name| &**name)
        .collect();
    assert_eq!(names, vec!["A", "B", "C", "D"]);
}

#[test]
fn an_object_pattern_records_its_static_keys() {
    let info = parse(
        "export function request(url, { retries = 3, signal: abort, 'quoted': q, [dynamic]: d, 1: n, toString, nested: { deep }, flag }: Options = {}) {
           if (flag) {}
           if (!abort) {}
         }
         export function written({ a, b }) { a = 2; }
         export function defaulted({ a } = { a: 1 }) {}
         export function rest(...{ a }) {}",
    );
    let request = contract(&info, "request");
    let keys: Vec<_> = request.params[1]
        .keys
        .iter()
        .map(|key| {
            (
                &*key.key,
                &*key.binding,
                key.has_default,
                key.condition_reads,
                key.written,
            )
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            ("retries", "retries", true, 0, false),
            ("signal", "abort", false, 1, false),
            ("quoted", "q", false, 0, false),
            ("flag", "flag", false, 1, false),
        ]
    );
    assert!(request.params[0].keys.is_empty());
    let written: Vec<_> = contract(&info, "written").params[0]
        .keys
        .iter()
        .map(|key| (&*key.key, key.written))
        .collect();
    assert_eq!(written, vec![("a", true), ("b", false)]);
    assert!(contract(&info, "defaulted").params[0].keys.is_empty());
    assert!(contract(&info, "rest").params[0].keys.is_empty());
}

#[test]
fn an_object_literal_argument_records_its_keys() {
    let info = parse(
        "import { request } from './request';
         import { LIMIT } from './constants';
         const value = compute();
         request({ retries: 1, signal: value, limit: LIMIT, 'quoted': null, run() {}, get lazy() { return 1; }, retries: 2 });
         request({ ...value });
         request({ [value]: 1 });
         request({ 1: 'a' });
         request({ __proto__: value });
         request({});",
    );
    assert_eq!(
        import_call_args(&info),
        vec![
            vec![ArgFact::Object(vec![
                key(
                    "retries",
                    KeyValue::Literal(LiteralValue::Number(2f64.to_bits()))
                ),
                key("signal", KeyValue::Unknown),
                key(
                    "limit",
                    KeyValue::Binding(BindingArg {
                        root: BindingRoot::Import(1),
                        path: "".into(),
                    })
                ),
                key("quoted", KeyValue::Literal(LiteralValue::Null)),
                key("run", KeyValue::Unknown),
                key("lazy", KeyValue::Unknown),
            ])],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![ArgFact::Object(vec![])],
        ]
    );
}

#[test]
fn a_self_call_that_passes_an_option_key_on_marks_it() {
    let info = parse(
        "export function walk(node, { depth, seen = new Set(), moved }) {
           walk(node.child, { depth, seen: seen, moved: depth, extra: depth });
           moved = 1;
           walk(node.child, { moved });
         }
         export function other({ depth }) { walk(null, { depth }); }",
    );
    let calls = local_calls(&info, "walk");
    assert_eq!(
        calls[0].args,
        vec![
            ArgFact::Unknown,
            ArgFact::Object(vec![
                key("depth", KeyValue::PassThrough),
                key("seen", KeyValue::PassThrough),
                key("moved", KeyValue::Unknown),
                key("extra", KeyValue::Unknown),
            ]),
        ]
    );
    assert_eq!(
        calls[1].args,
        vec![
            ArgFact::Unknown,
            ArgFact::Object(vec![key("moved", KeyValue::Unknown)]),
        ]
    );
    assert_eq!(
        calls[2].args,
        vec![
            ArgFact::Literal(LiteralValue::Null),
            ArgFact::Object(vec![key("depth", KeyValue::Unknown)]),
        ]
    );
}

#[test]
fn a_local_binding_of_a_script_block_is_unknown_after_the_merge() {
    // Two script blocks of one host can each declare a `const` with the same
    // name and another value.
    let source = "<script lang=\"ts\">\nimport { f } from './f';\nimport { LIMIT } from './limits';\nconst SIZE = 1;\nf(SIZE, LIMIT);\n</script>\n<script setup lang=\"ts\">\nimport { f } from './f';\nconst SIZE = 2;\nf(SIZE);\n</script>\n<template><p /></template>";
    let info = parse_at("view.vue", source);
    let limits = info
        .imports
        .iter()
        .position(|import| import.local_name == "LIMIT")
        .and_then(|index| u32::try_from(index).ok())
        .expect("LIMIT import");
    let args = import_call_args(&info);
    assert_eq!(
        args,
        vec![
            vec![ArgFact::Unknown, import_binding(limits, "")],
            vec![ArgFact::Unknown],
        ]
    );
}
