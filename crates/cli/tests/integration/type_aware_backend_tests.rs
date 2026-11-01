#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! An experimental type-aware backend can refine findings, but its evidence
//! must not make a fix eligible. The corpus evidence for guarded fixes comes
//! from the default backend only.

use std::path::PathBuf;
use std::process::Command;

use crate::common::{configure_type_aware_sidecar, fallow_bin, fixture_path};

/// tsrs ships native binaries for these targets only.
const TSRS_TARGET: bool = cfg!(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ),
));

fn tsrs_installed() -> bool {
    let mut manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    manifest.pop();
    manifest.pop();
    manifest.push("tools/type-aware-sidecar/node_modules/tsrs/package.json");
    TSRS_TARGET && manifest.is_file()
}

/// Run `type-aware status` in one format. A `None` backend clears the selection.
fn type_aware_status(backend: Option<&str>, format: &str) -> String {
    let root = fixture_path("type-aware-class-method-impact");
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    match backend {
        Some(backend) => cmd.env("FALLOW_TYPE_AWARE_BACKEND", backend),
        None => cmd.env_remove("FALLOW_TYPE_AWARE_BACKEND"),
    };
    configure_type_aware_sidecar(&mut cmd);
    let output = cmd
        .args(["type-aware", "status", "--root"])
        .arg(&root)
        .args(["--format", format])
        .output()
        .expect("failed to run fallow binary");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert_eq!(output.status.code(), Some(0), "stdout: {stdout}");
    stdout
}

/// The human status line of a backend, with its fields from the JSON output.
fn expected_status_line(backend: Option<&str>, label: &str) -> String {
    let json: serde_json::Value =
        serde_json::from_str(&type_aware_status(backend, "json")).expect("type-aware status JSON");
    assert_eq!(json["available"], true, "the companion must start: {json}");
    format!(
        "[OK] Type-aware companion: available ({}, protocol {}, {label} {})",
        json["package_version"].as_str().expect("package version"),
        json["protocol_version"],
        json["backend_version"].as_str().expect("backend version"),
    )
}

#[test]
fn default_backend_status_line_keeps_the_typescript_label() {
    assert_eq!(
        type_aware_status(None, "human").trim_end(),
        expected_status_line(None, "TypeScript")
    );
}

#[test]
fn tsrs_status_line_names_tsrs() {
    if !tsrs_installed() {
        return;
    }
    assert_eq!(
        type_aware_status(Some("tsrs"), "human").trim_end(),
        expected_status_line(Some("tsrs"), "tsrs")
    );
}

#[test]
fn tsrs_evidence_never_makes_a_class_member_fix_eligible() {
    if !tsrs_installed() {
        return;
    }
    let root = fixture_path("type-aware-class-method-impact");
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("FALLOW_TYPE_AWARE_BACKEND", "tsrs");
    configure_type_aware_sidecar(&mut cmd);
    let output = cmd
        .args(["fix", "--root"])
        .arg(&root)
        .args(["--type-aware", "--dry-run", "--format", "json", "--quiet"])
        .output()
        .expect("failed to run fallow binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "stdout: {stdout}");
    let preview: serde_json::Value = serde_json::from_str(&stdout).expect("fix preview JSON");
    let fixes = preview["fixes"].as_array().expect("fix preview");
    assert!(
        fixes.iter().all(|fix| fix["closed_world_eligible"] != true),
        "a tsrs result must not enable a guarded fix: {stdout}"
    );
}
