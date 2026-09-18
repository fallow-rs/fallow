//! `request_outcomes` is the channel that survives `--quiet --format json`.
//!
//! Both shipped CI integrations invoke fallow with `--quiet` and a machine
//! format, and both swallow its exit code, so a fact that lives only on stderr
//! reaches nobody (issues #2687, #2688). These tests therefore assert the
//! envelope on runs shaped exactly like the integrations' own, and the quiet
//! parity case is the one that would have caught the whole class.
//!
//! They pin behaviour rather than constants: the reason tokens are asserted
//! against runs driven into each failure mode, and the honoured case is
//! asserted to carry neither a reason nor a message, so "always report
//! not-applied" and "always report applied" both fail.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{CommandOutput, parse_json, run_fallow_raw, run_fallow_raw_with_env};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Run git against a fixture with a hermetic identity, so a contributor's own
/// git config cannot change what the test measures.
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .status()
        .expect("git command failed");
    assert!(status.success(), "git {args:?} failed");
}

/// [`project`] committed to a real repository, so a ref failure is git
/// rejecting the ref rather than there being no repository at all.
fn committed_project() -> TempDir {
    let dir = project();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);
    dir
}

/// A project with one unreferenced module, so every command has something to
/// report and the run is not trivially empty.
fn project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"request-fx","version":"1.0.0","private":true,"main":"src/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (): number => 1;\n",
    )
    .expect("entry");
    std::fs::write(
        root.join("src/orphan.ts"),
        "export const orphan = (): number => 2;\n",
    )
    .expect("orphan");
    dir
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn run(args: &[&str]) -> CommandOutput {
    run_fallow_raw(args)
}

fn request<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    let entry = &envelope["request_outcomes"][name];
    assert!(
        !entry.is_null(),
        "expected a `{name}` entry, got {}",
        envelope["request_outcomes"]
    );
    entry
}

/// A diff whose paths name no file under any candidate base, so the run cannot
/// place it and reports at full scope.
fn foreign_diff(root: &Path) -> String {
    let path = root.join("foreign.diff");
    std::fs::write(
        &path,
        "diff --git a/nowhere/x.ts b/nowhere/x.ts\n\
         --- a/nowhere/x.ts\n\
         +++ b/nowhere/x.ts\n\
         @@ -0,0 +1,1 @@\n\
         +export const a = 1;\n",
    )
    .expect("foreign diff");
    path.to_str().expect("utf8").to_owned()
}

/// A diff that names a file the project really has, generated from the project
/// root, so it places cleanly.
fn placeable_diff(root: &Path) -> String {
    let path = root.join("real.diff");
    std::fs::write(
        &path,
        "diff --git a/src/orphan.ts b/src/orphan.ts\n\
         --- a/src/orphan.ts\n\
         +++ b/src/orphan.ts\n\
         @@ -1,1 +1,1 @@\n\
         +export const orphan = (): number => 2;\n",
    )
    .expect("placeable diff");
    path.to_str().expect("utf8").to_owned()
}

/// The headline invariant: a run that was asked for nothing is byte-identical
/// to one produced before the object existed, on every carrying command.
/// Without this the additive-field exemption in
/// `docs/backwards-compatibility.md` would not apply and six envelopes would
/// owe a `schema_version` bump.
#[test]
fn a_run_asked_for_nothing_emits_no_request_outcomes_key() {
    let project = project();
    let root = root_arg(&project);
    for args in [
        vec!["dead-code", "--root", root, "--format", "json", "--quiet"],
        vec!["dupes", "--root", root, "--format", "json", "--quiet"],
        vec!["health", "--root", root, "--format", "json", "--quiet"],
        vec!["security", "--root", root, "--format", "json", "--quiet"],
        vec!["--root", root, "--format", "json", "--quiet"],
    ] {
        let envelope = parse_json(&run(&args));
        assert!(
            envelope.get("request_outcomes").is_none(),
            "`{}` was asked for nothing and must carry no key: {}",
            args[0],
            envelope["request_outcomes"]
        );
    }
}

/// An unresolvable ref widens the report on every command that widens rather
/// than exits, and every one of them says so on the wire.
#[test]
fn an_unresolvable_changed_since_reports_not_applied_on_every_carrying_command() {
    let project = project();
    let root = root_arg(&project);
    for command in [
        vec!["dead-code"],
        vec!["dupes"],
        vec!["health"],
        vec!["security"],
        vec![],
    ] {
        let mut args = command.clone();
        args.extend_from_slice(&[
            "--root",
            root,
            "--changed-since",
            "refs/heads/does-not-exist",
            "--format",
            "json",
            "--quiet",
        ]);
        let envelope = parse_json(&run(&args));
        let entry = request(&envelope, "changed-since");
        assert_eq!(
            entry["status"], "not-applied",
            "`{command:?}` widened and must report it: {entry}"
        );
        assert_eq!(entry["requested"], "refs/heads/does-not-exist");
        assert!(
            entry["reason"].is_string(),
            "an unapplied request carries a reason token: {entry}"
        );
        let message = entry["message"].as_str().expect("a remedy sentence");
        assert!(
            message.contains("covers the whole project"),
            "the sentence must say the report widened: {message}"
        );
        assert!(
            !message.contains('\n'),
            "the sentence travels into a CI annotation and must stay on one line: {message}"
        );
    }
}

/// A ref that fails validation before git is spawned is a different remedy
/// from one git could not resolve, and the two must not collapse into one
/// token.
#[test]
fn the_changed_since_reason_names_the_cause() {
    let outside_any_repo = TempDir::new().expect("non-repo dir");
    let outside = outside_any_repo.path().to_str().expect("utf8");
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        outside,
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]));
    let entry = request(&envelope, "changed-since");
    assert_eq!(entry["status"], "not-applied");
    assert_eq!(
        entry["reason"], "not-a-repository",
        "a directory outside any repository is its own cause: {entry}"
    );

    let repo = committed_project();
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root_arg(&repo),
        "--changed-since",
        "refs/heads/does-not-exist",
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(
        request(&envelope, "changed-since")["reason"],
        "git-failed",
        "a ref git rejected is a different cause from a missing repository"
    );
}

/// A ref this build rejects before spawning git never reaches the recorder:
/// the flag's own parser fails the run with exit 2 and an error document. That
/// is the right side to err on and it is why `invalid-ref` is a reason no CLI
/// run emits, so nothing should read the absent entry as "the scope applied".
#[test]
fn a_ref_the_flag_parser_rejects_fails_the_run_instead_of_widening() {
    let repo = committed_project();
    let out = run(&[
        "dead-code",
        "--root",
        root_arg(&repo),
        "--changed-since",
        "bad{ref",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        out.code, 2,
        "a malformed ref is invalid input, not a widened report: {}{}",
        out.stdout, out.stderr
    );
    let envelope = parse_json(&out);
    assert_eq!(envelope["error"], true, "{envelope}");
    assert!(envelope.get("request_outcomes").is_none(), "{envelope}");
}

/// `fallow audit` exits 2 rather than widen, so it owes no entry. Publishing
/// one would say a report was widened by a run that refused to produce one.
#[test]
fn audit_refuses_rather_than_widening_and_publishes_no_entry() {
    let project = project();
    let root = root_arg(&project);
    let out = run(&[
        "audit",
        "--root",
        root,
        "--base",
        "refs/heads/does-not-exist",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        out.code, 2,
        "audit exits on an unresolvable base: {}{}",
        out.stdout, out.stderr
    );
}

/// The widening sentence belongs to the commands that widen. Audit produces no
/// report at all, and its flag is `--base`, so neither the claim nor the other
/// command's flag name may appear on the run that refused.
#[test]
fn audit_states_the_cause_without_claiming_a_whole_project_report() {
    let project = committed_project();
    let root = root_arg(&project);
    let out = run(&[
        "audit",
        "--root",
        root,
        "--base",
        "refs/heads/does-not-exist",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert!(
        !out.stderr.contains("covers the whole project"),
        "a run that produced no report cannot claim a whole-project one: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains("--changed-since"),
        "audit's flag is --base: {}",
        out.stderr
    );
    let envelope = parse_json(&out);
    let message = envelope["message"].as_str().expect("an error document");
    assert!(
        message.contains("refs/heads/does-not-exist"),
        "the document names the ref that failed: {message}"
    );
    assert!(
        message.contains("unknown revision") || message.contains("ambiguous argument"),
        "the document carries git's own cause: {message}"
    );
    assert!(
        !message.contains('\n'),
        "the cause is folded onto one line: {message}"
    );
}

/// One case per diff stand-down, each driven into the real failure mode rather
/// than asserted against the constant that produced it.
#[test]
fn every_diff_stand_down_reports_its_own_reason() {
    let project = project();
    let root_path = project.path();
    let root = root_arg(&project);

    let foreign = foreign_diff(root_path);
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root,
        "--diff-file",
        &foreign,
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(
        request(&envelope, "diff-filter")["reason"],
        "foreign-namespace"
    );

    let missing = root_path.join("absent.diff");
    let missing = missing.to_str().expect("utf8");
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root,
        "--diff-file",
        missing,
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(request(&envelope, "diff-filter")["reason"], "unreadable");

    let invalid = root_path.join("invalid.diff");
    std::fs::write(&invalid, [0xff, 0xfe, b'\n']).expect("invalid utf-8 diff");
    let invalid = invalid.to_str().expect("utf8");
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root,
        "--diff-file",
        invalid,
        "--format",
        "json",
        "--quiet",
    ]));
    assert_eq!(request(&envelope, "diff-filter")["reason"], "not-utf8");
}

/// The positive case, which is what lets a reviewer read "this report IS
/// scoped to the change" off the envelope. An honoured request carries neither
/// a reason nor a sentence, so a consumer rendering `message` never states
/// prose about a run that did exactly what it was told.
#[test]
fn an_honoured_request_reports_applied_with_no_reason_and_no_message() {
    let project = project();
    let root_path = project.path();
    let root = root_arg(&project);
    let diff = placeable_diff(root_path);
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root,
        "--diff-file",
        &diff,
        "--format",
        "json",
        "--quiet",
    ]));
    let entry = request(&envelope, "diff-filter");
    assert_eq!(entry["status"], "applied", "{entry}");
    assert!(
        entry["reason"].is_null() && entry["message"].is_null(),
        "an applied request states the scope and nothing else: {entry}"
    );
    assert!(
        entry["requested"]
            .as_str()
            .expect("the source label")
            .starts_with("--diff-file "),
        "the label names the channel as the user spelled it: {entry}"
    );
}

/// The test that would have caught the whole class. `$FALLOW_DIFF_FILE` plus
/// `--quiet` is the exact shape the GitHub Action and the GitLab template use,
/// and it is the one shape where the CLI prints nothing at all.
#[test]
fn the_object_is_identical_with_and_without_quiet() {
    let project = project();
    let root_path = project.path();
    let root = root_arg(&project);
    let foreign = foreign_diff(root_path);

    let quiet = run_fallow_raw_with_env(
        &["dead-code", "--root", root, "--format", "json", "--quiet"],
        &[("FALLOW_DIFF_FILE", &foreign)],
    );
    assert!(
        quiet.stderr.is_empty(),
        "the env-var channel under --quiet prints nothing, which is the defect: {}",
        quiet.stderr
    );
    let loud = run_fallow_raw_with_env(
        &["dead-code", "--root", root, "--format", "json"],
        &[("FALLOW_DIFF_FILE", &foreign)],
    );

    let quiet = parse_json(&quiet);
    let loud = parse_json(&loud);
    assert_eq!(
        quiet["request_outcomes"], loud["request_outcomes"],
        "the recorded outcome cannot depend on whether anyone was watching"
    );
    assert_eq!(
        quiet["request_outcomes"]["diff-filter"]["reason"],
        "foreign-namespace"
    );
}

/// Both channels at once, with the exit code unchanged: this object reports a
/// scope, never a verdict, so nothing here may turn a passing run into a
/// failing one.
#[test]
fn both_channels_report_together_without_changing_the_exit_code() {
    let project = project();
    let root_path = project.path();
    let root = root_arg(&project);
    let foreign = foreign_diff(root_path);

    let baseline = run(&["dead-code", "--root", root, "--format", "json", "--quiet"]);
    let both = run(&[
        "dead-code",
        "--root",
        root,
        "--changed-since",
        "refs/heads/does-not-exist",
        "--diff-file",
        &foreign,
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        baseline.code, both.code,
        "an unapplied request must not change the exit code"
    );
    let envelope = parse_json(&both);
    assert_eq!(request(&envelope, "changed-since")["status"], "not-applied");
    assert_eq!(request(&envelope, "diff-filter")["status"], "not-applied");
}

/// The rendered pull-request comment is the surface a reviewer actually reads,
/// and the whole point of #2688 is that the body must not imply a scope the
/// run did not apply.
#[test]
fn the_rendered_pr_comment_body_names_the_unapplied_request() {
    let project = project();
    let root_path = project.path();
    let root = root_arg(&project);
    let foreign = foreign_diff(root_path);
    let out = run(&[
        "dead-code",
        "--root",
        root,
        "--diff-file",
        &foreign,
        "--format",
        "pr-comment-github",
        "--quiet",
    ]);
    assert!(
        out.stdout
            .contains("Request outcomes: not applied diff-filter (foreign-namespace)"),
        "the comment body must say the filter stood down: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("wider than requested"),
        "and what that means for the findings under it: {}",
        out.stdout
    );
}

/// A SARIF document that was written is published as honoured, so a consumer
/// can tell "uploaded nothing because nothing was asked" from "uploaded
/// nothing because the write failed" (issue #2690).
#[test]
fn a_written_sarif_file_reports_applied_with_its_path() {
    let project = project();
    let root = root_arg(&project);
    let sarif = project.path().join("out").join("results.sarif");
    let sarif_arg = sarif.to_str().expect("utf8");
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root,
        "--sarif-file",
        sarif_arg,
        "--format",
        "json",
        "--quiet",
    ]));
    let entry = request(&envelope, "sarif-file");
    assert_eq!(entry["status"], "applied");
    assert_eq!(entry["requested"], sarif_arg);
    assert!(
        entry["reason"].is_null() && entry["message"].is_null(),
        "an honoured request carries neither: {entry}"
    );
    assert!(sarif.is_file(), "the file the entry claims must exist");
}

/// The defect itself: the document on stdout is complete, the exit code is the
/// one the findings produced, and until now nothing anywhere said the SARIF
/// artefact a consumer uploads was never written.
#[cfg(unix)]
#[test]
fn an_unwritable_sarif_target_reports_not_applied_without_moving_the_exit_code() {
    use std::os::unix::fs::PermissionsExt;

    let project = project();
    let root = root_arg(&project);
    let locked = project.path().join("locked");
    std::fs::create_dir_all(&locked).expect("locked dir");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))
        .expect("drop write permission");

    let baseline = run(&["dead-code", "--root", root, "--format", "json", "--quiet"]);
    for (target, reason) in [
        (locked.join("results.sarif"), "write-failed"),
        (
            locked.join("nested").join("results.sarif"),
            "directory-create-failed",
        ),
    ] {
        let sarif_arg = target.to_str().expect("utf8").to_owned();
        let out = run(&[
            "dead-code",
            "--root",
            root,
            "--sarif-file",
            &sarif_arg,
            "--format",
            "json",
            "--quiet",
        ]);
        assert_eq!(
            baseline.code, out.code,
            "a missing secondary artefact must not move the exit code"
        );
        let envelope = parse_json(&out);
        let entry = request(&envelope, "sarif-file");
        assert_eq!(entry["status"], "not-applied", "{entry}");
        assert_eq!(entry["reason"], reason, "{entry}");
        let message = entry["message"].as_str().expect("a remedy sentence");
        assert!(
            message.contains("code scanning") || message.contains("receives no findings"),
            "the sentence must say what the consumer loses: {message}"
        );
        assert!(
            !message.contains('\n'),
            "the sentence travels into a CI annotation and must stay on one line: {message}"
        );
        assert!(
            out.stderr.contains(message),
            "the wire message and the printed line must be the same string: {}",
            out.stderr
        );
    }

    // Restore write permission so the temporary directory can be removed.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("restore permission");
}

/// The failure line is printed whether or not `--quiet` was passed, which the
/// issue asks to keep, and the record does not depend on it either.
#[cfg(unix)]
#[test]
fn a_failed_sarif_write_is_reported_under_quiet_and_without_it() {
    use std::os::unix::fs::PermissionsExt;

    let project = project();
    let root = root_arg(&project);
    let locked = project.path().join("locked");
    std::fs::create_dir_all(&locked).expect("locked dir");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555))
        .expect("drop write permission");
    let sarif_arg = locked
        .join("results.sarif")
        .to_str()
        .expect("utf8")
        .to_owned();

    let quiet = run(&[
        "dead-code",
        "--root",
        root,
        "--sarif-file",
        &sarif_arg,
        "--format",
        "json",
        "--quiet",
    ]);
    let loud = run(&[
        "dead-code",
        "--root",
        root,
        "--sarif-file",
        &sarif_arg,
        "--format",
        "json",
    ]);
    assert_eq!(
        request(&parse_json(&quiet), "sarif-file"),
        request(&parse_json(&loud), "sarif-file")
    );
    for out in [&quiet, &loud] {
        assert!(
            out.stderr.contains("failed to write SARIF file"),
            "a failed write is printed either way: {}",
            out.stderr
        );
    }

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("restore permission");
}
