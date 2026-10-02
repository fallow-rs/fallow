//! The complexity section of Markdown and the job summary on a `--score` run
//! with and without a `--baseline`, and on a `--complexity --baseline` run.
//!
//! A score-only run does not list findings. Its section must give the count
//! of functions above a threshold, also when a baseline is loaded. A run that
//! lists findings, with a baseline that accepts every finding, stays clean.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{CommandOutput, run_fallow_in_root, run_fallow_raw};

const BASELINE: &str = "baseline.json";
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

/// The job summary from a saved envelope is identical to the direct render.
fn assert_saved_summary_parity(root: &Path, args: &[&str]) {
    let json = health(root, args, "json");
    let saved = root.join("results.json");
    std::fs::write(&saved, &json.stdout).expect("write saved envelope");
    let direct = health(root, args, "github-summary");
    let root_arg = root.display().to_string();
    let saved_arg = saved.display().to_string();
    let from_saved = run_fallow_raw(&[
        "report",
        "--from",
        &saved_arg,
        "--root",
        &root_arg,
        "--format",
        "github-summary",
        "--quiet",
    ]);
    assert_eq!(from_saved.code, 0, "stderr:\n{}", from_saved.stderr);
    assert_eq!(
        mask_elapsed(&from_saved.stdout),
        mask_elapsed(&direct.stdout),
        "report --from must render the job summary of the direct run"
    );
}

/// Markdown and the job summary give the same count and the same note.
fn assert_counted(root: &Path, args: &[&str], above: usize, baselined: bool) {
    let markdown = health(root, args, "markdown").stdout;
    let summary = health(root, args, "github-summary").stdout;
    assert!(
        markdown.contains(&format!(
            "## Fallow: {above} functions exceed complexity thresholds"
        )),
        "{markdown}"
    );
    assert!(
        summary.contains(&format!("**{above} functions exceed thresholds**")),
        "{summary}"
    );
    for body in [&markdown, &summary] {
        assert!(!body.contains("o functions exceed"), "{body}");
        assert_eq!(body.contains(BASELINE_NOTE), baselined, "{body}");
        assert!(body.contains("`fallow health --complexity`"), "{body}");
    }
    assert_saved_summary_parity(root, args);
}

#[test]
fn score_only_without_baseline_counts_unlisted_functions() {
    let dir = project();
    assert_counted(dir.path(), &["--score"], 2, false);
}

#[test]
fn score_only_with_every_finding_baselined_counts_unlisted_functions() {
    let dir = project();
    save_baseline(dir.path());
    let baseline = baseline(dir.path());
    assert_counted(dir.path(), &["--score", "--baseline", &baseline], 2, true);
}

#[test]
fn score_only_with_one_new_finding_counts_unlisted_functions() {
    let dir = project();
    save_baseline(dir.path());
    add_new_function(dir.path());
    let baseline = baseline(dir.path());
    assert_counted(dir.path(), &["--score", "--baseline", &baseline], 3, true);
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
    }
    assert_saved_summary_parity(dir.path(), &args);
}
