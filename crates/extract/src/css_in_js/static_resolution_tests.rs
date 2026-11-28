//! Work bounds for the static value resolution of the StyleX consumer scan.

use std::fmt::Write as _;
use std::path::Path;

use rustc_hash::FxHashSet;

use super::{
    MAX_STATIC_RESOLUTION_DEPTH, RESOLUTION_STEPS, StaticVisit, is_definitely_static_primitive,
};
use crate::css_in_js::{ConsumerQuery, css_in_js_consumer_scan};

/// Constants where each one reads the previous constant twice.
fn doubling_chain(length: usize, first: &str, call: &str) -> String {
    let mut source = String::from(
        "import * as stylex from '@stylexjs/stylex';\n\
         export const vars = stylex.defineVars({ color: 'red' });\n",
    );
    writeln!(source, "const a0 = {first};").unwrap();
    for index in 1..=length {
        let previous = index - 1;
        writeln!(source, "const a{index} = a{previous} + a{previous};").unwrap();
    }
    writeln!(source, "export const label = {call}(a{length});").unwrap();
    source
}

fn resolution_steps(source: &str) -> usize {
    let leaf_paths: FxHashSet<String> = std::iter::once("color".to_string()).collect();
    let query = ConsumerQuery::MemberBinding {
        alias: "vars",
        leaf_paths: &leaf_paths,
    };
    RESOLUTION_STEPS.with(|steps| steps.set(0));
    let _ = css_in_js_consumer_scan(source, Path::new("tokens.ts"), &[query]);
    RESOLUTION_STEPS.with(std::cell::Cell::get)
}

/// Run `scan` on a thread with a known large stack, so that the scan parses
/// on this thread and the thread-local step count sees its work.
fn on_scan_thread(scan: impl FnOnce() -> usize + Send + 'static) -> usize {
    const SCAN_STACK: usize = 256 << 20;
    std::thread::Builder::new()
        .stack_size(SCAN_STACK)
        .spawn(move || {
            crate::stack::register_thread_stack(SCAN_STACK);
            scan()
        })
        .expect("spawn scan thread")
        .join()
        .expect("scan thread")
}

// Each constant reads the previous one twice, so a resolution without a memo
// or a budget visits 2 to the power of the chain length constants. The lower
// bound shows that the count saw the scan: each constant expands at least once.

#[test]
fn primitive_check_resolves_a_doubling_chain_in_bounded_steps() {
    let steps = on_scan_thread(|| resolution_steps(&doubling_chain(16, "1", "String")));
    assert!((16..=4_096).contains(&steps), "resolution steps: {steps}");
}

#[test]
fn bigint_check_resolves_a_doubling_chain_in_bounded_steps() {
    let steps = on_scan_thread(|| resolution_steps(&doubling_chain(16, "1", "Math.abs")));
    assert!((16..=4_096).contains(&steps), "resolution steps: {steps}");
}

#[test]
fn a_long_doubling_chain_finishes_inside_the_step_budget() {
    for call in ["String", "Math.abs"] {
        let steps = on_scan_thread(move || resolution_steps(&doubling_chain(40, "1", call)));
        assert!(
            (40..=4_096).contains(&steps),
            "resolution steps for {call}: {steps}"
        );
    }
}

/// Check the arguments of the last statement, a call, in order with one
/// resolution state, as the scan checks the arguments of one call.
fn primitive_arguments(source: &str) -> Vec<bool> {
    use oxc_allocator::Allocator;
    use oxc_ast::ast::{BindingPattern, Expression, IdentifierReference, Statement};
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_span::{SourceType, Span};
    use rustc_hash::FxHashMap;

    #[derive(Default)]
    struct References(FxHashSet<Span>);
    impl Visit<'_> for References {
        fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'_>) {
            self.0.insert(identifier.span);
        }
    }

    let allocator = Allocator::default();
    let program = Parser::new(&allocator, source, SourceType::mjs())
        .parse()
        .program;
    let mut static_values: FxHashMap<&str, (u32, &Expression<'_>)> = FxHashMap::default();
    for statement in &program.body {
        if let Statement::VariableDeclaration(declaration) = statement {
            for declarator in &declaration.declarations {
                if let (BindingPattern::BindingIdentifier(binding), Some(init)) =
                    (&declarator.id, &declarator.init)
                {
                    static_values.insert(binding.name.as_str(), (declarator.span.start, init));
                }
            }
        }
    }
    let mut references = References::default();
    references.visit_program(&program);
    let Some(Statement::ExpressionStatement(statement)) = program.body.last() else {
        panic!("the source must end with a call");
    };
    let Expression::CallExpression(call) = &statement.expression else {
        panic!("the source must end with a call");
    };
    let mut visiting = StaticVisit::default();
    call.arguments
        .iter()
        .map(|argument| {
            is_definitely_static_primitive(
                argument.to_expression(),
                statement.span.start,
                &static_values,
                &references.0,
                &mut visiting,
            )
        })
        .collect()
}

#[test]
fn a_result_cut_by_the_depth_limit_is_not_memoized() {
    let length = MAX_STATIC_RESOLUTION_DEPTH + 6;
    let mut source = String::from("const k0 = 1;\n");
    for index in 1..=length {
        let previous = index - 1;
        writeln!(source, "const k{index} = k{previous} + 1;").unwrap();
    }
    writeln!(source, "String(k{length}, k10);").unwrap();

    // The long chain passes the depth limit and is not known to be static.
    // `k10` is on that chain, but on its own it is well inside the limit.
    assert_eq!(primitive_arguments(&source), [false, true]);
}
