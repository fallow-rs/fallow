#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::run_fallow_raw_with_env;

fn agent_temp_dir(suffix: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("fallow-agent-test-{}-{suffix}", std::process::id()));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "agent-test", "main": "index.ts"}"#,
    )
    .unwrap();
    let shipped = dir.join("node_modules/fallow/skills/fallow");
    fs::create_dir_all(&shipped).unwrap();
    fs::create_dir_all(dir.join("node_modules/.bin")).unwrap();
    fs::write(
        dir.join("node_modules/.bin/fallow-mcp"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    fs::write(
        dir.join("node_modules/fallow/package.json"),
        r#"{"name": "fallow", "version": "0.0.0-test"}"#,
    )
    .unwrap();
    fs::write(
        shipped.join("SKILL.md"),
        "---\nname: fallow\ndescription: Test skill.\nlicense: MIT\n---\n\n# Shipped\n",
    )
    .unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(status.success());
    dunce::canonicalize(&dir).unwrap()
}

fn run_agent(root: &Path, home: &Path, args: &[&str]) -> common::CommandOutput {
    let root_str = root.to_str().unwrap();
    let mut full: Vec<&str> = vec!["agent"];
    full.extend_from_slice(args);
    full.extend_from_slice(&["--root", root_str, "--format", "json"]);
    run_fallow_raw_with_env(
        &full,
        &[
            ("HOME", home.to_str().unwrap()),
            ("CLAUDECODE", ""),
            ("CODEX_THREAD_ID", ""),
            ("CURSOR_AGENT", ""),
        ],
    )
}

fn tree(root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = fs::read(&path).unwrap();
                out.push(format!("{rel}:{}", bytes.len()));
            }
        }
    }
    walk(root, root, &mut paths);
    paths.sort();
    paths
}

#[test]
fn install_dry_run_writes_nothing_and_reports_the_plan() {
    let dir = agent_temp_dir("dry-run");
    let home = dir.join("home");
    fs::create_dir_all(dir.join(".claude")).unwrap();
    fs::create_dir_all(&home).unwrap();
    let before = tree(&dir);

    let output = run_agent(&dir, &home, &["install", "--dry-run"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = common::parse_json(&output);
    assert_eq!(json["mode"], "install");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["harnesses"], serde_json::json!(["claude"]));
    assert_eq!(json["detected"], true);
    let steps = json["steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|s| s["path"] == "AGENTS.md" && s["status"] == "written")
    );
    assert!(
        steps
            .iter()
            .any(|s| s["path"] == "CLAUDE.md" && s["status"] == "written")
    );
    assert!(
        steps
            .iter()
            .any(|s| s["path"] == ".claude/settings.json" && s["step"] == "hooks")
    );
    assert_eq!(tree(&dir), before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn install_is_idempotent_and_uninstall_round_trips() {
    let dir = agent_temp_dir("round-trip");
    let home = dir.join("home");
    fs::create_dir_all(dir.join(".claude")).unwrap();
    fs::create_dir_all(dir.join(".codex")).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::write(
        dir.join(".mcp.json"),
        "{\n  \"mcpServers\": {\n    \"other\": {\n      \"command\": \"x\"\n    }\n  }\n}\n",
    )
    .unwrap();
    let before = tree(&dir);

    let first = run_agent(&dir, &home, &["install", "--approve"]);
    assert_eq!(first.code, 0, "stderr: {}", first.stderr);
    let json = common::parse_json(&first);
    assert_eq!(json["harnesses"], serde_json::json!(["claude", "codex"]));
    let steps = json["steps"].as_array().unwrap();
    let written: Vec<&str> = steps
        .iter()
        .filter(|s| s["status"] == "written")
        .filter_map(|s| s["path"].as_str())
        .collect();
    for expected in [
        "AGENTS.md",
        "CLAUDE.md",
        ".agents/skills/fallow",
        ".claude/skills/fallow",
        ".mcp.json",
        ".claude/settings.local.json",
        ".codex/config.toml",
        ".claude/settings.json",
        ".claude/hooks/fallow-gate.sh",
    ] {
        assert!(
            written.contains(&expected),
            "{expected} missing from {written:?}"
        );
    }
    let mcp: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(mcp["mcpServers"]["other"]["command"], "x");
    assert_eq!(mcp["mcpServers"]["fallow"]["type"], "stdio");
    let codex = fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
    assert!(codex.contains("[mcp_servers.fallow]"));
    assert!(
        json["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "codex-mcp-add")
    );
    let after_first = tree(&dir);

    let second = run_agent(&dir, &home, &["install", "--approve"]);
    assert_eq!(second.code, 0, "stderr: {}", second.stderr);
    let json = common::parse_json(&second);
    let statuses: Vec<&str> = json["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["status"].as_str())
        .collect();
    assert!(
        statuses
            .iter()
            .all(|s| *s == "unchanged" || *s == "skipped"),
        "{statuses:?}"
    );
    assert_eq!(tree(&dir), after_first);

    let status = run_agent(&dir, &home, &["status"]);
    assert_eq!(status.code, 0, "stderr: {}", status.stderr);
    let json = common::parse_json(&status);
    let surfaces = json["surfaces"].as_array().unwrap();
    assert!(
        surfaces
            .iter()
            .any(|s| s["path"] == ".claude/skills/fallow" && s["state"] == "installed")
    );
    assert!(
        surfaces
            .iter()
            .any(|s| s["path"] == ".mcp.json" && s["state"] == "installed")
    );

    let removed = run_agent(&dir, &home, &["uninstall"]);
    assert_eq!(removed.code, 0, "stderr: {}", removed.stderr);
    let mut after = tree(&dir);
    after.retain(|entry| !entry.starts_with(".claude/settings.json"));
    assert_eq!(after, before);
    let settings = fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
    assert!(!settings.contains("fallow"), "{settings}");
    assert!(!dir.join(".claude/settings.local.json").exists());
    assert!(!dir.join(".codex/config.toml").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nothing_detected_writes_only_neutral_files() {
    let dir = agent_temp_dir("neutral");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let output = run_agent(&dir, &home, &["install"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = common::parse_json(&output);
    assert_eq!(json["harnesses"], serde_json::json!([]));
    assert!(dir.join("AGENTS.md").is_file());
    assert!(!dir.join("CLAUDE.md").exists());
    assert!(!dir.join(".claude").exists());
    assert!(!dir.join(".mcp.json").exists());
    assert!(
        json["next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "choose-harness")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn foreign_skill_is_refused_with_exit_two() {
    let dir = agent_temp_dir("foreign-skill");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let skill = dir.join(".agents/skills/fallow");
    fs::create_dir_all(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), "---\nname: fallow\n---\nmine\n").unwrap();
    let output = run_agent(&dir, &home, &["install", "--harness", "cursor"]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    let json = common::parse_json(&output);
    let refused = json["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["status"] == "refused")
        .unwrap();
    assert_eq!(refused["reason"], "skill_name_taken");
    assert_eq!(
        fs::read_to_string(skill.join("SKILL.md")).unwrap(),
        "---\nname: fallow\n---\nmine\n"
    );
    assert!(dir.join(".cursor/mcp.json").is_file());
    let _ = fs::remove_dir_all(&dir);
}
