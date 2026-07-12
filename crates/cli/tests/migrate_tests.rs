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

/// Build a representative Next.js-shaped fixture project with files that
/// exercise the most common knip glob patterns. Returns the absolute root.
fn roundtrip_fixture(suffix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fallow-migrate-roundtrip-{}-{}",
        std::process::id(),
        suffix
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "roundtrip-fixture", "main": "app/page.tsx"}"#,
    )
    .unwrap();

    let kept = [
        "app/layout.tsx",
        "app/page.tsx",
        "app/api/route.ts",
        "components/button.tsx",
        "components/card.tsx",
        "lib/utils.ts",
        "lib/db.ts",
        "pages/_app.tsx",
        "pages/api/hello.ts",
    ];
    let ignored = [
        "__tests__/utils.test.ts",
        "lib/db.test.ts",
        "dist/bundle.js",
        "node_modules/foo/index.js",
        "scripts/build.ts",
    ];

    for rel in kept.iter().chain(ignored.iter()) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "export const x = 1;\n").unwrap();
    }

    dir
}

#[test]
fn migrate_roundtrip_globs_match_knip_documented_semantics() {
    let knip = r#"{
        "entry": [
            "app/**/*.{ts,tsx}",
            "pages/**/*.{ts,tsx}",
            "components/**/*.{ts,tsx}",
            "lib/**/*.ts"
        ],
        "ignore": [
            "**/*.test.ts",
            "dist/**",
            "node_modules/**",
            "scripts/**"
        ]
    }"#;

    let dir = roundtrip_fixture("globs");
    fs::write(dir.join("knip.json"), knip).unwrap();

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

    let expected: Vec<&str> = vec![
        "app/api/route.ts",
        "app/layout.tsx",
        "app/page.tsx",
        "components/button.tsx",
        "components/card.tsx",
        "lib/db.ts",
        "lib/utils.ts",
        "pages/_app.tsx",
        "pages/api/hello.ts",
    ];

    let normalised: Vec<String> = files.iter().map(|f| f.replace('\\', "/")).collect();
    assert_eq!(
        normalised, expected,
        "fallow's scoped file set diverged from knip's documented glob \
         semantics. If knip recently changed engines this is real drift; \
         otherwise check fallow's globset or the migrator's pattern copy."
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
