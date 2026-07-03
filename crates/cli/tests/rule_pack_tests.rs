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
