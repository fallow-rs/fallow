//! Shared helpers for the mutation fixture tests.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the helpers"
)]

use fallow_mutation::{Instrumented, Mutant, MutationOptions, Operator, SourceKind, instrument};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;

/// Parses `code` and runs the semantic syntax checks. Panics with the
/// diagnostics and the code when either step reports an error.
pub fn assert_valid(code: &str, kind: SourceKind) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, kind.source_type()).parse();
    assert!(
        !parsed.fatal_error && !parsed.diagnostics.has_errors(),
        "code does not parse: {:?}\n---\n{code}",
        parsed.diagnostics,
    );
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    assert!(
        !semantic.diagnostics.has_errors(),
        "code has syntax errors: {:?}\n---\n{code}",
        semantic.diagnostics,
    );
}

/// Applies one mutant to the original source.
pub fn apply(source: &str, mutant: &Mutant) -> String {
    let (start, end) = (mutant.span.0 as usize, mutant.span.1 as usize);
    format!(
        "{}{}{}",
        &source[..start],
        mutant.replacement,
        &source[end..]
    )
}

/// Instruments `source` with `options`, and checks that the output and each
/// mutated program parse.
pub fn run_with(source: &str, kind: SourceKind, options: &MutationOptions) -> Instrumented {
    let result = match instrument(source, "src/fixture.ts", kind, options) {
        Ok(result) => result,
        Err(error) => panic!("instrument failed: {error}"),
    };
    assert_valid(&result.code, kind);
    for mutant in &result.mutants {
        assert_eq!(
            &source[mutant.span.0 as usize..mutant.span.1 as usize],
            mutant.original
        );
        assert_valid(&apply(source, mutant), kind);
    }
    result
}

/// Instruments `source` with the default options.
pub fn run(source: &str, kind: SourceKind) -> Instrumented {
    run_with(source, kind, &MutationOptions::default())
}

/// The sorted `(original, replacement)` pairs of one operator.
pub fn pairs(result: &Instrumented, operator: Operator) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = result
        .mutants
        .iter()
        .filter(|mutant| mutant.operator == operator)
        .map(|mutant| (mutant.original.clone(), mutant.replacement.clone()))
        .collect();
    pairs.sort();
    pairs
}

/// Builds an owned pair list from string literals.
pub fn expected(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
    owned.sort();
    owned
}

/// Asserts that the source has mutants and that each one lies inside the
/// first occurrence of `control`.
pub fn assert_only_in_control(source: &str, kind: SourceKind, control: &str) {
    let start = match source.find(control) {
        Some(start) => start as u32,
        None => panic!("control {control:?} is not in the source"),
    };
    let end = start + control.len() as u32;
    let result = run(source, kind);
    assert!(
        !result.mutants.is_empty(),
        "the control {control:?} gave no mutant"
    );
    for mutant in &result.mutants {
        assert!(
            mutant.span.0 >= start && mutant.span.1 <= end,
            "mutant outside the control: {} {:?} -> {:?}",
            mutant.operator,
            mutant.original,
            mutant.replacement,
        );
    }
}
