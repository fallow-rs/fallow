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

// ---------------------------------------------------------------------------
// Migrate dry-run
// ---------------------------------------------------------------------------

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
    // Dry-run prints the generated config to stdout
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
    // TOML output should use = syntax
    assert!(
        output.stdout.contains('='),
        "TOML output should use = syntax"
    );
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// Output filename selection (--toml / --jsonc / auto-mirror)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Migrate error handling
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Glob roundtrip: migrate -> fallow list --files, asserting that fallow's
// discovered file set matches the set knip's `ignore` globs would exclude
// from analysis. Catches drift between knip's glob engine and fallow's
// `globset` for the highest-value class of patterns: ignore globs
// (`**/*.test.ts`, `dist/**`, `node_modules/**`, `scripts/**`). The "knip
// ground truth" is hand-recorded from knip's documented glob semantics; we
// do not invoke knip in the test (Node dep, network, flake).
//
// Note: fallow's source discovery walks every supported source file under
// `--root` and filters by `ignorePatterns`. The `entry` field marks
// always-reachable starting points for reachability analysis, not the
// discovery set, so this test does not directly exercise `entry` glob
// drift. Knip's `entry` and fallow's `entry` carry the same semantics
// (entry points), so the patterns under test below are still the ones
// most likely to drift in practice. See issue #457.
// ---------------------------------------------------------------------------

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

    // Files we expect knip's globs to INCLUDE.
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
    // Files that should NOT appear in fallow's discovered set. Two
    // categories:
    //   (1) excluded by patterns that the migrator must actually translate:
    //       `**/*.test.ts` and `scripts/**`. These prove the migrated
    //       ignorePatterns are doing real work.
    //   (2) excluded by fallow's built-in defaults regardless of migration:
    //       `dist/**` and `node_modules/**`. These would be excluded even
    //       if the migrator dropped them from the knip `ignore` array, so
    //       they do NOT independently validate migration. Kept in the
    //       fixture so the file tree resembles a realistic project.
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
        // Minimal TS source: every file gets a single export so it parses.
        fs::write(&path, "export const x = 1;\n").unwrap();
    }

    dir
}

#[test]
fn migrate_roundtrip_globs_match_knip_documented_semantics() {
    // Knip config covering the most common Next.js shapes: brace expansion,
    // `**` cross-segment, `lib/**/*.ts` directory-scoped, and ignore patterns
    // for tests, dist, node_modules, scripts. Each pattern is documented in
    // knip's docs and is structurally identical across both engines; if knip
    // ever changes its glob engine for any of them, this test surfaces the
    // drift loudly.
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

    // Step 1: run the migrator.
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

    // Step 2: run fallow list --files against the migrated config. This is
    // fallow's source-discovery surface: it returns precisely the set of
    // files scoped by `entry` + `ignorePatterns`.
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

    // Step 3: compare against the hand-recorded ground truth derived from
    // knip's documented glob semantics. If knip and fallow's globset
    // diverge on any of these patterns, this assertion fails loudly. The
    // list is sorted because `fallow list --files` returns sorted output.
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

    // Cross-platform: list --files returns forward slashes on every OS but
    // be defensive in case a future change drifts.
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
