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
