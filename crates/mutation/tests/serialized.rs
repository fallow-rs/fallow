//! A function that a test serializes with `toString` and runs in another
//! realm, such as a `node:vm` context or a browser page, has no prelude
//! there. The switch must then run the original code, not throw a
//! `ReferenceError`.

mod common;

use common::{run, run_node, run_node_active};
use fallow_mutation::SourceKind;

/// `inner` holds an expression site (`+`) and a statement site (the block
/// of the `if`). The script runs it in the current realm and in a fresh
/// `node:vm` context, the way a mock of `page.evaluate` does.
const SERIALIZED_FIXTURE: &str = r#"const { runInNewContext } = require("node:vm");
const inner = (argument) => {
  if (argument.limit > 2) {
    argument.limit = argument.limit - 1;
  }
  return argument.text.slice(0, argument.limit + 1);
};
const local = inner({ text: "Example", limit: 4 });
const remote = runInNewContext(`(${inner.toString()})(argument)`, {
  argument: { text: "Example", limit: 4 },
});
console.log(local, remote);
"#;

#[test]
fn a_serialized_function_runs_the_original_code_in_a_fresh_realm() {
    let Some(original) = run_node(SERIALIZED_FIXTURE) else {
        return;
    };
    assert_eq!(original.0, "Exam Exam\n");
    let result = run(SERIALIZED_FIXTURE, SourceKind::Js);
    assert!(
        result.code.contains("if (") && result.code.contains(" ? ("),
        "the fixture has no statement site or no expression site:\n{}",
        result.code
    );
    assert_eq!(run_node(&result.code), Some(original), "{}", result.code);
}

#[test]
fn a_serialized_function_keeps_the_mutants_of_the_current_realm() {
    if run_node(SERIALIZED_FIXTURE).is_none() {
        return;
    }
    let result = run(SERIALIZED_FIXTURE, SourceKind::Js);
    let plus = result
        .mutants
        .iter()
        .find(|mutant| mutant.original == "+")
        .map(|mutant| mutant.id.as_str().to_owned());
    let Some(id) = plus else {
        panic!("no `+` mutant:\n{}", result.code);
    };
    // The mutant changes `limit + 1` in the current realm only. The fresh
    // realm has no mutation runtime, so it runs the original code.
    let active = run_node_active(&result.code, &id).map(|(stdout, _)| stdout);
    assert_eq!(active.as_deref(), Some("Ex Exam\n"), "{}", result.code);
}
