//! The Node harness of the execution tests: it must keep strict mode and
//! tell thrown errors apart.

mod common;

use common::{run, run_node, run_node_active};
use fallow_mutation::SourceKind;

#[test]
fn errors_of_different_types_do_not_compare_equal() {
    let Some(type_error) = run_node("null.x;") else {
        return;
    };
    let reference_error = run_node("missing;");
    assert_ne!(Some(type_error), reference_error);
}

#[test]
fn the_active_runtime_keeps_strict_mode() {
    let source = "\"use strict\";\nfunction f() { return this === undefined; }\nconst k = 1 + 2;\nconsole.log(f(), k);\n";
    let Some(original) = run_node(source) else {
        return;
    };
    assert_eq!(original.0, "true 3\n");
    let result = run(source, SourceKind::Js);
    assert_eq!(run_node(&result.code), Some(original));
    let outside = result
        .mutants
        .iter()
        .find(|mutant| mutant.original == "+")
        .map(|mutant| mutant.id.as_str().to_owned());
    let Some(id) = outside else {
        panic!("no mutant outside f");
    };
    let active = run_node_active(&result.code, &id).map(|(stdout, _)| stdout);
    assert_eq!(active.as_deref(), Some("true -1\n"));
}
