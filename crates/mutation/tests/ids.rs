//! Mutant id stability: ids follow the enclosing function, not the absolute
//! file offset.

mod common;

use common::run;
use fallow_mutation::{Instrumented, Operator, SourceKind};

const BASE: &str = "function a(x) { return x + 1; }\nfunction b(y) { return y * 2; }\n";

fn ids_in(result: &Instrumented, function: &str) -> Vec<String> {
    result
        .mutants
        .iter()
        .filter(|m| {
            m.enclosing_function
                .as_ref()
                .and_then(|f| f.name.as_deref())
                == Some(function)
        })
        .map(|m| m.id.to_string())
        .collect()
}

#[test]
fn ids_are_sixteen_lowercase_hex_digits_and_unique() {
    let result = run(BASE, SourceKind::Js);
    assert!(!result.mutants.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    for mutant in &result.mutants {
        let id = mutant.id.as_str();
        assert_eq!(id.len(), 16, "{id}");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{id}"
        );
        assert!(seen.insert(id.to_owned()), "duplicate id {id}");
    }
}

#[test]
fn an_edit_in_another_function_keeps_the_ids() {
    let before = run(BASE, SourceKind::Js);
    let edited = BASE.replace("y * 2", "y * 2 + y");
    let after = run(&edited, SourceKind::Js);
    assert!(!ids_in(&before, "a").is_empty());
    assert_eq!(ids_in(&before, "a"), ids_in(&after, "a"));
    assert_ne!(ids_in(&before, "b"), ids_in(&after, "b"));
}

#[test]
fn an_edit_inside_the_function_changes_the_ids() {
    let before = run(BASE, SourceKind::Js);
    let edited = BASE.replace("x + 1", "x + 10");
    let after = run(&edited, SourceKind::Js);
    let before_a = ids_in(&before, "a");
    let after_a = ids_in(&after, "a");
    assert!(!before_a.is_empty());
    assert_eq!(before_a.len(), after_a.len());
    assert!(before_a.iter().all(|id| !after_a.contains(id)));
    assert_eq!(ids_in(&before, "b"), ids_in(&after, "b"));
}

#[test]
fn moving_code_down_the_file_keeps_the_ids() {
    let before = run(BASE, SourceKind::Js);
    let after = run(&format!("// header\n\n{BASE}"), SourceKind::Js);
    assert!(!ids_in(&before, "b").is_empty());
    assert_eq!(ids_in(&before, "a"), ids_in(&after, "a"));
    assert_eq!(ids_in(&before, "b"), ids_in(&after, "b"));
}

#[test]
fn the_path_is_part_of_the_id() {
    let options = fallow_mutation::MutationOptions::default();
    let first = fallow_mutation::instrument(BASE, "src/one.js", SourceKind::Js, &options);
    let second = fallow_mutation::instrument(BASE, "src/two.js", SourceKind::Js, &options);
    let ids = |result: &Result<Instrumented, _>| -> Vec<String> {
        result.as_ref().map_or_else(
            |_| Vec::new(),
            |r| r.mutants.iter().map(|m| m.id.to_string()).collect(),
        )
    };
    assert!(!ids(&first).is_empty());
    assert_ne!(ids(&first), ids(&second));
}

#[test]
fn identical_top_level_statements_get_distinct_ids() {
    let result = run("x = a < b;\nx = a < b;\n", SourceKind::Js);
    let relational: Vec<_> = result
        .mutants
        .iter()
        .filter(|m| m.operator == Operator::RelationalOperator)
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(relational.len(), 6);
    let unique: std::collections::BTreeSet<_> = relational.iter().collect();
    assert_eq!(unique.len(), 6);
}
