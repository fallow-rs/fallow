use super::{MigrationResult, source_label};

#[expect(
    clippy::expect_used,
    reason = "the migrated config is a JSON object with no null value, and clamp_to_toml_integers removes each number that TOML cannot hold"
)]
pub(super) fn generate_toml(result: &MigrationResult) -> String {
    let header = format!(
        "Migrated from {}",
        result
            .sources
            .iter()
            .map(|s| source_label(s))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut config = result.config.clone();
    if let Some(map) = config.as_object_mut() {
        map.retain(|_, value| !value.as_object().is_some_and(serde_json::Map::is_empty));
    }
    clamp_to_toml_integers(&mut config);
    fallow_config::render_toml_config(&config, Some(&header))
        .expect("migrated config holds only TOML-compatible values")
}

/// TOML integers are signed 64-bit. A migrated count above `i64::MAX`, for
/// example a jscpd `minTokens`, becomes `i64::MAX`: no project reaches either
/// limit, so the setting keeps its effect.
fn clamp_to_toml_integers(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) if number.is_u64() && number.as_i64().is_none() => {
            *value = serde_json::Value::from(i64::MAX);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(clamp_to_toml_integers),
        serde_json::Value::Object(map) => map.values_mut().for_each(clamp_to_toml_integers),
        _ => {}
    }
}
