//! An unknown key in the `boundaries` config stops the run with exit code 2,
//! as an unknown top-level key does. A misspelled key such as `publicFile`
//! would otherwise turn off a check without a message (issue #5292).

use std::path::Path;

use crate::common::run_fallow_in_root;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// Run `check` with `boundaries` set to `boundaries` and return its stderr.
fn check_with_boundaries(boundaries: &str) -> (i32, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "unknown-keys" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    write(
        root,
        ".fallowrc.json",
        &format!(r#"{{ "entry": ["src/main.ts"], "boundaries": {boundaries} }}"#),
    );
    let output = run_fallow_in_root("check", root, &["--quiet"]);
    (output.code, output.stderr)
}

#[test]
fn an_unknown_key_in_the_boundaries_config_exits_2() {
    let cases = [
        (
            r#"{ "zones": [{ "name": "billing", "patterns": ["src/billing/**"], "publicFile": ["src/billing/index.ts"] }] }"#,
            "publicFile",
        ),
        (
            r#"{ "preset": "layered", "presetPublicFile": true }"#,
            "presetPublicFile",
        ),
        (
            r#"{ "zones": [{ "name": "a", "patterns": ["src/**"] }], "rules": [{ "from": "a", "allows": [] }] }"#,
            "allows",
        ),
        (
            r#"{ "coverage": { "requireAllFile": true } }"#,
            "requireAllFile",
        ),
        (
            r#"{ "zones": [{ "name": "a", "patterns": ["src/**"] }], "calls": { "forbidden": [{ "from": "a", "calee": "fetch" }] } }"#,
            "calee",
        ),
        (r#"{ "calls": { "denied": [] } }"#, "denied"),
    ];
    for (boundaries, key) in cases {
        let (code, stderr) = check_with_boundaries(boundaries);
        assert_eq!(code, 2, "{boundaries}: {stderr}");
        assert!(
            stderr.contains(&format!("unknown field `{key}`")),
            "the error names the key `{key}`: {stderr}"
        );
    }
}

#[test]
fn the_known_boundaries_keys_still_load() {
    let (code, stderr) = check_with_boundaries(
        r#"{
    "presetPublicFiles": false,
    "zones": [{ "name": "a", "patterns": ["src/**"], "publicFiles": ["src/main.ts"] }],
    "rules": [{ "from": "a", "allow": [], "allowTypeOnly": [] }],
    "coverage": { "requireAllFiles": false, "allowUnmatched": [] },
    "calls": { "forbidden": [{ "from": "a", "callee": "fetch" }, { "from": "a", "callee": ["eval", "*.innerHTML"] }] }
  }"#,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("unknown field"), "{stderr}");
}

#[test]
fn boundaries_enabled_false_names_the_fix() {
    let (code, stderr) = check_with_boundaries(r#"{ "enabled": false }"#);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("remove the `boundaries` section to turn the checks off"),
        "{stderr}"
    );
}

#[test]
fn boundaries_enabled_in_fallow_toml_names_the_fix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "toml-config" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    write(
        root,
        "fallow.toml",
        "entry = [\"src/main.ts\"]\n\n[boundaries]\nenabled = false\n",
    );
    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("remove the `boundaries` section to turn the checks off"),
        "{}",
        output.stderr
    );
}

#[test]
fn boundaries_enabled_with_an_outside_base_config_names_the_fix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = dir.path();
    write(base, "shared/base.json", r#"{ "entry": ["src/main.ts"] }"#);
    let root = base.join("project");
    write(&root, "package.json", r#"{ "name": "outside-base" }"#);
    write(&root, "src/main.ts", "export const a = 1;\n");
    write(
        &root,
        ".fallowrc.json",
        r#"{ "extends": ["../shared/base.json"], "boundaries": { "enabled": false } }"#,
    );
    let output = run_fallow_in_root("check", &root, &["--quiet"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output
            .stderr
            .contains("remove the `boundaries` section to turn the checks off"),
        "{}",
        output.stderr
    );
}
