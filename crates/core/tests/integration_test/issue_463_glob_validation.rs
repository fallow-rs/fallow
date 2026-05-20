//! Integration tests for issue #463: config-sourced glob patterns are
//! validated at load time. Traversal patterns (`..`), absolute paths, and
//! invalid glob syntax all fail loud instead of silently no-op'ing.

use std::fs;

use fallow_config::FallowConfig;

fn write_config(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join(".fallowrc.json");
    fs::write(&path, body).expect("write config");
    path
}

fn load_err(body: &str) -> String {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = write_config(tmp.path(), body);
    let err = FallowConfig::load(&path).expect_err("load should reject invalid glob");
    err.to_string()
}

// ── entry ───────────────────────────────────────────────────────────────

#[test]
fn entry_traversal_pattern_rejected() {
    let msg = load_err(r#"{ "entry": ["../foo"] }"#);
    assert!(msg.contains("entry"), "msg: {msg}");
    assert!(msg.contains("../foo"), "msg: {msg}");
    assert!(msg.contains("'..'") || msg.contains(".."), "msg: {msg}");
}

#[test]
fn entry_absolute_unix_path_rejected() {
    let msg = load_err(r#"{ "entry": ["/etc/passwd"] }"#);
    assert!(msg.contains("entry"), "msg: {msg}");
    assert!(msg.contains("/etc/passwd"), "msg: {msg}");
    assert!(msg.contains("absolute"), "msg: {msg}");
}

#[test]
fn entry_invalid_glob_syntax_rejected() {
    let msg = load_err(r#"{ "entry": ["[invalid"] }"#);
    assert!(msg.contains("entry"), "msg: {msg}");
    assert!(msg.contains("[invalid"), "msg: {msg}");
}

// ── ignorePatterns ──────────────────────────────────────────────────────

#[test]
fn ignore_patterns_traversal_rejected() {
    let msg = load_err(r#"{ "ignorePatterns": ["../leak"] }"#);
    assert!(msg.contains("ignorePatterns"), "msg: {msg}");
    assert!(msg.contains("../leak"), "msg: {msg}");
}

#[test]
fn ignore_patterns_absolute_rejected() {
    let msg = load_err(r#"{ "ignorePatterns": ["/var/cache/**"] }"#);
    assert!(msg.contains("ignorePatterns"), "msg: {msg}");
    assert!(msg.contains("/var/cache/**"), "msg: {msg}");
}

// ── dynamicallyLoaded ───────────────────────────────────────────────────

#[test]
fn dynamically_loaded_traversal_rejected() {
    let msg = load_err(r#"{ "dynamicallyLoaded": ["../config/**"] }"#);
    assert!(msg.contains("dynamicallyLoaded"), "msg: {msg}");
    assert!(msg.contains("../config/**"), "msg: {msg}");
}

// ── duplicates.ignore ───────────────────────────────────────────────────

#[test]
fn duplicates_ignore_traversal_rejected() {
    let msg = load_err(r#"{ "duplicates": { "ignore": ["../sibling/**"] } }"#);
    assert!(msg.contains("duplicates.ignore"), "msg: {msg}");
    assert!(msg.contains("../sibling/**"), "msg: {msg}");
}

// ── health.ignore ───────────────────────────────────────────────────────

#[test]
fn health_ignore_traversal_rejected() {
    let msg = load_err(r#"{ "health": { "ignore": ["../vendor/**"] } }"#);
    assert!(msg.contains("health.ignore"), "msg: {msg}");
    assert!(msg.contains("../vendor/**"), "msg: {msg}");
}

// ── overrides[].files ───────────────────────────────────────────────────

#[test]
fn overrides_files_traversal_rejected() {
    let msg = load_err(r#"{ "overrides": [ { "files": ["../escape/**"], "rules": {} } ] }"#);
    assert!(msg.contains("overrides[].files"), "msg: {msg}");
    assert!(msg.contains("../escape/**"), "msg: {msg}");
}

// ── ignoreExports[].file ────────────────────────────────────────────────

#[test]
fn ignore_exports_traversal_rejected() {
    let msg = load_err(r#"{ "ignoreExports": [ { "file": "../foo", "exports": ["*"] } ] }"#);
    assert!(msg.contains("ignoreExports[].file"), "msg: {msg}");
    assert!(msg.contains("../foo"), "msg: {msg}");
}

// ── ignoreCatalogReferences[].consumer ──────────────────────────────────

#[test]
fn ignore_catalog_references_consumer_traversal_rejected() {
    let msg = load_err(
        r#"{ "ignoreCatalogReferences": [ { "package": "react", "consumer": "../foo/package.json" } ] }"#,
    );
    assert!(
        msg.contains("ignoreCatalogReferences[].consumer"),
        "msg: {msg}"
    );
    assert!(msg.contains("../foo/package.json"), "msg: {msg}");
}

// ── boundaries.zones[].patterns ─────────────────────────────────────────

#[test]
fn boundary_zone_pattern_traversal_rejected() {
    let msg = load_err(
        r#"{ "boundaries": { "zones": [ { "name": "ui", "patterns": ["../app/**"] } ], "rules": [] } }"#,
    );
    assert!(msg.contains("boundaries.zones[].patterns"), "msg: {msg}");
    assert!(msg.contains("../app/**"), "msg: {msg}");
}

// ── boundaries.zones[].root + autoDiscover (directory paths, not globs) ──

#[test]
fn boundary_zone_root_traversal_rejected() {
    let msg = load_err(
        r#"{ "boundaries": { "zones": [ { "name": "ui", "root": "../escape", "patterns": ["**/*"] } ], "rules": [] } }"#,
    );
    assert!(msg.contains("boundaries.zones[].root"), "msg: {msg}");
    assert!(msg.contains("../escape"), "msg: {msg}");
}

#[test]
fn boundary_zone_root_absolute_rejected() {
    let msg = load_err(
        r#"{ "boundaries": { "zones": [ { "name": "ui", "root": "/abs/dir", "patterns": ["**/*"] } ], "rules": [] } }"#,
    );
    assert!(msg.contains("boundaries.zones[].root"), "msg: {msg}");
    assert!(msg.contains("/abs/dir"), "msg: {msg}");
}

#[test]
fn boundary_zone_auto_discover_traversal_rejected() {
    let msg = load_err(
        r#"{ "boundaries": { "zones": [ { "name": "features", "autoDiscover": ["../escape"] } ], "rules": [] } }"#,
    );
    assert!(
        msg.contains("boundaries.zones[].autoDiscover"),
        "msg: {msg}"
    );
    assert!(msg.contains("../escape"), "msg: {msg}");
}

// ── framework[] inline plugin definitions ───────────────────────────────

#[test]
fn framework_file_exists_detection_traversal_rejected() {
    // The security-critical case: framework[].detection.fileExists.pattern
    // reaches glob::glob on disk via root.join(pattern) in
    // crates/core/src/plugins/registry/helpers.rs. A `..` here is a real
    // path traversal, not a no-op-match. See rust-reviewer BLOCK on first
    // review pass.
    let msg = load_err(
        r#"{ "framework": [{ "name": "evil", "detection": { "type": "fileExists", "pattern": "../../etc/passwd" } }] }"#,
    );
    assert!(msg.contains("framework[].detection"), "msg: {msg}");
    assert!(msg.contains("../../etc/passwd"), "msg: {msg}");
}

#[test]
fn framework_file_exists_detection_absolute_rejected() {
    let msg = load_err(
        r#"{ "framework": [{ "name": "evil", "detection": { "type": "fileExists", "pattern": "/etc/passwd" } }] }"#,
    );
    assert!(msg.contains("framework[].detection"), "msg: {msg}");
    assert!(msg.contains("/etc/passwd"), "msg: {msg}");
}

#[test]
fn framework_file_exists_nested_in_all_combinator_rejected() {
    // PluginDetection::All { conditions } must recurse so nested
    // FileExists patterns inside boolean combinators are also validated.
    let msg = load_err(
        r#"{
            "framework": [{
                "name": "nested-evil",
                "detection": {
                    "type": "all",
                    "conditions": [
                        { "type": "dependency", "package": "react" },
                        { "type": "fileExists", "pattern": "../../sneaky" }
                    ]
                }
            }]
        }"#,
    );
    assert!(msg.contains("framework[].detection"), "msg: {msg}");
    assert!(msg.contains("../../sneaky"), "msg: {msg}");
}

#[test]
fn framework_entry_points_traversal_rejected() {
    let msg = load_err(r#"{ "framework": [{ "name": "x", "entryPoints": ["../escape/**"] }] }"#);
    assert!(msg.contains("framework[].entryPoints"), "msg: {msg}");
    assert!(msg.contains("../escape/**"), "msg: {msg}");
}

#[test]
fn framework_always_used_traversal_rejected() {
    let msg =
        load_err(r#"{ "framework": [{ "name": "x", "alwaysUsed": ["../escape/setup.ts"] }] }"#);
    assert!(msg.contains("framework[].alwaysUsed"), "msg: {msg}");
}

#[test]
fn framework_used_exports_pattern_traversal_rejected() {
    let msg = load_err(
        r#"{ "framework": [{ "name": "x", "usedExports": [{ "pattern": "../foo/**", "exports": ["*"] }] }] }"#,
    );
    assert!(
        msg.contains("framework[].usedExports[].pattern"),
        "msg: {msg}"
    );
}

// ── multi-error collection ──────────────────────────────────────────────

#[test]
fn all_errors_reported_in_one_run() {
    let msg = load_err(
        r#"{
            "entry": ["../bad-entry"],
            "ignorePatterns": ["/abs/bad-ignore"],
            "dynamicallyLoaded": ["[bad-syntax"]
        }"#,
    );
    assert!(msg.contains("../bad-entry"), "msg: {msg}");
    assert!(msg.contains("/abs/bad-ignore"), "msg: {msg}");
    assert!(msg.contains("[bad-syntax"), "msg: {msg}");
}

// ── happy path ──────────────────────────────────────────────────────────

#[test]
fn valid_relative_patterns_accepted() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = write_config(
        tmp.path(),
        r#"{
            "entry": ["src/**/*.ts", "./entry.ts"],
            "ignorePatterns": ["generated/**", "**/*.snap"],
            "dynamicallyLoaded": ["plugins/*.ts"],
            "duplicates": { "ignore": ["**/*.test.ts"] },
            "health": { "ignore": ["src/legacy/**"] }
        }"#,
    );
    let config = FallowConfig::load(&path).expect("valid config should load");
    assert_eq!(config.entry, vec!["src/**/*.ts", "./entry.ts"]);
    assert_eq!(config.ignore_patterns.len(), 2);
}

#[test]
fn empty_config_still_loads() {
    // Sanity check: the validation pass doesn't reject a clean default config.
    let tmp = tempfile::tempdir().expect("create tempdir");
    let path = write_config(tmp.path(), r"{}");
    FallowConfig::load(&path).expect("empty config should load");
}
