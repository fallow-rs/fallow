#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
//! `--root` below the git toplevel: report paths and diff-filter keys.
//!
//! Both behaviors are invisible in the single-package case where `--root` IS
//! the repo toplevel, which is what every other fixture covers.

#[path = "common/mod.rs"]
mod common;

use std::path::Path;
use std::process::Command;

use common::{fallow_bin, parse_json};

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

/// A repo whose `packages/pkg` is the analysis root, holding one function that
/// trips a complexity rule.
fn monorepo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let pkg = root.join("packages/pkg/src");
    std::fs::create_dir_all(&pkg).expect("mkdir");

    std::fs::write(root.join("package.json"), r#"{"name":"mono"}"#).expect("write");
    std::fs::write(
        root.join("packages/pkg/package.json"),
        r#"{"name":"pkg","version":"1.0.0","main":"src/index.js"}"#,
    )
    .expect("write");
    std::fs::write(
        pkg.join("index.js"),
        "export function entry(a) {\n  return a\n}\n",
    )
    .expect("write");
    std::fs::write(pkg.join("complex.js"), COMPLEX_JS).expect("write");

    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "init"]);
    dir
}

const COMPLEX_JS: &str = "export function complex(a, b, c, d, e) {
  let r = 0
  if (a > 1) { r += 1 } else if (a < 0) { r -= 1 }
  if (b > 1) { r += 2 } else if (b < 0) { r -= 2 }
  if (c > 1) { r += 3 } else if (c < 0) { r -= 3 }
  if (d > 1) { r += 4 } else if (d < 0) { r -= 4 }
  if (e > 1) { r += 5 } else if (e < 0) { r -= 5 }
  for (const x of [a, b, c]) { if (x) { r += x } }
  while (r > 100) { r -= 10 }
  return r
}
";

fn run(root: &Path, args: &[&str]) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.current_dir(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn run_with_env(root: &Path, args: &[&str], env: &[(&str, &str)]) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.current_dir(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn review_comment_count(output: &common::CommandOutput) -> usize {
    parse_json(output)["comments"]
        .as_array()
        .map_or(0, Vec::len)
}

fn codeclimate_paths(output: &common::CommandOutput) -> Vec<String> {
    parse_json(output)
        .as_array()
        .expect("codeclimate output is an array")
        .iter()
        .map(|issue| issue["location"]["path"].as_str().expect("path").to_owned())
        .collect()
}

/// Ordering shifted between releases (a project-level `package.json` finding
/// began sorting first), so assert on membership, never on index.
fn check_names(output: &common::CommandOutput) -> Vec<String> {
    parse_json(output)
        .as_array()
        .expect("codeclimate output is an array")
        .iter()
        .map(|issue| issue["check_name"].as_str().expect("check_name").to_owned())
        .collect()
}

#[test]
fn codeclimate_paths_are_repo_root_relative_below_the_toplevel() {
    let dir = monorepo();
    let out = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );

    let paths = codeclimate_paths(&out);
    assert!(!paths.is_empty(), "expected findings, got {}", out.stdout);
    for path in &paths {
        assert!(
            path.starts_with("packages/pkg/"),
            "CI consumers address files from the repo root; got {path}"
        );
    }
}

#[test]
fn report_path_prefix_overrides_the_detected_offset() {
    let dir = monorepo();
    let out = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--report-path-prefix",
            "custom/base",
        ],
    );

    for path in codeclimate_paths(&out) {
        assert!(path.starts_with("custom/base/"), "got {path}");
    }
}

#[test]
fn empty_report_path_prefix_disables_rebasing() {
    let dir = monorepo();
    let out = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--report-path-prefix",
            "",
        ],
    );

    for path in codeclimate_paths(&out) {
        assert!(
            !path.starts_with("packages/"),
            "explicit empty prefix should emit --root-relative paths; got {path}"
        );
    }
}

#[test]
fn deprecated_annotations_path_prefix_alias_still_parses() {
    let dir = monorepo();
    let out = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--annotations-path-prefix",
            "custom/base",
        ],
    );

    assert_eq!(out.code, 0, "alias should be accepted: {}", out.stderr);
    for path in codeclimate_paths(&out) {
        assert!(path.starts_with("custom/base/"), "got {path}");
    }
}

/// `github-annotations` renders from JSON and applies its own rebase, so the
/// CodeClimate-side rebase must not double-prefix it.
#[test]
fn github_annotations_and_codeclimate_agree_on_path_shape() {
    let dir = monorepo();
    let cc = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );
    let annotations = run(
        dir.path(),
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "github-annotations",
        ],
    );

    for path in codeclimate_paths(&cc) {
        assert!(path.starts_with("packages/pkg/"), "got {path}");
        assert!(
            !path.starts_with("packages/pkg/packages/"),
            "double-prefixed"
        );
    }
    assert!(
        annotations
            .stdout
            .contains("file=packages/pkg/src/complex.js"),
        "annotations lost their rebase: {}",
        annotations.stdout
    );
    assert!(
        !annotations.stdout.contains("packages/pkg/packages/"),
        "annotations double-prefixed: {}",
        annotations.stdout
    );
}

/// The dangerous defect: a real `git diff` names paths from the repo toplevel.
/// Keyed against `--root` they matched nothing, and the run reported a clean
/// diff with no warning.
#[test]
fn repo_root_relative_diff_keeps_source_anchored_findings() {
    let dir = monorepo();
    let root = dir.path();
    let complex = root.join("packages/pkg/src/complex.js");
    std::fs::write(
        &complex,
        COMPLEX_JS.replace(
            "  return r\n",
            "  const touched = 1\n  return r + touched\n",
        ),
    )
    .expect("write");

    let diff = Command::new("git")
        .args(["diff"])
        .current_dir(root)
        .output()
        .expect("git diff");
    let diff_path = root.join("pr.diff");
    std::fs::write(&diff_path, &diff.stdout).expect("write diff");
    assert!(
        String::from_utf8_lossy(&diff.stdout).contains("+++ b/packages/pkg/src/complex.js"),
        "fixture diff should be repo-root-relative"
    );

    let unfiltered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );
    let filtered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    let names = check_names(&filtered);
    assert!(
        names.iter().any(|n| n == "fallow/high-crap-score"),
        "the complexity finding sits on an added line and must survive the \
         diff filter; got {names:?}"
    );
    assert_eq!(
        check_names(&unfiltered).len(),
        names.len(),
        "every finding in this fixture is in the diff, so the filter should \
         drop nothing"
    );
    assert!(
        !filtered.stderr.contains("warning [diff-file]"),
        "a matching diff must not warn: {}",
        filtered.stderr
    );
}

/// `git diff --relative`, run from the package directory, writes `--root`-relative
/// paths. That is a legitimate second convention, so the base is chosen by which
/// one the diff's paths actually name on disk rather than assumed.
#[test]
fn root_relative_diff_also_keeps_source_anchored_findings() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("relative.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/src/complex.js b/src/complex.js\n\
         --- a/src/complex.js\n\
         +++ b/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let out = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        check_names(&out)
            .iter()
            .any(|n| n == "fallow/high-crap-score"),
        "a --relative diff names real files under --root and must filter \
         against it; got {:?}",
        check_names(&out)
    );
    assert!(
        !out.stderr.contains("warning [diff-file]"),
        "a resolvable diff must not warn: {}",
        out.stderr
    );
}

/// A diff naming files that exist under neither the toplevel nor `--root`.
/// Silently reporting zero findings is what made the original defect look like
/// a clean diff, so this must warn.
#[test]
fn foreign_namespace_diff_warns_instead_of_reporting_clean() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("foreign.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/nowhere/ghost.js b/nowhere/ghost.js\n\
         --- a/nowhere/ghost.js\n\
         +++ b/nowhere/ghost.js\n\
         @@ -0,0 +1,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let out = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        out.stderr.contains("warning [diff-file]")
            && out.stderr.contains("relative to a different directory"),
        "expected a foreign-namespace warning, got stderr: {}",
        out.stderr
    );
}

/// When `--root` IS the toplevel, nothing about either behavior may move.
#[test]
fn analysis_root_at_the_toplevel_is_unchanged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"single","version":"1.0.0","main":"src/index.js"}"#,
    )
    .expect("write");
    std::fs::write(
        root.join("src/index.js"),
        "export function entry(a) {\n  return a\n}\n",
    )
    .expect("write");
    std::fs::write(root.join("src/complex.js"), COMPLEX_JS).expect("write");
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "init"]);

    let out = run(root, &["--quiet", "--format", "codeclimate"]);
    for path in codeclimate_paths(&out) {
        assert!(
            path.starts_with("src/"),
            "root == toplevel must emit unprefixed paths; got {path}"
        );
    }
}

/// A diff path that names a real file under BOTH the toplevel and `--root`
/// cannot be placed by existence alone. Guessing and staying quiet would
/// reproduce the empty-report-looks-clean failure this mechanism exists to
/// prevent, so the ambiguity is reported.
#[test]
fn ambiguous_diff_base_warns_instead_of_guessing_silently() {
    let dir = monorepo();
    let root = dir.path();
    // Now `src/complex.js` resolves under the toplevel AND under packages/pkg.
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/complex.js"), COMPLEX_JS).expect("write");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "top-level src"]);

    let diff_path = root.join("ambiguous.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/src/complex.js b/src/complex.js\n\
         --- a/src/complex.js\n\
         +++ b/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let out = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        out.stderr.contains("warning [diff-file]") && out.stderr.contains("ambiguous"),
        "an ambiguous diff base must be reported, not guessed silently; \
         stderr: {}",
        out.stderr
    );
}

/// An unambiguous diff must not acquire the ambiguity warning.
#[test]
fn unambiguous_diff_base_stays_quiet() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("clear.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/packages/pkg/src/complex.js b/packages/pkg/src/complex.js\n\
         --- a/packages/pkg/src/complex.js\n\
         +++ b/packages/pkg/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let out = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        !out.stderr.contains("warning [diff-file]"),
        "a diff that resolves under exactly one base must not warn: {}",
        out.stderr
    );
}

/// The review / sticky-summary filter matches issues against the diff. Its keys
/// must come from the analysis-root-relative path and the diff's own base, never
/// from the rendered path, or `--report-path-prefix` would silently decide which
/// inline comments survive.
#[test]
fn review_filter_is_independent_of_report_path_prefix() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("pr.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/packages/pkg/src/complex.js b/packages/pkg/src/complex.js\n\
         --- a/packages/pkg/src/complex.js\n\
         +++ b/packages/pkg/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");
    let diff = diff_path.to_str().expect("utf8");
    let env = [("FALLOW_DIFF_FILE", diff), ("FALLOW_DIFF_FILTER", "file")];
    let base_args = [
        "--root",
        "packages/pkg",
        "--quiet",
        "--format",
        "review-gitlab",
    ];

    let default = run_with_env(root, &base_args, &env);
    let expected = review_comment_count(&default);
    assert!(
        expected > 0,
        "fixture should produce an inline comment: {}",
        default.stdout
    );

    for prefix in ["", "custom/base"] {
        let mut args = base_args.to_vec();
        args.extend_from_slice(&["--report-path-prefix", prefix]);
        let out = run_with_env(root, &args, &env);
        assert_eq!(
            review_comment_count(&out),
            expected,
            "--report-path-prefix {prefix:?} changed which comments survive the \
             diff filter; it must only change how paths are rendered"
        );
    }
}

/// ...while still rendering the prefix it was given.
#[test]
fn report_path_prefix_still_renders_on_review_paths() {
    let dir = monorepo();
    let root = dir.path();
    let out = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "review-gitlab",
            "--report-path-prefix",
            "custom/base",
        ],
    );

    let paths: Vec<String> = parse_json(&out)["comments"]
        .as_array()
        .expect("comments")
        .iter()
        .map(|c| {
            c["position"]["new_path"]
                .as_str()
                .expect("new_path")
                .to_owned()
        })
        .collect();
    assert!(!paths.is_empty(), "expected comments: {}", out.stdout);
    for path in paths {
        assert!(path.starts_with("custom/base/"), "got {path}");
    }
}

/// Renamed files: GitLab needs `old_path` to place a discussion, and the diff's
/// rename pairs are keyed in the diff's namespace, not the rendered one. A
/// rendered-path lookup misses and `old_path` silently falls back to `new_path`,
/// telling GitLab a moved file never moved.
#[test]
fn renames_resolve_old_path_across_every_namespace() {
    let dir = monorepo();
    let root = dir.path();
    git(
        root,
        &[
            "mv",
            "packages/pkg/src/complex.js",
            "packages/pkg/src/renamed.js",
        ],
    );
    let toplevel_diff = root.join("rename.diff");
    let out = Command::new("git")
        .args(["diff", "-M", "HEAD"])
        .current_dir(root)
        .output()
        .expect("git diff");
    std::fs::write(&toplevel_diff, &out.stdout).expect("write diff");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("rename to packages/pkg/src/renamed.js"),
        "fixture diff should record the rename"
    );

    // The same rename, expressed the way `git diff --relative` would.
    let relative_diff = root.join("rename_rel.diff");
    std::fs::write(
        &relative_diff,
        String::from_utf8_lossy(&out.stdout).replace("packages/pkg/", ""),
    )
    .expect("write diff");

    let old_path = |diff: &std::path::Path, extra: &[&str]| -> (String, String) {
        let mut args = vec![
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "review-gitlab",
            "--diff-file",
            diff.to_str().expect("utf8"),
        ];
        args.extend_from_slice(extra);
        let out = run_with_env(root, &args, &[("FALLOW_DIFF_FILTER", "file")]);
        let position = &parse_json(&out)["comments"][0]["position"];
        (
            position["old_path"].as_str().unwrap_or_default().to_owned(),
            position["new_path"].as_str().unwrap_or_default().to_owned(),
        )
    };

    for (label, diff, extra, want_old, want_new) in [
        (
            "toplevel-relative diff",
            &toplevel_diff,
            &[][..],
            "packages/pkg/src/complex.js",
            "packages/pkg/src/renamed.js",
        ),
        (
            "custom presentation prefix",
            &toplevel_diff,
            &["--report-path-prefix", "custom/base"][..],
            "custom/base/src/complex.js",
            "custom/base/src/renamed.js",
        ),
        (
            "root-relative diff",
            &relative_diff,
            &[][..],
            "packages/pkg/src/complex.js",
            "packages/pkg/src/renamed.js",
        ),
        (
            "rebasing disabled",
            &toplevel_diff,
            &["--report-path-prefix", ""][..],
            "src/complex.js",
            "src/renamed.js",
        ),
    ] {
        let (old, new) = old_path(diff, extra);
        assert_eq!(old, want_old, "{label}: wrong old_path");
        assert_eq!(new, want_new, "{label}: wrong new_path");
        assert_ne!(
            old, new,
            "{label}: old_path fell back to new_path, so the rename was lost"
        );
    }
}

/// The review renderer applies the presentation prefix itself. Any emitter that
/// also rebases before handing it CodeClimate issues prefixes twice AND keys the
/// diff filter in the wrong namespace. Combined mode and the subcommands reach
/// the renderer by different routes, so both are pinned.
#[test]
fn review_paths_are_prefixed_exactly_once_on_every_route() {
    let dir = monorepo();
    let root = dir.path();
    let new_path = |args: &[&str]| -> String {
        let out = run(root, args);
        parse_json(&out)["comments"][0]["position"]["new_path"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };

    let combined = new_path(&[
        "--root",
        "packages/pkg",
        "--quiet",
        "--format",
        "review-gitlab",
    ]);
    let dead_code = new_path(&[
        "dead-code",
        "--root",
        "packages/pkg",
        "--quiet",
        "--format",
        "review-gitlab",
    ]);
    let health = new_path(&[
        "health",
        "--root",
        "packages/pkg",
        "--quiet",
        "--format",
        "review-gitlab",
    ]);

    for (route, path) in [
        ("combined", &combined),
        ("dead-code", &dead_code),
        ("health", &health),
    ] {
        assert!(
            path.starts_with("packages/pkg/"),
            "{route}: expected a repo-root-relative path, got {path}"
        );
        assert!(
            !path.contains("packages/pkg/packages/pkg/"),
            "{route}: path was prefixed twice: {path}"
        );
    }
}

/// `--diff-file` must scope inline review comments, not just the analysis
/// results. Gating that filter on `$FALLOW_DIFF_FILE` left the flag rendering
/// every comment while claiming the diff had been applied.
#[test]
fn diff_file_flag_scopes_review_comments_without_the_env_var() {
    let dir = monorepo();
    let root = dir.path();

    let touching = root.join("touching.diff");
    std::fs::write(
        &touching,
        "diff --git a/packages/pkg/src/complex.js b/packages/pkg/src/complex.js\n\
         --- a/packages/pkg/src/complex.js\n\
         +++ b/packages/pkg/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let unrelated = root.join("unrelated.diff");
    std::fs::write(
        &unrelated,
        "diff --git a/packages/pkg/src/index.js b/packages/pkg/src/index.js\n\
         --- a/packages/pkg/src/index.js\n\
         +++ b/packages/pkg/src/index.js\n\
         @@ -1,0 +2,1 @@\n\
         +// unrelated\n",
    )
    .expect("write diff");

    let comments = |diff: &std::path::Path| -> usize {
        let out = run_with_env(
            root,
            &[
                "dead-code",
                "--root",
                "packages/pkg",
                "--quiet",
                "--format",
                "review-gitlab",
                "--diff-file",
                diff.to_str().expect("utf8"),
            ],
            &[("FALLOW_DIFF_FILTER", "file")],
        );
        review_comment_count(&out)
    };

    assert_eq!(
        comments(&touching),
        1,
        "a finding in a file the diff touches must survive"
    );
    assert_eq!(
        comments(&unrelated),
        0,
        "--diff-file must scope review comments even with no FALLOW_DIFF_FILE"
    );
}

/// An ambiguous base means fallow cannot express findings in the diff's
/// namespace. `check::filtering` sets the convention: retain what cannot be
/// filtered. Dropping every finding on a guess is the failure this whole change
/// removes, so the diff is discarded rather than the findings.
#[test]
fn ambiguous_diff_base_fails_open_and_retains_findings() {
    let dir = monorepo();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/complex.js"), COMPLEX_JS).expect("write");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "top-level src"]);

    let diff_path = root.join("ambiguous.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/src/complex.js b/src/complex.js\n\
         --- a/src/complex.js\n\
         +++ b/src/complex.js\n\
         @@ -9,0 +10,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let unfiltered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );
    let ambiguous = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        ambiguous.stderr.contains("ambiguous"),
        "expected an ambiguity warning: {}",
        ambiguous.stderr
    );
    assert_eq!(
        check_names(&ambiguous).len(),
        check_names(&unfiltered).len(),
        "an ambiguous base must retain findings, not filter them away"
    );
}

/// A diff that PARSED but names no analyzable head-side file (deletion-only)
/// changed nothing a finding can be attributed to. That is a real, EMPTY scope,
/// not an unplaceable base: it must filter to zero, never fall open to full
/// scope. The original defect reported such a run at full scope.
#[test]
fn deletion_only_diff_filters_to_zero_not_full_scope() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("deletion.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/packages/pkg/src/removed.js b/packages/pkg/src/removed.js\n\
         deleted file mode 100644\n\
         --- a/packages/pkg/src/removed.js\n\
         +++ /dev/null\n\
         @@ -1,3 +0,0 @@\n\
         -one\n\
         -two\n\
         -three\n",
    )
    .expect("write diff");

    let unfiltered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );
    let filtered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        !check_names(&unfiltered).is_empty(),
        "fixture must produce findings at full scope: {}",
        unfiltered.stdout
    );
    assert!(
        check_names(&filtered).is_empty(),
        "a deletion-only diff analyzes no head-side file, so every \
         source-anchored finding must filter out; got {:?}",
        check_names(&filtered)
    );
    assert!(
        !filtered
            .stderr
            .contains("relative to a different directory")
            && !filtered.stderr.contains("ambiguous"),
        "an empty-scope diff is not foreign or ambiguous; got stderr: {}",
        filtered.stderr
    );
}

/// An empty diff (nothing staged) is the same empty scope: no head-side file
/// changed, so it filters to zero rather than falling open.
#[test]
fn empty_diff_filters_to_zero_not_full_scope() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("empty.diff");
    std::fs::write(&diff_path, "").expect("write diff");

    let filtered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        check_names(&filtered).is_empty(),
        "an empty diff analyzes nothing and must filter to zero; got {:?}",
        check_names(&filtered)
    );
}

/// A binary-only diff names no `+++ b/<path>` head-side text file, so it is the
/// same empty scope and filters to zero.
#[test]
fn binary_only_diff_filters_to_zero_not_full_scope() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("binary.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/packages/pkg/logo.png b/packages/pkg/logo.png\n\
         index 0000000..1111111 100644\n\
         Binary files a/packages/pkg/logo.png and b/packages/pkg/logo.png differ\n",
    )
    .expect("write diff");

    let filtered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert!(
        check_names(&filtered).is_empty(),
        "a binary-only diff touches no analyzable file and must filter to \
         zero; got {:?}",
        check_names(&filtered)
    );
}

/// The split's other half: a diff that names files but places them nowhere
/// (foreign base) is UNATTRIBUTABLE, not empty. It must fall open to full scope,
/// never collapse to the empty-scope zero. This is the case the empty-scope path
/// must not swallow.
#[test]
fn foreign_namespace_diff_fails_open_to_full_scope() {
    let dir = monorepo();
    let root = dir.path();
    let diff_path = root.join("foreign.diff");
    std::fs::write(
        &diff_path,
        "diff --git a/nowhere/ghost.js b/nowhere/ghost.js\n\
         --- a/nowhere/ghost.js\n\
         +++ b/nowhere/ghost.js\n\
         @@ -0,0 +1,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let unfiltered = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
        ],
    );
    let foreign = run(
        root,
        &[
            "--root",
            "packages/pkg",
            "--quiet",
            "--format",
            "codeclimate",
            "--diff-file",
            diff_path.to_str().expect("utf8"),
        ],
    );

    assert_eq!(
        check_names(&foreign).len(),
        check_names(&unfiltered).len(),
        "a foreign (unattributable) diff must retain findings at full scope, \
         never collapse to the empty-scope zero"
    );
}

/// The FALLOW_DIFF_FILE env path (the GitHub Action's standard route) must
/// honour the discard decision. When base detection discards an unplaceable
/// (foreign) diff and reports at full scope, the env-var comment filter must not
/// re-read the file and re-filter, which would drop every comment and disagree
/// with the full-scope decision the finding filter made.
#[test]
fn env_diff_file_honours_discard_and_keeps_comments_at_full_scope() {
    let dir = monorepo();
    let root = dir.path();
    let foreign = root.join("foreign.diff");
    std::fs::write(
        &foreign,
        "diff --git a/nowhere/ghost.js b/nowhere/ghost.js\n\
         --- a/nowhere/ghost.js\n\
         +++ b/nowhere/ghost.js\n\
         @@ -0,0 +1,1 @@\n\
         +  const touched = 1\n",
    )
    .expect("write diff");

    let base_args = [
        "--root",
        "packages/pkg",
        "--quiet",
        "--format",
        "review-gitlab",
    ];
    let no_diff = run(root, &base_args);
    let expected = review_comment_count(&no_diff);
    assert!(
        expected > 0,
        "fixture should produce comments: {}",
        no_diff.stdout
    );

    let with_env = run_with_env(
        root,
        &base_args,
        &[("FALLOW_DIFF_FILE", foreign.to_str().expect("utf8"))],
    );
    assert_eq!(
        review_comment_count(&with_env),
        expected,
        "a discarded (foreign) diff reports at full scope; the FALLOW_DIFF_FILE \
         comment filter must not re-filter and drop comments"
    );
}
