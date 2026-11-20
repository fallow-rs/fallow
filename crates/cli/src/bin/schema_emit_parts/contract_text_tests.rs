//! Tests that the committed output schema describes the JSON that the
//! commands print: the `kind` property, the fields that the variant texts
//! name, and the conditions that the field texts give.

use serde_json::Value;

use super::{COMMITTED_SCHEMA, FALLOW_OUTPUT_VARIANTS};

fn committed_document() -> Value {
    serde_json::from_str(COMMITTED_SCHEMA).expect("committed docs/output-schema.json must parse")
}

fn definition<'a>(document: &'a Value, name: &str) -> &'a Value {
    document
        .pointer(&format!("/definitions/{name}"))
        .unwrap_or_else(|| panic!("definition `{name}` is missing"))
}

fn property_description<'a>(document: &'a Value, name: &str, property: &str) -> &'a str {
    definition(document, name)
        .pointer(&format!("/properties/{property}/description"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("`{name}.{property}` has no description"))
}

/// The root envelope of each command prints `kind`, so the definition of
/// each envelope names it, with the value that the root branch requires.
#[test]
fn every_root_envelope_definition_names_its_kind() {
    let document = committed_document();
    for (kind, names, _) in FALLOW_OUTPUT_VARIANTS {
        for name in *names {
            let property = definition(&document, name)
                .pointer("/properties/kind")
                .unwrap_or_else(|| panic!("`{name}` has no `kind` property"));
            assert_eq!(
                property.get("const").and_then(Value::as_str),
                Some(*kind),
                "`{name}.kind` must be the constant `{kind}`"
            );
        }
    }
}

/// An envelope nested in another envelope (the `check` block of bare
/// `fallow`) prints no `kind`, so the property stays optional.
#[test]
fn the_kind_property_of_a_definition_is_optional() {
    let document = committed_document();
    for (_, names, _) in FALLOW_OUTPUT_VARIANTS {
        for name in *names {
            let required = definition(&document, name)
                .get("required")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            assert!(
                !required.iter().any(|item| item.as_str() == Some("kind")),
                "`{name}` must not require `kind`"
            );
        }
    }
}

/// The fields that the "Required ..." clause of a root branch text names.
/// The clause ends at the first `;` or `. `, and a field after "by " (a key
/// of a nested entry) does not count.
fn required_fields(description: &str) -> Vec<&str> {
    let Some((_, rest)) = description.split_once("Required") else {
        return Vec::new();
    };
    let clause = rest
        .split(';')
        .next()
        .and_then(|part| part.split(". ").next())
        .unwrap_or_default();
    let parts: Vec<&str> = clause.split('`').collect();
    parts
        .iter()
        .enumerate()
        .skip(1)
        .step_by(2)
        .filter(|(index, _)| !parts[index - 1].trim_end().ends_with(" by"))
        .filter_map(|(_, token)| token.split(':').next())
        .filter(|token| {
            !token.is_empty()
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch == '_' || ch.is_ascii_digit())
        })
        .collect()
}

/// A root branch text that says "Required `x`" names only fields that the
/// definitions of the branch hold.
#[test]
fn variant_texts_name_only_fields_of_their_definitions() {
    let document = committed_document();
    let mut failures = Vec::new();
    for (kind, names, description) in FALLOW_OUTPUT_VARIANTS {
        for field in required_fields(description) {
            let known = names.iter().any(|name| {
                definition(&document, name)
                    .pointer(&format!("/properties/{field}"))
                    .is_some()
            });
            if !known {
                failures.push(format!(
                    "`{kind}` text names `{field}`, which {names:?} lack"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A `--format json` run of the CLI always prints `_meta` with
/// `telemetry.analysis_run_id`. A `_meta` text that names `--explain` must
/// also name that block, so that it does not read as "only with `--explain`".
#[test]
fn meta_texts_that_name_explain_also_name_the_telemetry_block() {
    let document = committed_document();
    let definitions = document
        .get("definitions")
        .and_then(Value::as_object)
        .expect("definitions");
    let mut failures = Vec::new();
    for (name, schema) in definitions {
        let Some(text) = schema
            .pointer("/properties/_meta/description")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if text.contains("--explain") && !text.contains("analysis_run_id") {
            failures.push(format!("`{name}._meta`: {text}"));
        }
    }
    let meta = definition(&document, "Meta")
        .get("description")
        .and_then(Value::as_str)
        .expect("Meta description");
    if !meta.contains("analysis_run_id") {
        failures.push(format!("`Meta`: {meta}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A plain `fallow health` run prints `health_score` and `file_scores`.
#[test]
fn health_score_texts_describe_the_plain_run() {
    let document = committed_document();
    for property in ["health_score", "file_scores"] {
        let text = property_description(&document, "HealthOutput", property);
        assert!(
            text.contains("plain `fallow health` run"),
            "`HealthOutput.{property}` must say that a plain run prints it: {text}"
        );
    }
}

/// `import_specifier` holds the resolved path of the imported file, not the
/// text of the import statement.
#[test]
fn boundary_import_specifier_text_names_the_resolved_path() {
    let document = committed_document();
    let text = property_description(&document, "BoundaryViolation", "import_specifier");
    assert!(
        !text.contains("raw import specifier"),
        "`BoundaryViolation.import_specifier` is a resolved path: {text}"
    );
    assert!(
        text.contains("project-root-relative path"),
        "`BoundaryViolation.import_specifier` must name the path form: {text}"
    );
}
