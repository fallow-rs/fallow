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

use crate::common::{parse_json, run_fallow_in_root};
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
        "fallow dead-code --trace 'src/lib.js:x; touch made-by-step #'"
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
        "fallow dead-code --trace src/lib.js:unusedHelper"
    );
}
