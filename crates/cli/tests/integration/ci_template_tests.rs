//! `fallow ci-template`: help text and stdout behavior.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use crate::common::{run_fallow_raw, run_fallow_raw_in_dir};

#[test]
fn gitlab_vendor_help_names_only_the_directories_it_writes() {
    let output = run_fallow_raw(&["ci-template", "gitlab", "--help"]);

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("--vendor"),
        "stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("action/"),
        "--vendor writes only ci/ files, help: {}",
        output.stdout
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let vendor = dir.path().to_str().expect("UTF-8 tempdir");
    let vendored = run_fallow_raw(&["ci-template", "gitlab", "--vendor", vendor]);
    assert_eq!(vendored.code, 0, "stderr: {}", vendored.stderr);
    let top_level: Vec<String> = std::fs::read_dir(dir.path())
        .expect("read vendor dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(top_level, vec!["ci".to_string()]);
}

/// `fallow ci-template gitlab | head` closes the reader before fallow writes
/// the full template. Fallow must exit 0 without a panic message.
#[cfg(unix)]
#[test]
fn gitlab_template_with_closed_stdout_reader_exits_quietly() {
    let (reader, writer) = std::io::pipe().expect("create pipe");
    drop(reader);

    let output = std::process::Command::new(crate::common::fallow_bin())
        .args(["ci-template", "gitlab"])
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .stdout(std::process::Stdio::from(writer))
        .output()
        .expect("run fallow");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr must stay empty, got: {stderr}");
}

/// A link above a vendor directory that does not exist yet is checked from
/// the working directory, so no file lands outside it.
#[test]
#[cfg(unix)]
fn gitlab_vendor_refuses_a_link_above_a_missing_vendor_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let checkout = dir.path().join("checkout");
    let outside = dir.path().join("elsewhere");
    std::fs::create_dir_all(&checkout).expect("mkdir");
    std::fs::create_dir_all(&outside).expect("mkdir");
    std::os::unix::fs::symlink(&outside, checkout.join("linked")).expect("symlink");

    let output = run_fallow_raw_in_dir(
        &checkout,
        &["ci-template", "gitlab", "--vendor", "linked/vendor"],
    );

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(!outside.join("vendor").exists());
}
