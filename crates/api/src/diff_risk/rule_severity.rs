//! Rule severities that a change of the Fallow config made less strict.
//!
//! The detector compares the `rules` table and the `overrides` list that the
//! base and head config chains resolve to (see the `config_chain` module). A
//! side without a config file uses the default severities. A key that an
//! opaque source can set on either side does not compare.

use std::path::PathBuf;
use std::sync::LazyLock;

use fallow_config::{PartialRulesConfig, RulesConfig, Severity};
use rustc_hash::FxHashMap;
use serde_json::{Map, Value};

use super::config_chain::{ConfigChain, Lookup};
use super::config_change::{LineMatch, Weakening};
use fallow_output::WeakenedTestsKind;

/// The `weakened-tests` severity of the base chain, or `None` when an opaque
/// source can set it.
#[must_use]
pub fn weakened_tests_severity(chain: &ConfigChain) -> Option<Severity> {
    match rule_lookup(chain, "weakened-tests", &key_names("weakened-tests")) {
        Resolved::Known(severity, _) => Some(severity),
        Resolved::Unknown => None,
    }
}

/// Rule severities of the head chain that are less strict than in the base
/// chain.
#[must_use]
pub fn rule_weakenings(base: &ConfigChain, head: &ConfigChain) -> Vec<Weakening> {
    let mut changes = top_level_changes(base, head);
    changes.extend(override_changes(base, head));
    changes
}

/// A rule severity after the chain resolves it.
enum Resolved {
    /// The severity and the file that sets it, `None` for the default.
    Known(Severity, Option<PathBuf>),
    Unknown,
}

fn rule_lookup(chain: &ConfigChain, key: &str, names: &[&str]) -> Resolved {
    let lookup = chain.lookup(|map| {
        let rules = map.get("rules")?.as_object()?;
        names.iter().find_map(|name| rules.get(*name))
    });
    match lookup {
        Lookup::Unknown => Resolved::Unknown,
        Lookup::Unset => default_severity(key).map_or(Resolved::Unknown, |severity| {
            Resolved::Known(severity, None)
        }),
        Lookup::Set { value, abs } => serde_json::from_value(value.clone())
            .map_or(Resolved::Unknown, |severity| {
                Resolved::Known(severity, Some(abs.to_path_buf()))
            }),
    }
}

/// The default severity of each rule, by canonical name.
static DEFAULT_SEVERITIES: LazyLock<FxHashMap<String, Severity>> =
    LazyLock::new(|| severities(&RulesConfig::default()));

/// The alias names of each rule, by canonical name.
static ALIASES: LazyLock<FxHashMap<String, Vec<&'static str>>> = LazyLock::new(|| {
    let mut aliases: FxHashMap<String, Vec<&'static str>> = FxHashMap::default();
    for name in fallow_config::KNOWN_RULE_NAMES {
        let mut probe = Map::new();
        probe.insert((*name).to_owned(), Value::String("warn".to_owned()));
        for (canonical, _) in partial_severities(Value::Object(probe)).unwrap_or_default() {
            if canonical != *name {
                aliases.entry(canonical).or_default().push(name);
            }
        }
    }
    aliases
});

fn default_severity(key: &str) -> Option<Severity> {
    DEFAULT_SEVERITIES.get(key).copied()
}

fn top_level_changes(base: &ConfigChain, head: &ConfigChain) -> Vec<Weakening> {
    let mut keys: Vec<&String> = DEFAULT_SEVERITIES.keys().collect();
    keys.sort();
    let mut changes = Vec::new();
    for key in keys {
        let names = key_names(key);
        let (Resolved::Known(base_severity, base_file), Resolved::Known(head_severity, head_file)) = (
            rule_lookup(base, key, &names),
            rule_lookup(head, key, &names),
        ) else {
            continue;
        };
        if strictness(head_severity) < strictness(base_severity) {
            changes.push(Weakening {
                kind: WeakenedTestsKind::RuleSeverity,
                message: rule_message(&format!("{key}: {base_severity} -> {head_severity}")),
                evidence: format!("{key}: {base_severity} -> {head_severity}"),
                head_file,
                base_file,
                line_match: LineMatch::Rule {
                    key: key.clone(),
                    head: head_severity,
                },
            });
        }
    }
    changes
}

/// The `overrides` list of a chain, `None` when an opaque source can set it.
fn overrides_lookup(chain: &ConfigChain) -> Option<(Vec<OverrideRules>, Option<PathBuf>)> {
    match chain.lookup(|map| map.get("overrides")) {
        Lookup::Unknown => None,
        Lookup::Unset => Some((Vec::new(), None)),
        Lookup::Set { value, abs } => Some((
            value
                .as_array()
                .map(|entries| entries.iter().filter_map(override_rules).collect())
                .unwrap_or_default(),
            Some(abs.to_path_buf()),
        )),
    }
}

/// Override entries compare with the base entry that has the same `files`
/// globs, else with the base top level.
fn override_changes(base: &ConfigChain, head: &ConfigChain) -> Vec<Weakening> {
    let (Some((base_overrides, base_file)), Some((head_overrides, head_file))) =
        (overrides_lookup(base), overrides_lookup(head))
    else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for entry in &head_overrides {
        for (key, head_severity) in &entry.rules {
            let base_entry = base_overrides
                .iter()
                .rev()
                .filter(|base_entry| base_entry.files == entry.files)
                .find_map(|base_entry| base_entry.severity(key));
            let (base_severity, from) = match base_entry {
                Some(severity) => (severity, base_file.clone()),
                None => match rule_lookup(base, key, &key_names(key)) {
                    Resolved::Known(severity, file) => (severity, file),
                    Resolved::Unknown => continue,
                },
            };
            if strictness(*head_severity) < strictness(base_severity) {
                let evidence = format!(
                    "{key}: {base_severity} -> {head_severity} for {}",
                    entry.files.join(", ")
                );
                changes.push(Weakening {
                    kind: WeakenedTestsKind::RuleSeverity,
                    message: rule_message(&evidence),
                    evidence,
                    head_file: head_file.clone(),
                    base_file: from,
                    line_match: LineMatch::Rule {
                        key: key.clone(),
                        head: *head_severity,
                    },
                });
            }
        }
    }
    changes
}

fn rule_message(evidence: &str) -> String {
    format!("The change makes a rule less strict ({evidence})")
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
pub(super) fn key_names(key: &str) -> Vec<&str> {
    let mut names = vec![key];
    if let Some(aliases) = ALIASES.get(key) {
        names.extend(aliases.iter().copied());
    }
    names
}

/// Whether a config line holds the rule `key` as a JSON key or a TOML key.
pub(super) fn names_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.contains(&format!("\"{key}\""))
        || trimmed
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_risk::config_change::test_support::single_file_hits;

    fn evidence(base: &str, head: &str) -> Vec<(u32, String)> {
        single_file_hits(base, head)
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
        let chain = ConfigChain::from_text(
            std::path::Path::new("/r/.fallowrc.json"),
            "\u{feff}{\"rules\": {\"weakened-tests\": \"error\"}}",
        )
        .expect("chain");
        assert_eq!(weakened_tests_severity(&chain), Some(Severity::Error));
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
}
