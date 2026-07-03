#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{CommandOutput, parse_json, run_fallow_in_root};

fn run_guard(root: &std::path::Path, args: &[&str]) -> CommandOutput {
    run_fallow_in_root("guard", root, args)
}

fn write_base_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src/domain")).expect("create domain dir");
    std::fs::create_dir_all(root.join("src/shared")).expect("create shared dir");
    std::fs::write(root.join("package.json"), "{\"name\":\"guard-fixture\"}\n")
        .expect("write package json");
    std::fs::write(root.join("src/domain/user.ts"), "export const user = 1;\n")
        .expect("write source");
}

fn write_guard_project(root: &std::path::Path) {
    write_base_project(root);
    std::fs::create_dir_all(root.join("rule-packs")).expect("create rule pack dir");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "rules": {
    "boundary-violation": "error",
    "policy-violation": "warn"
  },
  "boundaries": {
    "zones": [
      { "name": "domain", "patterns": ["src/domain/**"] },
      { "name": "shared", "patterns": ["src/shared/**"] }
    ],
    "rules": [
      { "from": "domain", "allow": ["shared"], "allowTypeOnly": ["shared"] }
    ],
    "calls": {
      "forbidden": [
        { "from": "domain", "callee": "child_process.*" }
      ]
    }
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
  "rules": [
    {
      "id": "pure-domain",
      "kind": "banned-effect",
      "effects": ["network"],
      "files": ["src/domain/**"],
      "severity": "error",
      "message": "Domain code must inject network access through ports."
    }
  ]
}
"#,
    )
    .expect("write rule pack");
}

#[test]
fn guard_json_reports_zone_boundary_and_policy_rules() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_guard_project(dir.path());

    let output = run_guard(
        dir.path(),
        &["src/domain/user.ts", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let file = &json["files"][0];
    assert_eq!(json["kind"], "guard");
    assert_eq!(file["path"], "src/domain/user.ts");
    assert_eq!(file["zone"]["name"], "domain");
    assert_eq!(
        file["boundary"]["allowed_zones"],
        serde_json::json!(["domain", "shared"])
    );
    assert_eq!(
        file["boundary"]["forbidden_calls"],
        serde_json::json!(["child_process.*"])
    );
    assert_eq!(file["policy_rules"][0]["rule_id"], "pure-domain");
    assert_eq!(
        file["policy_rules"][0]["suppress_token"],
        "policy-violation:team-policy/pure-domain"
    );
}

#[test]
fn guard_json_reports_nonexistent_target_without_failing() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_guard_project(dir.path());

    let output = run_guard(
        dir.path(),
        &["src/domain/new-file.ts", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["files"][0]["exists"], false);
    assert_eq!(json["files"][0]["path"], "src/domain/new-file.ts");
}

#[test]
fn guard_rejects_path_outside_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_guard_project(dir.path());

    let output = run_guard(
        dir.path(),
        &["../outside.ts", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 2);
    assert!(output.stdout.contains("\"error\": true"));
    assert!(output.stdout.contains("outside project root"));
}

#[test]
fn guard_human_reports_zone_and_suppress_hint() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_guard_project(dir.path());

    let output = run_guard(dir.path(), &["src/domain/user.ts", "--quiet"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("src/domain/user.ts (zone: domain)"));
    assert!(output.stdout.contains("policy rules:"));
    assert!(
        output
            .stdout
            .contains("fallow-ignore-next-line policy-violation:team-policy/pure-domain")
    );
}

#[test]
fn guard_json_reports_unrestricted_when_no_boundaries_or_packs_configured() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_base_project(dir.path());
    std::fs::write(dir.path().join(".fallowrc.json"), "{}\n").expect("write config");

    let output = run_guard(
        dir.path(),
        &["src/domain/user.ts", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let file = &json["files"][0];
    assert_eq!(file["boundary"]["unrestricted"], true);
    assert!(
        file["notes"]
            .as_array()
            .is_some_and(|notes| !notes.is_empty())
    );
}
