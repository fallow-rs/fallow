#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! End-to-end tests for CODEOWNERS parsing that exercise the disk-read path.
//!
//! The unit tests in `crates/cli/src/codeowners.rs` cover the parser in
//! isolation. These tests cover `from_file` + `discover` + `load` through a
//! real tempdir so regressions in file I/O, probe-path resolution, or the
//! end-to-end pipeline are caught.
//!
//! Focused on the scenarios in issue #127 (GitLab CODEOWNERS format).

use std::path::Path;

use fallow_cli::codeowners::CodeOwners;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

#[test]
fn gitlab_codeowners_reproduction_from_issue_127() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let codeowners = "\
# Default section (no header, rules before first section)
* @default-owner

[Utilities] @utils-team
src/utils/

[UI Components] @ui-team
src/components/
";
    write(&dir.path().join(".gitlab/CODEOWNERS"), codeowners);

    let co = CodeOwners::discover(dir.path()).expect("discover succeeds");

    assert_eq!(co.owner_of(Path::new("README.md")), Some("@default-owner"));
    assert_eq!(
        co.owner_of(Path::new("src/utils/greet.ts")),
        Some("@utils-team")
    );
    assert_eq!(
        co.owner_of(Path::new("src/components/button.ts")),
        Some("@ui-team")
    );
}

#[test]
fn gitlab_codeowners_probed_at_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write(
        &dir.path().join("CODEOWNERS"),
        "[Section] @root-team\nsrc/\n",
    );
    write(
        &dir.path().join(".gitlab/CODEOWNERS"),
        "* @should-not-be-used\n",
    );

    let co = CodeOwners::discover(dir.path()).expect("discover succeeds");
    assert_eq!(co.owner_of(Path::new("src/lib.ts")), Some("@root-team"));
}

#[test]
fn gitlab_exclusion_pattern_clears_ownership_end_to_end() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let codeowners = "\
* @default
!src/vendor/
";
    write(&dir.path().join(".github/CODEOWNERS"), codeowners);

    let co = CodeOwners::discover(dir.path()).expect("discover succeeds");
    assert_eq!(co.owner_of(Path::new("README.md")), Some("@default"));
    assert_eq!(co.owner_of(Path::new("src/vendor/lib.js")), None);
}

#[test]
fn discover_returns_err_for_repo_without_codeowners() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let err = CodeOwners::discover(dir.path()).expect_err("no CODEOWNERS file");
    assert!(
        err.contains("no CODEOWNERS file found"),
        "unexpected error: {err}"
    );
}

#[test]
fn load_with_explicit_path_bypasses_probe() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write(
        &dir.path().join("custom/OWNERS"),
        "[Team A] @team-a\nsrc/\n",
    );

    let co = CodeOwners::load(dir.path(), Some("custom/OWNERS")).expect("load with explicit path");
    assert_eq!(co.owner_of(Path::new("src/a.ts")), Some("@team-a"));
}

#[test]
fn from_file_surfaces_parse_error_for_malformed_glob() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".github/CODEOWNERS");
    write(&path, "foo[unclosed @owner\n");

    let err = CodeOwners::from_file(&path).expect_err("parse should fail");
    assert!(
        err.contains("invalid CODEOWNERS pattern"),
        "unexpected error: {err}"
    );
}

#[test]
fn gitlab_section_shared_lead_owner_regression_133() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join(".gitlab/CODEOWNERS");
    write(
        &path,
        "\
[billing] @core-reviewers @alice @bob
src/billing/

[notifications] @core-reviewers @alice @bob
src/notifications/

[search] @core-reviewers @charlie @dave
src/search/

[admin] @core-reviewers @eve
src/admin/
",
    );

    let co = CodeOwners::discover(dir.path()).expect("discover GitLab CODEOWNERS");
    assert!(co.has_sections(), "parser should detect section headers");

    let billing = co
        .section_and_owners_of(Path::new("src/billing/invoice.ts"))
        .expect("billing match");
    let notifications = co
        .section_and_owners_of(Path::new("src/notifications/email.ts"))
        .expect("notifications match");
    let search = co
        .section_and_owners_of(Path::new("src/search/indexer.ts"))
        .expect("search match");
    let admin = co
        .section_and_owners_of(Path::new("src/admin/dashboard.ts"))
        .expect("admin match");

    assert_eq!(billing.0, Some("billing"));
    assert_eq!(notifications.0, Some("notifications"));
    assert_eq!(search.0, Some("search"));
    assert_eq!(admin.0, Some("admin"));
    assert_eq!(
        billing.1,
        ["@core-reviewers", "@alice", "@bob"]
            .map(String::from)
            .as_slice()
    );
    assert_eq!(
        admin.1,
        ["@core-reviewers", "@eve"].map(String::from).as_slice()
    );

    assert_eq!(
        co.owner_of(Path::new("src/billing/invoice.ts")),
        Some("@core-reviewers"),
    );
    assert_eq!(
        co.owner_of(Path::new("src/admin/dashboard.ts")),
        Some("@core-reviewers"),
    );
}
