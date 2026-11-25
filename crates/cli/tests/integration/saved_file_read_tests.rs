//! Saved files that a flag names, such as baselines, are read only when a
//! path below the project root or the working directory resolves inside it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use crate::common::run_fallow_raw;
use tempfile::TempDir;

const OUTSIDE_VALUE: &str = "value-from-the-other-file";

/// A project with one unused export, and a JSON file outside the project.
fn project_and_outside_file() -> (TempDir, TempDir, std::path::PathBuf) {
    let project = TempDir::new().expect("temp project");
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"saved-file-read","version":"1.0.0","private":true,"main":"src/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.js"), "export const used = 1;\n").expect("entry");
    std::fs::write(root.join("src/unused.js"), "export const unused = 2;\n").expect("unused");
    let elsewhere = TempDir::new().expect("temp dir");
    let outside = elsewhere.path().join("other.json");
    let text = format!(
        r#"{{"schema_version": "{OUTSIDE_VALUE}", "unused_exports": "{OUTSIDE_VALUE}", "clone_groups": "{OUTSIDE_VALUE}", "findings": "{OUTSIDE_VALUE}"}}"#
    );
    std::fs::write(&outside, text).expect("outside file");
    (project, elsewhere, outside)
}

#[test]
#[cfg(unix)]
fn baseline_flags_refuse_a_link_out_of_the_project() {
    for (command, flag) in [
        ("dead-code", "--baseline"),
        ("dupes", "--baseline"),
        ("health", "--baseline"),
        ("dead-code", "--regression-baseline"),
    ] {
        let (project, _elsewhere, outside) = project_and_outside_file();
        let link = project.path().join("baseline.json");
        std::os::unix::fs::symlink(&outside, &link).expect("link");
        let root = project.path().to_str().expect("UTF-8 path");
        let link = link.to_str().expect("UTF-8 path");
        let mut args = vec![command, "--root", root, flag, link, "--format", "json"];
        if flag == "--regression-baseline" {
            args.push("--fail-on-regression");
        }
        let output = run_fallow_raw(&args);

        assert_eq!(output.code, 2, "{command} {flag}: {}", output.stderr);
        assert!(
            !output.stdout.contains(OUTSIDE_VALUE) && !output.stderr.contains(OUTSIDE_VALUE),
            "{command} {flag}: {} {}",
            output.stdout,
            output.stderr
        );
        assert!(
            output.stdout.contains("outside") || output.stderr.contains("outside"),
            "{command} {flag} names the refusal: {} {}",
            output.stdout,
            output.stderr
        );
    }
}

/// A baseline file inside the project that does not parse is named by its
/// position only.
#[test]
fn baseline_parse_errors_name_the_position_only() {
    for (command, flag) in [
        ("dead-code", "--baseline"),
        ("dupes", "--baseline"),
        ("health", "--baseline"),
        ("dead-code", "--regression-baseline"),
    ] {
        let (project, _elsewhere, outside) = project_and_outside_file();
        let own = project.path().join("baseline.json");
        std::fs::copy(&outside, &own).expect("copy");
        let root = project.path().to_str().expect("UTF-8 path");
        let own = own.to_str().expect("UTF-8 path");
        let mut args = vec![command, "--root", root, flag, own, "--format", "json"];
        if flag == "--regression-baseline" {
            args.push("--fail-on-regression");
        }
        let output = run_fallow_raw(&args);

        assert_eq!(output.code, 2, "{command} {flag}: {}", output.stderr);
        assert!(
            !output.stdout.contains(OUTSIDE_VALUE) && !output.stderr.contains(OUTSIDE_VALUE),
            "{command} {flag}: {} {}",
            output.stdout,
            output.stderr
        );
    }
}
