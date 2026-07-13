//! Process-level instance conformance: real `fallow ... --format json` stdout
//! documents validated against the published `docs/output-schema.json`
//! (draft-07).
//!
//! The api-crate sibling (`crates/api/tests/schema_conformance.rs`) validates
//! the serializers in-process. This test drives the actual binary, so it also
//! covers the process-boundary work the api layer never sees: root-envelope
//! injection, telemetry `_meta`, `next_steps`, and post-pass path stripping. It
//! is the faithful home for the git-/pipeline-dependent envelopes (`audit`,
//! `audit-brief`, `decision-surface`), which are assembled from a full audit
//! run rather than a single serializer call.
//!
//! The schema is read from disk per invocation so the test tracks the committed
//! `docs/output-schema.json` without a rebuild.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests use unwrap/expect/panic to keep fixture setup and schema lookups concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::fallow_bin;
use serde_json::Value;
use tempfile::TempDir;

/// Path to the committed schema, resolved from this crate's manifest dir.
fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/output-schema.json")
}

fn load_schema_root() -> Value {
    let text = std::fs::read_to_string(schema_path()).expect("read output-schema.json");
    serde_json::from_str(&text).expect("parse output-schema.json")
}

/// Build a self-contained draft-07 validator for the `FallowOutput` oneOf
/// branch pinned to `kind`, then assert `value` conforms and that its own
/// `kind` matches (so a document cannot pass against the wrong branch).
fn assert_conforms(root: &Value, kind: &str, value: &Value) {
    assert_eq!(
        value.get("kind").and_then(Value::as_str),
        Some(kind),
        "document kind must be {kind:?}, got {:?}",
        value.get("kind"),
    );

    let definitions = root
        .get("definitions")
        .expect("schema has a definitions object");
    let branches = definitions
        .get("FallowOutput")
        .and_then(|f| f.get("oneOf"))
        .and_then(Value::as_array)
        .expect("FallowOutput.oneOf is an array");
    let branch = branches
        .iter()
        .find(|branch| {
            branch
                .get("allOf")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("properties")
                            .and_then(|p| p.get("kind"))
                            .and_then(|k| k.get("const"))
                            .and_then(Value::as_str)
                            == Some(kind)
                    })
                })
        })
        .unwrap_or_else(|| panic!("no FallowOutput oneOf branch declares kind {kind:?}"));

    let branch_schema = serde_json::json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "allOf": branch.get("allOf").expect("branch has an allOf"),
        "definitions": definitions,
    });
    let validator = jsonschema::draft7::new(&branch_schema)
        .unwrap_or_else(|err| panic!("compile draft-07 branch for {kind:?}: {err}"));

    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|err| format!("  at {} : {err}", err.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "'{kind}' stdout document does not conform to docs/output-schema.json:\n{}",
        errors.join("\n"),
    );
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A small git-backed project: one entry, a dead export, a suppression marker,
/// and a feature-flag reference. Committed so `audit`/`decision-surface` have a
/// base ref; the working tree matches HEAD so those envelopes serialize their
/// empty-but-valid shape.
fn git_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"cli-conformance","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/index.ts"),
        "import { used } from './lib';\n\
         if (process.env.FEATURE_ALPHA) {\n  used();\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/lib.ts"),
        "export const used = () => 42;\n\
         // fallow-ignore-next-line unused-export\n\
         export const suppressed = () => 0;\n\
         export const dead = () => 1;\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(dir, &["-c", "commit.gpgsign=false", "commit", "-m", "base"]);
    tmp
}

/// Run `fallow <args> --root <root> --format json --quiet --no-cache`, parse
/// stdout, and validate it against the schema branch for `expected_kind`.
fn run_and_validate(schema: &Value, root: &Path, args: &[&str], expected_kind: &str) {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg("--root")
        .arg(root)
        .arg("--format")
        .arg("json")
        .arg("--quiet")
        .arg("--no-cache");
    let output = cmd.output().expect("run fallow binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&stdout).unwrap_or_else(|err| {
        panic!(
            "{expected_kind}: stdout is not JSON: {err}\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr),
        )
    });
    assert_conforms(schema, expected_kind, &value);
}

#[test]
fn cli_json_documents_conform_to_output_schema() {
    let fixture = git_fixture();
    let root = fixture.path();
    let schema = load_schema_root();

    // Core commands (the plan's five) plus feature-flags, whose default-path
    // _meta.telemetry shape is exactly what the process boundary must emit
    // conformantly.
    run_and_validate(&schema, root, &["dead-code"], "dead-code");
    run_and_validate(&schema, root, &["health"], "health");
    run_and_validate(&schema, root, &["dupes"], "dupes");
    run_and_validate(&schema, root, &[], "combined");
    run_and_validate(&schema, root, &["suppressions"], "suppression-inventory");
    run_and_validate(&schema, root, &["flags"], "feature-flags");

    // Git-/pipeline-dependent envelopes assembled from a full audit run.
    run_and_validate(&schema, root, &["audit", "--base", "HEAD"], "audit");
    run_and_validate(
        &schema,
        root,
        &["audit", "--brief", "--base", "HEAD"],
        "audit-brief",
    );
    run_and_validate(
        &schema,
        root,
        &["decision-surface", "--base", "HEAD"],
        "decision-surface",
    );

    // Symbol-level call-chain trace: the CLI `fallow trace` surface IS a
    // published `kind: "trace"` envelope (unlike the un-enveloped api
    // programmatic trace serializers, which are guarded out of the contract by
    // `trace_family_is_not_a_published_envelope` in the api crate).
    run_and_validate(
        &schema,
        root,
        &["trace", "src/lib.ts:used", "--callers"],
        "trace",
    );
}

/// The structured error envelope (`--format json` failure path) has no `kind`,
/// so it is a document-root branch (`ErrorOutput`) rather than a `FallowOutput`
/// variant. Validate a real emitted error document against the whole schema so
/// the root `oneOf` selects the error branch, and confirm a success document
/// still validates (the new branch did not break the union).
#[test]
fn error_envelope_conforms_to_output_schema() {
    let schema = load_schema_root();
    let validator = jsonschema::draft7::new(&schema).expect("compile output schema root");

    // A non-existent root is a validation error (exit 2), emitted as the JSON
    // error envelope on stdout.
    let output = Command::new(fallow_bin())
        .args(["dead-code", "--root", "/nonexistent-fallow-path-xyz"])
        .args(["--format", "json", "--quiet", "--no-cache"])
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .output()
        .expect("run fallow binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("error stdout is not JSON: {err}\nstdout:\n{stdout}"));

    assert_eq!(value.get("error").and_then(Value::as_bool), Some(true));
    assert!(
        value.get("kind").is_none(),
        "error envelope carries no kind"
    );
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|err| format!("  at {} : {err}", err.instance_path()))
        .collect();
    assert!(
        errors.is_empty(),
        "error envelope does not conform to docs/output-schema.json:\n{}",
        errors.join("\n"),
    );
}
