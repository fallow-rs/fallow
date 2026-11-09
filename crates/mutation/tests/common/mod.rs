//! Shared helpers for the mutation fixture tests.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the helpers"
)]

use fallow_mutation::{Instrumented, Mutant, MutationOptions, Operator, SourceKind, instrument};
use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::SemanticBuilder;

/// Parses `code`, with the patterns of regular expression literals, and runs
/// the semantic syntax checks. Panics with the diagnostics and the code when
/// either step reports an error.
pub fn assert_valid(code: &str, kind: SourceKind) {
    let allocator = Allocator::default();
    let options = ParseOptions {
        parse_regular_expression: true,
        ..ParseOptions::default()
    };
    let parsed = Parser::new(&allocator, code, kind.source_type())
        .with_options(options)
        .parse();
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

/// The `--require` script of each Node run. It sets the mutation runtime
/// outside the source, so that a `"use strict"` directive stays the first
/// statement, and it prints each uncaught error, so that two runs that
/// throw different errors do not compare equal.
///
/// The line holds the error name, and the message only for a plain
/// `Error`. V8 writes the source of the failing expression into the message
/// of a `TypeError`, so the same failure reads `f(...) is not iterable` in
/// the applied mutant and `(intermediate value) is not iterable` in the
/// instrumented code.
const NODE_SETUP: &str = r#"const active = process.env.FALLOW_TEST_ACTIVE;
if (active) {
  globalThis.__fallowMutation = { a: (id) => id === active, c: () => {} };
}
const report = (error) => {
  const name = error && error.name;
  const message = name === "Error" ? `: ${error.message}` : "";
  process.stdout.write(`uncaught ${name}${message}\n`);
  process.exit(1);
};
process.on("uncaughtException", report);
process.on("unhandledRejection", report);
"#;

/// The path of the setup script, written once per test process.
fn node_setup() -> Option<&'static std::path::Path> {
    static SETUP: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    SETUP
        .get_or_init(|| {
            let path = std::env::temp_dir().join(format!(
                "fallow-mutation-node-setup-{}.cjs",
                std::process::id()
            ));
            std::fs::write(&path, NODE_SETUP).ok()?;
            Some(path)
        })
        .as_deref()
}

/// Runs a CommonJS script with `node`, with mutant `active` on. Returns the
/// standard output, with a line for an uncaught error, and the exit code,
/// or `None` when `node` is not available.
fn run_node_with(code: &str, active: Option<&str>) -> Option<(String, Option<i32>)> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut command = Command::new("node");
    command
        .arg("--require")
        .arg(node_setup()?)
        .args(["--input-type=commonjs", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    match active {
        Some(id) => command.env("FALLOW_TEST_ACTIVE", id),
        None => command.env_remove("FALLOW_TEST_ACTIVE"),
    };
    let mut child = command.spawn().ok()?;
    child.stdin.take()?.write_all(code.as_bytes()).ok()?;
    let output = child.wait_with_output().ok()?;
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code(),
    ))
}

/// Runs a CommonJS script with `node` and no active mutant.
pub fn run_node(code: &str) -> Option<(String, Option<i32>)> {
    run_node_with(code, None)
}

/// Runs instrumented `code` with only mutant `id` active.
pub fn run_node_active(code: &str, id: &str) -> Option<(String, Option<i32>)> {
    run_node_with(code, Some(id))
}

/// Checks with `node` that the instrumented code of `source` gives the
/// output of `source` when no mutant is active, and the output of the
/// applied mutant when one mutant is active. A machine without `node`
/// skips the check.
pub fn assert_runs_like_each_mutant(source: &str, options: &MutationOptions) -> usize {
    let Some(original) = run_node(source) else {
        return 0;
    };
    assert_eq!(original.1, Some(0), "the fixture fails: {original:?}");
    let result = run_with(source, SourceKind::Js, options);
    assert_eq!(
        run_node(&result.code),
        Some(original),
        "the inactive instrumented code differs\n---\n{}",
        result.code
    );
    for mutant in &result.mutants {
        let mutated = run_node(&apply(source, mutant));
        let switched = run_node_active(&result.code, mutant.id.as_str());
        assert_eq!(
            switched, mutated,
            "{} {:?} -> {:?}",
            mutant.operator, mutant.original, mutant.replacement
        );
    }
    result.mutants.len()
}
