//! A single `*` in a config path glob no longer matches `/`. When the old
//! rule matched discovered files that the new rule does not, fallow prints
//! one warning for each pattern on stderr. The warning names the files and
//! gives two fixes: write the names, or use a `**` rewrite. For `entry` and
//! `dynamicallyLoaded`, a file that another entry source declares does not
//! count. JSON on stdout does not change, and `--quiet` hides the warning.

#![allow(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use crate::common::{CommandOutput, parse_json, run_fallow_raw_with_env};
use tempfile::TempDir;

const PACKAGE_JSON: &str =
    r#"{"name":"legacy-glob","version":"1.0.0","private":true,"main":"src/index.ts"}"#;

/// A project with test files and generated files below the root only.
fn project(config: &str, package_json: &str) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    let files = [
        ("package.json", package_json),
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
    run_with_package_json(subcommand, config, PACKAGE_JSON, extra)
}

fn run_with_package_json(
    subcommand: &str,
    config: &str,
    package_json: &str,
    extra: &[&str],
) -> CommandOutput {
    let dir = project(config, package_json);
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

/// Text that every legacy glob warning holds.
const WARNING_MARKER: &str = "than before fallow 3.34.0";

fn assert_no_legacy_warning(output: &CommandOutput) {
    assert!(
        !output.stderr.contains(WARNING_MARKER) && !output.stderr.contains(" glob \""),
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
        "fallow: ignorePatterns glob \"src/generated/*\" matches 1 fewer file than before fallow \
         3.34.0. A single \"*\" or \"?\" now matches in one folder level only. This file no \
         longer matches: src/generated/nested/one.ts. To keep the one-level match and stop this \
         warning, write the names in place of \"*\" or \"?\", for example \"{name1,name2}\". To \
         match nested folders again, use \"src/generated/**\".",
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
        "fallow: ignoreExports[].file glob \"*\" matches 4 fewer files",
    );
    assert_warning(
        &output,
        "These files no longer match: src/deep/a.test.ts, src/deep/b.test.ts, \
         src/generated/nested/one.ts, and 1 more.",
    );
    assert_warning(&output, "To match nested folders again, use \"**\".");
}

#[test]
fn health_ignore_warns() {
    let output = run("health", r#"{"health":{"ignore":["*.test.ts"]}}"#, &[]);
    assert_warning(
        &output,
        "fallow: health.ignore glob \"*.test.ts\" matches 2 fewer files than before fallow \
         3.34.0.",
    );
    assert_warning(
        &output,
        "These files no longer match: src/deep/a.test.ts, src/deep/b.test.ts. To keep the \
         one-level match and stop this warning",
    );
    assert_warning(
        &output,
        "To match nested folders again, use \"**/*.test.ts\".",
    );
}

#[test]
fn duplicates_ignore_warns() {
    let output = run("dupes", r#"{"duplicates":{"ignore":["src/*.ts"]}}"#, &[]);
    assert_warning(
        &output,
        "fallow: duplicates.ignore glob \"src/*.ts\" matches 3 fewer files",
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
        "fallow: overrides[].files glob \"*.test.ts\" matches 2 fewer files",
    );
}

#[test]
fn ignore_findings_warns() {
    let output = run("dead-code", r#"{"ignoreFindings":["src/*.test.ts"]}"#, &[]);
    assert_warning(
        &output,
        "fallow: ignoreFindings glob \"src/*.test.ts\" matches 2 fewer files",
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
    assert!(!loud.stdout.contains(WARNING_MARKER));
    let _ = parse_json(&loud);
}

#[test]
fn entry_warns_with_the_files_that_no_other_source_declares() {
    let output = run("dead-code", r#"{"entry":["src/*.test.ts"]}"#, &[]);
    assert_warning(
        &output,
        "fallow: entry glob \"src/*.test.ts\" matches 2 fewer files than before fallow 3.34.0.",
    );
    assert_warning(
        &output,
        "These files no longer match: src/deep/a.test.ts, src/deep/b.test.ts.",
    );
}

#[test]
fn entry_skips_files_that_dynamically_loaded_declares() {
    let output = run(
        "dead-code",
        r#"{"entry":["src/*.test.ts"],"dynamicallyLoaded":["src/deep/a.test.ts"]}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: entry glob \"src/*.test.ts\" matches 1 fewer file",
    );
    assert_warning(&output, "This file no longer matches: src/deep/b.test.ts.");
}

#[test]
fn dynamically_loaded_warns_not_when_another_entry_source_declares_every_file() {
    let output = run(
        "dead-code",
        r#"{"dynamicallyLoaded":["src/*.test.ts"],"entry":["src/deep/*.test.ts"]}"#,
        &[],
    );
    assert_no_legacy_warning(&output);
}

#[test]
fn entry_skips_a_file_that_package_json_declares() {
    let output = run_with_package_json(
        "dead-code",
        r#"{"entry":["src/*.test.ts"]}"#,
        r#"{"name":"legacy-glob","version":"1.0.0","private":true,"main":"./src/deep/a.test.ts"}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: entry glob \"src/*.test.ts\" matches 1 fewer file",
    );
    assert_warning(&output, "This file no longer matches: src/deep/b.test.ts.");
}

#[test]
fn dynamically_loaded_warns_for_the_files_that_no_other_source_declares() {
    let output = run(
        "dead-code",
        r#"{"dynamicallyLoaded":["src/*.test.ts"],"entry":["src/deep/a.test.ts"]}"#,
        &[],
    );
    assert_warning(
        &output,
        "fallow: dynamicallyLoaded glob \"src/*.test.ts\" matches 1 fewer file",
    );
    assert_warning(&output, "This file no longer matches: src/deep/b.test.ts.");
}

#[test]
fn entry_skips_a_file_that_a_tooling_script_declares() {
    let output = run_with_package_json(
        "dead-code",
        r#"{"entry":["src/*.test.ts"]}"#,
        r#"{"name":"legacy-glob","version":"1.0.0","private":true,"main":"src/index.ts","scripts":{"lint":"node src/deep/a.test.ts"}}"#,
        &[],
    );
    assert_warning(&output, "This file no longer matches: src/deep/b.test.ts.");
}

/// A workspace export resolves to a path with a `./` part, for example
/// `packages/p/./lib/deep/a.ts`. The check must still see it as declared.
#[test]
fn entry_skips_a_file_that_a_workspace_export_declares() {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    let files = [
        (
            "package.json",
            r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "packages/p/package.json",
            r#"{"name":"p","exports":{".":"./lib/deep/a.ts"}}"#,
        ),
        ("packages/p/lib/deep/a.ts", "export const a = 1;\n"),
        ("packages/p/lib/deep/b.ts", "export const b = 2;\n"),
        (".fallowrc.json", r#"{"entry":["packages/p/lib/*.ts"]}"#),
    ];
    for (path, content) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, content).expect("write");
    }
    let root = root.to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw_with_env(
        &["dead-code", "--root", root, "--no-cache"],
        &[("RUST_LOG", "warn")],
    );
    assert_warning(
        &output,
        "This file no longer matches: packages/p/lib/deep/b.ts.",
    );
}
