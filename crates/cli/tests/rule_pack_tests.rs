#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::process::Command;

use common::{CommandOutput, fallow_bin, parse_json};

fn run_rule_pack(root: &std::path::Path, args: &[&str]) -> CommandOutput {
    common::run_fallow_in_root("rule-pack", root, args)
}

fn write_project(root: &std::path::Path) {
    std::fs::write(root.join("package.json"), "{\"name\":\"t\"}\n").expect("write package.json");
    std::fs::write(root.join(".fallowrc.json"), "{\n  \"rules\": {}\n}\n")
        .expect("write fallow config");
}

fn write_project_with_rule_pack(root: &std::path::Path) {
    write_project(root);
    std::fs::create_dir_all(root.join("rule-packs")).expect("create rule-packs dir");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "rules": {
    "policy-violation": "warn"
  },
  "rulePacks": ["rule-packs/team-policy.jsonc"]
}
"#,
    )
    .expect("write config");
    std::fs::write(
        root.join("rule-packs/team-policy.jsonc"),
        r#"{
  "version": 1,
  "name": "team-policy",
  "description": "Repository policy guardrails",
  "rules": [
    {
      "id": "no-moment",
      "kind": "banned-import",
      "specifiers": ["moment"],
      "severity": "error",
      "message": "Use date-fns."
    },
    {
      "id": "no-network",
      "kind": "banned-effect",
      "effects": ["network"],
      "files": ["src/domain/**"]
    }
  ]
}
"#,
    )
    .expect("write rule pack");
}

fn write_policy_test_project(root: &std::path::Path, banned_specifier: &str) {
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(root.join("packs")).expect("create packs dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "t",
  "main": "src/index.ts"
}
"#,
    )
    .expect("write package.json");
    std::fs::write(root.join(".fallowrc.json"), "{\n  \"rules\": {}\n}\n")
        .expect("write fallow config");
    std::fs::write(
        root.join("src/index.ts"),
        "import value from 'moment';\nconsole.log(value);\n",
    )
    .expect("write source");
    std::fs::write(
        root.join("packs/p.jsonc"),
        format!(
            r#"{{
  "version": 1,
  "name": "team-policy",
  "rules": [
    {{
      "id": "no-import",
      "kind": "banned-import",
      "specifiers": ["{banned_specifier}"]
    }}
  ]
}}
"#
        ),
    )
    .expect("write rule pack");
}

#[test]
fn rule_pack_schema_matches_legacy_top_level_command() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("package.json"), "{\"name\":\"t\"}\n")
        .expect("write package.json");

    let new = run_rule_pack(dir.path(), &["schema"]);
    let old = Command::new(fallow_bin())
        .arg("--root")
        .arg(dir.path())
        .arg("rule-pack-schema")
        .output()
        .expect("run legacy schema command");

    assert_eq!(new.code, 0, "stderr: {}", new.stderr);
    assert_eq!(new.stdout, String::from_utf8_lossy(&old.stdout));
    let schema = parse_json(&new);
    assert!(
        schema
            .get("properties")
            .and_then(|properties| properties.get("rules"))
            .is_some()
    );
}

#[test]
fn rule_pack_init_creates_pack_and_updates_json_config() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());

    let output = run_rule_pack(dir.path(), &["init"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(dir.path().join("rule-packs/team-policy.jsonc").exists());
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(".fallowrc.json")).unwrap())
            .expect("parse config");
    assert_eq!(
        config["rulePacks"],
        serde_json::json!(["rule-packs/team-policy.jsonc"])
    );
}

#[test]
fn rule_pack_init_refuses_to_overwrite_existing_pack() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());
    let first = run_rule_pack(dir.path(), &["init"]);
    assert_eq!(first.code, 0, "stderr: {}", first.stderr);

    let pack = dir.path().join("rule-packs/team-policy.jsonc");
    std::fs::write(&pack, "sentinel").expect("write sentinel");
    let second = run_rule_pack(dir.path(), &["init"]);

    assert_eq!(second.code, 2);
    assert!(second.stderr.contains("already exists"));
    assert_eq!(std::fs::read_to_string(pack).unwrap(), "sentinel");
}

#[test]
fn rule_pack_init_unknown_template_lists_available_templates() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());

    let output = run_rule_pack(dir.path(), &["init", "--template", "nope"]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("unknown rule-pack template"));
    assert!(output.stderr.contains("ai-safe-repo"));
}

#[test]
fn rule_pack_init_no_config_leaves_config_unchanged() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());
    let config_path = dir.path().join(".fallowrc.json");
    let before = std::fs::read_to_string(&config_path).expect("read config");

    let output = run_rule_pack(dir.path(), &["init", "--no-config"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(
        std::fs::read_to_string(config_path).expect("read config"),
        before
    );
    assert!(output.stdout.contains("\"rulePacks\""));
}

#[test]
fn rule_pack_init_json_output_reports_config_update() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());

    let output = run_rule_pack(dir.path(), &["init", "--format", "json"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "rule-pack-init");
    assert_eq!(json["pack_path"], "rule-packs/team-policy.jsonc");
    assert_eq!(json["template"], "starter");
    assert_eq!(json["config_updated"], true);
    assert_eq!(json["config_path"], ".fallowrc.json");
}

#[test]
fn rule_pack_list_json_reports_loaded_packs() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project_with_rule_pack(dir.path());

    let output = run_rule_pack(dir.path(), &["list", "--format", "json"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "rule-pack-list");
    assert_eq!(json["packs"][0]["name"], "team-policy");
    assert_eq!(json["packs"][0]["source"], "rule-packs/team-policy.jsonc");
    assert_eq!(
        json["packs"][0]["description"],
        "Repository policy guardrails"
    );
    assert_eq!(json["packs"][0]["rules"][0]["id"], "no-moment");
    assert_eq!(json["packs"][0]["rules"][0]["kind"], "banned-import");
    assert_eq!(json["packs"][0]["rules"][0]["severity"], "error");
    assert_eq!(
        json["packs"][0]["rules"][0]["patterns"],
        serde_json::json!(["moment"])
    );
    assert_eq!(json["packs"][0]["rules"][1]["id"], "no-network");
    assert_eq!(json["packs"][0]["rules"][1]["kind"], "banned-effect");
    assert_eq!(json["packs"][0]["rules"][1]["severity"], "warn");
    assert_eq!(
        json["packs"][0]["rules"][1]["patterns"],
        serde_json::json!(["network"])
    );
    assert_eq!(
        json["packs"][0]["rules"][1]["files"],
        serde_json::json!(["src/domain/**"])
    );
}

#[test]
fn rule_pack_list_empty_human_points_to_init() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());

    let output = run_rule_pack(dir.path(), &["list"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("No rule packs configured."));
    assert!(output.stdout.contains("fallow rule-pack init"));
}

#[test]
fn rule_pack_test_explicit_pack_reports_policy_findings() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_policy_test_project(dir.path(), "moment");

    let output = run_rule_pack(dir.path(), &["test", "packs/p.jsonc", "--format", "json"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["kind"], "rule-pack-test");
    assert_eq!(json["packs"], serde_json::json!(["team-policy"]));
    assert_eq!(json["forced_severity"], false);
    assert_eq!(json["rules"][0]["pack"], "team-policy");
    assert_eq!(json["rules"][0]["rule_id"], "no-import");
    assert_eq!(json["rules"][0]["kind"], "banned-import");
    assert_eq!(json["rules"][0]["findings"], 1);
    assert_eq!(json["findings"][0]["rule_id"], "no-import");
}

#[test]
fn rule_pack_test_lists_zero_finding_rules() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_policy_test_project(dir.path(), "lodash");

    let output = run_rule_pack(dir.path(), &["test", "packs/p.jsonc", "--format", "json"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["rules"][0]["findings"], 0);
    assert_eq!(json["findings"], serde_json::json!([]));
}

#[test]
fn rule_pack_test_without_pack_requires_configured_packs() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_project(dir.path());

    let output = run_rule_pack(dir.path(), &["test"]);

    assert_eq!(output.code, 2);
    assert!(
        output
            .stderr
            .contains("no rule packs configured; pass a pack path or run: fallow rule-pack init")
    );
}
