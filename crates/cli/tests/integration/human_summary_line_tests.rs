//! The summary lines of the human output agree with the result of the run.
//!
//! A `Failed:` line names only the analyses that fail the run. The mark and
//! the count of a health status line follow the findings that the reader
//! sees, so a finding that a baseline hides does not count. `health --css`
//! counts the styling findings, and an audit with inherited findings in its
//! changed files says that it found no introduced issues.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{CommandOutput, commit_all, git, run_fallow_in_root, run_fallow_raw};

const CONFIG: &str = r#"{
  "entry": ["src/index.js"],
  "health": { "maxCyclomatic": 5, "maxCognitive": 5, "maxCrap": 999999999 }
}"#;

/// One function above the complexity thresholds.
const COMPLEX: &str = "export function complex(x) {
  let total = 0;
  if (x === 1) total += 1;
  if (x === 2) total += 2;
  if (x === 3) total += 3;
  if (x === 4) total += 4;
  if (x === 5) total += 5;
  if (x === 6) total += 6;
  return total;
}
";

/// A body that `src/a.js` and `src/b.js` share, so the project has one clone
/// group that stays below the default duplication threshold of zero.
const CLONE_BODY: &str = "(n) => {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const value = i * 2 + 3;
    const scaled = value * 10;
    const offset = scaled - 42;
    total += offset > 0 ? offset : -offset;
  }
  const result = total * 10;
  const shifted = result - 42;
  return shifted > 0 ? shifted : -shifted;
};
";

/// Two styling findings: one selector with high complexity and one rule with
/// `!important` declarations.
const STYLES: &str = ".x .y .z .w div span a b i { color: #123456; margin: 7px; }
.p { color: #123457; padding: 13px; }
.q { color: #123458 !important; padding: 17px !important; }
";

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    fs::write(path, body).expect("write file");
}

/// The absolute path of the health baseline file in `dir`.
fn baseline_path(dir: &TempDir) -> String {
    dir.path()
        .join("health-baseline.json")
        .to_str()
        .expect("utf-8 path")
        .to_owned()
}

/// A project with one complex function, one clone group and a stylesheet.
fn project() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "summary-fixture", "private": true }"#,
    );
    write(root, ".fallowrc.json", CONFIG);
    write(
        root,
        "src/index.js",
        &format!(
            "import {{ a }} from './a';\nimport {{ b }} from './b';\nimport './styles.css';\nexport const run = () => a(1) + b(2) + complex(3);\n{COMPLEX}"
        ),
    );
    write(root, "src/a.js", &format!("export const a = {CLONE_BODY}"));
    write(root, "src/b.js", &format!("export const b = {CLONE_BODY}"));
    write(root, "src/styles.css", STYLES);
    dir
}

/// The `Failed:` line, or an empty string when the run printed none.
fn failed_line(output: &CommandOutput) -> &str {
    output
        .stderr
        .lines()
        .find(|line| line.starts_with("Failed: "))
        .unwrap_or_default()
}

/// The final health status line: the last line with "analyzed" in it.
fn health_status_line(output: &CommandOutput) -> String {
    let line = output
        .stderr
        .lines()
        .rev()
        .find(|line| line.contains(" analyzed"))
        .unwrap_or_default();
    redact_elapsed(line)
}

/// Replace the trailing elapsed time, for example ` (0.06s)`, with ` (Ns)`.
fn redact_elapsed(line: &str) -> String {
    match line.rfind(" (") {
        Some(at) if line.ends_with("s)") => format!("{} (Ns)", &line[..at]),
        _ => line.to_owned(),
    }
}

#[test]
fn bare_failed_line_omits_clone_groups_that_do_not_fail_the_run() {
    let dir = project();
    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("utf-8 path"),
        "--no-cache",
    ]);
    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(
        output.stdout.contains("Duplicates (1 clone group)"),
        "the fixture needs a clone group: {}{}",
        output.stdout,
        output.stderr
    );
    assert!(!failed_line(&output).contains("dupes"), "{}", output.stderr);
    insta::assert_snapshot!(
        "bare_failed_line_without_dupes",
        failed_line(&output).to_owned()
    );
}

#[test]
fn health_status_line_counts_only_findings_after_the_baseline() {
    let dir = project();
    let baseline = baseline_path(&dir);
    let saved = run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--save-baseline", &baseline],
    );
    assert!(
        dir.path().join("health-baseline.json").exists(),
        "{}",
        saved.stderr
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--baseline", &baseline],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        health_status_line(&output).starts_with("\u{2713} 0 above threshold"),
        "{}",
        output.stderr
    );
    insta::assert_snapshot!(
        "health_status_line_with_baseline",
        health_status_line(&output)
    );
}

#[test]
fn health_findings_header_counts_only_findings_after_the_baseline() {
    let dir = project();
    let baseline = baseline_path(&dir);
    run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--save-baseline", &baseline],
    );
    write(
        dir.path(),
        "src/c.js",
        &COMPLEX.replace("function complex", "function newer"),
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--baseline", &baseline],
    );
    let all = format!("{}{}", output.stdout, output.stderr);
    assert!(
        all.contains("High complexity functions (1)"),
        "the header counts the finding that the baseline does not accept: {all}"
    );
    assert!(!all.contains("--top 2"), "{all}");
    assert!(
        health_status_line(&output).contains("1 above threshold (1 in the baseline)"),
        "{}",
        output.stderr
    );
}

#[test]
fn bare_failed_line_omits_health_when_the_baseline_hides_every_finding() {
    let dir = project();
    let baseline = baseline_path(&dir);
    run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--save-baseline", &baseline],
    );
    write(dir.path(), "src/unused.js", "export const unused = 1;\n");

    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("utf-8 path"),
        "--no-cache",
        "--skip",
        "dupes",
        "--health-baseline",
        &baseline,
    ]);
    assert_eq!(output.code, 1, "{}", output.stderr);
    assert!(
        !failed_line(&output).contains("health"),
        "{}",
        output.stderr
    );
    insta::assert_snapshot!(
        "bare_failed_line_without_baselined_health",
        failed_line(&output).to_owned()
    );
}

#[test]
fn health_css_status_line_counts_styling_findings() {
    let dir = project();
    let output = run_fallow_in_root("health", dir.path(), &["--no-cache", "--css"]);
    let json = run_fallow_in_root(
        "health",
        dir.path(),
        &["--no-cache", "--css", "--format", "json", "--quiet"],
    );
    let report: serde_json::Value = serde_json::from_str(&json.stdout).expect("json output");
    assert_eq!(
        report["styling_findings"].as_array().map(Vec::len),
        Some(2),
        "{}",
        json.stdout
    );
    assert!(
        health_status_line(&output).contains("2 styling findings"),
        "{}",
        output.stderr
    );
    insta::assert_snapshot!("health_css_status_line", health_status_line(&output));
}

#[test]
fn audit_with_only_inherited_findings_says_no_introduced_issues() {
    let dir = project();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "core.autocrlf", "false"]);
    commit_all(root, "base");
    git(root, &["checkout", "-b", "head"]);
    write(
        root,
        "src/index.js",
        &format!(
            "import {{ a }} from './a';\nimport {{ b }} from './b';\nimport './styles.css';\nexport const run = () => a(1) + b(2) + complex(3);\n// changed\n{COMPLEX}"
        ),
    );
    commit_all(root, "head");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().expect("utf-8 path"),
        "--base",
        "main",
        "--no-cache",
    ]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stderr.contains("inherited finding"),
        "the fixture needs an inherited finding: {}",
        output.stderr
    );
    let status = output
        .stderr
        .lines()
        .find(|line| line.starts_with('\u{2713}') && line.contains("changed file"))
        .unwrap_or_default();
    assert!(status.contains("No introduced issues"), "{}", output.stderr);
    insta::assert_snapshot!("audit_status_line_with_inherited", redact_elapsed(status));
}
