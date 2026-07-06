use serde_json::Value;

use crate::codeclimate::codeclimate_fingerprint_hash;

/// Fingerprint key used in SARIF partialFingerprints and other CI formats.
pub const SARIF_FINGERPRINT_KEY: &str = "tools.fallow.fingerprint/v1";

/// Conventional SARIF key consumed by GitHub Code Scanning.
pub const GHAS_SARIF_FINGERPRINT_KEY: &str = "primaryLocationLineHash/v1";

/// Fields needed to build one SARIF result object.
#[derive(Debug, Clone, Copy)]
pub struct SarifResultInput<'a> {
    pub rule_id: &'a str,
    pub level: &'a str,
    pub message: &'a str,
    pub uri: &'a str,
    pub region: Option<(u32, u32)>,
    pub snippet: Option<&'a str>,
}

/// Normalized finding input for output-owned SARIF result assembly.
#[derive(Debug, Clone)]
pub struct SarifFindingInput<'a> {
    pub issue_code: &'a str,
    pub rule_id: &'a str,
    pub level: &'a str,
    pub message: &'a str,
    pub uri: &'a str,
    pub region: Option<(u32, u32)>,
    pub snippet: Option<&'a str>,
    pub properties: Option<Value>,
}

/// Fields needed to build one SARIF rule object.
#[derive(Debug, Clone, Copy)]
pub struct SarifRuleInput<'a> {
    pub id: &'a str,
    pub short_description: &'a str,
    pub level: &'a str,
    pub full_description: Option<&'a str>,
    pub help_uri: Option<&'a str>,
}

/// Fields needed to build a SARIF document envelope.
#[derive(Debug, Clone, Copy)]
pub struct SarifDocumentInput<'a> {
    pub results: &'a [Value],
    pub rules: &'a [Value],
    pub tool_version: &'a str,
}

/// Normalize a source snippet before it contributes to stable SARIF identity.
#[must_use]
pub fn normalize_sarif_snippet(snippet: &str) -> String {
    snippet
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Stable SARIF fingerprint for a finding with source snippet evidence.
#[must_use]
pub fn sarif_finding_fingerprint(rule_id: &str, path: &str, snippet: &str) -> String {
    let normalized = normalize_sarif_snippet(snippet);
    codeclimate_fingerprint_hash(&[rule_id, path, &normalized])
}

/// Build a single SARIF result object.
///
/// When `region` is `Some((line, col))`, a `region` block with 1-based
/// `startLine` and `startColumn` is included in the physical location.
#[must_use]
pub fn build_sarif_result(input: SarifResultInput<'_>) -> Value {
    let mut physical_location = serde_json::json!({
        "artifactLocation": { "uri": input.uri }
    });
    if let Some((line, col)) = input.region {
        physical_location["region"] = serde_json::json!({
            "startLine": line,
            "startColumn": col
        });
    }
    let line = input
        .region
        .map_or_else(String::new, |(line, _)| line.to_string());
    let col = input
        .region
        .map_or_else(String::new, |(_, col)| col.to_string());
    let normalized_snippet = input
        .snippet
        .map(normalize_sarif_snippet)
        .filter(|snippet| !snippet.is_empty());
    let partial_fingerprint = normalized_snippet.as_ref().map_or_else(
        || codeclimate_fingerprint_hash(&[input.rule_id, input.uri, &line, &col]),
        |snippet| codeclimate_fingerprint_hash(&[input.rule_id, input.uri, snippet]),
    );
    let partial_fingerprint_ghas = partial_fingerprint.clone();
    serde_json::json!({
        "ruleId": input.rule_id,
        "level": input.level,
        "message": { "text": input.message },
        "locations": [{ "physicalLocation": physical_location }],
        "partialFingerprints": {
            SARIF_FINGERPRINT_KEY: partial_fingerprint,
            GHAS_SARIF_FINGERPRINT_KEY: partial_fingerprint_ghas
        }
    })
}

/// Build a SARIF result from a normalized finding.
#[must_use]
pub fn build_sarif_finding(input: SarifFindingInput<'_>) -> Value {
    let mut result = build_sarif_result(SarifResultInput {
        rule_id: input.rule_id,
        level: input.level,
        message: input.message,
        uri: input.uri,
        region: input.region,
        snippet: input.snippet,
    });
    if let Some(properties) = input.properties {
        result["properties"] = properties;
    }
    result
}

/// Build a SARIF rule object.
#[must_use]
pub fn build_sarif_rule(input: SarifRuleInput<'_>) -> Value {
    let mut rule = serde_json::Map::new();
    rule.insert("id".to_string(), serde_json::json!(input.id));
    rule.insert(
        "shortDescription".to_string(),
        serde_json::json!({ "text": input.short_description }),
    );
    if let Some(full_description) = input.full_description {
        rule.insert(
            "fullDescription".to_string(),
            serde_json::json!({ "text": full_description }),
        );
    }
    if let Some(help_uri) = input.help_uri {
        rule.insert("helpUri".to_string(), serde_json::json!(help_uri));
    }
    rule.insert(
        "defaultConfiguration".to_string(),
        serde_json::json!({ "level": input.level }),
    );
    Value::Object(rule)
}

/// Build a SARIF 2.1.0 document envelope.
#[must_use]
pub fn build_sarif_document(input: SarifDocumentInput<'_>) -> Value {
    serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": input.tool_version,
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": input.rules
                }
            },
            "results": input.results
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sarif_result_includes_location_and_fingerprints() {
        let result = build_sarif_result(SarifResultInput {
            rule_id: "fallow/test",
            level: "warning",
            message: "description",
            uri: "src/app.ts",
            region: Some((7, 3)),
            snippet: Some("  export const value = 1;  "),
        });

        assert_eq!(result["ruleId"], "fallow/test");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["region"]["startLine"],
            7
        );
        assert!(result["partialFingerprints"][SARIF_FINGERPRINT_KEY].is_string());
        assert!(result["partialFingerprints"][GHAS_SARIF_FINGERPRINT_KEY].is_string());
    }

    #[test]
    fn sarif_finding_includes_custom_properties() {
        let finding = build_sarif_finding(SarifFindingInput {
            issue_code: "unused-export",
            rule_id: "fallow/unused-export",
            level: "warning",
            message: "Export is never imported",
            uri: "src/app.ts",
            region: Some((3, 14)),
            snippet: Some("export const unused = 1;"),
            properties: Some(serde_json::json!({ "is_re_export": true })),
        });

        assert_eq!(finding["ruleId"], "fallow/unused-export");
        assert_eq!(finding["properties"]["is_re_export"], true);
        assert!(finding["partialFingerprints"][SARIF_FINGERPRINT_KEY].is_string());
    }

    #[test]
    fn sarif_finding_omits_empty_properties() {
        let finding = build_sarif_finding(SarifFindingInput {
            issue_code: "unused-file",
            rule_id: "fallow/unused-file",
            level: "error",
            message: "File is unreachable",
            uri: "src/unused.ts",
            region: None,
            snippet: None,
            properties: None,
        });

        assert!(finding.get("properties").is_none());
    }

    #[test]
    fn sarif_rule_omits_optional_docs_when_absent() {
        let rule = build_sarif_rule(SarifRuleInput {
            id: "fallow/test",
            short_description: "short",
            level: "warning",
            full_description: None,
            help_uri: None,
        });

        assert!(rule.get("fullDescription").is_none());
        assert!(rule.get("helpUri").is_none());
    }

    #[test]
    fn sarif_document_uses_supplied_version() {
        let document = build_sarif_document(SarifDocumentInput {
            results: &[],
            rules: &[],
            tool_version: "1.2.3",
        });

        assert_eq!(document["version"], "2.1.0");
        assert_eq!(document["runs"][0]["tool"]["driver"]["version"], "1.2.3");
    }
}
