//! Project strings with control characters in text output, and project
//! strings in the `next_steps[].command` text.
//!
//! An import specifier, an export name and a file name come from the analyzed
//! project. A line break in one of them must not start a new output line in a
//! text format or on stderr, because a CI runner reads some line prefixes as
//! commands. A `next_steps[].command` must stay one shell command that runs as
//! written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{CommandOutput, fallow_command, parse_json, run_fallow_in_root};
use tempfile::tempdir;

/// The prefix of a forged line that a CI runner reads as a command. Fallow
/// prints its own `::notice::` lines in the `github-annotations` format.
const RUNNER_PREFIX: &str = "::notice title=Note";

fn write_package(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"text-output","type":"module"}"#,
    )
    .unwrap();
}

/// No line of `text` starts with `prefix`, also after leading spaces.
fn assert_no_line_starts_with(text: &str, prefix: &str, context: &str) {
    for line in text.lines() {
        assert!(
            !line.trim_start().starts_with(prefix),
            "{context}: a project string started a new line:\n{text}"
        );
    }
}

#[test]
fn an_import_specifier_with_a_line_break_stays_on_its_line_in_text_formats() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(
        root.join("src/index.ts"),
        "import { helper } from \"./helper\\n::notice title=Note::text\";\nhelper();\n",
    )
    .unwrap();

    for format in ["human", "compact", "markdown"] {
        let output = run_fallow_in_root("dead-code", root, &["--format", format]);
        assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, format);
        assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, format);
        assert!(
            output
                .stdout
                .contains(r"./helper\n::notice title=Note::text"),
            "{format}: the specifier must stay readable with an escaped line break:\n{}",
            output.stdout
        );
    }
}

#[cfg(unix)]
#[test]
fn a_parse_error_gate_file_name_with_a_line_break_stays_on_its_line() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"entry":["src/index.ts"],"failOnParseError":true}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    std::fs::write(
        root.join("src/broken\n::notice title=Note::text\n# Heading\n.ts"),
        "export const = (\n",
    )
    .unwrap();

    for format in [
        "human",
        "compact",
        "markdown",
        "github-annotations",
        "pr-comment-github",
    ] {
        let output = run_fallow_in_root("dead-code", root, &["--format", format]);
        assert_eq!(output.code, 1, "{format}: stderr:\n{}", output.stderr);
        assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, format);
        assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, format);
        assert_no_line_starts_with(&output.stdout, "# Heading", format);
        assert!(
            output
                .stderr
                .contains(r"src/broken\n::notice title=Note::text\n# Heading\n.ts"),
            "{format}: the stderr gate note must name the file with escaped line breaks:\n{}",
            output.stderr
        );
    }
}

fn trace_unused_export_command(root: &Path) -> String {
    let output = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let step = json["next_steps"]
        .as_array()
        .expect("next_steps array")
        .iter()
        .find(|step| step["id"] == "trace-unused-export")
        .expect("trace-unused-export step");
    step["command"].as_str().expect("command text").to_owned()
}

#[test]
fn a_trace_next_step_quotes_an_export_name_with_shell_syntax() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(
        root.join("src/index.js"),
        "import { used } from './lib.js';\nused;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.js"),
        "export const used = 1;\nconst value = 2;\nexport { value as \"x; touch made-by-step #\" };\n",
    )
    .unwrap();

    assert_eq!(
        trace_unused_export_command(root),
        "fallow dead-code --trace='src/lib.js:x; touch made-by-step #'"
    );
}

#[test]
fn a_trace_next_step_keeps_a_plain_target_unquoted() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(
        root.join("src/index.js"),
        "import { used } from './lib.js';\nused;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.js"),
        "export const used = 1;\nexport const unusedHelper = 2;\n",
    )
    .unwrap();

    assert_eq!(
        trace_unused_export_command(root),
        "fallow dead-code --trace=src/lib.js:unusedHelper"
    );
}

/// The terminal sequence that hides all later text.
const HIDE_SEQUENCE: &str = "\u{1b}[8m";

/// Run fallow in `root` with colors forced on or left off.
fn run_with_color(root: &Path, args: &[&str], color: bool) -> CommandOutput {
    let mut cmd = fallow_command();
    if color {
        cmd.env_remove("NO_COLOR").env("CLICOLOR_FORCE", "1");
    }
    cmd.args(args).arg("--root").arg(root);
    CommandOutput::from_process(cmd.output().expect("run fallow"))
}

#[cfg(unix)]
#[test]
fn a_terminal_sequence_in_a_file_name_is_escaped_with_color_on_and_off() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(root.join(".fallowrc.json"), r#"{"entry":["src/index.ts"]}"#).unwrap();
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    std::fs::write(root.join("src/hid\u{1b}[8mden.ts"), "export const x = 1;\n").unwrap();

    for color in [false, true] {
        let output = run_with_color(root, &["dead-code"], color);
        assert!(
            !output.stdout.contains(HIDE_SEQUENCE),
            "color {color}: a file name sent a terminal sequence:\n{}",
            output.stdout
        );
        assert!(
            output.stdout.contains(r"hid\u{001B}[8mden.ts"),
            "color {color}: the file name must stay readable:\n{}",
            output.stdout
        );
        assert_eq!(
            output.stdout.contains('\u{1b}'),
            color,
            "color {color}: fallow's own colors follow the color setting:\n{}",
            output.stdout
        );
    }
}

/// Run fallow in `root` with the default log filter, so the warnings that a
/// default run prints reach stderr.
fn run_with_default_log(root: &Path, args: &[&str]) -> CommandOutput {
    let mut cmd = fallow_command();
    cmd.env_remove("RUST_LOG")
        .args(args)
        .arg("--root")
        .arg(root);
    CommandOutput::from_process(cmd.output().expect("run fallow"))
}

#[cfg(unix)]
#[test]
fn a_parse_degraded_note_for_a_file_name_with_a_line_break_stays_on_its_line() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    std::fs::write(
        root.join("src/br\n::notice title=Note::text\n.ts"),
        "export const = (\n",
    )
    .unwrap();

    for args in [&["dead-code"][..], &["health", "--complexity"][..]] {
        let output = run_with_default_log(root, args);
        let context = args.join(" ");
        assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, &context);
        assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, &context);
        assert!(
            output
                .stderr
                .contains(r"src/br\n::notice title=Note::text\n.ts"),
            "{context}: the stderr warning must name the file with escaped line breaks:\n{}",
            output.stderr
        );
    }
}

#[test]
fn a_package_name_with_a_line_break_stays_on_its_row_in_grouped_health() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("packages/a/src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a/package.json"),
        r#"{"name":"a\n::notice title=Note::text","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a/src/index.ts"),
        "export const f = (a: number) => (a > 1 ? a : 0);\n",
    )
    .unwrap();

    let output = run_fallow_in_root("health", root, &["--group-by", "package"]);
    assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, "health --group-by");
    assert!(
        output.stdout.contains(r"a\n::notice title=Note::text"),
        "the package name must stay readable:\n{}",
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn list_prints_file_and_script_names_on_their_lines() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"list","scripts":{"go\n::notice title=Note::text":"node src/main.js"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/main.js"), "export const x = 1;\n").unwrap();
    std::fs::write(
        root.join("src/a\n::notice title=Note::text.js"),
        "export {};\n",
    )
    .unwrap();

    let output = run_fallow_in_root("list", root, &["--files", "--entry-points"]);
    assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, "list");
    assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, "list");
    assert!(
        output
            .stdout
            .contains(r"src/a\n::notice title=Note::text.js"),
        "the file name must stay readable:\n{}",
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn a_baseline_path_with_a_line_break_stays_on_its_stderr_line() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    for command in ["dead-code", "dupes"] {
        std::fs::write(root.join("src/old.ts"), "export const old = 1;\n").unwrap();
        let name = format!("{command}\n::notice title=Note::text.json");
        let baseline = root.join(&name);
        let baseline = baseline.to_str().unwrap();
        let saved = run_fallow_in_root(command, root, &["--save-baseline", baseline]);
        assert!(saved.code < 2, "{command}: stderr:\n{}", saved.stderr);
        std::fs::remove_file(root.join("src/old.ts")).unwrap();
        let output = run_fallow_in_root(command, root, &["--baseline", baseline]);
        assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, command);
        assert!(
            output.stderr.contains(&name.replace('\n', r"\n")),
            "{command}: the baseline path must stay readable:\n{}",
            output.stderr
        );
    }
}

#[cfg(unix)]
#[test]
fn a_config_value_with_a_line_break_stays_on_its_warning_line() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    write_package(&root);
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    let outside = "outside\n::notice title=Note::text.json";
    std::fs::write(dir.path().join(outside), "{}").unwrap();
    let config = serde_json::json!({ "plugins": [format!("../{outside}")] });
    std::fs::write(root.join(".fallowrc.json"), config.to_string()).unwrap();

    let output = run_with_default_log(&root, &["dead-code"]);
    assert_no_line_starts_with(&output.stderr, RUNNER_PREFIX, "plugin warning");
    assert!(
        output
            .stderr
            .contains(r"outside\n::notice title=Note::text.json"),
        "the warning must name the path with an escaped line break:\n{}",
        output.stderr
    );
}

#[cfg(unix)]
#[test]
fn uncolored_health_values_drop_their_terminal_sequence_with_color_on() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("packages/a/src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a/package.json"),
        r#"{"name":"a\u001b[8mhidden","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a/src/index.ts"),
        "export const f = (a: number) => (a > 1 ? a : 0);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/a/src/br\u{1b}[8mx.ts"),
        "export const = (\n",
    )
    .unwrap();

    for args in [
        &["health", "--group-by", "package"][..],
        &["health", "--complexity"][..],
    ] {
        let output = run_with_color(root, args, true);
        assert!(
            !output.stdout.contains(HIDE_SEQUENCE),
            "{}: a project value sent a terminal sequence:\n{}",
            args.join(" "),
            output.stdout
        );
    }
}

#[cfg(unix)]
#[test]
fn a_health_baseline_path_with_a_line_break_stays_on_its_stderr_line() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
    let name = "health\n::notice title=Note::text.json";
    let baseline = root.join(name);
    let baseline = baseline.to_str().unwrap();

    let saved = run_fallow_in_root("health", root, &["--save-baseline", baseline]);
    let output = run_fallow_in_root("health", root, &["--baseline", baseline]);
    for run in [&saved, &output] {
        assert_no_line_starts_with(&run.stderr, RUNNER_PREFIX, "health baseline");
    }
    assert!(
        output.stderr.contains(&name.replace('\n', r"\n")),
        "the baseline path must stay readable:\n{}",
        output.stderr
    );
}

#[cfg(unix)]
#[test]
fn a_markdown_clone_family_suggestion_keeps_its_file_name_on_its_line() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_package(root);
    use std::fmt::Write as _;
    let mut body = String::new();
    for i in 0..8 {
        let _ = write!(
            body,
            "export function f{i}(a: number, b: number) {{\n  const c = a * {i} + b;\n  \
             if (c > {i}) {{ return c - a; }}\n  return c + b * {i};\n}}\n"
        );
    }
    std::fs::write(root.join("src/a\n::notice title=Note::x.ts"), &body).unwrap();
    std::fs::write(root.join("src/b.ts"), &body).unwrap();

    let output = run_fallow_in_root("dupes", root, &["--format", "markdown"]);
    assert!(
        output.stdout.contains("Clone Families"),
        "the fixture must give a clone family:\n{}",
        output.stdout
    );
    assert_no_line_starts_with(&output.stdout, RUNNER_PREFIX, "dupes markdown");
}
