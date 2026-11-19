//! `--no-production` and an off `FALLOW_PRODUCTION` value turn off a config
//! `production: true` on `list`, `fix`, `viz`, `flags` and `suppressions`,
//! in the same order as `fallow dead-code`: the flag, then the variable, then
//! the config. `watch` has a unit test of its config load in `watch.rs`.
//!
//! The project has one test file. Production mode does not analyze it, so a
//! production run does not list it, reports the export that only the test
//! file reads, and finds no feature flag and no suppression in it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, fallow_bin, git, scrub_analysis_env};

const TEST_FILE: &str = "src/util.test.ts";

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"production-override","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './util';\nconsole.log(used);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.ts"),
        "export const used = 1;\nexport const testOnly = 2;\n",
    )
    .unwrap();
    std::fs::write(
        root.join(TEST_FILE),
        "import { testOnly } from './util';\n\
         // fallow-ignore-next-line unused-export\n\
         export const fixture = 3;\n\
         if (process.env.FEATURE_BETA) {\n  console.log(testOnly);\n}\n",
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), config).unwrap();
    git(root, &["init", "-q"]);
    dir
}

fn run(root: &Path, args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    for name in [
        "FALLOW_PRODUCTION",
        "FALLOW_PRODUCTION_DEAD_CODE",
        "FALLOW_PRODUCTION_HEALTH",
        "FALLOW_PRODUCTION_DUPES",
        "FALLOW_FORMAT",
        "FALLOW_QUIET",
    ] {
        cmd.env_remove(name);
    }
    for (name, value) in env {
        cmd.env(name, value);
    }
    let output = cmd
        .current_dir(root)
        .args(args)
        .arg("--root")
        .arg(root)
        .output()
        .expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn json(output: &CommandOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|err| panic!("json output: {err}\n{}\n{}", output.stdout, output.stderr))
}

/// Whether a run of a command, with the extra arguments and the variables,
/// analyzed the test file.
type Probe = fn(&Path, &[&str], &[(&str, &str)]) -> bool;

fn lists_the_test_file(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> bool {
    let mut args = vec!["list", "--files", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = json(&run(root, &args, env));
    output["files"]
        .as_array()
        .expect("files")
        .iter()
        .any(|file| file == TEST_FILE)
}

fn fix_keeps_the_export_of_the_test_file(
    root: &Path,
    extra: &[&str],
    env: &[(&str, &str)],
) -> bool {
    let mut args = vec!["fix", "--dry-run", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = json(&run(root, &args, env));
    !output["fixes"]
        .as_array()
        .expect("fixes")
        .iter()
        .any(|fix| fix["name"] == "testOnly")
}

fn flags_finds_the_flag_of_the_test_file(
    root: &Path,
    extra: &[&str],
    env: &[(&str, &str)],
) -> bool {
    let mut args = vec!["flags", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = json(&run(root, &args, env));
    output["feature_flags"]
        .as_array()
        .expect("feature_flags")
        .iter()
        .any(|flag| flag["path"] == TEST_FILE)
}

fn suppressions_lists_the_test_file(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> bool {
    let mut args = vec!["suppressions", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = json(&run(root, &args, env));
    output["files"]
        .as_array()
        .expect("files")
        .iter()
        .any(|file| file["path"] == TEST_FILE)
}

fn viz_draws_the_test_file(root: &Path, extra: &[&str], env: &[(&str, &str)]) -> bool {
    let mut args = vec!["viz", "--viz-format", "dot", "--no-open", "--quiet"];
    args.extend_from_slice(extra);
    let output = run(root, &args, env);
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    output.stdout.contains(TEST_FILE)
}

const PROBES: [(&str, Probe); 5] = [
    ("list", lists_the_test_file),
    ("fix", fix_keeps_the_export_of_the_test_file),
    ("flags", flags_finds_the_flag_of_the_test_file),
    ("suppressions", suppressions_lists_the_test_file),
    ("viz", viz_draws_the_test_file),
];

#[test]
fn config_production_applies_without_a_flag() {
    let dir = project(r#"{"production":true}"#);
    for (command, probe) in PROBES {
        assert!(!probe(dir.path(), &[], &[]), "{command}");
    }
}

#[test]
fn the_production_flag_beats_an_off_env_value() {
    let dir = project(r#"{"production":false}"#);
    for (command, probe) in PROBES {
        assert!(
            !probe(
                dir.path(),
                &["--production"],
                &[("FALLOW_PRODUCTION", "false")]
            ),
            "{command}"
        );
    }
}

/// Two tests for one command: `--no-production`, and `FALLOW_PRODUCTION=false`
/// without a flag, each with a config `production: true`.
macro_rules! off_beats_config {
    ($flag_test:ident, $env_test:ident, $probe:ident) => {
        #[test]
        fn $flag_test() {
            let dir = project(r#"{"production":true}"#);
            assert!($probe(dir.path(), &["--no-production"], &[]));
        }

        #[test]
        fn $env_test() {
            let dir = project(r#"{"production":true}"#);
            assert!($probe(dir.path(), &[], &[("FALLOW_PRODUCTION", "false")]));
        }
    };
}

off_beats_config!(
    list_no_production_beats_config_production,
    list_env_off_beats_config_production,
    lists_the_test_file
);
off_beats_config!(
    fix_no_production_beats_config_production,
    fix_env_off_beats_config_production,
    fix_keeps_the_export_of_the_test_file
);
off_beats_config!(
    flags_no_production_beats_config_production,
    flags_env_off_beats_config_production,
    flags_finds_the_flag_of_the_test_file
);
off_beats_config!(
    suppressions_no_production_beats_config_production,
    suppressions_env_off_beats_config_production,
    suppressions_lists_the_test_file
);
off_beats_config!(
    viz_no_production_beats_config_production,
    viz_env_off_beats_config_production,
    viz_draws_the_test_file
);

/// `fix` reads the global flags through the same resolver as the production
/// flags, so `--include-entry-exports` makes an unused export of the entry
/// file a fix candidate.
#[test]
fn fix_applies_include_entry_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"entry-exports","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const main = 1;\n").unwrap();
    git(root, &["init", "-q"]);

    let removes_main = |extra: &[&str]| -> bool {
        let mut args = vec!["fix", "--dry-run", "--format", "json", "--quiet"];
        args.extend_from_slice(extra);
        json(&run(root, &args, &[]))["fixes"]
            .as_array()
            .expect("fixes")
            .iter()
            .any(|fix| {
                fix["type"] == "remove_export"
                    && fix["path"] == "src/index.ts"
                    && fix["name"] == "main"
            })
    };
    assert!(!removes_main(&[]));
    assert!(removes_main(&["--include-entry-exports"]));
}

/// The duplicated lines of `path` in the data of a viz page.
fn viz_dup_lines(page: &str, path: &str) -> u64 {
    let start = page
        .find(r#"id="fallow-data""#)
        .and_then(|at| page[at..].find('>').map(|end| at + end + 1))
        .expect("viz data script");
    let end = start + page[start..].find("</script>").expect("end of viz data");
    let data: serde_json::Value = serde_json::from_str(&page[start..end]).expect("viz data");
    let keys = data["files"]["$k"].as_array().expect("file keys");
    let column = keys
        .iter()
        .position(|key| key == "dup_lines")
        .expect("dup_lines key");
    let row = data["files"]["$r"]
        .as_array()
        .expect("file rows")
        .iter()
        .find(|row| row[0] == path)
        .unwrap_or_else(|| panic!("no row for {path}"));
    row[column].as_u64().expect("dup_lines value")
}

/// Two equal functions under `tests/`, with a config that runs dead code
/// outside production mode and duplication in it. The map shows the files
/// of the dead-code mode, and production mode does not analyze `tests/`, so
/// the files have no duplicated lines.
#[test]
fn viz_duplication_uses_the_dupes_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"viz-dupes","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"production":{"deadCode":false,"dupes":true}}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const main = 1;\n").unwrap();
    let body = "export function compute(values: number[]): number {\n  let total = 0;\n  \
                for (const value of values) {\n    if (value > 10) {\n      total += value * 2;\n    \
                } else if (value > 5) {\n      total += value + 3;\n    } else {\n      \
                total -= value;\n    }\n  }\n  const scaled = total * 1.5;\n  \
                const rounded = Math.round(scaled);\n  return rounded > 100 ? rounded - 100 : rounded;\n}\n";
    std::fs::write(root.join("tests/a.ts"), body).unwrap();
    std::fs::write(root.join("tests/b.ts"), body).unwrap();
    git(root, &["init", "-q"]);

    let out = tempfile::tempdir().expect("output dir");
    let html = out.path().join("map.html");
    let output = run(
        root,
        &[
            "viz",
            "--no-open",
            "--quiet",
            "--out",
            html.to_str().expect("utf-8 path"),
        ],
        &[],
    );
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    let page = std::fs::read_to_string(&html).expect("viz page");
    assert_eq!(viz_dup_lines(&page, "tests/a.ts"), 0);

    // Without production mode, duplication finds the clone.
    std::fs::write(root.join(".fallowrc.json"), r#"{"production":false}"#).unwrap();
    let output = run(
        root,
        &[
            "viz",
            "--no-open",
            "--quiet",
            "--out",
            html.to_str().expect("utf-8 path"),
        ],
        &[],
    );
    assert_eq!(output.code, 0, "{}\n{}", output.stdout, output.stderr);
    let page = std::fs::read_to_string(&html).expect("viz page");
    assert!(viz_dup_lines(&page, "tests/a.ts") > 0);
}
