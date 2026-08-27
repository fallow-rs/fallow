//! JSON Schema documents and zero-config rule defaults, re-exported from
//! `fallow-config` for embedders that must not depend on the config crate
//! directly (the MCP server's `fallow://schema/*` and `fallow://issue-types`
//! resources). Each function returns the exact document the matching CLI
//! command prints (`fallow config-schema`, `fallow plugin-schema`,
//! `fallow rule-pack-schema`), so a cached resource and a CLI dump agree.

use std::sync::LazyLock;

use fallow_config::{ExternalPluginDef, FallowConfig, RulePackDef, RulesConfig};

/// `RulesConfig::default()` serialized once; the struct is compile-time
/// constant, so the map never changes within a process.
static DEFAULT_RULE_SEVERITIES: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::to_value(RulesConfig::default())
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()))
});

/// JSON Schema of the fallow config file (`fallow config-schema`).
#[must_use]
pub fn config_schema() -> serde_json::Value {
    FallowConfig::json_schema()
}

/// JSON Schema of a user-authored external plugin (`fallow plugin-schema`).
#[must_use]
pub fn plugin_schema() -> serde_json::Value {
    ExternalPluginDef::json_schema()
}

/// JSON Schema of a declarative rule pack (`fallow rule-pack-schema`).
#[must_use]
pub fn rule_pack_schema() -> serde_json::Value {
    RulePackDef::json_schema()
}

/// The zero-config `rules.*` severities as a flat JSON object keyed by config
/// key (`unused-exports`, `security-sink`, ...), serialized once from
/// `RulesConfig::default()`. This is the single source of default severities
/// for `fallow schema` and the MCP issue-type resource. Infallible in
/// practice (a flat struct of `Severity` enums); the empty-object fallback
/// keeps callers panic-free and simply yields no default severity if
/// serialization ever changed shape.
#[must_use]
pub fn default_rule_severities() -> serde_json::Value {
    DEFAULT_RULE_SEVERITIES.clone()
}

/// Whether `key` names a `rules.*` config field (a kebab-case key of
/// [`default_rule_severities`]).
#[must_use]
pub fn is_rule_severity_key(key: &str) -> bool {
    DEFAULT_RULE_SEVERITIES
        .get(key)
        .is_some_and(serde_json::Value::is_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_json_objects_with_properties() {
        for (label, schema) in [
            ("config", config_schema()),
            ("plugin", plugin_schema()),
            ("rule-pack", rule_pack_schema()),
        ] {
            assert!(
                schema
                    .get("properties")
                    .is_some_and(serde_json::Value::is_object),
                "{label} schema must be an object schema with properties"
            );
        }
    }

    #[test]
    fn default_severities_are_keyed_by_config_key() {
        let defaults = default_rule_severities();
        assert_eq!(defaults["unused-exports"], "error");
        assert_eq!(defaults["security-sink"], "off");
        assert!(
            defaults.get("unused_exports").is_none(),
            "keys are kebab-case"
        );
        assert!(is_rule_severity_key("coverage-gaps"));
        assert!(!is_rule_severity_key("code-duplication"));
    }
}
