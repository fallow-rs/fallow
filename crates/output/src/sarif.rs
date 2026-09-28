use std::path::{Path, PathBuf};

use fallow_types::identity::IdentifiedFinding;
use rustc_hash::FxHashMap;
use serde_json::Value;

use crate::codeclimate::codeclimate_fingerprint_hash;

/// Fingerprint key used in SARIF partialFingerprints and other CI formats.
pub const SARIF_FINGERPRINT_KEY: &str = "tools.fallow.fingerprint/v1";

/// Conventional SARIF key consumed by GitHub Code Scanning.
pub const GHAS_SARIF_FINGERPRINT_KEY: &str = "primaryLocationLineHash/v1";

/// `partialFingerprints` key that holds the stable dead-code `finding_id`.
///
/// Unlike the two location-based keys, the value does not depend on the line,
/// the column or the source text. The uniqueness pass never rewrites it.
pub const SARIF_FINDING_ID_KEY: &str = "fallowFinding/v1";

/// Fields needed to build one SARIF result object.
#[derive(Debug, Clone, Copy)]
pub struct SarifResultInput<'a> {
    /// SARIF rule id the result references, e.g. `fallow/unused-file`.
    pub rule_id: &'a str,
    /// SARIF level: `error`, `warning`, or `note`.
    pub level: &'a str,
    /// Human-readable result message text.
    pub message: &'a str,
    /// Artifact URI relative to the analysed root.
    pub uri: &'a str,
    /// 1-based `(start_line, start_column)` region, when known.
    pub region: Option<(u32, u32)>,
    /// Source snippet that feeds the stable fingerprint and region context.
    pub snippet: Option<&'a str>,
}

/// Normalized finding input for output-owned SARIF result assembly.
#[derive(Debug, Clone)]
pub struct SarifFindingInput<'a> {
    /// Fallow issue code the finding originated from, e.g. `unused-file`.
    pub issue_code: &'a str,
    /// SARIF rule id the result references.
    pub rule_id: &'a str,
    /// SARIF level: `error`, `warning`, or `note`.
    pub level: &'a str,
    /// Human-readable result message text.
    pub message: &'a str,
    /// Artifact URI relative to the analysed root.
    pub uri: &'a str,
    /// 1-based `(start_line, start_column)` region, when known.
    pub region: Option<(u32, u32)>,
    /// Source snippet that feeds the stable fingerprint and region context.
    pub snippet: Option<&'a str>,
    /// Extra `properties` bag copied onto the SARIF result verbatim.
    pub properties: Option<Value>,
    /// Stable dead-code `finding_id`, written under [`SARIF_FINDING_ID_KEY`].
    /// `None` omits the key.
    pub finding_id: Option<&'a str>,
}

/// Intermediate fields extracted from one issue for SARIF result construction.
#[derive(Debug, Clone)]
pub struct SarifFindingFields {
    /// SARIF rule id the result references.
    pub rule_id: &'static str,
    /// SARIF level: `error`, `warning`, or `note`.
    pub level: &'static str,
    /// Human-readable result message text.
    pub message: String,
    /// Artifact URI relative to the analysed root.
    pub uri: String,
    /// 1-based `(start_line, start_column)` region, when known.
    pub region: Option<(u32, u32)>,
    /// Absolute source path used to load the fingerprint snippet.
    pub source_path: Option<PathBuf>,
    /// Extra `properties` bag copied onto the SARIF result verbatim.
    pub properties: Option<Value>,
}

/// Fields needed to build one SARIF rule object.
#[derive(Debug, Clone, Copy)]
pub struct SarifRuleInput<'a> {
    /// SARIF rule id, e.g. `fallow/unused-file`.
    pub id: &'a str,
    /// One-line rule description shown in SARIF viewers.
    pub short_description: &'a str,
    /// Default SARIF level for the rule's `defaultConfiguration`.
    pub level: &'a str,
    /// Longer rule description, when the rule has one.
    pub full_description: Option<&'a str>,
    /// Public documentation URL for the rule.
    pub help_uri: Option<&'a str>,
}

/// Fields needed to build a SARIF document envelope.
#[derive(Debug, Clone, Copy)]
pub struct SarifDocumentInput<'a> {
    /// Pre-built SARIF result objects for the single run.
    pub results: &'a [Value],
    /// Pre-built tool-driver rule objects for the single run.
    pub rules: &'a [Value],
    /// Fallow version reported as the SARIF tool driver version.
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
///
/// `col` is the 1-based start column the finding reports, and is what separates
/// two findings of the same rule that share a source line.
#[must_use]
pub fn sarif_finding_fingerprint(rule_id: &str, path: &str, snippet: &str, col: u32) -> String {
    let normalized = normalize_sarif_snippet(snippet);
    codeclimate_fingerprint_hash(&[rule_id, path, &normalized, &col.to_string()])
}

/// Lazily reads source files so SARIF result builders can attach stable line snippets.
#[derive(Debug, Default)]
pub struct SarifSourceSnippetCache {
    root: Option<PathBuf>,
    files: FxHashMap<PathBuf, Vec<String>>,
}

impl SarifSourceSnippetCache {
    /// Create a snippet cache that resolves relative finding paths against the
    /// analyzed project root.
    #[must_use]
    pub fn with_root(root: &Path) -> Self {
        Self {
            root: Some(root.to_path_buf()),
            files: FxHashMap::default(),
        }
    }

    /// Return the 1-based source line from a file, caching the file contents.
    pub fn line(&mut self, path: &Path, line: u32) -> Option<String> {
        if line == 0 {
            return None;
        }
        let resolved = if path.is_relative() {
            self.root
                .as_deref()
                .map_or_else(|| path.to_path_buf(), |root| root.join(path))
        } else {
            path.to_path_buf()
        };
        if !self.files.contains_key(&resolved) {
            let lines = std::fs::read_to_string(&resolved)
                .ok()
                .map(|source| source.lines().map(str::to_owned).collect())
                .unwrap_or_default();
            self.files.insert(resolved.clone(), lines);
        }
        self.files
            .get(&resolved)
            .and_then(|lines| lines.get(line.saturating_sub(1) as usize))
            .cloned()
    }
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
    // The snippet replaces the LINE, which moves under any edit above it, and
    // not the COLUMN, which is as stable as the snippet itself: two findings on
    // one line are two alerts, and GitHub code scanning treats one
    // `partialFingerprints` value as one alert identity. Dropping the column
    // here collapsed every re-export in a one-line barrel, every member of a
    // one-line enum, and every dependency in a compact `package.json` into a
    // single alert.
    let partial_fingerprint = normalized_snippet.as_ref().map_or_else(
        || codeclimate_fingerprint_hash(&[input.rule_id, input.uri, &line, &col]),
        |snippet| codeclimate_fingerprint_hash(&[input.rule_id, input.uri, snippet, &col]),
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
    if let Some(finding_id) = input.finding_id {
        result["partialFingerprints"][SARIF_FINDING_ID_KEY] = Value::from(finding_id);
    }
    if let Some(properties) = input.properties {
        result["properties"] = properties;
    }
    result
}

/// Build a single SARIF result object with optional source snippet evidence.
#[must_use]
pub fn build_sarif_result_with_snippet(
    rule_id: &str,
    level: &str,
    message: &str,
    uri: &str,
    region: Option<(u32, u32)>,
    snippet: Option<&str>,
) -> Value {
    build_sarif_result(SarifResultInput {
        rule_id,
        level,
        message,
        uri,
        region,
        snippet,
    })
}

/// Append SARIF findings by extracting normalized fields from typed issues.
///
/// Each item gives exactly one result, so the `finding_id` of the item becomes
/// the [`SARIF_FINDING_ID_KEY`] of its result.
pub fn append_sarif_findings<T: IdentifiedFinding>(
    sarif_results: &mut Vec<Value>,
    items: &[T],
    snippets: &mut SarifSourceSnippetCache,
    mut extract: impl FnMut(&T) -> SarifFindingFields,
) {
    for item in items {
        let fields = extract(item);
        let source_snippet = fields
            .source_path
            .as_deref()
            .zip(fields.region)
            .and_then(|(path, (line, _))| snippets.line(path, line));
        let result = build_sarif_finding(SarifFindingInput {
            issue_code: issue_code_from_rule_id(fields.rule_id),
            rule_id: fields.rule_id,
            level: fields.level,
            message: &fields.message,
            uri: &fields.uri,
            region: fields.region,
            snippet: source_snippet.as_deref(),
            properties: fields.properties,
            finding_id: item.finding_id(),
        });
        sarif_results.push(result);
    }
}

/// Give every result in one run its own `partialFingerprints` value.
///
/// GitHub code scanning treats that value as alert identity, so two results
/// sharing one are one alert and the second finding is never surfaced. Rule id,
/// URI, snippet, and column already separate findings that differ anywhere a
/// reader can see; what is left is a file that reports the same rule twice with
/// byte-identical evidence, such as the same declaration written twice. The
/// first occurrence keeps the value it computed, so an alert that already exists
/// is never disturbed, and each repeat mixes in its occurrence index.
pub fn ensure_unique_result_fingerprints(results: &mut [Value]) {
    let mut occurrences: FxHashMap<String, u32> = FxHashMap::default();
    for result in results {
        let Some(fingerprint) = result
            .get("partialFingerprints")
            .and_then(|prints| prints.get(SARIF_FINGERPRINT_KEY))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let occurrence = occurrences.entry(fingerprint.clone()).or_insert(0);
        let index = *occurrence;
        *occurrence += 1;
        if index == 0 {
            continue;
        }
        let unique = codeclimate_fingerprint_hash(&[&fingerprint, &index.to_string()]);
        result["partialFingerprints"][SARIF_FINGERPRINT_KEY] = Value::from(unique.clone());
        result["partialFingerprints"][GHAS_SARIF_FINGERPRINT_KEY] = Value::from(unique);
    }
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

fn issue_code_from_rule_id(rule_id: &str) -> &str {
    rule_id.strip_prefix("fallow/").unwrap_or(rule_id)
}

/// Build a SARIF 2.1.0 document envelope.
///
/// Applies [`ensure_unique_result_fingerprints`], so every run this builds
/// satisfies the one-alert-per-finding property. A caller that replaces
/// `/runs/0/results` afterwards has to apply it again.
#[must_use]
pub fn build_sarif_document(input: SarifDocumentInput<'_>) -> Value {
    let mut results = input.results.to_vec();
    ensure_unique_result_fingerprints(&mut results);
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
            "results": results
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

    fn fingerprint_of(result: &Value) -> &str {
        result["partialFingerprints"][SARIF_FINGERPRINT_KEY]
            .as_str()
            .expect("fingerprint")
    }

    /// A one-line re-export barrel, a one-line enum, and a compact
    /// `package.json` all put two findings of one rule on one source line, so
    /// the snippet is identical and only the column tells them apart. GitHub
    /// code scanning keys alert identity on this value, so a shared value is a
    /// lost alert.
    #[test]
    fn two_findings_on_one_line_get_different_fingerprints() {
        let at_column = |col: u32| {
            build_sarif_result(SarifResultInput {
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Re-export is never imported by other modules",
                uri: "src/barrel.ts",
                region: Some((1, col)),
                snippet: Some("export { alpha, beta } from './m';"),
            })
        };

        assert_ne!(
            fingerprint_of(&at_column(10)),
            fingerprint_of(&at_column(17))
        );
    }

    /// The column is the only position in the fingerprint: a snippet that
    /// survives an edit above it has to keep its identity, or every open alert
    /// on the file below the edit closes and reopens.
    #[test]
    fn moving_a_finding_to_another_line_keeps_its_fingerprint() {
        let at_line = |line: u32| {
            build_sarif_result(SarifResultInput {
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Export is never imported by other modules",
                uri: "src/lib.ts",
                region: Some((line, 14)),
                snippet: Some("export const alpha = 1;"),
            })
        };

        assert_eq!(fingerprint_of(&at_line(3)), fingerprint_of(&at_line(41)));
    }

    /// Two byte-identical declarations in one file leave the snippet and the
    /// column identical, so position alone cannot separate them.
    #[test]
    fn identical_results_are_separated_by_occurrence() {
        let result = || {
            build_sarif_result(SarifResultInput {
                rule_id: "fallow/duplicate-export",
                level: "warning",
                message: "Export 'Video' appears in multiple modules",
                uri: "src/types.ts",
                region: Some((309, 18)),
                snippet: Some("export type Video = {"),
            })
        };
        let mut results = vec![result(), result(), result()];
        let first_before = fingerprint_of(&results[0]).to_owned();

        ensure_unique_result_fingerprints(&mut results);

        assert_eq!(
            fingerprint_of(&results[0]),
            first_before,
            "the first occurrence keeps the identity an existing alert was opened under"
        );
        let unique: std::collections::BTreeSet<&str> = results.iter().map(fingerprint_of).collect();
        assert_eq!(unique.len(), 3, "{results:?}");
        for result in &results {
            assert_eq!(
                result["partialFingerprints"][SARIF_FINGERPRINT_KEY],
                result["partialFingerprints"][GHAS_SARIF_FINGERPRINT_KEY],
                "both keys name the same alert"
            );
        }
    }

    /// Finding ids are unique by construction, so the uniqueness pass rewrites
    /// only the two location-based keys and keeps each `finding_id` key.
    #[test]
    fn the_uniqueness_pass_keeps_the_finding_id_key() {
        let result = |finding_id: &str| {
            build_sarif_finding(SarifFindingInput {
                issue_code: "unused-class-member",
                rule_id: "fallow/unused-class-member",
                level: "warning",
                message: "Class member 'run' is never used",
                uri: "src/service.ts",
                region: Some((4, 3)),
                snippet: Some("run() {}"),
                properties: None,
                finding_id: Some(finding_id),
            })
        };
        let ids = [
            "dc1:unused-class-member:0123456789abcdef",
            "dc1:unused-class-member:0123456789abcdef~1",
        ];
        let mut results = ids.map(result).to_vec();

        ensure_unique_result_fingerprints(&mut results);

        assert_ne!(
            fingerprint_of(&results[0]),
            fingerprint_of(&results[1]),
            "the pass must still separate the location-based keys"
        );
        for (result, id) in results.iter().zip(ids) {
            assert_eq!(result["partialFingerprints"][SARIF_FINDING_ID_KEY], id);
        }
    }

    /// The `finding_id` key is additive: every other byte of the result stays
    /// the same, so GitHub code scanning keeps each existing alert.
    #[test]
    fn a_finding_id_adds_one_key_and_keeps_the_rest() {
        let result = |finding_id: Option<&str>| {
            build_sarif_finding(SarifFindingInput {
                issue_code: "unused-export",
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Export 'helper' is never imported by other modules",
                uri: "src/utils.ts",
                region: Some((3, 14)),
                snippet: Some("export const helper = 1;"),
                properties: None,
                finding_id,
            })
        };
        let without = result(None);
        let mut with = result(Some("dc1:unused-export:0123456789abcdef"));

        assert!(
            without["partialFingerprints"]
                .get(SARIF_FINDING_ID_KEY)
                .is_none()
        );
        let removed = with["partialFingerprints"]
            .as_object_mut()
            .and_then(|prints| prints.remove(SARIF_FINDING_ID_KEY));
        assert_eq!(
            removed,
            Some(Value::from("dc1:unused-export:0123456789abcdef"))
        );
        assert_eq!(with, without);
    }

    /// A run whose results already differ must come out byte-identical, so the
    /// pass never churns an alert that was already unique.
    #[test]
    fn distinct_results_are_left_alone() {
        let mut results = vec![
            build_sarif_result(SarifResultInput {
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Export 'alpha' is never imported by other modules",
                uri: "src/lib.ts",
                region: Some((1, 14)),
                snippet: Some("export const alpha = 1;"),
            }),
            build_sarif_result(SarifResultInput {
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Export 'beta' is never imported by other modules",
                uri: "src/lib.ts",
                region: Some((2, 14)),
                snippet: Some("export const beta = 2;"),
            }),
        ];
        let before = results.clone();

        ensure_unique_result_fingerprints(&mut results);

        assert_eq!(results, before);
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
            finding_id: None,
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
            finding_id: None,
        });

        assert!(finding.get("properties").is_none());
    }

    #[test]
    fn append_sarif_findings_attaches_snippet_and_properties() {
        let temp = tempfile::tempdir().expect("tempdir");
        let source = temp.path().join("src.ts");
        std::fs::write(&source, "\nexport const unused = 1;\n").expect("write source");
        let mut snippets = SarifSourceSnippetCache::default();
        let mut results = Vec::new();
        let mut finding = fallow_types::output_dead_code::UnusedFileFinding::with_actions(
            fallow_types::results::UnusedFile { path: source },
        );
        finding.finding_id = Some("dc1:unused-file:0123456789abcdef".to_owned());

        append_sarif_findings(
            &mut results,
            std::slice::from_ref(&finding),
            &mut snippets,
            |finding| SarifFindingFields {
                rule_id: "fallow/unused-export",
                level: "warning",
                message: "Export is never imported".to_string(),
                uri: "src.ts".to_string(),
                region: Some((2, 1)),
                source_path: Some(finding.file.path.clone()),
                properties: Some(serde_json::json!({ "is_re_export": true })),
            },
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["ruleId"], "fallow/unused-export");
        assert_eq!(results[0]["properties"]["is_re_export"], true);
        assert!(results[0]["partialFingerprints"][SARIF_FINGERPRINT_KEY].is_string());
        assert_eq!(
            results[0]["partialFingerprints"][SARIF_FINDING_ID_KEY],
            "dc1:unused-file:0123456789abcdef"
        );
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
