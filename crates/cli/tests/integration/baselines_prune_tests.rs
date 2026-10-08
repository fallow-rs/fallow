//! `fallow baselines prune` (issue #3281).
//!
//! A fixed finding leaves its entry in a committed baseline, and a whole-project
//! `--fail-on-stale-baseline` run then fails. Prune removes exactly the entries
//! that match no current finding, so the stale gate passes and the baseline
//! still hides the same findings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use crate::common::{CommandOutput, parse_json, run_fallow_raw, run_fallow_raw_with_env};
use serde_json::Value;
use tempfile::TempDir;

const CONFIG: &str = r#"{
  "audit": {
    "deadCodeBaseline": "baselines/dead-code.json",
    "healthBaseline": "baselines/health.json",
    "dupesBaseline": "baselines/dupes.json"
  }
}
"#;

/// A function long enough to form a clone group with a copy of itself.
fn clone_source(name: &str, constant: u32) -> String {
    format!(
        "export function {name}(items: number[], factor: number): number[] {{
  const out: number[] = [];
  for (const item of items) {{
    if (item > {constant}) {{
      out.push(item * factor + {constant});
    }} else if (item < -{constant}) {{
      out.push(item - factor * {constant});
    }} else {{
      out.push(item);
    }}
  }}
  const total = out.reduce((a, b) => a + b, 0);
  if (total > {constant} * 100) {{
    console.log(\"big total {constant}\", total);
  }}
  const sorted = [...out].sort((a, b) => a - b);
  const mid = sorted[Math.floor(sorted.length / 2)];
  console.log(\"median {constant}\", mid);
  return out.map((v) => v + mid - {constant});
}}
"
    )
}

/// A function above the default cyclomatic threshold.
fn complex_source(name: &str) -> String {
    let mut branches = String::new();
    for value in 0..30 {
        writeln!(branches, "  if (input === {value}) return {value};").unwrap();
    }
    format!("export function {name}(input: number): number {{\n{branches}  return -1;\n}}\n")
}

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Two unused files, two clone pairs and two complex functions, all reached
/// from the entry point except the unused files.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"prune-fx","version":"1.0.0","private":true,"main":"src/index.ts"}"#,
    );
    write(root, ".fallowrc.json", CONFIG);
    write(
        root,
        "src/index.ts",
        "import { a1 } from './a';\nimport { a2 } from './b';\nimport { c1 } from './c';\n\
         import { c2 } from './d';\nimport { first, second } from './complex';\n\
         console.log(a1, a2, c1, c2, first, second);\n",
    );
    write(root, "src/a.ts", &clone_source("a1", 7));
    write(root, "src/b.ts", &clone_source("a2", 7));
    write(root, "src/c.ts", &clone_source("c1", 13));
    write(root, "src/d.ts", &clone_source("c2", 13));
    write(
        root,
        "src/complex.ts",
        &format!("{}{}", complex_source("first"), complex_source("second")),
    );
    write(root, "src/old.ts", "export const old = 1;\n");
    write(root, "src/dead.ts", "export const dead = 2;\n");
    dir
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn save_all(dir: &TempDir) {
    for kind in ["dead-code", "health", "dupes"] {
        let path = dir.path().join(format!("baselines/{kind}.json"));
        let output = run_fallow_raw(&[
            kind,
            "--root",
            root_arg(dir),
            "--quiet",
            "--no-cache",
            "--save-baseline",
            path.to_str().unwrap(),
        ]);
        assert!(
            path.is_file(),
            "{kind} saved no baseline: {}",
            output.stderr
        );
    }
}

fn prune(dir: &TempDir, extra: &[&str]) -> (CommandOutput, Value) {
    let mut args = vec![
        "baselines",
        "prune",
        "--root",
        root_arg(dir),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw(&args);
    let json = parse_json(&output);
    (output, json)
}

fn file_status<'a>(json: &'a Value, kind: &str) -> &'a Value {
    json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|file| file["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} file in {json}"))
}

fn stale_gate(dir: &TempDir, kind: &str) -> CommandOutput {
    let path = dir.path().join(format!("baselines/{kind}.json"));
    run_fallow_raw(&[
        kind,
        "--root",
        root_arg(dir),
        "--quiet",
        "--no-cache",
        "--baseline",
        path.to_str().unwrap(),
        "--fail-on-stale-baseline",
    ])
}

/// Fix one finding of each kind: delete an unused file, one copy of a clone
/// pair, and one complex function.
fn fix_one_of_each(dir: &TempDir) {
    let root = dir.path();
    std::fs::remove_file(root.join("src/dead.ts")).unwrap();
    write(root, "src/d.ts", "export const c2 = 2;\n");
    write(
        root,
        "src/complex.ts",
        &format!("{}export const second = 2;\n", complex_source("first")),
    );
}

#[test]
fn a_fresh_save_has_nothing_to_prune() {
    let dir = project();
    save_all(&dir);
    let (output, json) = prune(&dir, &[]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    for kind in ["dead-code", "health", "dupes"] {
        assert_eq!(file_status(&json, kind)["status"], "unchanged", "{json}");
    }
    assert_eq!(json["actions"], serde_json::json!([]));
}

#[test]
fn prune_removes_fixed_entries_so_the_stale_gate_passes() {
    let dir = project();
    save_all(&dir);
    fix_one_of_each(&dir);
    for kind in ["dead-code", "health", "dupes"] {
        assert_eq!(
            stale_gate(&dir, kind).code,
            1,
            "{kind}: the fix leaves a stale entry"
        );
    }

    let (output, json) = prune(&dir, &[]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    for kind in ["dead-code", "health", "dupes"] {
        let file = file_status(&json, kind);
        assert_eq!(file["status"], "pruned", "{json}");
        assert!(
            file["entries_after"].as_u64() < file["entries_before"].as_u64(),
            "{file}"
        );
    }
    assert_eq!(json["actions"][0]["type"], "stage-baselines");

    for kind in ["dead-code", "health", "dupes"] {
        let gate = stale_gate(&dir, kind);
        assert_eq!(gate.code, 0, "{kind}: {}", gate.stderr);
    }
}

#[test]
fn check_reports_prunable_entries_and_writes_nothing() {
    let dir = project();
    save_all(&dir);
    fix_one_of_each(&dir);
    let before = std::fs::read(dir.path().join("baselines/dead-code.json")).unwrap();

    let (output, json) = prune(&dir, &["--check"]);
    assert_eq!(output.code, 1, "{}", output.stderr);
    assert_eq!(file_status(&json, "dead-code")["status"], "would-prune");
    assert_eq!(
        std::fs::read(dir.path().join("baselines/dead-code.json")).unwrap(),
        before
    );
    assert!(
        json["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["type"] == "prune-baselines"),
        "{json}"
    );
}

#[test]
fn prune_never_adds_a_new_finding() {
    let dir = project();
    save_all(&dir);
    write(dir.path(), "src/new-unused.ts", "export const fresh = 3;\n");
    let (output, json) = prune(&dir, &[]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(file_status(&json, "dead-code")["status"], "unchanged");
}

#[test]
fn prune_skips_a_baseline_with_shared_clone_keys() {
    let dir = project();
    save_all(&dir);
    let legacy =
        r#"{"kind":"dupes","normalized_clone_fingerprints":["dup:c77b3abb6f87acd9-r1:2"]}"#;
    write(dir.path(), "baselines/dupes.json", legacy);

    let (output, json) = prune(&dir, &[]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    let dupes = file_status(&json, "dupes");
    assert_eq!(dupes["status"], "refused");
    assert_eq!(dupes["reason_code"], "shared-clone-keys");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("baselines/dupes.json")).unwrap(),
        legacy
    );
    assert!(
        json["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["type"] == "resave-baseline"
                && action["command"] == "fallow dupes --save-baseline baselines/dupes.json"),
        "{json}"
    );
}

#[test]
fn prune_rejects_a_narrowed_run() {
    let dir = project();
    save_all(&dir);
    let (output, _) = prune(&dir, &["--changed-since", "HEAD"]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("--changed-since"),
        "{}",
        output.stdout
    );
}

#[test]
fn prune_without_a_configured_baseline_exits_2() {
    let dir = project();
    std::fs::remove_file(dir.path().join(".fallowrc.json")).unwrap();
    let (output, _) = prune(&dir, &[]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("audit.deadCodeBaseline"),
        "{}",
        output.stdout
    );
}

#[test]
fn a_cli_path_overrides_the_config_and_prunes_one_file() {
    let dir = project();
    save_all(&dir);
    std::fs::remove_file(dir.path().join(".fallowrc.json")).unwrap();
    std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();
    let path = dir.path().join("baselines/dead-code.json");
    let (output, json) = prune(&dir, &["--dead-code-baseline", path.to_str().unwrap()]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let files = json["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "{json}");
    assert_eq!(files[0]["status"], "pruned");
}

#[test]
fn a_missing_file_fails_alone_and_the_others_are_pruned() {
    let dir = project();
    save_all(&dir);
    std::fs::remove_file(dir.path().join("baselines/health.json")).unwrap();
    std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();

    let (output, json) = prune(&dir, &[]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert_eq!(file_status(&json, "health")["status"], "error");
    assert_eq!(file_status(&json, "dead-code")["status"], "pruned");
}

#[test]
fn prune_rejects_a_diff_from_the_environment() {
    let dir = project();
    save_all(&dir);
    let diff = dir.path().join("changes.diff");
    std::fs::write(
        &diff,
        "diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -1 +1 @@\n-x\n+y\n",
    )
    .unwrap();
    let output = run_fallow_raw_with_env(
        &[
            "baselines",
            "prune",
            "--root",
            root_arg(&dir),
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_DIFF_FILE", diff.to_str().unwrap())],
    );
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(
        output.stdout.contains("does not accept `FALLOW_DIFF_FILE`"),
        "{}",
        output.stdout
    );
}

/// The pre-commit hook of `fallow hooks install --target git --prune-baselines`,
/// run through a real `git commit`. The hook is a POSIX shell script.
#[cfg(unix)]
mod git_hook {
    use super::{project, root_arg, save_all, write};
    use crate::common::{commit_all, fallow_bin, git, git_capture, git_command, run_fallow_raw};
    use tempfile::TempDir;

    /// A committed project with saved baselines and the pruning pre-commit hook,
    /// on a feature branch off `main`.
    fn project_with_prune_hook() -> TempDir {
        let dir = project();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        save_all(&dir);
        commit_all(dir.path(), "baselines");
        git(dir.path(), &["checkout", "-q", "-b", "feature"]);
        let install = run_fallow_raw(&[
            "hooks",
            "install",
            "--target",
            "git",
            "--prune-baselines",
            "--root",
            root_arg(&dir),
        ]);
        assert_eq!(install.code, 0, "{}", install.stderr);
        dir
    }

    /// Commit through the installed hook, with this build of fallow on `PATH`.
    fn commit_through_hook(dir: &TempDir, args: &[&str]) -> std::process::Output {
        let bin_dir = fallow_bin().parent().unwrap().to_path_buf();
        let path = std::env::join_paths(
            std::iter::once(bin_dir)
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        git_command(dir.path())
            .env("PATH", path)
            .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", "change"])
            .args(args)
            .output()
            .unwrap()
    }

    #[test]
    fn the_pre_commit_hook_prunes_and_stages_the_baselines() {
        let dir = project_with_prune_hook();
        std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();
        git(dir.path(), &["add", "-A"]);

        let commit = commit_through_hook(&dir, &[]);
        assert!(
            commit.status.success(),
            "{}",
            String::from_utf8_lossy(&commit.stderr)
        );
        let changed = git_capture(dir.path(), &["show", "--name-only", "--format=", "HEAD"]);
        assert!(changed.contains("baselines/dead-code.json"), "{changed}");
        assert_eq!(git_capture(dir.path(), &["status", "--porcelain"]), "");
        let committed = git_capture(dir.path(), &["show", "HEAD:baselines/dead-code.json"]);
        assert!(!committed.contains("src/dead.ts"), "{committed}");
    }

    #[test]
    fn the_pre_commit_hook_skips_the_prune_with_unstaged_changes() {
        let dir = project_with_prune_hook();
        std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();
        git(dir.path(), &["add", "-A"]);
        write(dir.path(), "src/old.ts", "export const old = 10;\n");

        assert_prune_skipped(&dir, &[], "unstaged or untracked changes");
    }

    /// Commit through the hook, and check that the hook skipped the prune and
    /// that the commit carries no baseline change.
    fn assert_prune_skipped(dir: &TempDir, args: &[&str], reason: &str) {
        let commit = commit_through_hook(dir, args);
        let stderr = String::from_utf8_lossy(&commit.stderr);
        assert!(commit.status.success(), "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
        let changed = git_capture(dir.path(), &["show", "--name-only", "--format=", "HEAD"]);
        assert!(!changed.contains("baselines/"), "{changed}");
    }

    #[test]
    fn the_pre_commit_hook_skips_the_prune_with_an_untracked_file() {
        let dir = project_with_prune_hook();
        std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();
        git(dir.path(), &["add", "-A"]);
        write(dir.path(), "notes.txt", "draft\n");

        assert_prune_skipped(&dir, &[], "unstaged or untracked changes");
    }

    #[test]
    fn the_pre_commit_hook_skips_the_prune_for_a_commit_with_paths() {
        let dir = project_with_prune_hook();
        git(dir.path(), &["rm", "-q", "src/dead.ts"]);

        assert_prune_skipped(&dir, &["--", "src/dead.ts"], "temporary index");
        assert_eq!(git_capture(dir.path(), &["status", "--porcelain"]), "");
    }

    #[test]
    fn the_pre_commit_hook_prunes_on_commit_all() {
        let dir = project_with_prune_hook();
        std::fs::remove_file(dir.path().join("src/dead.ts")).unwrap();

        let commit = commit_through_hook(&dir, &["-a"]);
        assert!(
            commit.status.success(),
            "{}",
            String::from_utf8_lossy(&commit.stderr)
        );
        let changed = git_capture(dir.path(), &["show", "--name-only", "--format=", "HEAD"]);
        assert!(changed.contains("baselines/dead-code.json"), "{changed}");
        assert_eq!(git_capture(dir.path(), &["status", "--porcelain"]), "");
    }
}
