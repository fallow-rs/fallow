//! The stderr warning of a deprecated option.
//!
//! The `fallow dead-code` options for the architecture issue types are
//! deprecated. They still select the findings, and each one prints one
//! warning that names the replacement. `--quiet` removes the warning.

use std::path::Path;

use crate::common::{CommandOutput, run_fallow_in_root};

/// A project with one import cycle between two files.
fn cycle_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let write = |rel: &str, body: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, body).expect("write file");
    };
    write(
        "package.json",
        r#"{ "name": "cycle-project", "private": true, "main": "src/index.ts" }"#,
    );
    write(
        "src/index.ts",
        "import { a } from './a';\nconsole.log(a);\n",
    );
    write(
        "src/a.ts",
        "import { b } from './b';\nexport const a = b + 1;\n",
    );
    write(
        "src/b.ts",
        "import { a } from './a';\nexport const b = 1;\nexport const c = () => a;\n",
    );
    dir
}

fn run(command: &str, root: &Path, args: &[&str]) -> CommandOutput {
    let mut full = vec!["--no-cache", "--format", "json"];
    full.extend_from_slice(args);
    run_fallow_in_root(command, root, &full)
}

const CIRCULAR_WARNING: &str = "warning: `fallow dead-code --circular-deps` is deprecated and will be removed in the next major; use `fallow architecture --cycles`.";

#[test]
fn a_deprecated_option_prints_one_warning_and_still_selects_its_findings() {
    let dir = cycle_project();
    let output = run("dead-code", dir.path(), &["--circular-deps"]);
    assert_eq!(
        output.stderr.matches(CIRCULAR_WARNING).count(),
        1,
        "stderr: {}",
        output.stderr
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).expect("json stdout");
    assert!(
        json["circular_dependencies"]
            .as_array()
            .is_some_and(|cycles| !cycles.is_empty()),
        "{}",
        output.stdout
    );
}

#[test]
fn each_deprecated_option_prints_its_own_warning() {
    let dir = cycle_project();
    let output = run(
        "dead-code",
        dir.path(),
        &["--circular-deps", "--boundary-violations"],
    );
    assert!(
        output.stderr.contains(CIRCULAR_WARNING),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains(
            "warning: `fallow dead-code --boundary-violations` is deprecated and will be removed in the next major; use `fallow architecture --boundaries`."
        ),
        "{}",
        output.stderr
    );
}

#[test]
fn quiet_and_current_options_print_no_deprecation_warning() {
    let dir = cycle_project();
    let quiet = run("dead-code", dir.path(), &["--circular-deps", "--quiet"]);
    assert!(!quiet.stderr.contains("is deprecated"), "{}", quiet.stderr);

    let current = run("architecture", dir.path(), &["--cycles"]);
    assert!(
        !current.stderr.contains("is deprecated"),
        "{}",
        current.stderr
    );
}
