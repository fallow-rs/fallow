#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{parse_json, run_fallow_raw};
use std::fs;

/// Create a temp dir with a knip config for migration testing.
fn migrate_temp_dir(suffix: &str, config_name: &str, config_content: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-migrate-test-{}-{}",
        std::process::id(),
        suffix
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "migrate-test", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join(config_name), config_content).unwrap();
    dir
}

fn cleanup(dir: &std::path::Path) {
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn migrate_dry_run_outputs_config() {
    let dir = migrate_temp_dir(
        "dryrun",
        "knip.json",
        r#"{"entry": ["src/index.ts"], "ignore": ["dist/**"]}"#,
    );
    let output = run_fallow_raw(&[
        "migrate",
        "--dry-run",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "migrate --dry-run should exit 0, stderr: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("entry") || output.stdout.contains("$schema"),
        "dry-run should output the migrated config"
    );
    cleanup(&dir);
}

#[test]
fn migrate_dry_run_toml_output() {
    let dir = migrate_temp_dir("toml", "knip.json", r#"{"entry": ["src/index.ts"]}"#);
    let output = run_fallow_raw(&[
        "migrate",
        "--dry-run",
        "--toml",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "migrate --dry-run --toml should exit 0");
    assert!(
        output.stdout.contains('='),
        "TOML output should use = syntax"
    );
    cleanup(&dir);
}

#[test]
fn migrate_writes_fallowrc_json_when_source_is_knip_json() {
    let dir = migrate_temp_dir("out-json", "knip.json", r#"{"entry": ["src/index.ts"]}"#);
    let output = run_fallow_raw(&["migrate", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        dir.join(".fallowrc.json").exists(),
        ".fallowrc.json should be written for knip.json source"
    );
    assert!(
        !dir.join(".fallowrc.jsonc").exists(),
        ".fallowrc.jsonc should NOT be written for knip.json source"
    );
    cleanup(&dir);
}

/// Issue #1794: with a local `node_modules/fallow/schema.json` present,
/// `fallow migrate` writes the local schema path instead of the remote URL,
/// and the migrated config still loads through the real config loader.
#[test]
fn migrate_schema_prefers_local_when_node_modules_fallow_present() {
    let dir = migrate_temp_dir(
        "schema-local",
        "knip.json",
        r#"{"entry": ["src/index.ts"]}"#,
    );
    fs::create_dir_all(dir.join("node_modules/fallow")).unwrap();
    fs::write(dir.join("node_modules/fallow/schema.json"), "{}").unwrap();

    let output = run_fallow_raw(&["migrate", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);

    let config_path = dir.join(".fallowrc.json");
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("\"$schema\": \"./node_modules/fallow/schema.json\""),
        "expected local schema path with node_modules/fallow present, got: {content}"
    );
    assert!(!content.contains("raw.githubusercontent.com"));

    fallow_config::FallowConfig::load(&config_path)
        .unwrap_or_else(|e| panic!("migrated output with local schema must load: {e:?}"));
    cleanup(&dir);
}

#[test]
fn migrate_auto_writes_fallowrc_jsonc_when_source_is_knip_jsonc() {
    let dir = migrate_temp_dir(
        "out-jsonc-auto",
        "knip.jsonc",
        "{\n  // header comment\n  \"entry\": [\"src/index.ts\"]\n}\n",
    );
    let output = run_fallow_raw(&["migrate", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        dir.join(".fallowrc.jsonc").exists(),
        ".fallowrc.jsonc should be written when source is knip.jsonc"
    );
    assert!(
        !dir.join(".fallowrc.json").exists(),
        ".fallowrc.json should NOT be written when source is knip.jsonc"
    );
    cleanup(&dir);
}

#[test]
fn migrate_explicit_jsonc_flag_overrides_json_source() {
    let dir = migrate_temp_dir(
        "out-jsonc-flag",
        "knip.json",
        r#"{"entry": ["src/index.ts"]}"#,
    );
    let output = run_fallow_raw(&[
        "migrate",
        "--jsonc",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        dir.join(".fallowrc.jsonc").exists(),
        "--jsonc must force .fallowrc.jsonc even when source is knip.json"
    );
    assert!(!dir.join(".fallowrc.json").exists());
    cleanup(&dir);
}

#[test]
fn migrate_jsonc_and_toml_are_mutually_exclusive() {
    let dir = migrate_temp_dir("exclusive", "knip.json", r#"{"entry": ["src/index.ts"]}"#);
    let output = run_fallow_raw(&[
        "migrate",
        "--jsonc",
        "--toml",
        "--dry-run",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_ne!(
        output.code, 0,
        "clap should reject --jsonc and --toml together"
    );
    assert!(
        output.stderr.contains("cannot be used with") || output.stderr.contains("conflicts"),
        "expected clap conflict error, got stderr: {}",
        output.stderr
    );
    cleanup(&dir);
}

#[test]
fn migrate_existing_fallowrc_jsonc_blocks_run() {
    let dir = migrate_temp_dir(
        "blocked-jsonc",
        "knip.json",
        r#"{"entry": ["src/index.ts"]}"#,
    );
    fs::write(dir.join(".fallowrc.jsonc"), "{}").unwrap();
    let output = run_fallow_raw(&["migrate", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        output.code, 2,
        "migrate should refuse to overwrite existing .fallowrc.jsonc"
    );
    assert!(
        output.stderr.contains(".fallowrc.jsonc already exists"),
        "stderr should mention the blocking file, got: {}",
        output.stderr
    );
    cleanup(&dir);
}

/// Build a fixture where a plugin-owned entry imports one source file while a
/// second source file is unused and ignored only at reporting time.
fn graph_preserving_fixture(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-migrate-roundtrip-{}-{}",
        std::process::id(),
        suffix
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "graph-preserving-fixture", "devDependencies": {"vitest": "latest"}}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("vitest.config.ts"),
        "import './src/feature';\nexport default {};\n",
    )
    .unwrap();
    fs::write(dir.join("src/feature.ts"), "export const feature = true;\n").unwrap();
    fs::write(dir.join("src/hidden.ts"), "export const hidden = true;\n").unwrap();

    dir
}

#[test]
fn migrate_knip_ignore_suppresses_findings_without_removing_files() {
    let dir = graph_preserving_fixture("ignore-findings");
    fs::write(dir.join("knip.json"), r#"{"ignore": ["src/hidden.ts"]}"#).unwrap();

    let migrate = run_fallow_raw(&["migrate", "--root", dir.to_str().unwrap(), "--quiet"]);
    assert_eq!(
        migrate.code, 0,
        "migrate should exit 0, stderr: {}",
        migrate.stderr
    );
    assert!(
        dir.join(".fallowrc.json").exists(),
        ".fallowrc.json should be written"
    );
    let migrated = fs::read_to_string(dir.join(".fallowrc.json")).unwrap();
    assert!(migrated.contains("\"ignoreFindings\""));
    assert!(!migrated.contains("\"ignorePatterns\""));

    let list = run_fallow_raw(&[
        "list",
        "--files",
        "--format",
        "json",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(
        list.code, 0,
        "list --files should exit 0, stderr: {}",
        list.stderr
    );

    let body = parse_json(&list);
    let files: Vec<String> = body
        .get("files")
        .and_then(|v| v.as_array())
        .expect("list --files JSON should carry a files array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();

    let normalised: Vec<String> = files.iter().map(|f| f.replace('\\', "/")).collect();
    assert!(
        normalised.iter().any(|path| path == "src/hidden.ts"),
        "ignored findings must not remove their source file from discovery: {normalised:?}"
    );

    let dead_code = run_fallow_raw(&[
        "dead-code",
        "--format",
        "json",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    let findings = parse_json(&dead_code);
    let unused_files = findings["unused_files"].as_array().unwrap();
    assert!(
        unused_files
            .iter()
            .all(|finding| finding["path"] != "src/hidden.ts"),
        "ignored source finding leaked into dead-code output: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .all(|finding| finding["path"] != "src/feature.ts"),
        "the imported source should remain reachable through the plugin entry: {unused_files:?}"
    );

    cleanup(&dir);
}

#[test]
fn migrate_knip_ignore_warns_for_invalid_entries_without_dropping_valid_patterns() {
    let dir = migrate_temp_dir(
        "ignore-warning",
        "knip.json",
        r#"{"ignore": ["src/**", 7, null, "!src/keep.ts"]}"#,
    );
    let output = run_fallow_raw(&["migrate", "--dry-run", "--root", dir.to_str().unwrap()]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("\"ignoreFindings\""));
    assert!(output.stdout.contains("\"!src/keep.ts\""));
    assert!(!output.stdout.contains("\"ignorePatterns\""));
    assert!(output.stderr.contains("Warnings (2):"));
    assert!(output.stderr.contains("ignore[1]"));
    assert!(output.stderr.contains("ignore[2]"));

    cleanup(&dir);
}

#[test]
fn migrate_knip_workspace_ignore_warns_instead_of_guessing_a_root() {
    let dir = migrate_temp_dir(
        "workspace-ignore-warning",
        "knip.json",
        r#"{"workspaces":{"packages/*":{"ignore":["src/generated/**"]}}}"#,
    );
    let output = run_fallow_raw(&["migrate", "--dry-run", "--root", dir.to_str().unwrap()]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("$schema"));
    assert!(!output.stdout.contains("ignoreFindings"));
    assert!(output.stderr.contains("workspaces.packages/*.ignore"));
    assert!(
        output
            .stderr
            .contains("project-root-relative ignoreFindings")
    );

    cleanup(&dir);
}

#[test]
fn migrate_no_config_exits_2() {
    let dir = std::env::temp_dir().join(format!("fallow-migrate-noconfig-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("package.json"), r#"{"name": "no-config"}"#).unwrap();

    let output = run_fallow_raw(&[
        "migrate",
        "--dry-run",
        "--root",
        dir.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "migrate with no source config should exit 2"
    );
    let _ = fs::remove_dir_all(&dir);
}
