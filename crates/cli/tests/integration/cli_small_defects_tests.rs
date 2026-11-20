//! Exit codes, single messages, quiet output and stable order of small CLI
//! paths: the bare badge run, `trace --path`, `config-schema` into a closed
//! pipe, the `--file` warning, quiet tracing warnings and the
//! `fix --dry-run` order.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::io::Read as _;
use std::process::{Command, Stdio};

use crate::common::{
    fallow_bin, parse_json, run_fallow_in_root, run_fallow_raw_in_dir, scrub_analysis_env,
};
use tempfile::tempdir;

/// `index -> used`, plus `orphan` that nothing imports, and a boundary zone
/// whose pattern matches no file.
fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"small-defects","type":"module"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"entry":["src/index.ts"],"boundaries":{"zones":[{"name":"ui","patterns":["src/ui/**"]},{"name":"api","patterns":["src/api/**"]}],"rules":[{"from":"ui","allow":[]}]}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './used';\nused();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/used.ts"),
        "export const used = () => 1;\nexport const unusedA = 2;\n",
    )
    .unwrap();
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 3;\n").unwrap();
}

#[test]
fn a_bare_badge_run_prints_one_error_and_exits_with_invalid_input() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_raw_in_dir(dir.path(), &["--format", "badge"]);
    let stderr = &output.stderr;
    assert_eq!(output.code, 2, "stderr:\n{stderr}");
    assert_eq!(
        stderr
            .matches("badge format is only supported for the health command")
            .count(),
        1,
        "stderr:\n{stderr}"
    );
}

#[test]
fn trace_path_without_a_route_prints_the_answer_once() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "trace",
        dir.path(),
        &["--path", "src/orphan.ts", "src/index.ts"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        output.stdout.contains("No import path."),
        "{}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("no import path from"),
        "the reason repeats the answer:\n{}",
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn config_schema_into_a_closed_pipe_exits_cleanly() {
    let mut child = Command::new(fallow_bin())
        .env("RUST_LOG", "")
        .arg("config-schema")
        .arg("--pretty")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Read one byte, then close the read end, as `| head -c 1` does.
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0_u8; 1];
    stdout.read_exact(&mut first).unwrap();
    drop(stdout);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("Broken pipe"), "stderr:\n{stderr}");
    assert!(!stderr.contains("panicked"), "stderr:\n{stderr}");
}

#[test]
fn the_file_warning_names_the_missing_path() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--file",
            "src/used.ts",
            "--file",
            "src/missing.ts",
            "--format",
            "json",
        ],
    );

    let warnings: Vec<&str> = output
        .stderr
        .lines()
        .filter(|line| line.contains("was not found in the project"))
        .collect();
    assert_eq!(warnings.len(), 1, "stderr:\n{}", output.stderr);
    assert!(
        warnings[0].starts_with("Warning: --file 'src/missing.ts' (resolved to '")
            && warnings[0].contains("missing.ts') was not found"),
        "{}",
        warnings[0]
    );
}

#[test]
fn quiet_hides_the_empty_boundary_zone_warning() {
    let dir = tempdir().unwrap();
    write_project(dir.path());

    let run = |quiet: bool| {
        let mut cmd = Command::new(fallow_bin());
        cmd.env_remove("RUST_LOG").env("NO_COLOR", "1");
        scrub_analysis_env(&mut cmd);
        cmd.arg("dead-code")
            .arg("--root")
            .arg(dir.path())
            .args(["--format", "json", "--no-cache"]);
        if quiet {
            cmd.arg("--quiet");
        }
        String::from_utf8_lossy(&cmd.output().unwrap().stderr).to_string()
    };

    assert!(
        run(false).contains("boundary zone 'api' matched 0 files"),
        "the warning shows without --quiet"
    );
    let quiet = run(true);
    assert!(
        !quiet.contains("matched 0 files"),
        "--quiet stderr:\n{quiet}"
    );
}

#[test]
fn fix_dry_run_lists_the_files_in_path_order() {
    let dir = tempdir().unwrap();
    write_project(dir.path());
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let mut index = String::new();
    for name in names {
        std::fs::write(
            dir.path().join(format!("src/{name}.ts")),
            format!("export const {name}Used = 1;\nexport const {name}Unused = 2;\n"),
        )
        .unwrap();
        writeln!(
            index,
            "import {{ {name}Used }} from './{name}';\nconsole.log({name}Used);"
        )
        .unwrap();
    }
    index.push_str("import { used } from './used';\nused();\n");
    std::fs::write(dir.path().join("src/index.ts"), index).unwrap();

    let output = run_fallow_in_root(
        "fix",
        dir.path(),
        &["--dry-run", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    let value = parse_json(&output);
    let paths: Vec<String> = value["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["type"] == "remove_export")
        .map(|fix| fix["path"].as_str().unwrap().to_string())
        .collect();
    assert!(paths.len() > names.len(), "paths: {paths:?}");
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}
