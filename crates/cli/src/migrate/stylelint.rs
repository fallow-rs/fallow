use super::MigrationWarning;

const STYLELINT_SELECTOR_RULES: &[&str] = &[
    "declaration-no-important",
    "max-nesting-depth",
    "no-descending-specificity",
    "selector-max-attribute",
    "selector-max-class",
    "selector-max-combinators",
    "selector-max-compound-selectors",
    "selector-max-id",
    "selector-max-specificity",
    "selector-max-type",
    "selector-max-universal",
];

const UNSUPPORTED_STYLELINT_RULES: &[(&str, &str)] = &[
    ("alpha-value-notation", "value notation stays in Stylelint"),
    (
        "at-rule-no-vendor-prefix",
        "vendor prefix policy stays in Stylelint",
    ),
    (
        "color-function-notation",
        "color formatting stays in Stylelint",
    ),
    ("color-hex-case", "color formatting stays in Stylelint"),
    ("color-hex-length", "color formatting stays in Stylelint"),
    (
        "custom-property-pattern",
        "naming conventions stay in Stylelint",
    ),
    (
        "declaration-block-no-duplicate-properties",
        "syntax linting stays in Stylelint",
    ),
    (
        "declaration-block-single-line-max-declarations",
        "formatting stays in Stylelint",
    ),
    (
        "declaration-empty-line-before",
        "formatting stays in Stylelint",
    ),
    ("font-family-name-quotes", "formatting stays in Stylelint"),
    ("function-name-case", "formatting stays in Stylelint"),
    ("length-zero-no-unit", "formatting stays in Stylelint"),
    ("no-empty-source", "syntax linting stays in Stylelint"),
    ("order/order", "declaration ordering stays in Stylelint"),
    (
        "order/properties-order",
        "declaration ordering stays in Stylelint",
    ),
    (
        "property-no-vendor-prefix",
        "vendor prefix policy stays in Stylelint",
    ),
    ("rule-empty-line-before", "formatting stays in Stylelint"),
    (
        "selector-class-pattern",
        "naming conventions stay in Stylelint",
    ),
    (
        "selector-id-pattern",
        "naming conventions stay in Stylelint",
    ),
    (
        "selector-no-vendor-prefix",
        "vendor prefix policy stays in Stylelint",
    ),
    ("string-quotes", "formatting stays in Stylelint"),
    (
        "value-no-vendor-prefix",
        "vendor prefix policy stays in Stylelint",
    ),
];

pub(super) fn migrate_stylelint(
    stylelint: &serde_json::Value,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(obj) = stylelint.as_object() else {
        warnings.push(MigrationWarning {
            source: "stylelint",
            field: "(root)".to_string(),
            message: "expected an object, got something else".to_string(),
            suggestion: None,
        });
        return;
    };

    let Some(rules) = obj.get("rules").and_then(serde_json::Value::as_object) else {
        warnings.push(MigrationWarning {
            source: "stylelint",
            field: "rules".to_string(),
            message: "no Stylelint rules object found to map".to_string(),
            suggestion: Some("keep Stylelint for formatting and syntax linting".to_string()),
        });
        return;
    };

    if rules
        .iter()
        .any(|(rule, value)| STYLELINT_SELECTOR_RULES.contains(&rule.as_str()) && is_enabled(value))
    {
        let rule_map = ensure_object(config, "rules");
        rule_map
            .entry("css-selector-complexity".to_string())
            .or_insert_with(|| serde_json::Value::String("warn".to_string()));
        let audit_map = ensure_object(config, "audit");
        audit_map
            .entry("css".to_string())
            .or_insert(serde_json::Value::Bool(true));
        audit_map
            .entry("cssDeep".to_string())
            .or_insert(serde_json::Value::Bool(true));
    }

    push_unsupported_stylelint_warnings(rules, warnings);
}

fn ensure_object<'a>(
    config: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, serde_json::Value> {
    let entry = config
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entry.is_object() {
        *entry = serde_json::Value::Object(serde_json::Map::new());
    }
    match entry {
        serde_json::Value::Object(obj) => obj,
        _ => unreachable!("entry was just made object"),
    }
}

fn is_enabled(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(enabled) => *enabled,
        serde_json::Value::Array(items) => items.first().is_some_and(is_enabled),
        _ => true,
    }
}

fn push_unsupported_stylelint_warnings(
    rules: &serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
) {
    for (rule, message) in UNSUPPORTED_STYLELINT_RULES {
        if rules.get(*rule).is_some_and(is_enabled) {
            warnings.push(MigrationWarning {
                source: "stylelint",
                field: (*rule).to_string(),
                message: (*message).to_string(),
                suggestion: Some("leave this rule in Stylelint".to_string()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_config() -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }

    #[test]
    fn migrate_stylelint_selector_rules_enable_styling_audit() {
        let stylelint: serde_json::Value = serde_json::from_str(
            r#"{"rules":{"selector-max-id":0,"max-nesting-depth":[3],"declaration-no-important":true}}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();

        migrate_stylelint(&stylelint, &mut config, &mut warnings);

        assert_eq!(
            config["rules"]["css-selector-complexity"],
            serde_json::json!("warn")
        );
        assert_eq!(config["audit"]["css"], serde_json::json!(true));
        assert_eq!(config["audit"]["cssDeep"], serde_json::json!(true));
        assert!(warnings.is_empty());
    }

    #[test]
    fn migrate_stylelint_unsupported_rules_warn_without_mapping() {
        let stylelint: serde_json::Value = serde_json::from_str(
            r#"{"rules":{"color-hex-case":"lower","selector-class-pattern":"^[a-z]+$"}}"#,
        )
        .unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();

        migrate_stylelint(&stylelint, &mut config, &mut warnings);

        assert!(config.is_empty());
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.field == "color-hex-case")
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.field == "selector-class-pattern")
        );
    }

    #[test]
    fn migrate_stylelint_disabled_rule_is_ignored() {
        let stylelint: serde_json::Value =
            serde_json::from_str(r#"{"rules":{"selector-max-id":null}}"#).unwrap();
        let mut config = empty_config();
        let mut warnings = Vec::new();

        migrate_stylelint(&stylelint, &mut config, &mut warnings);

        assert!(config.is_empty());
        assert!(warnings.is_empty());
    }
}
