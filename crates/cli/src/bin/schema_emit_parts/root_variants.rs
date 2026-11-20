//! The `FallowOutput` root branches and the `kind` property of each root
//! envelope definition.

use fallow_output::LEAN_PROFILE_KINDS;
use serde_json::{Map, Value};

use super::FALLOW_OUTPUT_VARIANTS;

/// Name the `kind` discriminator in the definition of each root envelope, so
/// that a reader of one definition sees the field that the command prints.
/// The property stays optional: an envelope nested in another envelope, such
/// as the `check` block of bare `fallow`, prints no `kind`.
pub fn add_kind_properties(definitions: &mut Map<String, Value>) -> Result<(), String> {
    for (kind, names, _) in FALLOW_OUTPUT_VARIANTS {
        for name in *names {
            let properties = definitions
                .get_mut(*name)
                .and_then(|schema| schema.get_mut("properties"))
                .and_then(Value::as_object_mut)
                .ok_or_else(|| format!("derived schema has no properties for `{name}`"))?;
            properties.insert(
                "kind".to_string(),
                serde_json::json!({
                    "type": "string",
                    "const": kind,
                    "description": format!(
                        "Output discriminator. The root envelope always has the value `{kind}`. An envelope nested in another envelope has no `kind`."
                    ),
                }),
            );
        }
    }
    Ok(())
}

pub fn fallow_output_variant_schema(kind: &str, definitions: &[&str], description: &str) -> Value {
    let mut discriminator = serde_json::json!({
        "type": "object",
        "properties": { "kind": { "type": "string", "const": kind } },
        "required": ["kind"]
    });
    // A `--lean` document validates against `LeanOutput`, not the full envelope.
    if LEAN_PROFILE_KINDS.contains(&kind) {
        discriminator["not"] = serde_json::json!({ "required": ["profile"] });
    }
    serde_json::json!({
        "description": description,
        "allOf": [fallow_output_payload_schema(definitions), discriminator]
    })
}

fn fallow_output_payload_schema(definitions: &[&str]) -> Value {
    if definitions.len() == 1 {
        serde_json::json!({ "$ref": format!("#/definitions/{}", definitions[0]) })
    } else {
        serde_json::json!({
            "oneOf": definitions
                .iter()
                .map(|definition| serde_json::json!({ "$ref": format!("#/definitions/{definition}") }))
                .collect::<Vec<_>>()
        })
    }
}

pub fn fallow_output_kind_list() -> String {
    FALLOW_OUTPUT_VARIANTS
        .iter()
        .map(|(kind, _, _)| format!("`{kind}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
