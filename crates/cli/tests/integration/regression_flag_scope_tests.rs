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
fn recovery_preserves_remote_config_permission_and_a_dash_prefixed_filename() {
    let (_dir, root) = write_project();
    let output = run_fallow_raw_in_dir(
        &root,
        &[
            "dead-code",
            "--allow-remote-extends",
            "--fail-on-regression",
            "--regression-baseline=--quiet",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.code, 2);
    let message = error_message(&output);
    let (_, command) = message.rsplit_once(": fallow ").unwrap();
    assert_eq!(
        command,
        "dead-code --save-regression-baseline=--quiet --allow-remote-extends"
    );
    let args: Vec<_> = command.split_whitespace().collect();
    assert_eq!(run_fallow_raw_in_dir(&root, &args).code, 0);
    let baseline: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("--quiet")).unwrap()).unwrap();
    assert!(baseline["check"].is_object());
}

#[test]
fn entry_weight_gate_without_a_baseline_path_recovers_the_selected_project_and_config() {
    use std::fs;

    use crate::common::{copy_fixture_tree, fixture_path};

    let caller = tempfile::tempdir().unwrap();
    let root = caller.path().join("project");
    copy_fixture_tree(&fixture_path("startup-import-weight"), &root);
    fs::write(root.join("src/extra-entry.ts"), "console.log(42);\n").unwrap();
    fs::write(
        caller.path().join("custom.json"),
        r#"{"entry":["src/extra-entry.ts"]}"#,
    )
    .unwrap();
    let output = run_fallow_raw_in_dir(
        caller.path(),
        &[
            "list",
            "--entry-weight",
            "--root",
            "project",
            "--config",
            "custom.json",
            "--fail-on-regression",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    let error = parse_json(&output);
    let (_, hint) = error["message"]
        .as_str()
        .unwrap()
        .rsplit_once(": fallow ")
        .expect("recovery command");
    let command = hint.replace("<PATH>", "chosen.json");
    let args: Vec<_> = command.split_whitespace().collect();
    let saved = run_fallow_raw_in_dir(caller.path(), &args);
    assert_eq!(saved.code, 0, "{}", saved.stderr);
    let baseline: serde_json::Value =
        serde_json::from_slice(&fs::read(caller.path().join("chosen.json")).unwrap()).unwrap();
    let paths: Vec<_> = baseline["entry_weight"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["src/extra-entry.ts", "src/index.ts"]);
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
    let rejected = run_fallow_raw_in_dir(
        &root,
        &["dead-code", "--fail-on-regression", "--format", "json"],
    );
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

#[test]
fn recovery_runs_from_the_original_directory_with_the_selected_project() {
    let (_dir, root) = write_project();
    let caller = root.parent().unwrap();
    let config = "custom.json";
    std::fs::write(caller.join(config), r#"{"typeAware":{"enabled":true}}"#).unwrap();
    for command in [
        &["dead-code", "--no-type-aware"][..],
        &["flags", "--retirement"],
        &["list", "--entry-weight"],
    ] {
        let mut args = command.to_vec();
        args.extend([
            "--root",
            "project",
            "--config",
            config,
            "--regression-baseline",
            "counts.json",
            "--fail-on-regression",
            "--format",
            "json",
            "--quiet",
        ]);
        let error = run_fallow_raw_in_dir(caller, &args);
        assert_eq!(error.code, 2, "{}", error.stdout);
        let message = error_message(&error);
        let (_, hint) = message
            .rsplit_once(": fallow ")
            .expect("a runnable recovery command");
        // These fixture paths contain no shell metacharacters.
        let mut save: Vec<_> = hint.split_whitespace().collect();
        assert!(
            save.contains(&"--root") && save.contains(&"--config"),
            "{hint}"
        );
        if command[0] == "dead-code" {
            assert!(save.contains(&"--no-type-aware"), "{hint}");
        }
        save.extend(["--format", "json", "--quiet"]);
        let saved = run_fallow_raw_in_dir(caller, &save);
        assert_eq!(saved.code, 0, "{}", saved.stdout);
        assert!(caller.join("counts.json").is_file());
        assert!(!root.join("counts.json").exists());
        std::fs::remove_file(caller.join("counts.json")).unwrap();
    }
}

#[test]
fn recovery_saves_the_production_counts() {
    let project = crate::common::copy_fixture("production-mode");
    let root = project.path();
    let counts = |path: &str| {
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap();
        saved["check"].clone()
    };
    for (flag, path) in [
        ("--no-production", "all.json"),
        ("--production", "prod.json"),
    ] {
        let saved = run_fallow_raw_in_dir(
            root,
            &["dead-code", flag, "--save-regression-baseline", path],
        );
        assert_ne!(saved.code, 2, "{}", saved.stderr);
    }
    assert_ne!(counts("all.json"), counts("prod.json"));
    let output = run_fallow_raw_in_dir(
        root,
        &[
            "dead-code",
            "--production",
            "--regression-baseline",
            "missing.json",
            "--fail-on-regression",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 2);
    let message = error_message(&output);
    let (_, hint) = message.rsplit_once(": fallow ").expect("recovery command");
    assert_eq!(
        hint,
        "dead-code --save-regression-baseline missing.json --production"
    );
    let args: Vec<_> = hint.split_whitespace().collect();
    assert_ne!(run_fallow_raw_in_dir(root, &args).code, 2);
    assert_eq!(counts("missing.json"), counts("prod.json"));
}

#[test]
fn combined_recovery_saves_the_type_coupling_identity() {
    let project = crate::common::copy_fixture("type-aware-unused-export-refinement");
    std::fs::write(
        project.path().join(".fallowrc.json"),
        r#"{"typeAware":{"require":"complete"}}"#,
    )
    .unwrap();
    let run = |args: &[&str]| {
        let output = crate::common::type_aware_sidecar_command(args)
            .current_dir(project.path())
            .args(["--format", "json", "--quiet"])
            .output()
            .unwrap();
        CommandOutput::from_process(output)
    };
    let gate = [
        "--type-aware",
        "--type-aware-require",
        "best-effort",
        "--fail-on-regression",
        "--regression-baseline",
        "counts.json",
    ];
    let missing = run(&gate);
    assert_eq!(missing.code, 2, "{}", missing.stdout);
    let message = error_message(&missing);
    let (_, hint) = message.rsplit_once(": fallow ").expect("recovery command");
    let saved = run(&hint.split_whitespace().collect::<Vec<_>>());
    assert_eq!(saved.code, 0, "{}", saved.stdout);
    let baseline: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.path().join("counts.json")).unwrap())
            .unwrap();
    assert!(
        baseline["analysis_identity"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "type-coupling"),
        "{baseline}"
    );
    let checked = run(&gate);
    assert_eq!(checked.code, 0, "{}", checked.stdout);
}

#[test]
fn recovery_explains_syntactic_fallback_and_saves_with_the_sidecar() {
    let project = crate::common::copy_fixture("type-aware-unused-export-refinement");
    let root = project.path();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"typeAware":{"enabled":true,"require":"best-effort"}}"#,
    )
    .unwrap();
    let missing = root.join("missing-sidecar");
    let output = crate::common::run_fallow_raw_in_dir_with_env(
        root,
        &[
            "dead-code",
            "--regression-baseline",
            "missing.json",
            "--fail-on-regression",
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_TYPE_AWARE_BIN", missing.to_str().unwrap())],
    );
    assert_eq!(output.code, 2);
    let message = error_message(&output);
    assert_eq!(
        message,
        "no regression baseline found at 'missing.json'.\n\
         The comparison ran in fallback syntactic mode. When the type-aware sidecar is available, \
         run: fallow dead-code --type-aware --save-regression-baseline missing.json"
    );
}

#[test]
fn recovery_keeps_the_selected_issue_types_and_entry_exports() {
    let project = crate::common::copy_fixture("production-mode");
    let root = project.path();
    let output = run_fallow_raw_in_dir(
        root,
        &[
            "dead-code",
            "--unused-exports",
            "--include-entry-exports",
            "--max-file-size",
            "2",
            "--fail-on-regression",
            "--regression-baseline",
            "missing.json",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 2);
    let message = error_message(&output);
    let (_, hint) = message.rsplit_once(": fallow ").expect("recovery command");
    assert_eq!(
        hint,
        "dead-code --save-regression-baseline missing.json --max-file-size 2 --unused-exports --include-entry-exports"
    );
}
