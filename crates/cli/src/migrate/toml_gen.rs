use super::{MigrationResult, source_head};

#[expect(
    clippy::expect_used,
    reason = "the migrated config is a JSON object with no null value"
)]
pub(super) fn generate_toml(result: &MigrationResult) -> String {
    let header = format!(
        "Migrated from {}",
        result
            .sources
            .iter()
            .map(|s| source_head(s))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut config = result.config.clone();
    if let Some(map) = config.as_object_mut() {
        map.retain(|_, value| !value.as_object().is_some_and(serde_json::Map::is_empty));
    }
    fallow_config::render_toml_config(&config, Some(&header))
        .expect("migrated config holds only TOML-compatible values")
}
