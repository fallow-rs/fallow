//! The complexity section of Markdown and the job summary on runs that do not
//! list complexity findings (`--score`, `--hotspots`, `--file-scores`,
//! `--targets`), with and without a `--baseline`, and on a
//! `--complexity --baseline` run.
//!
//! A run that does not list findings must not say that no function exceeds a
//! threshold. The job summary gives the count of functions above a threshold,
//! and with a baseline it tells how many of them the baseline accepts. A run
//! that lists findings, with a baseline that accepts every finding, stays
//! clean. The envelope names the sections it produced in `sections`, and an
//! older envelope without that member keeps the earlier rule.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{CommandOutput, parse_json, run_fallow_in_root, run_fallow_raw};

const BASELINE: &str = "baseline.json";
/// The note of an envelope that does not carry
/// `summary.baseline_staleness.remaining_findings`.
const BASELINE_NOTE: &str = "The count includes the functions that the baseline accepts.";

/// A function with `branches` independent `if` statements.
fn branchy(name: &str, branches: usize) -> String {
    let mut body = format!("export function {name}(x: number): number {{\n  let n = 0;\n");
    for i in 0..branches {
        let _ = writeln!(body, "  if (x > {i}) {{ n += {i}; }}");
    }
    body.push_str("  return n;\n}\n");
    body
}

/// Two complex functions in one file.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "score-baseline", "private": true, "main": "src/index.ts" }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("src/index.ts"),
        "export * from \"./a\";\nexport * from \"./b\";\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/a.ts"),
        format!("{}{}", branchy("alpha", 30), branchy("beta", 30)),
    )
    .expect("write a.ts");
    std::fs::write(root.join("src/b.ts"), "export const b = 1;\n").expect("write b.ts");
    dir
}

/// Save a baseline that accepts the current findings.
fn save_baseline(root: &Path) {
    let output = run_fallow_in_root(
        "health",
        root,
        &[
            "--save-baseline",
            &baseline(root),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    assert!(root.join(BASELINE).is_file(), "baseline not saved");
}

/// The baseline path. A relative path resolves against the working directory.
fn baseline(root: &Path) -> String {
    root.join(BASELINE).display().to_string()
}

/// Add one complex function in a new file. The baseline does not accept it.
fn add_new_function(root: &Path) {
    std::fs::write(root.join("src/b.ts"), branchy("gamma", 30)).expect("write b.ts");
}

fn health(root: &Path, args: &[&str], format: &str) -> CommandOutput {
    let mut all = args.to_vec();
    all.extend(["--format", format, "--quiet"]);
    let output = run_fallow_in_root("health", root, &all);
    assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
    output
}

/// Mask the elapsed time, which differs between two processes.
fn mask_elapsed(body: &str) -> String {
    let Some(offset) = body.find("\u{b7} ") else {
        return body.to_owned();
    };
    let start = offset + "\u{b7} ".len();
    let len = body[start..].bytes().take_while(u8::is_ascii_digit).count();
    format!("{}<n>{}", &body[..start], &body[start + len..])
}

/// Render a saved envelope with `report --from`.
fn report_from(root: &Path, saved: &Path, format: &str) -> CommandOutput {
    let root_arg = root.display().to_string();
    let saved_arg = saved.display().to_string();
    let output = run_fallow_raw(&[
        "report", "--from", &saved_arg, "--root", &root_arg, "--format", format, "--quiet",
    ]);
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    output
}

/// The job summary from a saved envelope is identical to the direct render.
fn assert_saved_summary_parity(root: &Path, args: &[&str]) {
    let json = health(root, args, "json");
    let saved = root.join("results.json");
    std::fs::write(&saved, &json.stdout).expect("write saved envelope");
    let direct = health(root, args, "github-summary");
    let from_saved = report_from(root, &saved, "github-summary");
    assert_eq!(
        mask_elapsed(&from_saved.stdout),
        mask_elapsed(&direct.stdout),
        "report --from must render the job summary of the direct run"
    );
}

/// The sentence of a baselined run that tells how many functions the
/// baseline accepts and how many are new.
fn remaining_note(accepted: usize, remaining: usize) -> String {
    format!(
        "The baseline accepts {accepted}. {remaining} {} new.",
        if remaining == 1 { "is" } else { "are" }
    )
}

/// Markdown and the job summary give the same count and the same note.
///
/// `remaining` is the number of functions that the baseline does not accept,
/// or `None` for a run without a baseline.
fn assert_counted(root: &Path, args: &[&str], above: usize, remaining: Option<usize>) {
    let markdown = health(root, args, "markdown").stdout;
    let summary = health(root, args, "github-summary").stdout;
    assert!(
        markdown.contains(&format!(
            "## Fallow: {above} functions exceed complexity thresholds"
        )),
        "{markdown}"
    );
    assert_summary_counted(&summary, above, remaining);
    assert_note(&markdown, above, remaining);
    assert_saved_summary_parity(root, args);
}

/// The job summary gives the count and the note of a run that does not list
/// complexity findings.
fn assert_summary_counted(summary: &str, above: usize, remaining: Option<usize>) {
    assert!(
        summary.contains(&format!("**{above} functions exceed thresholds**")),
        "{summary}"
    );
    assert_note(summary, above, remaining);
}

fn assert_note(body: &str, above: usize, remaining: Option<usize>) {
    assert!(!body.contains("o functions exceed"), "{body}");
    assert!(!body.contains(BASELINE_NOTE), "{body}");
    assert!(
        body.contains("This run does not list the functions."),
        "{body}"
    );
    match remaining {
        None => {
            assert!(!body.contains("baseline"), "{body}");
            assert!(body.contains("`fallow health --complexity`"), "{body}");
        }
        Some(remaining) => {
            assert!(
                body.contains(&remaining_note(above - remaining, remaining)),
                "{body}"
            );
            assert_eq!(
                body.contains("Run `fallow health --complexity` with the same `--baseline`"),
                remaining > 0,
                "{body}"
            );
        }
    }
}

/// The `sections` member and the remaining count of the JSON envelope.
fn envelope_facts(root: &Path, args: &[&str]) -> (Vec<String>, Option<u64>) {
    let json = parse_json(&health(root, args, "json"));
    let sections = json["sections"]
        .as_array()
        .unwrap_or_else(|| panic!("no sections member: {json}"))
        .iter()
        .map(|value| value.as_str().expect("section token").to_owned())
        .collect();
    let remaining = json["summary"]["baseline_staleness"]["remaining_findings"].as_u64();
    (sections, remaining)
}

#[test]
fn score_only_without_baseline_counts_unlisted_functions() {
    let dir = project();
    assert_counted(dir.path(), &["--score"], 2, None);
    let (sections, remaining) = envelope_facts(dir.path(), &["--score"]);
    assert_eq!(sections, ["score"]);
    assert_eq!(remaining, None);
}

#[test]
fn score_only_with_every_finding_baselined_counts_unlisted_functions() {
    let dir = project();
    save_baseline(dir.path());
    let baseline = baseline(dir.path());
    let args = ["--score", "--baseline", baseline.as_str()];
    assert_counted(dir.path(), &args, 2, Some(0));
    assert_eq!(envelope_facts(dir.path(), &args).1, Some(0));
}

#[test]
fn score_only_with_one_new_finding_counts_unlisted_functions() {
    let dir = project();
    save_baseline(dir.path());
    add_new_function(dir.path());
    let baseline = baseline(dir.path());
    let args = ["--score", "--baseline", baseline.as_str()];
    assert_counted(dir.path(), &args, 3, Some(1));
    assert_eq!(envelope_facts(dir.path(), &args).1, Some(1));
}

#[test]
fn complexity_with_every_finding_baselined_stays_clean() {
    let dir = project();
    save_baseline(dir.path());
    let baseline = baseline(dir.path());
    let args = ["--complexity", "--baseline", baseline.as_str()];
    let markdown = health(dir.path(), &args, "markdown").stdout;
    let summary = health(dir.path(), &args, "github-summary").stdout;
    assert!(
        summary.contains("**No functions exceed complexity thresholds**"),
        "{summary}"
    );
    for body in [&markdown, &summary] {
        assert!(
            !body.contains("functions exceed complexity thresholds\n"),
            "{body}"
        );
        assert!(!body.contains("exceed thresholds**"), "{body}");
        assert!(!body.contains("--complexity"), "{body}");
        assert!(!body.contains(BASELINE_NOTE), "{body}");
        assert!(!body.contains("The baseline accepts"), "{body}");
    }
    let (sections, remaining) = envelope_facts(dir.path(), &args);
    assert!(sections.iter().any(|s| s == "complexity"), "{sections:?}");
    assert_eq!(remaining, Some(0));
    assert_saved_summary_parity(dir.path(), &args);
}

/// The sections that a run without complexity findings asks for. The human
/// output of these runs has no complexity section, and Markdown follows it.
const UNLISTED_RUNS: [&str; 3] = ["--hotspots", "--file-scores", "--targets"];

#[test]
fn unlisted_sections_without_baseline_count_unlisted_functions() {
    let dir = project();
    for flag in UNLISTED_RUNS {
        let args = [flag];
        let markdown = health(dir.path(), &args, "markdown").stdout;
        assert!(
            !markdown.contains("exceed complexity thresholds"),
            "{flag}: {markdown}"
        );
        let summary = health(dir.path(), &args, "github-summary").stdout;
        assert_summary_counted(&summary, 2, None);
        let (sections, remaining) = envelope_facts(dir.path(), &args);
        assert!(
            !sections.iter().any(|s| s == "complexity"),
            "{flag}: {sections:?}"
        );
        assert!(
            sections.iter().any(|s| s == &flag[2..]),
            "{flag}: {sections:?}"
        );
        assert_eq!(remaining, None, "{flag}");
        assert_saved_summary_parity(dir.path(), &args);
    }
}

#[test]
fn unlisted_sections_with_baseline_do_not_report_a_clean_run() {
    let dir = project();
    save_baseline(dir.path());
    add_new_function(dir.path());
    let baseline = baseline(dir.path());
    for flag in UNLISTED_RUNS {
        let args = [flag, "--baseline", baseline.as_str()];
        let markdown = health(dir.path(), &args, "markdown").stdout;
        assert!(
            !markdown.contains("exceed complexity thresholds"),
            "{flag}: {markdown}"
        );
        assert!(
            !markdown.contains("o functions exceed"),
            "{flag}: {markdown}"
        );
        let summary = health(dir.path(), &args, "github-summary").stdout;
        assert_summary_counted(&summary, 3, Some(1));
        assert_eq!(envelope_facts(dir.path(), &args).1, Some(1), "{flag}");
        assert_saved_summary_parity(dir.path(), &args);
    }
}

/// `report --from` on an envelope from an older fallow, without `sections`
/// and without `remaining_findings`, keeps the earlier rule and note.
#[test]
fn older_envelope_without_the_new_members_keeps_the_earlier_rule() {
    let dir = project();
    save_baseline(dir.path());
    add_new_function(dir.path());
    let baseline = baseline(dir.path());
    let args = ["--score", "--baseline", baseline.as_str()];
    let mut json = parse_json(&health(dir.path(), &args, "json"));
    json.as_object_mut().expect("envelope").remove("sections");
    if let Some(staleness) = json["summary"]["baseline_staleness"].as_object_mut() {
        staleness.remove("remaining_findings");
    }
    let saved = dir.path().join("old.json");
    std::fs::write(&saved, serde_json::to_string(&json).expect("serialize"))
        .expect("write old envelope");
    let output = report_from(dir.path(), &saved, "github-summary").stdout;
    assert!(
        output.contains("**3 functions exceed thresholds**"),
        "{output}"
    );
    assert!(output.contains(BASELINE_NOTE), "{output}");
    assert!(!output.contains("The baseline accepts"), "{output}");
}

/// Only health reports the remaining count. Dead code and duplication do not
/// emit it.
#[test]
fn dead_code_and_dupes_do_not_emit_the_remaining_count() {
    let dir = project();
    let root = dir.path();
    for command in ["dead-code", "dupes"] {
        let path = root
            .join(format!("{command}-baseline.json"))
            .display()
            .to_string();
        let saved = run_fallow_in_root(command, root, &["--save-baseline", &path, "--quiet"]);
        assert!(matches!(saved.code, 0 | 1), "stderr:\n{}", saved.stderr);
        let output = run_fallow_in_root(
            command,
            root,
            &["--baseline", &path, "--format", "json", "--quiet"],
        );
        assert!(matches!(output.code, 0 | 1), "stderr:\n{}", output.stderr);
        let json = parse_json(&output);
        let staleness = &json["baseline_staleness"];
        assert!(staleness.is_object(), "{command}: {json}");
        assert!(
            staleness.get("remaining_findings").is_none(),
            "{command}: {staleness}"
        );
    }
}
