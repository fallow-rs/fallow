//! Rule severities that a change of the Fallow config file made less strict.
//!
//! The detector compares the `rules` table and the `overrides[].rules`
//! entries of the base and head text of the config file at the analysis root.
//! A missing, empty or comment-only file reads as an empty config, so a new or
//! deleted file compares with the default severities. The detector reads only
//! that file. When either side has an `extends` entry, a rule that one side
//! does not set can take its value from another file, so only a rule that both
//! sides set compares.

use fallow_config::{PartialRulesConfig, RulesConfig, Severity};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Map, Value};

use super::text::{LineHit, added_lines};

/// Whether `rel_path` names a Fallow config file at the analysis root. The
/// config loader reads the first config file that it finds from the root
/// upward, so a config file in a subdirectory has no effect.
#[must_use]
pub fn is_fallow_config_path(rel_path: &str) -> bool {
    !rel_path.contains('/') && fallow_config::CONFIG_FILE_NAMES.contains(&rel_path)
}

/// The `weakened-tests` severity of the top-level `rules` table, or `None`
/// when the text does not parse.
#[must_use]
pub fn weakened_tests_severity(text: &str) -> Option<Severity> {
    let config = ConfigRules::parse(text)?;
    Some(config.rules.weakened_tests)
}

/// Rule severities of the head text that are less strict than in the base
/// text. A text that does not parse gives no hit.
#[must_use]
pub fn weakened_rule_hits(base: &str, head: &str) -> Vec<LineHit> {
    let (Some(base_config), Some(head_config)) =
        (ConfigRules::parse(base), ConfigRules::parse(head))
    else {
        return Vec::new();
    };
    let mut changes = top_level_changes(&base_config, &head_config);
    changes.extend(override_changes(&base_config, &head_config));
    attach_lines(base, head, changes)
}

fn top_level_changes(base: &ConfigRules, head: &ConfigRules) -> Vec<Change> {
    let base_rules = severities(&base.rules);
    let inherits = base.extends || head.extends;
    let mut changes = Vec::new();
    for (key, head_severity) in severities(&head.rules) {
        if inherits && !(base.explicit.contains(&key) && head.explicit.contains(&key)) {
            continue;
        }
        let Some(base_severity) = base_rules.get(&key).copied() else {
            continue;
        };
        if strictness(head_severity) < strictness(base_severity) {
            changes.push(Change {
                key,
                base: base_severity,
                head: head_severity,
                files: None,
            });
        }
    }
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    changes
}

/// Override entries compare with the base entry that has the same `files`
/// globs, else with the base top level.
fn override_changes(base: &ConfigRules, head: &ConfigRules) -> Vec<Change> {
    let base_rules = severities(&base.rules);
    let inherits = base.extends || head.extends;
    let mut changes = Vec::new();
    for entry in &head.overrides {
        for (key, head_severity) in &entry.rules {
            let base_top_level = (!inherits || base.explicit.contains(key))
                .then(|| base_rules.get(key).copied())
                .flatten();
            let base_severity = base
                .overrides
                .iter()
                .rev()
                .filter(|base_entry| base_entry.files == entry.files)
                .find_map(|base_entry| base_entry.severity(key))
                .or(base_top_level);
            let Some(base_severity) = base_severity else {
                continue;
            };
            if strictness(*head_severity) < strictness(base_severity) {
                changes.push(Change {
                    key: key.clone(),
                    base: base_severity,
                    head: *head_severity,
                    files: Some(entry.files.join(", ")),
                });
            }
        }
    }
    changes
}

/// Give each change the first free added line that names its rule, and
/// prefer a line that also holds the new severity.
fn attach_lines(base: &str, head: &str, changes: Vec<Change>) -> Vec<LineHit> {
    let added = added_lines(base, head);
    let mut used_lines = FxHashSet::default();
    changes
        .into_iter()
        .map(|change| {
            let names = key_names(&change.key);
            let free = |line: &u32| !used_lines.contains(line);
            let names_rule = |text: &str| names.iter().any(|name| names_key(text, name));
            let head_value = format!("\"{}\"", change.head);
            let line = added
                .iter()
                .find(|(line, text)| free(line) && names_rule(text) && text.contains(&head_value))
                .or_else(|| {
                    added
                        .iter()
                        .find(|(line, text)| free(line) && names_rule(text))
                })
                .map(|(line, _)| *line);
            if let Some(line) = line {
                used_lines.insert(line);
            }
            LineHit {
                line: line.unwrap_or(0),
                evidence: change.evidence(),
            }
        })
        .collect()
}

/// One rule whose severity the change lowered.
struct Change {
    key: String,
    base: Severity,
    head: Severity,
    /// The `files` globs of an `overrides` entry, `None` for the top level.
    files: Option<String>,
}

impl Change {
    fn evidence(&self) -> String {
        let mut evidence = format!("{}: {} -> {}", self.key, self.base, self.head);
        if let Some(files) = &self.files {
            evidence.push_str(" for ");
            evidence.push_str(files);
        }
        evidence
    }
}

/// The rule parts of one config text.
struct ConfigRules {
    /// The top-level rules, with the defaults for rules the text does not set.
    rules: RulesConfig,
    /// Canonical names of the rules that the top-level table sets.
    explicit: FxHashSet<String>,
    /// Whether the text has a non-empty `extends` entry.
    extends: bool,
    overrides: Vec<OverrideRules>,
}

/// The rule part of one `overrides` entry.
struct OverrideRules {
    /// The `files` globs, sorted, so an entry that only reorders them matches.
    files: Vec<String>,
    /// Canonical rule names and the severities that the entry sets.
    rules: Vec<(String, Severity)>,
}

impl OverrideRules {
    fn severity(&self, key: &str) -> Option<Severity> {
        self.rules
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, severity)| *severity)
    }
}

impl ConfigRules {
    fn parse(text: &str) -> Option<Self> {
        let value = parse_value(text)?;
        let object = value.as_object()?;
        let rules_value = object
            .get("rules")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new()));
        let rules: RulesConfig = serde_json::from_value(rules_value.clone()).ok()?;
        let explicit = partial_severities(rules_value)?
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let extends = object.get("extends").is_some_and(|extends| match extends {
            Value::String(entry) => !entry.is_empty(),
            Value::Array(entries) => !entries.is_empty(),
            _ => false,
        });
        let overrides = object
            .get("overrides")
            .and_then(Value::as_array)
            .map(|entries| entries.iter().filter_map(override_rules).collect())
            .unwrap_or_default();
        Some(Self {
            rules,
            explicit,
            extends,
            overrides,
        })
    }
}

/// Parse a config text as JSONC, else as TOML. The text decides the format,
/// not the file name, so a rename from `.fallowrc.json` to `fallow.toml`
/// still parses the base text. An empty or comment-only text is an empty
/// config.
fn parse_value(text: &str) -> Option<Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match fallow_config::jsonc::parse_to_value::<Option<Value>>(text) {
        Ok(Some(value)) => Some(value),
        Ok(None) => Some(Value::Object(Map::new())),
        Err(_) => fallow_config::toml_input::from_str(text).ok(),
    }
}

fn override_rules(entry: &Value) -> Option<OverrideRules> {
    let mut files: Vec<String> = entry
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|glob| glob.as_str().map(str::to_owned))
        .collect();
    files.sort();
    let rules = partial_severities(entry.get("rules")?.clone())?;
    Some(OverrideRules { files, rules })
}

/// The rules that a partial rules table sets, by canonical name. An alias
/// key reads as its canonical name.
fn partial_severities(rules: Value) -> Option<Vec<(String, Severity)>> {
    let partial: PartialRulesConfig = serde_json::from_value(rules).ok()?;
    let Value::Object(set) = serde_json::to_value(partial).ok()? else {
        return None;
    };
    Some(severity_pairs(set))
}

/// Every rule of a full rules table, by canonical name.
fn severities(rules: &RulesConfig) -> FxHashMap<String, Severity> {
    let Ok(Value::Object(map)) = serde_json::to_value(rules) else {
        return FxHashMap::default();
    };
    severity_pairs(map).into_iter().collect()
}

fn severity_pairs(map: Map<String, Value>) -> Vec<(String, Severity)> {
    let mut pairs: Vec<(String, Severity)> = map
        .into_iter()
        .filter_map(|(key, value)| {
            let severity = serde_json::from_value(value).ok()?;
            Some((key, severity))
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

/// A higher value is a stricter severity.
pub(super) const fn strictness(severity: Severity) -> u8 {
    match severity {
        Severity::Off => 0,
        Severity::Warn => 1,
        Severity::Error => 2,
    }
}

/// The canonical name `key` and every alias that reads as it.
fn key_names(key: &str) -> Vec<&str> {
    let mut names = vec![key];
    for name in fallow_config::KNOWN_RULE_NAMES {
        if *name == key {
            continue;
        }
        let mut probe = Map::new();
        probe.insert((*name).to_owned(), Value::String("warn".to_owned()));
        let canonical = partial_severities(Value::Object(probe));
        if canonical.is_some_and(|pairs| pairs.iter().any(|(canonical, _)| canonical == key)) {
            names.push(name);
        }
    }
    names
}

/// Whether a config line holds the rule `key` as a JSON key or a TOML key.
fn names_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.contains(&format!("\"{key}\""))
        || trimmed
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(base: &str, head: &str) -> Vec<(u32, String)> {
        weakened_rule_hits(base, head)
            .into_iter()
            .map(|hit| (hit.line, hit.evidence))
            .collect()
    }

    #[test]
    fn a_lower_top_level_severity_reports_on_its_line() {
        let base = "{\n  \"rules\": {\n    \"weakened-tests\": \"error\"\n  }\n}\n";
        let head = "{\n  \"rules\": {\n    \"weakened-tests\": \"off\"\n  }\n}\n";
        assert_eq!(
            evidence(base, head),
            vec![(3, "weakened-tests: error -> off".to_owned())]
        );
    }

    #[test]
    fn a_removed_rule_reports_when_its_default_is_weaker() {
        let base = r#"{"rules": {"weakened-tests": "error", "unused-files": "error"}}"#;
        let head = r#"{"rules": {}}"#;
        assert_eq!(
            evidence(base, head),
            vec![(0, "weakened-tests: error -> warn".to_owned())]
        );
    }

    #[test]
    fn a_stricter_or_equal_severity_does_not_report() {
        let base = r#"{"rules": {"unused-exports": "warn"}}"#;
        let head = r#"{"rules": {"unused-exports": "error", "weakened-tests": "warn"}}"#;
        assert!(evidence(base, head).is_empty());
    }

    #[test]
    fn an_alias_key_reads_as_its_rule() {
        let base = r#"{"rules": {"unused-exports": "error"}}"#;
        let head = r#"{"rules": {"unused-export": "off"}}"#;
        assert_eq!(
            evidence(base, head),
            vec![(1, "unused-exports: error -> off".to_owned())]
        );
    }

    #[test]
    fn a_new_override_entry_compares_with_the_base_top_level() {
        let base = "{\"rules\": {\"weakened-tests\": \"error\"}}\n";
        let head = "{\n\"rules\": {\"weakened-tests\": \"error\"},\n\"overrides\": [{\"files\": [\"src/**\"], \"rules\": {\n\"weakened-tests\": \"off\"\n}}]\n}\n";
        assert_eq!(
            evidence(base, head),
            vec![(4, "weakened-tests: error -> off for src/**".to_owned())]
        );
    }

    #[test]
    fn an_unchanged_override_entry_does_not_report() {
        let config =
            r#"{"overrides": [{"files": ["b/**", "a/**"], "rules": {"unused-files": "off"}}]}"#;
        let reordered =
            r#"{"overrides": [{"files": ["a/**", "b/**"], "rules": {"unused-files": "off"}}]}"#;
        assert!(evidence(config, reordered).is_empty());
    }

    #[test]
    fn toml_configs_compare_too() {
        let base = "[rules]\nunused-exports = \"error\"\n";
        let head = "[rules]\nunused-exports = \"warn\"\n";
        assert_eq!(
            evidence(base, head),
            vec![(2, "unused-exports: error -> warn".to_owned())]
        );
    }

    #[test]
    fn with_extends_only_rules_that_both_sides_set_compare() {
        let base = r#"{"extends": "./base.json", "rules": {"unused-exports": "error", "unused-files": "error"}}"#;
        let head = r#"{"extends": "./base.json", "rules": {"unused-exports": "off"}}"#;
        assert_eq!(
            evidence(base, head),
            vec![(1, "unused-exports: error -> off".to_owned())]
        );
    }

    #[test]
    fn a_new_or_emptied_config_compares_with_the_defaults() {
        assert_eq!(
            evidence("", r#"{"rules": {"weakened-tests": "off"}}"#),
            vec![(1, "weakened-tests: warn -> off".to_owned())]
        );
        assert_eq!(
            evidence(
                r#"{"rules": {"unused-files": "error", "weakened-tests": "error"}}"#,
                "// no rules\n"
            ),
            vec![(0, "weakened-tests: error -> warn".to_owned())]
        );
        assert_eq!(
            weakened_tests_severity("\u{feff}{\"rules\": {\"weakened-tests\": \"error\"}}"),
            Some(Severity::Error)
        );
    }

    #[test]
    fn a_renamed_config_parses_each_side_by_its_text() {
        let base = r#"{"rules": {"unused-exports": "error"}}"#;
        let head = "[rules]\nunused-exports = \"off\"\n";
        assert_eq!(
            evidence(base, head),
            vec![(2, "unused-exports: error -> off".to_owned())]
        );
    }

    #[test]
    fn with_extends_an_override_compares_only_with_a_rule_the_base_sets() {
        let base = r#"{"extends": "./base.json"}"#;
        let head = r#"{"extends": "./base.json", "overrides": [{"files": ["src/**"], "rules": {"unused-files": "warn"}}]}"#;
        assert!(evidence(base, head).is_empty());
    }

    #[test]
    fn a_config_that_does_not_parse_gives_no_hit() {
        assert!(evidence(r#"{"rules": {}}"#, "{ not json").is_empty());
    }

    #[test]
    fn config_paths_match_fallow_config_names_only() {
        assert!(is_fallow_config_path(".fallowrc.json"));
        assert!(is_fallow_config_path("fallow.toml"));
        assert!(!is_fallow_config_path("packages/app/fallow.toml"));
        assert!(!is_fallow_config_path("vitest.config.ts"));
        assert!(!is_fallow_config_path("package.json"));
    }
}
