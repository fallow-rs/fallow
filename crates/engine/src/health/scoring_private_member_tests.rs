//! Coverage lookup for private class members (`#name`).

use super::tests::{istanbul_crap_default, make_fn_complexity, write_single_file_istanbul_fixture};
use super::*;

/// istanbul-lib-instrument emits no `fnMap` identity for a private class
/// member, so any candidate that a fallback reaches belongs to an
/// enclosing function.
#[test]
fn private_class_member_never_takes_enclosing_coverage() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/vault.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 7,
                "decl": { "start": { "line": 1, "column": 24 }, "end": { "line": 1, "column": 25 } },
                "loc": { "start": { "line": 7, "column": 5 }, "end": { "line": 7, "column": 20 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // `#wipe` sits in the arrow's header span and the arrow ran, but the
    // private member has no record of its own and never ran.
    let mut private_member = make_fn_complexity(1);
    private_member.name = "#wipe".to_string();
    private_member.is_private_member = true;
    private_member.line = 3;
    private_member.col = 9;
    let result = istanbul_crap_default(&[private_member], Some(file_coverage), false);
    assert_eq!(result.matched, 0);
    assert_eq!(
        result.per_function[0].coverage_source,
        fallow_output::CoverageSource::Estimated
    );
    assert_eq!(file_coverage.lookup("<arrow>", 1, 24), Some(100.0));
}

#[test]
fn quoted_hash_method_keeps_exact_istanbul_coverage() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/vault.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "class Vault { '#wipe'() {} }\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "#wipe",
                "line": 1,
                "decl": { "start": { "line": 1, "column": 14 }, "end": { "line": 1, "column": 21 } },
                "loc": { "start": { "line": 1, "column": 24 }, "end": { "line": 1, "column": 26 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    let mut quoted_public_method = make_fn_complexity(1);
    quoted_public_method.name = "#wipe".to_string();
    quoted_public_method.line = 1;
    quoted_public_method.col = 14;
    let result = istanbul_crap_default(&[quoted_public_method], Some(file_coverage), false);
    assert_eq!(result.matched, 1);
    assert_eq!(
        result.per_function[0].coverage_source,
        fallow_output::CoverageSource::Istanbul
    );
    assert_eq!(result.per_function[0].coverage_pct, Some(100.0));
}

/// A private member takes the one record named with its own `#name`, as
/// the V8 converter writes it: `decl` at the key, `loc` at the body, and
/// the extracted position at the parameter list in between (#3816). An
/// uncalled member gets its own zero. Two `#name` records near a member
/// (a private getter and setter) give no coverage.
#[test]
fn private_class_member_takes_its_own_named_record() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/vault.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "#wipe",
                "line": 2,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 7 } },
                "loc": { "start": { "line": 2, "column": 14 }, "end": { "line": 2, "column": 16 } }
            },
            "1": {
                "name": "#gone",
                "line": 3,
                "decl": { "start": { "line": 3, "column": 2 }, "end": { "line": 3, "column": 7 } },
                "loc": { "start": { "line": 3, "column": 14 }, "end": { "line": 3, "column": 16 } }
            },
            "2": {
                "name": "get #seal",
                "line": 10,
                "decl": { "start": { "line": 10, "column": 6 }, "end": { "line": 10, "column": 11 } },
                "loc": { "start": { "line": 10, "column": 14 }, "end": { "line": 10, "column": 16 } }
            },
            "3": {
                "name": "set #seal",
                "line": 11,
                "decl": { "start": { "line": 11, "column": 6 }, "end": { "line": 11, "column": 11 } },
                "loc": { "start": { "line": 11, "column": 15 }, "end": { "line": 11, "column": 17 } }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0, "2": 1, "3": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    let private_member = |name: &str, line: u32, col: u32| {
        let mut function = make_fn_complexity(1);
        function.name = name.to_string();
        function.is_private_member = true;
        function.line = line;
        function.col = col;
        function
    };
    let functions = [
        private_member("#wipe", 2, 7),
        private_member("#gone", 3, 7),
        private_member("#seal", 10, 11),
    ];

    let result = istanbul_crap_default(&functions, Some(file_coverage), false);
    assert_eq!(result.matched, 2);
    assert_eq!(
        result.per_function[0].coverage_source,
        fallow_output::CoverageSource::Istanbul
    );
    assert_eq!(result.per_function[0].coverage_pct, Some(100.0));
    assert_eq!(
        result.per_function[1].coverage_source,
        fallow_output::CoverageSource::Istanbul
    );
    assert_eq!(result.per_function[1].coverage_pct, Some(0.0));
    assert_eq!(
        result.per_function[2].coverage_source,
        fallow_output::CoverageSource::Estimated
    );
}
