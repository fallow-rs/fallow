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
