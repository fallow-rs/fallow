#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The regression flags work only on the commands that own a regression
//! baseline. Every other command rejects them with exit code 2, instead of a
//! run that exits 0 and writes or compares nothing.

use std::path::{Path, PathBuf};

use crate::common::{CommandOutput, parse_json, run_fallow_raw_in_dir};

/// A small project with one source file.
fn write_project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name": "regression-scope"}"#).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").unwrap();
    (dir, root)
}

/// Run `fallow <command...> --root <root> <args...> --format json --quiet`
/// from `root`, so a relative save path resolves inside the project.
fn run(root: &Path, command: &[&str], args: &[&str]) -> CommandOutput {
    let root_arg = root.to_string_lossy().into_owned();
    let mut full: Vec<&str> = command.to_vec();
    full.extend(["--root", root_arg.as_str()]);
    full.extend(args);
    full.extend(["--format", "json", "--quiet"]);
    run_fallow_raw_in_dir(root, &full)
}

/// The `message` of the JSON error envelope.
fn error_message(output: &CommandOutput) -> String {
    parse_json(output)["message"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Each regression flag, with the arguments that turn it on.
const REGRESSION_FLAGS: [(&str, &[&str]); 4] = [
    (
        "--save-regression-baseline",
        &["--save-regression-baseline"],
    ),
    (
        "--save-regression-baseline",
        &["--save-regression-baseline", "regression.json"],
    ),
    ("--fail-on-regression", &["--fail-on-regression"]),
    (
        "--regression-baseline",
        &["--regression-baseline", "regression.json"],
    ),
];

#[test]
fn dupes_rejects_the_save_regression_baseline_flag_and_writes_no_file() {
    let (_dir, root) = write_project();
    let output = run(
        &root,
        &["dupes"],
        &["--save-regression-baseline", "regression.json"],
    );
    assert_eq!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let message = error_message(&output);
    assert!(
        message.contains("`fallow dupes`") && message.contains("--save-regression-baseline"),
        "{message}"
    );
    assert!(message.contains("fallow dupes --baseline"), "{message}");
    assert!(!root.join("regression.json").exists());
    assert!(!root.join(".fallowrc.json").exists());
}

#[test]
fn analysis_commands_without_a_regression_baseline_reject_every_regression_flag() {
    let (_dir, root) = write_project();
    for command in [&["dupes"][..], &["health"][..], &["audit"][..]] {
        for (flag, args) in REGRESSION_FLAGS {
            let output = run(&root, command, args);
            assert_eq!(
                output.code, 2,
                "{command:?} {args:?}: stdout: {}\nstderr: {}",
                output.stdout, output.stderr
            );
            let message = error_message(&output);
            assert!(message.contains(flag), "{command:?} {args:?}: {message}");
            assert!(
                message.contains(&format!("`fallow {}`", command[0])),
                "{command:?} {args:?}: {message}"
            );
        }
    }
    assert!(!root.join("regression.json").exists());
    assert!(!root.join(".fallowrc.json").exists());
}

#[test]
fn a_non_default_tolerance_is_rejected_where_no_regression_gate_runs() {
    let (_dir, root) = write_project();
    let output = run(&root, &["dupes"], &["--tolerance", "2"]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(error_message(&output).contains("--tolerance"));
}

#[test]
fn a_bare_run_without_dead_code_rejects_the_regression_flags() {
    let (_dir, root) = write_project();
    for selection in [
        &["--only", "dupes"][..],
        &["--only", "health"][..],
        &["--skip", "dead-code"][..],
    ] {
        let mut args = selection.to_vec();
        args.extend(["--save-regression-baseline", "regression.json"]);
        let output = run(&root, &[], &args);
        assert_eq!(output.code, 2, "{selection:?}: {}", output.stdout);
        let message = error_message(&output);
        assert!(
            message.contains("dead-code") && message.contains("--save-regression-baseline"),
            "{selection:?}: {message}"
        );
    }
    assert!(!root.join("regression.json").exists());
}

#[test]
fn commands_with_a_regression_baseline_still_save_it() {
    let (_dir, root) = write_project();
    for (command, file) in [
        (&["dead-code"][..], "dead-code.json"),
        (&["architecture"][..], "architecture.json"),
        (&[][..], "bare.json"),
        (&["list", "--entry-weight"][..], "entry-weight.json"),
    ] {
        let output = run(&root, command, &["--save-regression-baseline", file]);
        assert_ne!(
            output.code, 2,
            "{command:?}: stdout: {}\nstderr: {}",
            output.stdout, output.stderr
        );
        assert!(root.join(file).exists(), "{command:?}: {}", output.stderr);
    }
}

#[test]
fn a_command_outside_the_analysis_commands_rejects_the_regression_flags() {
    let (_dir, root) = write_project();
    let output = run(&root, &["fix"], &["--dry-run", "--fail-on-regression"]);
    assert_eq!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let message = error_message(&output);
    assert!(
        message.contains("This command") && message.contains("--fail-on-regression"),
        "{message}"
    );
}

#[test]
fn file_only_commands_recover_their_own_baseline() {
    let (_dir, root) = write_project();
    let file = "regression data.json";
    let path = root.join(file);
    let old = r#"{"schema_version":99,"fallow_version":"9","timestamp":"t"}"#;
    let quoted = if cfg!(windows) {
        r#""regression data.json""#
    } else {
        "'regression data.json'"
    };
    for (command, section) in [
        (&["flags", "--retirement"][..], "flags"),
        (&["list", "--entry-weight"][..], "entry_weight"),
    ] {
        let save = format!(
            "fallow {} --save-regression-baseline {quoted}",
            command.join(" ")
        );
        let check = ["--fail-on-regression", "--regression-baseline", file];
        let missing = run(&root, command, &check);
        assert_eq!(missing.code, 2);
        assert!(
            error_message(&missing).contains(&save),
            "{}",
            missing.stdout
        );
        std::fs::write(&path, old).unwrap();
        for args in [&check[..], &["--save-regression-baseline", file][..]] {
            let rejected = run(&root, command, args);
            assert_eq!(rejected.code, 2);
            let message = error_message(&rejected);
            assert!(
                message.contains(&save) && message.contains("backup"),
                "{message}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        }
        std::fs::rename(&path, root.join(format!("{section}.bak"))).unwrap();
        let saved = run(&root, command, &["--save-regression-baseline", file]);
        assert_eq!(saved.code, 0, "{}", saved.stdout);
        let baseline: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(baseline[section].is_object());
        assert_eq!(run(&root, command, &check).code, 0);
        std::fs::remove_file(&path).unwrap();
    }
}

#[test]
fn embedded_identity_hint_saves_into_the_config() {
    let (_dir, root) = write_project();
    let save = run(&root, &["dead-code"], &["--save-regression-baseline"]);
    assert_eq!(save.code, 0, "{}", save.stdout);
    let path = root.join(".fallowrc.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    config["regression"]["baseline"]["analysisIdentity"]["mode"] = "type-aware".into();
    std::fs::write(&path, config.to_string()).unwrap();
    let rejected = run(&root, &["dead-code"], &["--fail-on-regression"]);
    assert_eq!(rejected.code, 2);
    let message = error_message(&rejected);
    assert!(
        message.ends_with("fallow dead-code --save-regression-baseline"),
        "{message}"
    );
}

#[test]
fn regression_deltas_only_use_counts_the_baseline_can_store() {
    let fixture = crate::common::copy_fixture("unused-component-prop");
    let root = fixture.path();
    let save = run(root, &["dead-code"], &["--save-regression-baseline"]);
    assert_eq!(save.code, 0, "{}", save.stdout);
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 1;").unwrap();
    let check = run_fallow_raw_in_dir(root, &["dead-code", "--fail-on-regression"]);
    assert_eq!(check.code, 1);
    assert!(
        check.stderr.contains("unused_files: +1"),
        "{}",
        check.stderr
    );
    assert!(
        !check.stderr.contains("unused_component_props:"),
        "{}",
        check.stderr
    );

    let file = "zero.json";
    std::fs::write(root.join(file), r#"{"schema_version":2,"fallow_version":"3","timestamp":"t","check":{"unused_component_props":0}}"#).unwrap();
    let check = run_fallow_raw_in_dir(
        root,
        &[
            "dead-code",
            "--fail-on-regression",
            "--regression-baseline",
            file,
        ],
    );
    assert_eq!(check.code, 1);
    assert!(
        check.stderr.contains("unused_component_props: +2"),
        "{}",
        check.stderr
    );
}
