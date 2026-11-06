//! Fallow tells Git to ignore its cache directory (`.fallow/.gitignore` holds
//! `*`). A baseline saved into that directory cannot be committed, so every
//! command that takes `--save-baseline` warns on stderr when the path is in
//! the cache directory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, fallow_bin, parse_json, run_fallow_raw, scrub_analysis_env};
use tempfile::TempDir;

const WARNING: &str = "is in the fallow cache directory .fallow";

fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cache-dir-baseline","version":"1.0.0","private":true,"main":"src/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(root.join("src/index.js"), "export const used = 1;\n").expect("entry");
    std::fs::write(root.join("src/unused.js"), "export const unused = 2;\n").expect("unused");
    dir
}

/// Run `command` (empty for bare `fallow`) with `--save-baseline <relative>`.
fn save(dir: &TempDir, command: &str, relative: &str) -> CommandOutput {
    let root = dir.path().to_str().expect("UTF-8 path");
    let path = dir.path().join(relative);
    let path = path.to_str().expect("UTF-8 path");
    let mut args = Vec::new();
    if !command.is_empty() {
        args.push(command);
    }
    args.extend([
        "--root",
        root,
        "--save-baseline",
        path,
        "--format",
        "json",
        "--quiet",
    ]);
    let output = run_fallow_raw(&args);
    assert!(
        output.code == 0 || output.code == 1,
        "{command}: {}",
        output.stderr
    );
    assert!(
        dir.path().join(relative).exists(),
        "{command} saved no baseline: {}",
        output.stderr
    );
    // The warning goes to stderr, so stdout stays one JSON document.
    parse_json(&output);
    output
}

#[test]
fn every_save_baseline_command_warns_about_the_cache_dir() {
    for (command, name) in [
        ("", "combined.json"),
        ("dead-code", "dead-code.json"),
        ("architecture", "architecture.json"),
        ("dupes", "dupes.json"),
        ("health", "health.json"),
    ] {
        let dir = project();
        let output = save(&dir, command, &format!(".fallow/{name}"));
        assert!(
            output
                .stderr
                .contains(&format!("the baseline .fallow/{name} {WARNING}")),
            "`fallow {command}` gave no cache-dir warning: {}",
            output.stderr
        );
        assert!(
            output.stderr.contains(&format!("fallow-baselines/{name}")),
            "`fallow {command}` names no committed path: {}",
            output.stderr
        );
    }
}

#[test]
fn a_committed_baseline_path_gives_no_warning() {
    for command in ["dead-code", "dupes", "health"] {
        let dir = project();
        let output = save(&dir, command, &format!("fallow-baselines/{command}.json"));
        assert!(
            !output.stderr.contains("fallow cache directory"),
            "`fallow {command}`: {}",
            output.stderr
        );
    }
}

/// A relative `--save-baseline` path is relative to the working directory,
/// not to `--root`. Run from the project directory with a `--root` spelled
/// through a subdirectory and `..`.
#[test]
fn a_relative_path_resolves_from_the_working_directory() {
    let dir = project();
    let mut cmd = std::process::Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .current_dir(dir.path().join("src"))
        .args([
            "dead-code",
            "--root",
            "..",
            "--save-baseline",
            "../.fallow/dead-code.json",
            "--format",
            "json",
            "--quiet",
        ])
        .output()
        .expect("run fallow");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        dir.path().join(".fallow/dead-code.json").exists(),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("the baseline .fallow/dead-code.json {WARNING}")),
        "{stderr}"
    );
}
