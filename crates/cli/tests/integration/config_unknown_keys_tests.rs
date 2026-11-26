//! An unknown key in a nested config section stops the run with exit code 2,
//! as an unknown top-level key does. A misspelled key such as `minToken`
//! would otherwise turn off a setting without a message.

use std::path::Path;

use crate::common::run_fallow_in_root;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// Run `check` with `extra` added to the config, and return the exit code
/// and stderr.
fn check_with(extra: &str) -> (i32, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "unknown-keys" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    write(
        root,
        ".fallowrc.json",
        &format!(r#"{{ "entry": ["src/main.ts"], {extra} }}"#),
    );
    let output = run_fallow_in_root("check", root, &["--quiet"]);
    (output.code, output.stderr)
}

#[test]
fn an_unknown_key_in_a_config_section_exits_2() {
    let cases = [
        (r#""duplicates": { "minToken": 10 }"#, "minToken"),
        (
            r#""duplicates": { "normalization": { "ignoreIdentifier": true } }"#,
            "ignoreIdentifier",
        ),
        (r#""flags": { "envPrefix": ["FF_"] }"#, "envPrefix"),
        (
            r#""flags": { "sdkPatterns": [{ "function": "useFlag", "nameArgs": 0 }] }"#,
            "nameArgs",
        ),
        (
            r#""health": { "ownership": { "botPattern": ["bot"] } }"#,
            "botPattern",
        ),
        (r#""resolve": { "condition": ["worker"] }"#, "condition"),
        (
            r#""fix": { "catalog": { "deleteComments": "never" } }"#,
            "deleteComments",
        ),
        (r#""fix": { "catalogs": {} }"#, "catalogs"),
        (r#""audit": { "gates": "all" }"#, "gates"),
    ];
    for (extra, key) in cases {
        let (code, stderr) = check_with(extra);
        assert_eq!(code, 2, "{extra}: {stderr}");
        assert!(
            stderr.contains(&format!("unknown field `{key}`")),
            "the error names the key `{key}`: {stderr}"
        );
    }
}

#[test]
fn an_unknown_key_in_the_ignore_exports_object_exits_2() {
    let (code, stderr) = check_with(r#""ignoreExportsUsedInFile": { "types": true }"#);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("unknown field `types`") && stderr.contains("did you mean `type`?"),
        "{stderr}"
    );
}

#[test]
fn the_boolean_ignore_exports_form_still_loads() {
    let (code, stderr) = check_with(r#""ignoreExportsUsedInFile": true"#);
    assert_eq!(code, 0, "{stderr}");
    let (code, stderr) = check_with(r#""ignoreExportsUsedInFile": "yes""#);
    assert_eq!(code, 2, "{stderr}");
}

#[test]
fn the_known_keys_of_the_config_sections_still_load() {
    let (code, stderr) = check_with(
        r#""duplicates": {
      "enabled": true, "minTokens": 50, "minLines": 5,
      "normalization": { "ignoreIdentifiers": false, "ignoreStringValues": false,
        "ignoreNumericValues": false, "consistentIdentifiers": false }
    },
    "flags": {
      "sdkPatterns": [{ "function": "useFlag", "nameArg": 0, "provider": "custom" }],
      "envPrefixes": ["FF_"], "configObjectHeuristics": false
    },
    "health": { "ownership": { "botPatterns": ["bot"], "emailMode": "handle" } },
    "resolve": { "conditions": ["worker"] },
    "ignoreExportsUsedInFile": { "type": true, "interface": false },
    "fix": { "catalog": { "deletePrecedingComments": "never" } },
    "audit": { "gate": "all", "deadCode": true, "health": true, "dupes": true }"#,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(!stderr.contains("unknown field"), "{stderr}");
}

#[test]
fn an_unknown_key_close_to_a_known_key_names_the_known_key() {
    let cases = [
        (r#""duplicates": { "minToken": 10 }"#, "minTokens"),
        (r#""audit": { "gates": "all" }"#, "gate"),
        (
            r#""boundaries": { "zones": [{ "name": "a", "patterns": ["src/**"], "publicFile": [] }] }"#,
            "publicFiles",
        ),
        (r#""entryy": ["src/main.ts"]"#, "entry"),
        // The error lists two valid keys as "`a` or `b`".
        (
            r#""health": { "ownership": { "botPattern": ["bot"] } }"#,
            "botPatterns",
        ),
        // The error lists one valid key as "`a`".
        (r#""fix": { "catalogs": {} }"#, "catalog"),
        (r#""regression": { "baselin": {} }"#, "baseline"),
        (r#""resolve": { "condition": ["worker"] }"#, "conditions"),
        // A key that differs in case only.
        (r#""duplicates": { "mintokens": 10 }"#, "minTokens"),
    ];
    for (extra, known) in cases {
        let (code, stderr) = check_with(extra);
        assert_eq!(code, 2, "{extra}: {stderr}");
        assert!(
            stderr.contains(&format!("did you mean `{known}`?")),
            "the error names `{known}`: {stderr}"
        );
    }
}

#[test]
fn an_unknown_key_far_from_every_known_key_has_no_suggestion() {
    let (code, stderr) = check_with(r#""duplicates": { "somethingElse": 1 }"#);
    assert_eq!(code, 2, "{stderr}");
    assert!(!stderr.contains("did you mean"), "{stderr}");
}

#[test]
fn an_unknown_key_in_the_regression_baseline_still_loads() {
    // A newer fallow can write a count that this version does not know.
    let (code, stderr) =
        check_with(r#""regression": { "baseline": { "totalIssues": 0, "aNewCount": 0 } }"#);
    assert_eq!(code, 0, "{stderr}");
}

/// Run `check` in a project with the config file `name` set to `body`, and
/// return the exit code and stderr.
fn check_with_file(name: &str, body: &str) -> (i32, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "unknown-keys" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    write(root, name, body);
    let output = run_fallow_in_root("check", root, &["--quiet"]);
    (output.code, output.stderr)
}

#[test]
fn an_unknown_key_in_fallow_toml_exits_2_with_a_suggestion() {
    let (code, stderr) = check_with_file(
        "fallow.toml",
        "entry = [\"src/main.ts\"]\n\n[duplicates]\nminToken = 10\n",
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("unknown field `minToken`") && stderr.contains("did you mean `minTokens`?"),
        "{stderr}"
    );
}

#[test]
fn the_boolean_ignore_exports_form_still_loads_from_fallow_toml() {
    let (code, stderr) = check_with_file(
        "fallow.toml",
        "entry = [\"src/main.ts\"]\nignoreExportsUsedInFile = true\n",
    );
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn an_unknown_key_in_fallowrc_jsonc_exits_2() {
    let (code, stderr) = check_with_file(
        ".fallowrc.jsonc",
        "{\n  // comment\n  \"entry\": [\"src/main.ts\"],\n  \"flags\": { \"envPrefix\": [\"FF_\"] }\n}\n",
    );
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("unknown field `envPrefix`")
            && stderr.contains("did you mean `envPrefixes`?"),
        "{stderr}"
    );
}

#[test]
fn an_unknown_key_in_an_extended_config_names_that_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "unknown-keys" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    write(
        root,
        "config/base.json",
        r#"{ "duplicates": { "minToken": 10 } }"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{ "extends": ["./config/base.json"], "entry": ["src/main.ts"] }"#,
    );
    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output.stderr.contains("unknown field `minToken`"),
        "{}",
        output.stderr
    );
}
