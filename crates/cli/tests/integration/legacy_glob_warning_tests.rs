//! A single `*` in a config path glob no longer matches `/`. When the old
//! rule matched discovered files that the new rule does not, fallow prints
//! one warning for each pattern on stderr, with a `**` rewrite. JSON on
//! stdout does not change, and `--quiet` hides the warning.

#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, parse_json, run_fallow_raw_with_env};
use tempfile::TempDir;

/// A project with test files and generated files below the root only.
fn project(config: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    let files = [
        (
            "package.json",
            r#"{"name":"legacy-glob","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
        ),
        ("src/index.ts", "export const main = (): number => 1;\n"),
        ("src/deep/a.test.ts", "export const a = 1;\n"),
        ("src/deep/b.test.ts", "export const b = 2;\n"),
        ("src/generated/nested/one.ts", "export const one = 1;\n"),
        (".fallowrc.json", config),
    ];
    for (path, content) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, content).expect("write");
    }
    dir
}

/// Run with the default warning level. The harness turns tracing off with an
/// empty `RUST_LOG`, and this warning goes through tracing.
fn run(subcommand: &str, config: &str, extra: &[&str]) -> CommandOutput {
    let dir = project(config);
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let mut args = vec![subcommand, "--root", root, "--no-cache"];
    args.extend_from_slice(extra);
    run_fallow_raw_with_env(&args, &[("RUST_LOG", "warn")])
}

fn assert_warning(output: &CommandOutput, expected: &str) {
    assert!(
        output.stderr.contains(expected),
        "expected warning:\n{expected}\nstderr:\n{}",
        output.stderr
    );
}

fn assert_no_legacy_warning(output: &CommandOutput) {
    assert!(
        !output.stderr.contains("no longer matches"),
        "unexpected warning:\n{}",
        output.stderr
    );
}

#[test]
fn ignore_patterns_warns_with_the_globstar_rewrite() {
    let output = run(
        "dead-code",
        r#"{"ignorePatterns":["src/generated/*"]}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: ignorePatterns glob \"src/generated/*\" no longer matches 1 file, because \"*\" \
         and \"?\" no longer match \"/\". To keep the old match, use \"src/generated/**\".",
    );
}

#[test]
fn ignore_exports_file_warns() {
    let output = run(
        "dead-code",
        r#"{"ignoreExports":[{"file":"*","exports":["*"]}]}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: ignoreExports[].file glob \"*\" no longer matches 4 files",
    );
    assert_warning(&output, "To keep the old match, use \"**\".");
}

#[test]
fn health_ignore_warns() {
    let output = run("health", r#"{"health":{"ignore":["*.test.ts"]}}"#, &[]);
    assert_warning(
        &output,
        "fallow: health.ignore glob \"*.test.ts\" no longer matches 2 files, because \"*\" and \
         \"?\" no longer match \"/\". To keep the old match, use \"**/*.test.ts\".",
    );
}

#[test]
fn duplicates_ignore_warns() {
    let output = run("dupes", r#"{"duplicates":{"ignore":["src/*.ts"]}}"#, &[]);
    assert_warning(
        &output,
        "fallow: duplicates.ignore glob \"src/*.ts\" no longer matches 3 files",
    );
    assert_warning(&output, "use \"src/**/*.ts\".");
}

#[test]
fn overrides_files_warns() {
    let output = run(
        "dead-code",
        r#"{"overrides":[{"files":["*.test.ts"],"rules":{"unused-exports":"off"}}]}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: overrides[].files glob \"*.test.ts\" no longer matches 2 files",
    );
}

#[test]
fn ignore_findings_warns() {
    let output = run("dead-code", r#"{"ignoreFindings":["src/*.test.ts"]}"#, &[]);
    assert_warning(
        &output,
        "fallow: ignoreFindings glob \"src/*.test.ts\" no longer matches 2 files",
    );
    assert_warning(&output, "use \"src/**/*.test.ts\".");
}

#[test]
fn no_warning_when_the_pattern_uses_globstar() {
    let output = run(
        "dead-code",
        r#"{"ignorePatterns":["src/generated/**"],"health":{"ignore":["**/*.test.ts"]}}"#,
        &[],
    );
    assert_no_legacy_warning(&output);
}

#[test]
fn no_warning_when_both_rules_match_the_same_files() {
    let output = run(
        "dead-code",
        r#"{"ignorePatterns":["src/deep/*.test.ts"]}"#,
        &[],
    );
    assert_no_legacy_warning(&output);
}

#[test]
fn quiet_hides_the_warning_and_json_stdout_stays_valid() {
    let output = run(
        "dead-code",
        r#"{"ignorePatterns":["src/generated/*"]}"#,
        &["--quiet", "--format", "json"],
    );
    assert_no_legacy_warning(&output);
    let _ = parse_json(&output);

    let loud = run(
        "dead-code",
        r#"{"ignorePatterns":["src/generated/*"]}"#,
        &["--format", "json"],
    );
    assert_warning(&loud, "ignorePatterns glob \"src/generated/*\"");
    assert!(!loud.stdout.contains("no longer matches"));
    let _ = parse_json(&loud);
}
