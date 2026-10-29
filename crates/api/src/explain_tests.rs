use super::*;
use serde_json::json;

fn meta_value(meta: fallow_types::envelope::Meta) -> Value {
    serde_json::to_value(meta).expect("metadata should serialize")
}

fn check_meta() -> Value {
    meta_value(fallow_output::check_meta())
}

fn health_meta() -> Value {
    meta_value(fallow_output::health_meta())
}

fn dupes_meta() -> Value {
    meta_value(fallow_output::dupes_meta())
}

#[test]
fn audit_rules_resolve_by_bare_token_and_report_under_audit() {
    for rule in AUDIT_RULES {
        let bare = bare_rule_id(rule);
        assert_eq!(rule_by_token(bare).map(|found| found.id), Some(rule.id));
        assert_eq!(rule_command(rule), "audit");
        assert_eq!(rule_severity_key(rule), Some(bare));
    }
}

#[test]
fn all_rules_walks_every_registry_in_order() {
    let ids: Vec<&str> = all_rules().map(|rule| rule.id).collect();
    let expected: Vec<&str> = CHECK_RULES
        .iter()
        .chain(HEALTH_RULES)
        .chain(DUPES_RULES)
        .chain(FLAGS_RULES)
        .chain(SECURITY_RULES)
        .chain(AUDIT_RULES)
        .map(|rule| rule.id)
        .collect();
    assert_eq!(ids, expected);
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        ids.len(),
        "duplicate rule id across registries"
    );
}

#[test]
fn rule_command_names_the_owning_registry() {
    assert_eq!(rule_command(&CHECK_RULES[0]), "dead-code");
    assert_eq!(rule_command(&HEALTH_RULES[0]), "health");
    assert_eq!(rule_command(&DUPES_RULES[0]), "dupes");
    assert_eq!(rule_command(&FLAGS_RULES[0]), "flags");
    assert_eq!(rule_command(&SECURITY_RULES[0]), "security");
}

#[test]
fn severity_key_follows_the_gating_rule() {
    let by_id = |id: &str| rule_by_id(id).unwrap();
    assert_eq!(bare_rule_id(by_id("fallow/unused-export")), "unused-export");
    assert_eq!(
        bare_rule_id(by_id("security/sql-injection")),
        "sql-injection"
    );
    assert_eq!(
        rule_severity_key(by_id("fallow/unused-export")),
        Some("unused-exports")
    );
    assert_eq!(
        rule_severity_key(by_id("fallow/untested-file")),
        Some("coverage-gaps")
    );
    assert_eq!(
        rule_severity_key(by_id("security/sql-injection")),
        Some("security-sink")
    );
    assert_eq!(
        rule_severity_key(by_id("security/client-server-leak")),
        Some("security-client-server-leak")
    );
    assert_eq!(
        rule_severity_key(by_id("fallow/high-cyclomatic-complexity")),
        None
    );
    assert_eq!(
        rule_severity_key(by_id("fallow/code-duplication")),
        None,
        "a suppression token that is not a rules.* key never becomes a severity key"
    );
    let defaults = crate::schemas::default_rule_severities();
    for rule in all_rules() {
        if let Some(key) = rule_severity_key(rule) {
            assert!(
                defaults.get(key).is_some_and(serde_json::Value::is_string),
                "severity key {key} for {} is not a rules.* config key",
                rule.id
            );
        }
    }
}

#[test]
fn rule_by_id_finds_check_rule() {
    let rule = rule_by_id("fallow/unused-file").unwrap();
    assert_eq!(rule.name, "Unused Files");
}

#[test]
fn rule_by_id_finds_health_rule() {
    let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
    assert_eq!(rule.name, "High Cyclomatic Complexity");
}

#[test]
fn rule_by_id_finds_dupes_rule() {
    let rule = rule_by_id("fallow/code-duplication").unwrap();
    assert_eq!(rule.name, "Code Duplication");
}

#[test]
fn rule_by_id_finds_security_rule() {
    let rule = rule_by_id("security/tainted-sink").unwrap();
    assert_eq!(rule.name, "Tainted Sink Candidates");
}

#[test]
fn rule_by_id_finds_flags_rule() {
    let rule = rule_by_id("fallow/feature-flag").unwrap();
    assert_eq!(rule.name, "Feature Flags");
}

#[test]
fn rule_by_id_returns_none_for_unknown() {
    assert!(rule_by_id("fallow/nonexistent").is_none());
    assert!(rule_by_id("").is_none());
}

#[test]
fn rule_docs_url_format() {
    let rule = rule_by_id("fallow/unused-export").unwrap();
    let url = rule_docs_url(rule);
    assert!(url.starts_with("https://fallow.tools/docs/"));
    assert!(url.contains("unused-exports"));
}

#[test]
fn explain_output_prefers_issue_result_registry_contract_fields() {
    let output = explain_issue_type("unused-type").unwrap();
    let meta = fallow_types::issue_meta::issue_result_meta_by_code("unused-type").unwrap();
    assert_eq!(output.name, meta.meta_name);
    assert_eq!(output.summary, meta.sarif_description);
    assert_eq!(
        output.docs,
        fallow_output::rule_docs_url(meta.meta_docs_path)
    );
}

#[test]
fn dependency_override_explanations_cover_supported_package_managers() {
    let unused = explain_issue_type("unused-dependency-override").unwrap();
    assert_eq!(unused.name, "Unused dependency override");
    assert!(unused.summary.contains("Package-manager override"));
    assert!(unused.rationale.contains("npm"));
    assert!(unused.rationale.contains("Bun"));

    let misconfigured = explain_issue_type("misconfigured-dependency-override").unwrap();
    assert_eq!(misconfigured.name, "Misconfigured dependency override");
    assert!(misconfigured.summary.contains("Package-manager override"));
    assert!(misconfigured.rationale.contains("npm"));
    assert!(misconfigured.rationale.contains("Bun"));
}

#[test]
fn result_sarif_rule_ids_have_explain_metadata() {
    for contract in fallow_output::issue_output_contracts() {
        for rule_id in contract.sarif_rule_ids {
            assert!(
                rule_by_id(&rule_id).is_some(),
                "result metadata code {} has SARIF rule id {rule_id} without RuleDef",
                contract.code
            );
        }
    }
}

#[test]
fn registry_dead_code_tokens_resolve_to_explain_rules() {
    for meta in fallow_types::issue_meta::ISSUE_KIND_META {
        let Some(expected) = CHECK_RULES
            .iter()
            .find(|rule| rule.id.strip_prefix("fallow/") == Some(meta.code))
        else {
            continue;
        };
        assert_registry_token(expected, meta.code);
        for token in meta.aliases {
            assert_registry_token(expected, token);
        }
        if let Some(token) = meta.config_key {
            assert_registry_token(expected, token);
        }
        if let Some(token) = meta.mcp_issue_type {
            assert_registry_token(expected, token);
        }
        if let Some(token) = meta.filter_flag {
            assert_registry_token(expected, token);
        }
        if let Some(token) = meta.suppress_token {
            assert_registry_token(expected, token);
        }
    }
}

fn assert_registry_token(expected: &RuleDef, token: &str) {
    if !registry_token_is_unique(token) {
        return;
    }
    let actual = rule_by_token(token)
        .unwrap_or_else(|| panic!("registry token {token} did not resolve to an explain rule"));
    assert_eq!(
        actual.id, expected.id,
        "registry token {token} resolved to the wrong explain rule"
    );
}

fn registry_token_is_unique(token: &str) -> bool {
    fallow_types::issue_meta::ISSUE_KIND_META
        .iter()
        .filter(|meta| {
            CHECK_RULES
                .iter()
                .any(|rule| rule.id.strip_prefix("fallow/") == Some(meta.code))
                && fallow_types::issue_meta::issue_meta_matches_contract_token(meta, token)
        })
        .count()
        == 1
}

#[test]
fn check_rules_all_have_fallow_prefix() {
    for rule in CHECK_RULES {
        assert!(
            rule.id.starts_with("fallow/"),
            "rule {} should start with fallow/",
            rule.id
        );
    }
}

#[test]
fn check_rules_all_have_docs_path() {
    for rule in CHECK_RULES {
        assert!(
            !rule.docs_path.is_empty(),
            "rule {} should have a docs_path",
            rule.id
        );
    }
}

#[test]
fn check_rules_no_duplicate_ids() {
    let mut seen = rustc_hash::FxHashSet::default();
    for rule in CHECK_RULES
        .iter()
        .chain(HEALTH_RULES)
        .chain(DUPES_RULES)
        .chain(FLAGS_RULES)
        .chain(SECURITY_RULES)
    {
        assert!(seen.insert(rule.id), "duplicate rule id: {}", rule.id);
    }
}

#[test]
fn check_meta_has_docs_and_rules() {
    let meta = check_meta();
    assert!(meta.get("docs").is_some());
    assert!(meta.get("rules").is_some());
    let rules = meta["rules"].as_object().unwrap();
    assert_eq!(rules.len(), CHECK_RULES.len());
    assert!(rules.contains_key("unused-file"));
    assert!(rules.contains_key("unused-export"));
    assert!(rules.contains_key("unused-type"));
    assert!(rules.contains_key("unused-dependency"));
    assert!(rules.contains_key("unused-dev-dependency"));
    assert!(rules.contains_key("unused-optional-dependency"));
    assert!(rules.contains_key("unused-enum-member"));
    assert!(rules.contains_key("unused-class-member"));
    assert!(rules.contains_key("unresolved-import"));
    assert!(rules.contains_key("unlisted-dependency"));
    assert!(rules.contains_key("duplicate-export"));
    assert!(rules.contains_key("type-only-dependency"));
    assert!(rules.contains_key("circular-dependency"));
}

#[test]
fn check_meta_documents_per_finding_auto_fixable() {
    let meta = check_meta();
    let defs = meta["field_definitions"].as_object().unwrap();
    let note = defs["actions[].auto_fixable"].as_str().unwrap();
    assert!(
        note.contains("PER FINDING"),
        "auto_fixable note must call out per-finding evaluation"
    );
    assert!(
        note.contains("remove-catalog-entry"),
        "auto_fixable note must cite remove-catalog-entry per-instance flip"
    );
    assert!(
        note.contains("used_in_workspaces"),
        "auto_fixable note must cite the dependency-action per-instance flip"
    );
    assert!(
        note.contains("ignoreExports"),
        "auto_fixable note must cite the duplicate-exports config-fixable flip"
    );
    assert!(defs.contains_key("actions[]"));
}

#[test]
fn health_and_dupes_meta_share_actions_field_definitions() {
    for meta in [health_meta(), dupes_meta()] {
        let defs = meta["field_definitions"].as_object().unwrap();
        assert_eq!(
            defs["actions[]"].as_str().unwrap(),
            fallow_output::ACTIONS_FIELD_DEFINITION,
        );
        assert_eq!(
            defs["actions[].auto_fixable"].as_str().unwrap(),
            fallow_output::ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION,
        );
    }
}

#[test]
fn check_meta_rule_has_required_fields() {
    let meta = check_meta();
    let rules = meta["rules"].as_object().unwrap();
    for (key, value) in rules {
        assert!(value.get("name").is_some(), "rule {key} missing 'name'");
        assert!(
            value.get("description").is_some(),
            "rule {key} missing 'description'"
        );
        assert!(value.get("docs").is_some(), "rule {key} missing 'docs'");
    }
}

#[test]
fn health_meta_has_metrics() {
    let meta = health_meta();
    assert!(meta.get("docs").is_some());
    let metrics = meta["metrics"].as_object().unwrap();
    assert!(metrics.contains_key("cyclomatic"));
    assert!(metrics.contains_key("cognitive"));
    assert!(metrics.contains_key("maintainability_index"));
    assert!(metrics.contains_key("complexity_density"));
    assert!(metrics.contains_key("fan_in"));
    assert!(metrics.contains_key("fan_out"));
}

#[test]
fn dupes_meta_has_metrics() {
    let meta = dupes_meta();
    assert!(meta.get("docs").is_some());
    let metrics = meta["metrics"].as_object().unwrap();
    assert!(metrics.contains_key("duplication_percentage"));
    assert!(metrics.contains_key("token_count"));
    assert!(metrics.contains_key("clone_groups"));
    assert!(metrics.contains_key("clone_families"));
}

#[test]
fn coverage_setup_meta_has_docs_fields_enums_and_warnings() {
    let meta = coverage_setup_meta();
    assert_eq!(meta["docs_url"], fallow_output::COVERAGE_SETUP_DOCS);
    assert!(
        meta["field_definitions"]
            .as_object()
            .unwrap()
            .contains_key("members[]")
    );
    assert!(
        meta["field_definitions"]
            .as_object()
            .unwrap()
            .contains_key("config_written")
    );
    assert!(
        meta["field_definitions"]
            .as_object()
            .unwrap()
            .contains_key("members[].package_manager")
    );
    assert!(
        meta["field_definitions"]
            .as_object()
            .unwrap()
            .contains_key("members[].warnings")
    );
    assert!(
        meta["enums"]
            .as_object()
            .unwrap()
            .contains_key("framework_detected")
    );
    assert!(
        meta["warnings"]
            .as_object()
            .unwrap()
            .contains_key("No runtime workspace members were detected")
    );
    assert!(
        meta["warnings"]
            .as_object()
            .unwrap()
            .contains_key("Package manager was not detected")
    );
}

#[test]
fn coverage_analyze_meta_documents_data_source_and_action_vocabulary() {
    let meta = coverage_analyze_meta();
    assert_eq!(meta["docs_url"], fallow_output::COVERAGE_ANALYZE_DOCS);
    let fields = meta["field_definitions"].as_object().unwrap();
    assert!(fields.contains_key("runtime_coverage.summary.data_source"));
    assert!(fields.contains_key("runtime_coverage.summary.last_received_at"));
    assert!(fields.contains_key("runtime_coverage.findings[].evidence.test_coverage"));
    assert!(fields.contains_key("runtime_coverage.findings[].actions[].type"));
    let enums = meta["enums"].as_object().unwrap();
    assert_eq!(enums["data_source"], json!(["local", "cloud"]));
    assert_eq!(enums["test_coverage"], json!(["covered", "not_covered"]));
    assert_eq!(enums["v8_tracking"], json!(["tracked", "untracked"]));
    assert_eq!(
        enums["action_type"],
        json!(["delete-cold-code", "review-runtime"])
    );
    let warnings = meta["warnings"].as_object().unwrap();
    assert!(warnings.contains_key("cloud_functions_unmatched"));
}

#[test]
fn health_rules_all_have_fallow_prefix() {
    for rule in HEALTH_RULES {
        assert!(
            rule.id.starts_with("fallow/"),
            "health rule {} should start with fallow/",
            rule.id
        );
    }
}

#[test]
fn health_rules_all_have_docs_path() {
    for rule in HEALTH_RULES {
        assert!(
            !rule.docs_path.is_empty(),
            "health rule {} should have a docs_path",
            rule.id
        );
    }
}

#[test]
fn health_rules_all_have_non_empty_fields() {
    for rule in HEALTH_RULES {
        assert!(
            !rule.name.is_empty(),
            "health rule {} missing name",
            rule.id
        );
        assert!(
            !rule.short.is_empty(),
            "health rule {} missing short description",
            rule.id
        );
        assert!(
            !rule.full.is_empty(),
            "health rule {} missing full description",
            rule.id
        );
    }
}

#[test]
fn dupes_rules_all_have_fallow_prefix() {
    for rule in DUPES_RULES {
        assert!(
            rule.id.starts_with("fallow/"),
            "dupes rule {} should start with fallow/",
            rule.id
        );
    }
}

#[test]
fn dupes_rules_all_have_docs_path() {
    for rule in DUPES_RULES {
        assert!(
            !rule.docs_path.is_empty(),
            "dupes rule {} should have a docs_path",
            rule.id
        );
    }
}

#[test]
fn dupes_rules_all_have_non_empty_fields() {
    for rule in DUPES_RULES {
        assert!(!rule.name.is_empty(), "dupes rule {} missing name", rule.id);
        assert!(
            !rule.short.is_empty(),
            "dupes rule {} missing short description",
            rule.id
        );
        assert!(
            !rule.full.is_empty(),
            "dupes rule {} missing full description",
            rule.id
        );
    }
}

#[test]
fn security_rules_all_have_security_prefix() {
    for rule in SECURITY_RULES {
        assert!(
            rule.id.starts_with("security/"),
            "security rule {} should start with security/",
            rule.id
        );
    }
}

#[test]
fn security_rules_all_have_docs_path() {
    for rule in SECURITY_RULES {
        assert_eq!(
            rule.docs_path, "cli/security",
            "security rule {} should point at security docs",
            rule.id
        );
    }
}

#[test]
fn security_rules_all_have_non_empty_fields() {
    for rule in SECURITY_RULES {
        assert!(
            !rule.name.is_empty(),
            "security rule {} missing name",
            rule.id
        );
        assert!(
            !rule.short.is_empty(),
            "security rule {} missing short description",
            rule.id
        );
        assert!(
            !rule.full.is_empty(),
            "security rule {} missing full description",
            rule.id
        );
    }
}

#[test]
fn check_rules_all_have_non_empty_fields() {
    for rule in CHECK_RULES {
        assert!(!rule.name.is_empty(), "check rule {} missing name", rule.id);
        assert!(
            !rule.short.is_empty(),
            "check rule {} missing short description",
            rule.id
        );
        assert!(
            !rule.full.is_empty(),
            "check rule {} missing full description",
            rule.id
        );
    }
}

#[test]
fn rule_docs_url_health_rule() {
    let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
    let url = rule_docs_url(rule);
    assert!(url.starts_with("https://fallow.tools/docs/"));
    assert!(url.contains("health"));
}

#[test]
fn rule_docs_url_dupes_rule() {
    let rule = rule_by_id("fallow/code-duplication").unwrap();
    let url = rule_docs_url(rule);
    assert!(url.starts_with("https://fallow.tools/docs/"));
    assert!(url.contains("duplication"));
}

#[test]
fn rule_docs_url_security_rule() {
    let rule = rule_by_id("security/sql-injection").unwrap();
    let url = rule_docs_url(rule);
    assert_eq!(url, "https://fallow.tools/docs/cli/security/");
}

#[test]
fn health_meta_all_metrics_have_name_and_description() {
    let meta = health_meta();
    let metrics = meta["metrics"].as_object().unwrap();
    for (key, value) in metrics {
        assert!(
            value.get("name").is_some(),
            "health metric {key} missing 'name'"
        );
        assert!(
            value.get("description").is_some(),
            "health metric {key} missing 'description'"
        );
        assert!(
            value.get("interpretation").is_some(),
            "health metric {key} missing 'interpretation'"
        );
    }
}

#[test]
fn health_meta_has_all_expected_metrics() {
    let meta = health_meta();
    let metrics = meta["metrics"].as_object().unwrap();
    let expected = [
        "cyclomatic",
        "cognitive",
        "line_count",
        "lines",
        "maintainability_index",
        "complexity_density",
        "dead_code_ratio",
        "fan_in",
        "fan_out",
        "score",
        "weighted_commits",
        "median_files_per_commit",
        "median_lines_per_commit",
        "degree",
        "trend",
        "priority",
        "efficiency",
        "effort",
        "confidence",
        "bus_factor",
        "contributor_count",
        "share",
        "stale_days",
        "drift",
        "unowned",
        "runtime_coverage_verdict",
        "runtime_coverage_state",
        "runtime_coverage_confidence",
        "production_invocations",
        "percent_dead_in_production",
    ];
    for key in &expected {
        assert!(
            metrics.contains_key(*key),
            "health_meta missing expected metric: {key}"
        );
    }
}

#[test]
fn dupes_meta_all_metrics_have_name_and_description() {
    let meta = dupes_meta();
    let metrics = meta["metrics"].as_object().unwrap();
    for (key, value) in metrics {
        assert!(
            value.get("name").is_some(),
            "dupes metric {key} missing 'name'"
        );
        assert!(
            value.get("description").is_some(),
            "dupes metric {key} missing 'description'"
        );
    }
}

#[test]
fn dupes_meta_has_line_count() {
    let meta = dupes_meta();
    let metrics = meta["metrics"].as_object().unwrap();
    assert!(metrics.contains_key("line_count"));
}

#[test]
fn check_docs_url_valid() {
    assert!(fallow_output::CHECK_DOCS.starts_with("https://"));
    assert!(fallow_output::CHECK_DOCS.contains("dead-code"));
}

#[test]
fn health_docs_url_valid() {
    assert!(fallow_output::HEALTH_DOCS.starts_with("https://"));
    assert!(fallow_output::HEALTH_DOCS.contains("health"));
}

#[test]
fn dupes_docs_url_valid() {
    assert!(fallow_output::DUPES_DOCS.starts_with("https://"));
    assert!(fallow_output::DUPES_DOCS.contains("dupes"));
}

#[test]
fn check_meta_docs_url_matches_constant() {
    let meta = check_meta();
    assert_eq!(meta["docs"].as_str().unwrap(), fallow_output::CHECK_DOCS);
}

#[test]
fn health_meta_docs_url_matches_constant() {
    let meta = health_meta();
    assert_eq!(meta["docs"].as_str().unwrap(), fallow_output::HEALTH_DOCS);
}

#[test]
fn dupes_meta_docs_url_matches_constant() {
    let meta = dupes_meta();
    assert_eq!(meta["docs"].as_str().unwrap(), fallow_output::DUPES_DOCS);
}

#[test]
fn check_rules_count() {
    assert_eq!(CHECK_RULES.len(), 52);
}

#[test]
fn health_rules_count() {
    assert_eq!(HEALTH_RULES.len(), 22);
}

#[test]
fn dupes_rules_count() {
    assert_eq!(DUPES_RULES.len(), 1);
}

#[test]
fn flags_rules_count() {
    assert_eq!(FLAGS_RULES.len(), 2);
}

#[test]
fn security_rules_count() {
    assert_eq!(
        SECURITY_RULES.len(),
        matcher_entries_from_security_catalogue().len() + 3
    );
}

#[test]
fn security_rules_cover_every_catalogue_matcher() {
    let mut rule_ids = rustc_hash::FxHashSet::default();
    for rule in SECURITY_RULES {
        rule_ids.insert(rule.id);
    }

    for matcher in matcher_entries_from_security_catalogue() {
        let rule_id = format!("security/{}", matcher.id);
        assert!(
            rule_ids.contains(rule_id.as_str()),
            "security matcher {} has no explain rule",
            matcher.id
        );
    }
}

#[test]
fn security_catalogue_rules_match_catalogue_title_and_cwe() {
    for matcher in matcher_entries_from_security_catalogue() {
        let rule_id = format!("security/{}", matcher.id);
        let rule = rule_by_id(&rule_id)
            .unwrap_or_else(|| panic!("security matcher {} has no explain rule", matcher.id));
        let cwe = format!("CWE-{}", matcher.cwe);
        assert_eq!(
            rule.name, matcher.title,
            "security matcher {} has stale explain title",
            matcher.id
        );
        assert!(
            rule.short.contains(&cwe),
            "security matcher {} explain summary does not mention {cwe}",
            matcher.id
        );
        assert!(
            rule.full.contains(&cwe),
            "security matcher {} explain rationale does not mention {cwe}",
            matcher.id
        );
    }
}

/// Every registered rule must declare a category. The PR/MR sticky
/// renderer reads this via `category_for_rule`; without an entry the
/// rule silently falls into the "Dead code" default and reviewers may
/// see it grouped under an unexpected section. Catching this here is
/// the same pattern as `check_rules_count` for the rule count itself.
#[test]
fn every_rule_declares_a_category() {
    let allowed = [
        "Dead code",
        "Dependencies",
        "Duplication",
        "Health",
        "Architecture",
        "Suppressions",
        "Security",
        "Policy",
        "Flags",
    ];
    for rule in CHECK_RULES
        .iter()
        .chain(HEALTH_RULES)
        .chain(DUPES_RULES)
        .chain(FLAGS_RULES)
        .chain(SECURITY_RULES)
    {
        assert!(
            !rule.category.is_empty(),
            "rule {} has empty category",
            rule.id
        );
        assert!(
            allowed.contains(&rule.category),
            "rule {} has unrecognised category {:?}; add to allowlist or pick from {:?}",
            rule.id,
            rule.category,
            allowed
        );
    }
}

#[derive(Debug)]
struct MatcherEntry {
    id: &'static str,
    title: &'static str,
    cwe: &'static str,
}

fn matcher_entries_from_security_catalogue() -> Vec<MatcherEntry> {
    let toml = include_str!("../../security/data/security_matchers.toml");
    let mut entries = Vec::new();
    let mut in_matcher = false;
    let mut id = None;
    let mut title = None;
    let mut cwe = None;

    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed == "[[matcher]]" {
            if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
                entries.push(MatcherEntry { id, title, cwe });
            }
            in_matcher = true;
            continue;
        }
        if trimmed.starts_with("[[") {
            if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
                entries.push(MatcherEntry { id, title, cwe });
            }
            in_matcher = false;
            continue;
        }
        if !in_matcher {
            continue;
        }
        if let Some(value) = trimmed
            .strip_prefix("id = \"")
            .and_then(|value| value.strip_suffix('"'))
        {
            id = Some(value);
        } else if let Some(value) = trimmed
            .strip_prefix("title = \"")
            .and_then(|value| value.strip_suffix('"'))
        {
            title = Some(value);
        } else if let Some(value) = trimmed.strip_prefix("cwe = ") {
            cwe = Some(value);
        }
    }

    if let (Some(id), Some(title), Some(cwe)) = (id.take(), title.take(), cwe.take()) {
        entries.push(MatcherEntry { id, title, cwe });
    }

    let mut seen = rustc_hash::FxHashSet::default();
    entries
        .into_iter()
        .filter(|entry| seen.insert(entry.id))
        .collect()
}

/// A one-edit typo used to get the same hardcoded example list as a
/// completely novel token, so the caller never saw the token that was
/// actually one edit away.
#[test]
fn unknown_explain_error_names_the_token_one_edit_away() {
    let error = unknown_explain_error("unused-exportz");
    assert_eq!(error.code.as_deref(), Some("unknown_issue_type"));
    assert!(error.message.contains("unused-export"), "{}", error.message);
    assert_eq!(error.help.as_deref(), Some("fallow explain unused-export"));
    assert!(
        rule_by_token("unused-export").is_some(),
        "suggestion must resolve"
    );
}

/// A token that resembles nothing registered must stay silent rather than
/// point at an unrelated rule.
#[test]
fn unknown_explain_error_keeps_the_example_list_for_a_novel_token() {
    let error = unknown_explain_error("banana-split-detector");
    assert!(!error.message.contains("Did you mean"), "{}", error.message);
    assert_eq!(error.help, None);
}
