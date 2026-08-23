#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{fallow_bin, parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
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
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_all(dir: &std::path::Path, message: &str) {
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", message],
    );
}

/// Create a temp git repo with a commit, suitable for audit testing.
/// Returns the `TempDir` guard so the directory lives as long as the caller holds it.
fn create_audit_fixture(_suffix: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "dependencies": {"unused-pkg": "1.0.0"}}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = () => 0;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/orphan.ts"),
        "export const orphaned = 'nobody';\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
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
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);

    tmp
}

#[test]
fn audit_json_uses_the_selected_presentation_style() {
    let fixture = create_audit_fixture("json-style");
    let root = fixture
        .path()
        .to_str()
        .expect("fixture path should be UTF-8");
    let compact = run_fallow_raw(&[
        "audit", "--root", root, "--base", "HEAD", "--format", "json", "--quiet",
    ]);
    let pretty = run_fallow_raw(&[
        "audit", "--root", root, "--base", "HEAD", "--format", "json", "--pretty", "--quiet",
    ]);

    assert_eq!(
        compact.code, pretty.code,
        "presentation must not change the verdict"
    );
    assert_eq!(
        compact.stdout.lines().count(),
        1,
        "audit JSON should be compact"
    );
    assert!(
        pretty.stdout.lines().count() > 1,
        "--pretty should indent audit JSON"
    );
    serde_json::from_str::<serde_json::Value>(&compact.stdout)
        .expect("compact audit JSON should parse");
    serde_json::from_str::<serde_json::Value>(&pretty.stdout)
        .expect("pretty audit JSON should parse");
}

#[test]
fn audit_performance_json_uses_the_selected_presentation_style() {
    let fixture = create_audit_fixture("performance-json-style");
    fs::write(
        fixture.path().join("src/new.ts"),
        "export const changed = () => 1;\n",
    )
    .expect("changed file should be written");
    let root = fixture
        .path()
        .to_str()
        .expect("fixture path should be UTF-8");
    let pretty = run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "HEAD",
        "--format",
        "json",
        "--pretty",
        "--quiet",
        "--performance",
    ]);

    assert!(
        pretty.stderr.contains("\n  \""),
        "--pretty should indent audit performance JSON: {}",
        pretty.stderr
    );
}

fn write_branchy_change(dir: &std::path::Path) {
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\n\
         used();\n\
         function branchy(n: number): number {\n\
           if (n < 0) return -1;\n\
           if (n === 0) return 0;\n\
           if (n < 10) return 1;\n\
           if (n < 100) return 2;\n\
           if (n < 1000) return 3;\n\
           if (n < 10000) return 4;\n\
           return 5;\n\
         }\n\
         branchy(used());\n",
    )
    .unwrap();
    commit_all(dir, "add branchy");
}

fn write_branchy_istanbul_coverage(coverage_path: &std::path::Path, coverage_source_path: &str) {
    fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        coverage_source_path.to_string(),
        serde_json::json!({
            "path": coverage_source_path,
            "statementMap": {},
            "fnMap": {
                "0": {
                    "name": "branchy",
                    "line": 3,
                    "decl": {
                        "start": { "line": 3, "column": 9 },
                        "end": { "line": 3, "column": 16 }
                    },
                    "loc": {
                        "start": { "line": 3, "column": 35 },
                        "end": { "line": 11, "column": 10 }
                    }
                }
            },
            "branchMap": {},
            "s": {},
            "f": { "0": 1 },
            "b": {}
        }),
    );
    fs::write(coverage_path, serde_json::to_string(&coverage).unwrap()).unwrap();
}

fn run_fallow_raw_with_env(
    args: &[&str],
    env: &[(&str, &std::path::Path)],
) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    common::scrub_coverage_env(&mut cmd);
    for (key, value) in env {
        cmd.env(key, value);
    }
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn audit_cache_paths(root: &Path, base_sha: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    audit_cache_paths_in(root, base_sha, &std::env::temp_dir())
}

/// Compute the cache entry paths for `root` under an explicit scan root.
///
/// For a git-repo fixture root the requested root IS the repo toplevel, so a
/// single canonical hash fills both the repo and root slots. Source it from
/// the production helper (dunce canonicalization + platform path-identity
/// bytes) so the fixtures land where the spawned binary enumerates them;
/// recomputing via `Path::canonicalize` + UTF-8 bytes diverges on Windows,
/// where std canonicalize keeps the `\\?\` verbatim prefix (production strips
/// it via dunce) and the identity is hashed as UTF-16LE bytes.
///
/// Prune tests MUST pass a private scan root and redirect the spawned
/// binary's temp dir onto it (see [`run_prune_with_scan_root`]): prune's
/// cross-repo pass scans the whole temp dir, so running it against the shared
/// system temp would race parallel test binaries and could reclaim a
/// developer's real warm caches.
fn audit_cache_paths_in(
    root: &Path,
    base_sha: &str,
    scan_root: &Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let hash = fallow_cli::canonical_root_hash(root);
    let new_path = scan_root.join(format!(
        "fallow-audit-base-cache-{hash:016x}-root-{hash:016x}"
    ));
    let legacy_path = scan_root.join(format!(
        "fallow-audit-base-cache-{hash:016x}-{}",
        &base_sha[..16]
    ));
    (new_path, legacy_path)
}

/// Run `fallow` with its temp dir redirected onto `scan_root` so the prune
/// cross-repo pass only ever sees this test's own fixtures.
fn run_prune_with_scan_root(args: &[&str], scan_root: &Path) -> common::CommandOutput {
    run_fallow_raw_with_env(
        args,
        &[
            ("TMPDIR", scan_root),
            ("TMP", scan_root),
            ("TEMP", scan_root),
        ],
    )
}

/// Backdate a file's mtime by whole days.
fn backdate_days(path: &Path, days: u64) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("file should open for backdating");
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    file.set_modified(when)
        .expect("set_modified should succeed");
}

/// Deterministic snapshot of a directory's entry names, sizes, and mtimes,
/// for byte-level "dry run must not mutate" assertions.
/// Name, size, and file mtime per entry. Directory mtimes are deliberately
/// excluded: NTFS updates a directory's timestamp lazily, so it can still move
/// after the last write to that directory has returned, which makes an
/// unchanged tree compare unequal. Seeding and lock files remain visible
/// through the file set, the sizes, and the file mtimes.
fn snapshot_dir_entries(dir: &Path) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
    let mut entries: Vec<(String, u64, Option<std::time::SystemTime>)> = fs::read_dir(dir)
        .expect("directory should be readable")
        .flatten()
        .map(|entry| {
            let metadata = entry.metadata().expect("entry metadata should be readable");
            let mtime = if metadata.is_dir() {
                None
            } else {
                Some(metadata.modified().expect("entry mtime should be readable"))
            };
            (
                entry.file_name().to_string_lossy().into_owned(),
                metadata.len(),
                mtime,
            )
        })
        .collect();
    entries.sort();
    entries
}

/// Materialize an aged cache entry (directory with content, `.sha`, and a
/// backdated owner-recording `.last-used`) at `cache_path`.
fn write_aged_cache_entry(cache_path: &Path, base_sha: &str, owner: &Path, age_days: u64) {
    fs::create_dir_all(cache_path.join("node_modules")).expect("cache tree should be created");
    fs::write(cache_path.join("node_modules/blob.bin"), vec![0u8; 2048])
        .expect("cache content should be written");
    fs::write(cache_sidecar(cache_path, ".sha"), format!("{base_sha}\n"))
        .expect("readiness sidecar should be written");
    fs::write(
        cache_sidecar(cache_path, ".last-used"),
        format!("{}\n", owner.display()),
    )
    .expect("last-used sidecar should be written");
    backdate_days(&cache_sidecar(cache_path, ".last-used"), age_days);
}

#[test]
fn audit_cache_prune_dry_run_reports_without_filesystem_mutation() {
    let fixture = create_audit_fixture("cache-prune-dry-run");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (aged_path, _) = audit_cache_paths_in(root, base_sha, scan.path());
    write_aged_cache_entry(&aged_path, base_sha, root, 40);
    let before = snapshot_dir_entries(scan.path());

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--dry-run",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "prune dry-run should succeed: {}{}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["kind"], serde_json::json!("audit-cache-prune"));
    assert_eq!(json["schema_version"], serde_json::json!(1));
    assert_eq!(json["command"], serde_json::json!("audit-cache prune"));
    assert_eq!(json["dry_run"], serde_json::json!(true));
    assert_eq!(json["max_age_days"], serde_json::json!(30));
    assert_eq!(json["max_age_source"], serde_json::json!("default"));
    assert_eq!(json["found"], serde_json::json!(1));
    assert_eq!(json["removed"], serde_json::json!(1));
    assert_eq!(json["kept"], serde_json::json!(0));
    assert_eq!(json["skipped"], serde_json::json!(0));
    assert_eq!(json["failed"], serde_json::json!(0));
    assert_eq!(json["complete"], serde_json::json!(true));
    let entries = json["entries"]
        .as_array()
        .expect("entries should be an array");
    assert_eq!(entries.len(), 1, "found must equal entries.len()");
    let entry = &entries[0];
    assert_eq!(entry["pass"], serde_json::json!("owned"));
    assert_eq!(entry["disposition"], serde_json::json!("removed"));
    assert_eq!(entry["reason"], serde_json::json!("aged-out"));
    assert_eq!(entry["age_days"], serde_json::json!(40));
    assert!(
        entry["size_bytes"]
            .as_u64()
            .is_some_and(|size| size >= 2048),
        "size walk should count the cache content: {entry}"
    );
    assert_eq!(json["reclaimed_bytes"], entry["size_bytes"]);
    assert_eq!(
        snapshot_dir_entries(scan.path()),
        before,
        "dry run must leave the scan root byte-identical (no removal, no `.lock`, no seeding)",
    );
}

#[test]
fn audit_cache_prune_applies_policy_and_closes_counts() {
    let fixture = create_audit_fixture("cache-prune-apply");
    let root = fixture.path();
    let other = create_audit_fixture("cache-prune-apply-other");
    let scan = TempDir::new().expect("scan root should be created");
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (aged_path, fresh_path) = audit_cache_paths_in(root, base_sha, scan.path());
    write_aged_cache_entry(&aged_path, base_sha, root, 40);
    fs::create_dir_all(&fresh_path).expect("fresh cache should be created");
    fs::write(cache_sidecar(&fresh_path, ".sha"), format!("{base_sha}\n"))
        .expect("fresh readiness sidecar should be written");
    fs::write(
        cache_sidecar(&fresh_path, ".last-used"),
        format!("{}\n", root.display()),
    )
    .expect("fresh last-used sidecar should be written");
    let (foreign_path, _) = audit_cache_paths_in(other.path(), base_sha, scan.path());
    write_aged_cache_entry(&foreign_path, base_sha, other.path(), 40);

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "prune should succeed: {}{}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["dry_run"], serde_json::json!(false));
    assert_eq!(json["found"], serde_json::json!(3));
    assert_eq!(json["removed"], serde_json::json!(1));
    assert_eq!(json["kept"], serde_json::json!(2));
    assert_eq!(json["skipped"], serde_json::json!(0));
    assert_eq!(json["failed"], serde_json::json!(0));
    assert_eq!(json["complete"], serde_json::json!(true));
    let entries = json["entries"]
        .as_array()
        .expect("entries should be an array");
    assert_eq!(entries.len(), 3, "found must equal entries.len()");
    let by_path = |path: &Path| {
        entries
            .iter()
            .find(|entry| entry["path"] == serde_json::json!(path))
            .unwrap_or_else(|| panic!("entry for {} should be reported", path.display()))
    };
    let aged = by_path(&aged_path);
    assert_eq!(aged["disposition"], serde_json::json!("removed"));
    assert_eq!(aged["reason"], serde_json::json!("aged-out"));
    assert_eq!(aged["pass"], serde_json::json!("owned"));
    let fresh = by_path(&fresh_path);
    assert_eq!(fresh["disposition"], serde_json::json!("kept"));
    assert_eq!(fresh["reason"], serde_json::json!("fresh"));
    assert_eq!(fresh["pass"], serde_json::json!("legacy"));
    assert_eq!(fresh["age_days"], serde_json::json!(0));
    let foreign = by_path(&foreign_path);
    assert_eq!(foreign["disposition"], serde_json::json!("kept"));
    assert_eq!(foreign["reason"], serde_json::json!("owner-live"));
    assert_eq!(foreign["pass"], serde_json::json!("foreign"));
    assert_eq!(
        foreign["owner_root"],
        serde_json::json!(other.path().display().to_string()),
        "the recorded live owner root must be reported",
    );
    assert!(
        json["reclaimed_bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes >= 2048),
        "reclaimed_bytes should carry the removed entry's size: {json}"
    );

    assert!(!aged_path.exists(), "the aged entry must be removed");
    assert!(!cache_sidecar(&aged_path, ".sha").exists());
    assert!(!cache_sidecar(&aged_path, ".last-used").exists());
    assert!(
        cache_sidecar(&aged_path, ".lock").exists(),
        "the `.lock` sidecar must never be deleted",
    );
    assert!(fresh_path.is_dir(), "a fresh entry must be kept");
    assert!(
        foreign_path.is_dir(),
        "a foreign entry with a live owner must be kept",
    );
}

#[test]
fn audit_cache_prune_human_output_lists_rows_and_hints() {
    let fixture = create_audit_fixture("cache-prune-human");
    let root = fixture.path();
    let other = create_audit_fixture("cache-prune-human-other");
    let scan = TempDir::new().expect("scan root should be created");
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (aged_path, _) = audit_cache_paths_in(root, base_sha, scan.path());
    write_aged_cache_entry(&aged_path, base_sha, root, 40);
    let (foreign_path, _) = audit_cache_paths_in(other.path(), base_sha, scan.path());
    write_aged_cache_entry(&foreign_path, base_sha, other.path(), 40);
    fs::write(
        scan.path()
            .join("fallow-audit-base-cache-1111111111111111-root-2222222222222222.lock"),
        "",
    )
    .expect("bare lock sidecar should be written");

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--dry-run",
            "--max-age-days",
            "30",
            "--root",
            root.to_str().expect("root should be utf-8"),
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "prune dry-run should succeed: {}{}",
        output.stdout, output.stderr
    );
    let stdout = &output.stdout;
    assert!(
        stdout.contains("audit cache prune (threshold 30d from flag) in "),
        "header should name the threshold, source, and scan root: {stdout}"
    );
    assert!(
        stdout.contains("would remove") && stdout.contains("aged 40d"),
        "dry-run rows should use would-remove wording with the age: {stdout}"
    );
    assert!(
        stdout.contains("owner live: "),
        "live-owner rows should name the owner: {stdout}"
    );
    assert!(
        stdout.contains("1 lock sidecar with no cache remain (harmless, kept by design)"),
        "lock-only entries should aggregate into one line: {stdout}"
    );
    assert!(
        stdout.contains(
            "kept 1 entry owned by other live projects; run `fallow audit-cache remove --root <path> --yes` in a project to clear its own cache"
        ),
        "live-owner rows should come with a next-step hint: {stdout}"
    );
    assert!(
        stdout.contains("would reclaim "),
        "dry-run summary should use would-reclaim wording: {stdout}"
    );
}

#[test]
fn audit_cache_prune_zero_max_age_reclaims_orphans_only() {
    let fixture = create_audit_fixture("cache-prune-zero-age");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (aged_path, orphan_path) = audit_cache_paths_in(root, base_sha, scan.path());
    write_aged_cache_entry(&aged_path, base_sha, root, 40);
    fs::write(cache_sidecar(&orphan_path, ".sha"), format!("{base_sha}\n"))
        .expect("orphan readiness sidecar should be written");
    fs::write(cache_sidecar(&orphan_path, ".last-used"), "")
        .expect("orphan last-used sidecar should be written");

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--max-age-days",
            "0",
            "--root",
            root.to_str().expect("root should be utf-8"),
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "prune with a zero threshold should succeed: {}{}",
        output.stdout, output.stderr
    );
    assert!(
        output.stdout.contains(
            "audit cache prune (age-based reclaim disabled by --max-age-days 0; reclaiming orphaned entries only) in "
        ),
        "the zero-threshold header must say age-based reclaim is disabled: {}",
        output.stdout
    );
    assert!(
        !cache_sidecar(&orphan_path, ".sha").exists()
            && !cache_sidecar(&orphan_path, ".last-used").exists(),
        "orphaned sidecars must still be reclaimed with age-based GC disabled",
    );
    assert!(
        aged_path.is_dir(),
        "no age-based reclaim may happen under --max-age-days 0",
    );
}

/// Register pre-#1815-style git worktrees at both the CURRENT cache path and
/// a released SHA-keyed path for `root` under `scan_root`, returning
/// `(current_path, released_path)`.
fn register_legacy_cache_worktrees(
    root: &Path,
    scan_root: &Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (current_path, released_path) = audit_cache_paths_in(root, base_sha, scan_root);
    for path in [&current_path, &released_path] {
        git(
            root,
            &[
                "worktree",
                "add",
                "--detach",
                "--quiet",
                path.to_str().expect("cache path should be utf-8"),
                "HEAD",
            ],
        );
    }
    (current_path, released_path)
}

/// Pre-#1815 upgrade edge (issue #2255 follow-up), dry-run half: the row and
/// summary must announce the pending deregistration without claiming removal,
/// and preview mode must not touch either registered entry.
#[test]
fn audit_cache_prune_dry_run_previews_legacy_deregistration() {
    let fixture = create_audit_fixture("cache-prune-legacy-dry-run");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    let (current_path, released_path) = register_legacy_cache_worktrees(root, scan.path());

    let dry_run = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--dry-run",
            "--root",
            root.to_str().expect("root should be utf-8"),
        ],
        scan.path(),
    );

    assert_eq!(
        dry_run.code, 0,
        "prune dry-run should succeed: {}{}",
        dry_run.stdout, dry_run.stderr
    );
    assert!(
        dry_run.stdout.contains("would deregister")
            && dry_run
                .stdout
                .contains("legacy git registration; cache stays warm on disk"),
        "the dry-run row must announce deregistration without claiming removal: {}",
        dry_run.stdout
    );
    assert!(
        dry_run.stdout.contains(
            "would deregister 1 legacy git registration; the cache stays warm on disk (not counted as reclaimed)"
        ),
        "the dry-run summary must carry the deregistered count: {}",
        dry_run.stdout
    );
    assert!(
        current_path.is_dir() && released_path.is_dir(),
        "dry run must not touch either registered entry",
    );
}

/// Pre-#1815 upgrade edge (issue #2255 follow-up): a mixed-version git
/// registration at the CURRENT cache path is only deregistered and stays warm
/// on disk, so its measured size must never inflate `reclaimed_bytes`; it
/// surfaces as its own `deregistered` count instead. A released SHA-keyed
/// registration is genuinely removed and stays counted.
#[test]
fn audit_cache_prune_excludes_deregistered_current_path_from_reclaimed_bytes() {
    let fixture = create_audit_fixture("cache-prune-legacy-registered");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    let (current_path, released_path) = register_legacy_cache_worktrees(root, scan.path());

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "prune should succeed: {}{}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["deregistered"], serde_json::json!(1));
    assert_eq!(json["removed"], serde_json::json!(1));
    let entries = json["entries"]
        .as_array()
        .expect("entries should be an array");
    let by_reason = |reason: &str| {
        entries
            .iter()
            .find(|entry| entry["reason"] == serde_json::json!(reason))
            .unwrap_or_else(|| panic!("an entry with reason {reason} should be reported"))
    };
    let deregistered = by_reason("legacy-deregistered");
    assert_eq!(deregistered["disposition"], serde_json::json!("kept"));
    assert_eq!(deregistered["pass"], serde_json::json!("owned"));
    assert!(
        deregistered["size_bytes"]
            .as_u64()
            .is_some_and(|size| size > 0),
        "the kept-warm entry still reports what it occupies: {deregistered}"
    );
    let removed = by_reason("legacy-registered");
    assert_eq!(removed["disposition"], serde_json::json!("removed"));
    assert_eq!(
        json["reclaimed_bytes"], removed["size_bytes"],
        "reclaimed_bytes must carry only the genuinely removed entry's size",
    );

    assert!(
        current_path.is_dir(),
        "the deregistered current-path cache must stay warm on disk",
    );
    assert_eq!(
        fs::read_to_string(current_path.join(".git")).expect(".git stub should be readable"),
        "gitdir: fallow-audit-unregistered\n",
        "the current-path cache must be deregistered in place",
    );
    assert!(
        !released_path.is_dir(),
        "the released SHA-keyed cache must be removed",
    );
}

#[test]
fn audit_cache_prune_reports_lock_contention_with_exit_zero() {
    let fixture = create_audit_fixture("cache-prune-contended");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (aged_path, _) = audit_cache_paths_in(root, base_sha, scan.path());
    write_aged_cache_entry(&aged_path, base_sha, root, 2);
    fs::write(cache_sidecar(&aged_path, ".lock"), "").expect("lock sidecar should be written");
    let held_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(cache_sidecar(&aged_path, ".lock"))
        .expect("cache lock should open");
    held_lock.try_lock().expect("cache lock should be held");

    let output = run_prune_with_scan_root(
        &[
            "audit-cache",
            "prune",
            "--max-age-days",
            "1",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        scan.path(),
    );

    assert_eq!(
        output.code, 0,
        "contention is a deferral, not a failure: {}{}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["found"], serde_json::json!(1));
    assert_eq!(json["skipped"], serde_json::json!(1));
    assert_eq!(json["removed"], serde_json::json!(0));
    assert_eq!(json["complete"], serde_json::json!(false));
    let entries = json["entries"]
        .as_array()
        .expect("entries should be an array");
    assert_eq!(entries[0]["disposition"], serde_json::json!("skipped"));
    assert_eq!(entries[0]["reason"], serde_json::json!("lock-contention"));
    assert!(aged_path.is_dir(), "a contended entry must remain");
    drop(held_lock);
}

#[test]
fn audit_cache_prune_defaults_root_to_cwd_and_honors_threshold_precedence() {
    let fixture = create_audit_fixture("cache-prune-cwd");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");

    // No --root: prune defaults to the current directory (remove keeps its
    // explicit-root requirement, covered by the remove tests above).
    let no_root = Command::new(common::fallow_bin())
        .current_dir(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .env("TMPDIR", scan.path())
        .env("TMP", scan.path())
        .env("TEMP", scan.path())
        .args([
            "audit-cache",
            "prune",
            "--dry-run",
            "--format",
            "json",
            "--quiet",
        ])
        .output()
        .expect("fallow should run");
    assert_eq!(
        no_root.status.code(),
        Some(0),
        "prune must not require an explicit --root: {}{}",
        String::from_utf8_lossy(&no_root.stdout),
        String::from_utf8_lossy(&no_root.stderr),
    );
    let json: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&no_root.stdout))
        .expect("prune JSON should parse");
    assert_eq!(json["kind"], serde_json::json!("audit-cache-prune"));

    // Threshold precedence: the flag beats the env var, the env var beats the
    // default.
    let flag_wins = run_fallow_raw_with_env(
        &[
            "audit-cache",
            "prune",
            "--dry-run",
            "--max-age-days",
            "9",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        &[
            ("TMPDIR", scan.path()),
            ("TMP", scan.path()),
            ("TEMP", scan.path()),
            ("FALLOW_AUDIT_CACHE_MAX_AGE_DAYS", Path::new("7")),
        ],
    );
    let flag_json = parse_json(&flag_wins);
    assert_eq!(flag_json["max_age_days"], serde_json::json!(9));
    assert_eq!(flag_json["max_age_source"], serde_json::json!("flag"));
    let env_wins = run_fallow_raw_with_env(
        &[
            "audit-cache",
            "prune",
            "--dry-run",
            "--root",
            root.to_str().expect("root should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
        &[
            ("TMPDIR", scan.path()),
            ("TMP", scan.path()),
            ("TEMP", scan.path()),
            ("FALLOW_AUDIT_CACHE_MAX_AGE_DAYS", Path::new("7")),
        ],
    );
    let env_json = parse_json(&env_wins);
    assert_eq!(env_json["max_age_days"], serde_json::json!(7));
    assert_eq!(env_json["max_age_source"], serde_json::json!("env"));
}

#[test]
fn audit_gc_debug_diagnostics_require_explicit_rust_log() {
    let fixture = create_audit_fixture("gc-diagnostics");
    let root = fixture.path();
    let scan = TempDir::new().expect("scan root should be created");
    // One reclaim candidate so the sweep has an entry to report: a bare
    // foreign lock sidecar.
    fs::write(
        scan.path()
            .join("fallow-audit-base-cache-3333333333333333-root-4444444444444444.lock"),
        "",
    )
    .expect("bare lock sidecar should be written");
    // Uncommitted change so audit does real changed-code work (the sweep only
    // runs then).
    fs::write(root.join("src/new.ts"), "export const changed = () => 1;\n")
        .expect("changed file should be written");
    let args = [
        "audit",
        "--root",
        root.to_str().expect("root should be utf-8"),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ];

    let with_debug = run_fallow_raw_with_env(
        &args,
        &[
            ("TMPDIR", scan.path()),
            ("TMP", scan.path()),
            ("TEMP", scan.path()),
            ("RUST_LOG", Path::new("fallow=debug")),
        ],
    );
    assert!(
        with_debug
            .stderr
            .contains("audit cache sweep considered entry"),
        "RUST_LOG=fallow=debug should surface one decision line per considered entry: {}",
        with_debug.stderr
    );
    assert!(
        // ANSI styling may sit between the field name and its value, so match
        // the reason value alone.
        with_debug.stderr.contains("\"lock-only\""),
        "the decision line should carry the reason field: {}",
        with_debug.stderr
    );

    let without_debug = run_prune_with_scan_root(&args, scan.path());
    assert!(
        !without_debug
            .stderr
            .contains("audit cache sweep considered entry"),
        "without an explicit RUST_LOG the audit stderr must stay diagnostics-free: {}",
        without_debug.stderr
    );
}

fn cache_sidecar(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .expect("cache path should have a name")
        .to_os_string();
    name.push(suffix);
    path.parent()
        .expect("cache path should have a parent")
        .join(name)
}

#[test]
fn audit_cache_remove_requires_explicit_root_and_removes_sidecars_but_keeps_locks() {
    let fixture = create_audit_fixture("cache-remove");
    let root = fixture.path();
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (new_path, legacy_path) = audit_cache_paths(root, base_sha);
    for path in [&new_path, &legacy_path] {
        fs::create_dir_all(path).expect("cache directory should be created");
        fs::write(cache_sidecar(path, ".sha"), format!("{base_sha}\n"))
            .expect("readiness sidecar should be written");
        fs::write(cache_sidecar(path, ".last-used"), "")
            .expect("last-used sidecar should be written");
        fs::write(cache_sidecar(path, ".lock"), "").expect("lock sidecar should be written");
    }

    let missing_root = Command::new(fallow_bin())
        .current_dir(root)
        .env("RUST_LOG", "")
        .env("NO_COLOR", "1")
        .args(["audit-cache", "remove", "--quiet"])
        .output()
        .expect("fallow should run");
    assert_eq!(missing_root.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing_root.stderr).contains("explicit `--root <path>`")
            || String::from_utf8_lossy(&missing_root.stdout).contains("explicit `--root <path>`"),
        "missing-root error should explain the explicit root requirement",
    );
    assert!(
        new_path.is_dir(),
        "validation failure must not remove caches"
    );
    assert!(
        legacy_path.is_dir(),
        "validation failure must not remove legacy caches"
    );

    let preview = run_fallow_raw(&[
        "audit-cache",
        "remove",
        "--root",
        root.to_str().expect("root should be utf-8"),
        "--dry-run",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        preview.code, 0,
        "dry-run should succeed: {}{}",
        preview.stdout, preview.stderr
    );
    let preview_json = parse_json(&preview);
    assert_eq!(preview_json["dry_run"], serde_json::json!(true));
    assert_eq!(preview_json["would_remove"], serde_json::json!(2));
    assert_eq!(preview_json["complete"], serde_json::json!(true));
    assert!(new_path.is_dir(), "dry-run must preserve the current cache");
    assert!(
        legacy_path.is_dir(),
        "dry-run must preserve the legacy cache"
    );

    let unconfirmed = run_fallow_raw(&[
        "audit-cache",
        "remove",
        "--root",
        root.to_str().expect("root should be utf-8"),
        "--quiet",
    ]);
    assert_eq!(
        unconfirmed.code, 2,
        "non-interactive removal must require --yes"
    );
    assert!(
        new_path.is_dir(),
        "unconfirmed removal must preserve caches"
    );

    let output = run_fallow_raw(&[
        "audit-cache",
        "remove",
        "--root",
        root.to_str().expect("root should be utf-8"),
        "--yes",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "audit-cache remove should succeed: {}{}",
        output.stdout, output.stderr
    );
    for path in [&new_path, &legacy_path] {
        assert!(!path.exists(), "cache directory should be removed");
        assert!(!cache_sidecar(path, ".sha").exists());
        assert!(!cache_sidecar(path, ".last-used").exists());
        assert!(cache_sidecar(path, ".lock").exists());
        fs::remove_file(cache_sidecar(path, ".lock"))
            .expect("test lock sidecar should be removable");
    }
}

#[test]
fn audit_cache_remove_reports_lock_contention_as_incomplete() {
    let fixture = create_audit_fixture("cache-remove-contended");
    let root = fixture.path();
    let base_sha = "0123456789abcdef0123456789abcdef01234567";
    let (cache_path, _) = audit_cache_paths(root, base_sha);
    fs::create_dir_all(&cache_path).expect("cache directory should be created");
    fs::write(cache_sidecar(&cache_path, ".sha"), base_sha)
        .expect("readiness sidecar should be written");
    fs::write(cache_sidecar(&cache_path, ".lock"), "").expect("lock sidecar should be written");
    let held_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(cache_sidecar(&cache_path, ".lock"))
        .expect("cache lock should open");
    held_lock.try_lock().expect("cache lock should be held");

    let output = run_fallow_raw(&[
        "audit-cache",
        "remove",
        "--root",
        root.to_str().expect("root should be utf-8"),
        "--yes",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "partial removal must not report success");
    let json = parse_json(&output);
    assert_eq!(json["removed"], serde_json::json!(0));
    assert_eq!(json["skipped"], serde_json::json!(1));
    assert_eq!(json["complete"], serde_json::json!(false));
    assert!(cache_path.is_dir(), "held cache must remain");
    drop(held_lock);
    fs::remove_dir_all(&cache_path).expect("cache fixture should be removed");
    fs::remove_file(cache_sidecar(&cache_path, ".sha"))
        .expect("readiness fixture should be removed");
    fs::remove_file(cache_sidecar(&cache_path, ".lock")).expect("lock fixture should be removed");
}

#[test]
fn audit_json_has_verdict_and_schema() {
    let dir = create_audit_fixture("verdict");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit with no changes should exit 0. stderr: {}",
        output.stderr
    );

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["command"].as_str(),
        Some("audit"),
        "command should be 'audit'"
    );
    assert_eq!(
        json["schema_version"].as_u64(),
        Some(u64::from(fallow_output::AUDIT_SCHEMA_VERSION)),
        "audit JSON should use the audit envelope version"
    );
}

#[test]
fn audit_pass_verdict_when_no_changes() {
    let dir = create_audit_fixture("nochanges");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 0, "no changes should give exit 0");

    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "no changes should give pass verdict"
    );
    assert_eq!(
        json["changed_files_count"].as_u64(),
        Some(0),
        "should report 0 changed files"
    );
}

#[test]
fn audit_css_selector_complexity_error_escalates_verdict() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"audit-css"}"#).unwrap();
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"css-selector-complexity":"error"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("src/styles.css"),
        "#app .card .title { color: red; }\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 1,
        "css selector error escalation should fail audit. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    let findings = json["complexity"]["styling_findings"]
        .as_array()
        .expect("styling findings should be present");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-selector-complexity"
                && finding["sub_kind"] == "high-specificity"
        }),
        "styling findings include selector complexity: {findings:#?}"
    );
}

#[test]
fn audit_css_selector_complexity_new_only_ignores_inherited_styling() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"audit-css-pr-diff"}"#).unwrap();
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"css-selector-complexity":"error"}}"#,
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    fs::write(
        root.join("src/styles.css"),
        "#app .legacy .title { color: red; }\n",
    )
    .unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join("src/styles.css"),
        "#app .legacy .title { color: red; }\n.plain { color: blue; }\n",
    )
    .unwrap();
    let inherited_output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        inherited_output.code, 0,
        "inherited styling should not fail new-only audit. stdout: {}\nstderr: {}",
        inherited_output.stdout, inherited_output.stderr
    );
    let inherited_json = parse_json(&inherited_output);
    assert_eq!(inherited_json["attribution"]["styling_introduced"], 0);
    assert_eq!(inherited_json["attribution"]["styling_inherited"], 1);
    assert_eq!(
        inherited_json["complexity"]["styling_findings"][0]["introduced"],
        false
    );

    fs::write(
        root.join("src/styles.css"),
        "#app .legacy .title { color: red; }\n.plain { color: blue; }\n#app .introduced .title { color: green; }\n",
    )
    .unwrap();
    let introduced_output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        introduced_output.code, 1,
        "introduced styling should fail new-only audit. stdout: {}\nstderr: {}",
        introduced_output.stdout, introduced_output.stderr
    );
    let json = parse_json(&introduced_output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(json["attribution"]["styling_introduced"], 1);
    assert_eq!(json["attribution"]["styling_inherited"], 1);
    let findings = json["complexity"]["styling_findings"]
        .as_array()
        .expect("styling findings should be present");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-selector-complexity"
                && finding["sub_kind"] == "high-specificity"
                && finding["line"] == 3
                && finding["introduced"] == true
        }),
        "introduced selector line should appear in styling findings: {findings:#?}"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding["line"] == 1 && finding["introduced"] == false),
        "inherited selector should be explicitly attributed: {findings:#?}"
    );
}

#[test]
fn audit_css_token_drift_gates_introduced_raw_style_value() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"audit-raw-css"}"#).unwrap();
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"css-token-drift":"error"}}"#,
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    fs::write(
        root.join("src/styles.css"),
        ":root { --text-size: 1rem; }\n.title { font-size: var(--text-size); }\n",
    )
    .unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join("src/styles.css"),
        ":root { --text-size: 1rem; }\n.title { font-size: var(--text-size); }\n.card { font-size: 15.75px; }\n",
    )
    .unwrap();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 1,
        "introduced raw style value near an existing token should fail when css-token-drift is error. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    let findings = json["complexity"]["styling_findings"]
        .as_array()
        .expect("styling findings should be present");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "font-size font-size: 15.75px"
                && finding["nearest_token"]["name"] == "--text-size"
        }),
        "raw style value near an existing token should appear as token drift: {findings:#?}"
    );
}

#[test]
fn audit_css_surfaces_unused_styled_export_as_dead_surface() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    write_dead_style_export_base(root);
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    write_dead_style_export_changed_files(root);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 1,
        "unused styled export should still fail through dead-code. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let findings = json["complexity"]["styling_findings"]
        .as_array()
        .expect("styling findings should be present");
    assert_dead_style_finding(findings, "unused-styled-binding", "Card");
    for (sub_kind, value) in [
        ("unused-styled-binding", "EmotionCard"),
        ("unused-stylex-binding", "styles"),
        ("unused-vanilla-extract-binding", "vanillaCard"),
        ("unused-panda-binding", "pandaClass"),
        ("unused-cva-binding", "button"),
    ] {
        assert_dead_style_finding(findings, sub_kind, value);
    }
}

fn write_dead_style_export_base(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-dead-styled","dependencies":{"styled-components":"^6.1.0","@emotion/styled":"^11.0.0","@stylexjs/stylex":"^0.1.0","@vanilla-extract/css":"^1.0.0","@pandacss/dev":"^0.54.0","class-variance-authority":"^0.7.0"}}"#,
    )
    .unwrap();
}

fn write_dead_style_export_changed_files(root: &Path) {
    for (path, source) in [
        (
            "src/index.ts",
            "import { usedValue } from './card';\n\
             import { emotionUsed } from './emotion';\n\
             import { stylexUsed } from './stylex';\n\
             import { vanillaUsed } from './vanilla.css';\n\
             import { pandaUsed } from './panda';\n\
             import { cvaUsed } from './cva';\n\
             console.log(usedValue, emotionUsed, stylexUsed, vanillaUsed, pandaUsed, cvaUsed);\n",
        ),
        (
            "src/card.tsx",
            "import styled from 'styled-components';\n\
             export const usedValue = 1;\n\
             export const Card = styled.div`\n\
               color: red;\n\
               padding: 8px;\n\
             `;\n",
        ),
        (
            "src/emotion.tsx",
            "import styled from '@emotion/styled';\n\
             export const emotionUsed = 1;\n\
             export const EmotionCard = styled.div`\n\
               color: red;\n\
             `;\n",
        ),
        (
            "src/stylex.ts",
            "import * as stylex from '@stylexjs/stylex';\n\
             export const stylexUsed = 1;\n\
             export const styles = stylex.create({ root: { color: 'red' } });\n",
        ),
        (
            "src/vanilla.css.ts",
            "import { style } from '@vanilla-extract/css';\n\
             export const vanillaUsed = 1;\n\
             export const vanillaCard = style({ color: 'red' });\n",
        ),
        (
            "src/panda.ts",
            "import { css } from '../styled-system/css';\n\
             export const pandaUsed = 1;\n\
             export const pandaClass = css({ color: 'red' });\n",
        ),
        (
            "src/cva.ts",
            "import { cva } from 'class-variance-authority';\n\
             export const cvaUsed = 1;\n\
             export const button = cva('px-3 py-2');\n",
        ),
    ] {
        fs::write(root.join(path), source).unwrap();
    }
}

fn assert_dead_style_finding(findings: &[serde_json::Value], sub_kind: &str, value: &str) {
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-dead-surface"
                && finding["sub_kind"] == sub_kind
                && finding["value"]
                    .as_str()
                    .is_some_and(|actual| actual.contains(value))
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
        }),
        "{sub_kind} {value} should appear as a styling dead surface: {findings:#?}"
    );
}

#[test]
fn audit_css_surfaces_cva_duplicate_variant_blocks() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-cva","dependencies":{"class-variance-authority":"^0.7.0","tailwindcss":"^4.0.0"}}"#,
    )
    .unwrap();
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join("src/button.ts"),
        "import { cva } from 'class-variance-authority';\n\
         export const button = cva('inline-flex', {\n\
           variants: {\n\
             tone: {\n\
               primary: 'px-3 py-2 text-sm font-medium bg-[#f05a28]',\n\
               secondary: 'px-3 py-2 text-sm font-medium bg-[#f05a28]',\n\
             },\n\
           },\n\
         });\n",
    )
    .unwrap();
    fs::write(
        root.join("src/styles.css"),
        "@theme {\n  --color-brand: #f05a28;\n}\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&output);
    let findings = json["complexity"]["styling_findings"]
        .as_array()
        .expect("styling findings should be present");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-duplicate-block"
                && finding["sub_kind"] == "cva-duplicate-variant-block"
                && finding["value"].as_str().is_some_and(|value| {
                    value.contains("px-3 py-2 text-sm font-medium bg-[#f05a28]")
                })
                && finding["confidence"] == "high"
                && finding["agent_disposition"] == "fix-confidently"
        }),
        "CVA duplicate variant block should appear as a styling finding: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "cva-variant-token-drift"
                && finding["value"]
                    .as_str()
                    .is_some_and(|value| value.contains("bg-[#f05a28]"))
                && finding["nearest_token"]["name"] == "--color-brand"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
                && finding["fix_hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("reuse --color-brand"))
        }),
        "CVA variant arbitrary value should point at the existing theme token: {findings:#?}"
    );
}

#[test]
fn audit_human_splits_styling_by_confidence_budget() {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("package.json"), r#"{"name":"audit-styling-ux"}"#).unwrap();
    fs::write(root.join("src/index.ts"), "export const ok = true;\n").unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join("src/styles.css"),
        ":root { --text-size: 1rem; }\n#app .legacy .title { color: red; }\n.card { font-size: 15.75px; }\n",
    )
    .unwrap();

    let output = run_fallow_raw(&["audit", "--root", root.to_str().unwrap(), "--base", "HEAD"]);
    assert!(
        output.stdout.contains("Fix confidently"),
        "human styling output should include high-confidence group. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("Verify first"),
        "human styling output should include verify-first group. stdout: {}",
        output.stdout
    );
}

#[test]
fn audit_css_deep_surfaces_cross_file_styling_findings() {
    let dir = create_audit_css_deep_fixture();
    let root = dir.path();

    let default_json = audit_css_json(root, &[], 0);
    let default_findings = default_json["complexity"]["styling_findings"]
        .as_array()
        .expect("default audit should include deep styling findings");
    assert_has_deep_css_findings(default_findings);

    let shallow_json = audit_css_json(root, &["--no-css-deep"], 0);
    let shallow_findings = shallow_json["complexity"]["styling_findings"]
        .as_array()
        .expect("shallow audit should keep local styling findings");
    assert_has_raw_style_value(shallow_findings);
    assert_no_deep_css_findings(shallow_findings);

    let deep_json = audit_css_json(root, &["--css-deep"], 0);
    let findings = deep_json["complexity"]["styling_findings"]
        .as_array()
        .expect("deep styling findings should be present");
    assert_has_deep_css_findings(findings);
}

fn create_audit_css_deep_fixture() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-css-deep","devDependencies":{"@pandacss/dev":"^1.0.0","@stylexjs/stylex":"^0.10.0","@vanilla-extract/css":"^1.17.0","tailwindcss":"^4.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("src/app.jsx"),
        "export const App = () => <div />;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/tokens.stylex.ts"),
        "import * as stylex from '@stylexjs/stylex';\nexport const vars = stylex.defineVars({ color: { brand: '#123456' } });\n",
    )
    .unwrap();
    fs::write(
        root.join("src/vanilla.css.ts"),
        "import { createGlobalTheme } from '@vanilla-extract/css';\nexport const veVars = createGlobalTheme(':root', { color: { accent: '#654321' } });\n",
    )
    .unwrap();
    fs::write(
        root.join("src/panda.ts"),
        "import { defineTokens } from '@pandacss/dev';\nexport const pandaTokens = defineTokens({ colors: { info: { value: '#abcdef' } } });\n",
    )
    .unwrap();
    fs::write(
        root.join("panda.config.ts"),
        "import { defineConfig } from '@pandacss/dev';\nexport default defineConfig({ theme: { tokens: { colors: { config: { value: '#fedcba' } } } } });\n",
    )
    .unwrap();
    fs::write(
        root.join("src/styles.css"),
        "@theme {\n  --color-zbrand: #f05a28;\n  --color-danger: red;\n  --text-body: 14px;\n  --spacing-zcard: 1rem;\n}\n.btn-primary { color: red; font-size: 14px; }\n",
    )
    .unwrap();
    git(root, &["init", "-b", "main"]);
    commit_all(root, "initial");

    fs::write(
        root.join("src/app.jsx"),
        "export const App = () => <div className=\"btn-prmary bg-abrand\" />;\n",
    )
    .unwrap();
    fs::write(
        root.join("src/styles.css"),
        "@theme {\n  --color-zbrand: #f05a28;\n  --color-danger: red;\n  --color-abrand: rgb(240 90 41);\n  --color-status-queued-bg: rgb(240 90 42);\n  --color-secondary: hsl(40 6% 93%);\n  --color-muted: hsl(40 6% 93%);\n  --text-body: 14px;\n  --spacing-zcard: 1rem;\n  --spacing-acard: 16.25px;\n  --shadow-glow: 0 0 8px red;\n}\n:root { --color-notice: #00ff00; }\n.btn-primary { color: red; font-size: 14px; }\n.notice { background-color: #00ff00; }\n.stylex-match { color: #123456; }\n.vanilla-match { color: #654321; }\n.panda-match { color: #abcdef; }\n.panda-config-match { color: #fedcba; }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/tokens.stylex.ts"),
        "import * as stylex from '@stylexjs/stylex';\nexport const vars = stylex.defineVars({ color: { brand: '#123456', signal: '#123457' } });\n",
    )
    .unwrap();
    dir
}

fn audit_css_json(root: &Path, extra_args: &[&str], expected_code: i32) -> serde_json::Value {
    let root = root.to_str().unwrap();
    let mut args = vec!["audit", "--root", root, "--base", "HEAD"];
    args.extend_from_slice(extra_args);
    args.extend_from_slice(&["--format", "json", "--quiet"]);
    let output = run_fallow_raw(&args);
    assert_eq!(
        output.code, expected_code,
        "audit exited unexpectedly. stderr: {}",
        output.stderr
    );
    parse_json(&output)
}

fn assert_has_deep_css_findings(findings: &[serde_json::Value]) {
    assert_has_raw_style_value(findings);
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-dead-surface"
                && finding["sub_kind"] == "unused-theme-token"
                && finding["value"] == "--shadow-glow"
                && finding["blast_radius"] == 0
        }),
        "deep audit should include dead theme token with blast radius: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-broken-reference"
                && finding["sub_kind"] == "unresolved-class-reference"
                && finding["value"] == "btn-prmary -> btn-primary"
        }),
        "deep audit should include unresolved class reference: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "near-duplicate-theme-token"
                && finding["value"] == "--color-abrand: rgb(240 90 41)"
                && finding["nearest_token"]["name"] == "--color-zbrand"
                && finding["confidence"] == "high"
                && finding["agent_disposition"] == "fix-confidently"
        }),
        "deep audit should include near-duplicate theme token with target: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "near-duplicate-theme-token"
                && finding["value"] == "--spacing-acard: 16.25px"
                && finding["nearest_token"]["name"] == "--spacing-zcard"
                && finding["confidence"] == "high"
                && finding["agent_disposition"] == "fix-confidently"
        }),
        "deep audit should include numeric near-duplicate theme token with target: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "near-duplicate-theme-token"
                && finding["value"] == "--color-status-queued-bg: rgb(240 90 42)"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
        }),
        "semantic product color aliases should be review-first: {findings:#?}"
    );
    assert!(
        !findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "near-duplicate-theme-token"
                && finding["value"].as_str().is_some_and(|value| {
                    value.starts_with("--color-secondary:") || value.starts_with("--color-muted:")
                })
        }),
        "semantic shadcn-style aliases must not be near-duplicate token findings: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "near-duplicate-css-in-js-token"
                && finding["value"] == "vars.color.signal: #123457"
                && finding["nearest_token"]["name"] == "vars.color.brand"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
        }),
        "deep audit should include near-duplicate CSS-in-JS token with target: {findings:#?}"
    );
}

fn assert_has_raw_style_value(findings: &[serde_json::Value]) {
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color color: red"
                && finding["nearest_token"]["name"] == "--color-danger"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
                && finding["fix_hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("reuse --color-danger"))
        }),
        "audit should include local raw color value with nearest token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "font-size font-size: 14px"
                && finding["nearest_token"]["name"] == "--text-body"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
        }),
        "audit should include local raw font-size value with nearest token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color background-color: #0f0"
                && finding["nearest_token"]["name"] == "--color-notice"
                && finding["fix_hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("reuse --color-notice"))
        }),
        "audit should include raw color matched to a CSS custom property token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color color: #123456"
                && finding["nearest_token"]["name"] == "vars.color.brand"
                && finding["fix_hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("reuse vars.color.brand"))
        }),
        "audit should include raw color matched to a CSS-in-JS token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color color: #654321"
                && finding["nearest_token"]["name"] == "veVars.color.accent"
        }),
        "audit should include raw color matched to a vanilla-extract token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color color: #abcdef"
                && finding["nearest_token"]["name"] == "pandaTokens.colors.info"
        }),
        "audit should include raw color matched to a Panda token: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-token-drift"
                && finding["sub_kind"] == "raw-style-value"
                && finding["value"] == "color color: #fedcba"
                && finding["nearest_token"]["name"] == "pandaConfig.colors.config"
        }),
        "audit should include raw color matched to a Panda config token: {findings:#?}"
    );
}

fn assert_no_deep_css_findings(findings: &[serde_json::Value]) {
    assert!(
        !findings.iter().any(|finding| {
            matches!(
                finding["sub_kind"].as_str(),
                Some(
                    "unused-theme-token"
                        | "unresolved-class-reference"
                        | "near-duplicate-theme-token"
                        | "near-duplicate-css-in-js-token"
                )
            )
        }),
        "shallow audit should not emit cross-file styling findings: {findings:#?}"
    );
}

/// Audit's HEAD analyses and base-snapshot computation run concurrently via
/// `rayon::join`; inside the base snapshot, check and dupes also run
/// concurrently. Verify nondeterministic scheduling does not leak into the
/// rendered JSON: repeated runs against the same fixture must produce
/// byte-identical output once wall-clock fields are stripped.
#[test]
fn audit_parallel_output_is_deterministic() {
    let dir = create_audit_fixture("determinism");

    fs::write(
        dir.path().join("src/new.ts"),
        "export const dupA = (x: number) => x + 1;\nexport const dupB = (x: number) => x + 1;\n",
    )
    .unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", "add new file"])
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("elapsed_ms");
                map.remove("head_sha");
                if let Some(telemetry) = map
                    .get_mut("_meta")
                    .and_then(|meta| meta.get_mut("telemetry"))
                    .and_then(|telemetry| telemetry.as_object_mut())
                {
                    telemetry.remove("analysis_run_id");
                }
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    normalize(v);
                }
            }
            _ => {}
        }
    }

    let mut canonicalized: Vec<String> = std::iter::repeat_with(|| {
        let output = run_fallow_raw(&[
            "audit",
            "--root",
            dir.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
        ]);
        assert!(
            output.code == 0 || output.code == 1,
            "audit run should not crash: stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
        let mut value = parse_json(&output);
        normalize(&mut value);
        serde_json::to_string(&value).expect("re-serialize canonical json")
    })
    .take(3)
    .collect();

    let first = canonicalized.remove(0);
    for (idx, run) in canonicalized.iter().enumerate() {
        assert_eq!(
            &first,
            run,
            "audit parallel run #{} differed from run #0",
            idx + 1
        );
    }
}

#[test]
fn audit_json_has_summary_with_changes() {
    let dir = create_audit_fixture("summary");

    fs::write(
        dir.path().join("src/new.ts"),
        "export const newThing = 'added';\n",
    )
    .unwrap();

    Command::new("git")
        .args(["add", "."])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["-c", "commit.gpgsign=false", "commit", "-m", "add new file"])
        .current_dir(dir.path())
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash, got exit {}. stderr: {}",
        output.code,
        output.stderr
    );

    let json = parse_json(&output);
    assert!(
        json.get("summary").is_some(),
        "audit JSON should have summary"
    );
    let summary = &json["summary"];
    assert!(
        summary.get("dead_code_issues").is_some(),
        "summary should have dead_code_issues"
    );
}

/// Create a fixture whose legacy file already has several unused exports,
/// then branch and touch that file without introducing new issues.
///
/// Returns the `TempDir` guard. The fixture is on a branch named
/// `feature`; the default branch is `main`.
fn create_audit_baseline_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-baseline-test", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler"},"include":["src"]}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/legacy.ts"),
        "export const used = 1;\n\
         export const unusedA = 'a';\n\
         export const unusedB = 'b';\n\
         export const unusedC = 'c';\n\
         export const unusedD = 'd';\n\
         export const unusedE = 'e';\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './legacy';\nconsole.log(used);\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
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
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    let legacy = fs::read_to_string(dir.join("src/legacy.ts")).unwrap();
    fs::write(dir.join("src/legacy.ts"), format!("{legacy}// touched\n")).unwrap();
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "touch legacy"]);

    tmp
}

#[test]
fn audit_default_gate_ignores_inherited_issues() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit should pass when touched file has only inherited issues. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
    let dead_code_issues = json["summary"]["dead_code_issues"]
        .as_u64()
        .expect("summary.dead_code_issues should be present");
    assert!(
        dead_code_issues >= 5,
        "expected at least 5 pre-existing unused exports, got {dead_code_issues}"
    );
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0)
    );
    assert!(
        json["attribution"]["dead_code_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 5),
        "expected inherited dead-code attribution"
    );
    let inherited_exports = json["dead_code"]["unused_exports"]
        .as_array()
        .expect("dead_code.unused_exports should be an array");
    assert!(
        inherited_exports
            .iter()
            .all(|item| item["introduced"] == false),
        "all touched legacy exports should be annotated as inherited"
    );
}

#[test]
fn audit_new_only_inherits_shifted_duplicate_group() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();

    let duplicate = "export function sharedBlock(x: number): number {\n\
          const a = x + 1;\n\
          const b = a * 2;\n\
          const c = b - 3;\n\
          const d = c * c;\n\
          const e = d + a;\n\
          const f = e - b;\n\
          const g = f + c;\n\
          const h = g * d;\n\
          const i = h - e;\n\
          return a + b + c + d + e + f + g + h + i;\n\
        }\n";
    fs::write(dir.join("fileB.ts"), duplicate).unwrap();

    use std::fmt::Write as _;
    let mut shifted_source = String::new();
    for n in 1..=120 {
        writeln!(shifted_source, "export const v{n} = {n};").unwrap();
    }
    shifted_source.push_str(duplicate);
    fs::write(dir.join("fileA.ts"), &shifted_source).unwrap();

    git(dir, &["init", "-b", "main"]);
    // The clone fingerprint is hashed over the raw fragment text, so CRLF vs LF
    // shifts it. `fallow audit` spawns its own `git worktree add` for the base
    // snapshot, which inherits the runner's global git config (Windows defaults
    // to `core.autocrlf=true`), so the checked-out base would get CRLF while the
    // head file written via `fs::write` keeps LF, making the inherited clone look
    // introduced. Pin the repo to LF so base and head fingerprints match.
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "edit"]);

    fs::write(
        dir.join("fileA.ts"),
        format!("export const NEW_TOP_CONST = 0;\n{shifted_source}"),
    )
    .unwrap();
    commit_all(dir, "shift unchanged duplicate");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--performance",
        "--dupes-mode",
        "strict",
        "--dupes-min-tokens",
        "10",
        "--dupes-min-lines",
        "3",
    ]);

    assert_eq!(
        output.code, 0,
        "audit should pass when only line numbers changed for an inherited duplicate. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["base_snapshot_skipped"].as_bool(), Some(false));
    assert_eq!(
        json["attribution"]["duplication_introduced"].as_u64(),
        Some(0)
    );
    assert!(
        json["attribution"]["duplication_inherited"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "expected inherited duplicate attribution"
    );

    let groups = json["duplication"]["clone_groups"]
        .as_array()
        .expect("duplication.clone_groups should be an array");
    assert!(!groups.is_empty(), "expected at least one clone group");
    assert!(
        groups.iter().all(|group| group["introduced"] == false),
        "all duplicate groups should be marked inherited"
    );
    assert!(
        groups.iter().any(|group| {
            group["instances"].as_array().is_some_and(|instances| {
                let has_shifted_file = instances
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some("fileA.ts"));
                let has_peer_file = instances
                    .iter()
                    .any(|instance| instance["file"].as_str() == Some("fileB.ts"));
                has_shifted_file && has_peer_file
            })
        }),
        "expected a clone group spanning fileA.ts and fileB.ts"
    );
}

const DEMOTION_HELPER: &str = "export function eq(xs: string[], ys: string[]): boolean {\n  if (xs.length !== ys.length) {\n    return false;\n  }\n  for (let index = 0; index < xs.length; index += 1) {\n    if (xs[index] !== ys[index]) {\n      return false;\n    }\n  }\n  return true;\n}\n";

const DEMOTION_SCAFFOLDING: &str = "export function selfTest(): boolean {\n  const alpha = ['alpha', 'beta', 'gamma'];\n  const beta = ['alpha', 'beta', 'gamma'];\n  const gamma = ['delta', 'epsilon', 'zeta'];\n  const first = eq(alpha, beta);\n  const second = eq(alpha, gamma);\n  const third = eq(beta, gamma);\n  const outcomes = [first, !second, !third];\n  const labels = ['same', 'differs', 'differs'];\n  const combined = outcomes.map((outcome, index) => `${labels[index]}:${outcome}`);\n  return combined.length === outcomes.length && outcomes.every((outcome) => outcome === true);\n}\n";

/// Repo whose working tree extracts an identical helper out of two committed
/// modules (the issue #2164 refactor). The surviving scaffolding clone group
/// contains no added line, so the new-only gate demotes it to inherited and
/// the demotion diff source becomes observable (issue #2220).
fn reshaped_clone_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-demotion-reshape","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { selfTest as a } from './a';\nimport { selfTest as b } from './b';\na();\nb();\n",
    )
    .unwrap();
    let base_module = format!("{DEMOTION_HELPER}{DEMOTION_SCAFFOLDING}");
    fs::write(dir.join("src/a.ts"), &base_module).unwrap();
    fs::write(dir.join("src/b.ts"), &base_module).unwrap();

    git(dir, &["init", "-b", "main"]);
    // Pin LF so base-worktree checkouts on Windows keep the head fingerprints.
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "initial");

    let refactored = format!("import {{ eq }} from './lib';\n{DEMOTION_SCAFFOLDING}");
    fs::write(dir.join("src/a.ts"), &refactored).unwrap();
    fs::write(dir.join("src/b.ts"), &refactored).unwrap();
    fs::write(dir.join("src/lib.ts"), DEMOTION_HELPER).unwrap();
    tmp
}

/// Without an opt-in shared diff the merge-base worktree diff decides the
/// demotion, and both the demotion note and the `--explain` detail must name
/// it (issue #2220, `DupeDemotionDiffSource::Worktree`).
#[test]
fn audit_demotion_note_names_worktree_diff_source_under_explain() {
    let fixture = reshaped_clone_fixture();
    let root = fixture.path().to_str().unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "HEAD",
        "--gate",
        "new-only",
        "--no-cache",
        "--explain",
    ]);

    assert_eq!(
        output.code, 0,
        "the demoted refactor must pass the gate. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert!(
        output.stderr.contains(
            "of which 1 clone group was reclassified as pre-existing: no duplicated line was added by this change (diff source: merge-base worktree diff vs HEAD)"
        ),
        "the demotion note must name the worktree diff source: {}",
        output.stderr
    );
    assert!(
        output.stdout.contains("demoted dup:")
            && output.stdout.contains(
                "no instance overlaps an added line (rule: no-added-lines, gate: new-only)"
            ),
        "--explain must list the demoted group: {}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("demotion diff source: merge-base worktree diff vs HEAD"),
        "--explain must name the deciding diff source: {}",
        output.stdout
    );
}

/// An opt-in shared diff (`--diff-file`) must take precedence over the
/// merge-base worktree diff for the demotion decision (issue #2220,
/// `DupeDemotionDiffSource::Shared` via `shared_diff_source_label()`): a diff
/// whose added lines fall inside a clone instance keeps the group gating as
/// introduced, where the worktree diff alone would have demoted it.
#[test]
fn audit_demotion_shared_diff_source_takes_precedence_over_worktree_diff() {
    let fixture = reshaped_clone_fixture();
    let root = fixture.path().to_str().unwrap();

    let control = run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "HEAD",
        "--gate",
        "new-only",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        control.code, 0,
        "control run should pass: {}{}",
        control.stdout, control.stderr
    );
    let control_json = parse_json(&control);
    assert_eq!(control_json["attribution"]["duplication_demoted"], 1);
    assert_eq!(control_json["attribution"]["duplication_introduced"], 0);

    // A shared diff claiming an added line INSIDE the scaffolding clone
    // instance (src/a.ts spans lines 2-12 after the refactor). The worktree
    // diff sees no added line there, so any behavior change below is the
    // shared source deciding.
    let diff_dir = TempDir::new().expect("diff dir should be created");
    let diff_path = diff_dir.path().join("touch-instance.diff");
    fs::write(
        &diff_path,
        "diff --git a/src/a.ts b/src/a.ts\n\
         --- a/src/a.ts\n\
         +++ b/src/a.ts\n\
         @@ -4,0 +5,1 @@\n\
         +  const injected = 'x';\n",
    )
    .unwrap();

    let shared = run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "HEAD",
        "--gate",
        "new-only",
        "--no-cache",
        "--diff-file",
        diff_path.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        shared.code, 0,
        "introduced duplication warns without failing by default: {}{}",
        shared.stdout, shared.stderr
    );
    let shared_json = parse_json(&shared);
    assert_eq!(
        shared_json["attribution"]["duplication_demoted"], 0,
        "a shared diff touching an instance must veto the demotion: {shared_json}"
    );
    assert_eq!(shared_json["attribution"]["duplication_introduced"], 1);
    let groups = shared_json["duplication"]["clone_groups"]
        .as_array()
        .expect("clone groups array");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["introduced"], true);
    assert!(
        groups[0].get("demotion_reason").is_none(),
        "a non-demoted group must not carry the marker: {}",
        groups[0]
    );
}

/// When no diff can be obtained at all the demotion check is skipped, every
/// introduced clone group keeps gating, and `--explain` says so (issue #2220,
/// `DupeDemotionDiffSource::Skipped`). A missing external diff command makes
/// the merge-base worktree diff fail while changed-file discovery still runs.
#[test]
fn audit_demotion_skipped_diff_source_prints_explain_line() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-demotion-skipped","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"duplicates":{"threshold":1.0}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { selfTest as a } from './a';\na();\n",
    )
    .unwrap();
    let module = format!("{DEMOTION_HELPER}{DEMOTION_SCAFFOLDING}");
    fs::write(dir.join("src/a.ts"), &module).unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "initial");

    // The paste: the new clone instance is all added lines, so it gates.
    fs::write(dir.join("src/b.ts"), &module).unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { selfTest as a } from './a';\nimport { selfTest as b } from './b';\na();\nb();\n",
    )
    .unwrap();
    // `--name-only` does not invoke an external diff, so changed-file
    // discovery succeeds. Reading the actual diff does invoke it and fails.
    let missing_external_diff = dir.join("missing-external-diff");
    let diff_env = [("GIT_EXTERNAL_DIFF", missing_external_diff.as_path())];

    let output = run_fallow_raw_with_env(
        &[
            "audit",
            "--root",
            dir.to_str().unwrap(),
            "--base",
            "HEAD",
            "--gate",
            "new-only",
            "--no-cache",
            "--explain",
        ],
        &diff_env,
    );

    assert_eq!(
        output.code, 1,
        "the pasted clone must keep gating when the demotion check is skipped. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert!(
        output
            .stdout
            .contains("demotion check skipped: no diff available"),
        "--explain must surface the skipped demotion check: {}",
        output.stdout
    );

    let json_run = run_fallow_raw_with_env(
        &[
            "audit",
            "--root",
            dir.to_str().unwrap(),
            "--base",
            "HEAD",
            "--gate",
            "new-only",
            "--no-cache",
            "--format",
            "json",
            "--quiet",
        ],
        &diff_env,
    );
    assert_eq!(json_run.code, 1);
    let json = parse_json(&json_run);
    assert_eq!(json["attribution"]["duplication_demoted"], 0);
    assert_eq!(json["attribution"]["duplication_introduced"], 1);
}

#[test]
fn audit_gate_all_reports_preexisting_issues() {
    let tmp = create_audit_baseline_fixture();
    fs::write(tmp.path().join("fallow.toml"), "[audit]\ngate = \"all\"\n").unwrap();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--config",
        tmp.path().join("fallow.toml").to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "audit should fail when audit.gate=all and touched file has pre-existing issues. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(json["attribution"]["gate"].as_str(), Some("all"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "gate=all should skip base attribution work"
    );
    assert_eq!(
        json["attribution"]["dead_code_inherited"].as_u64(),
        Some(0),
        "gate=all should skip base attribution work"
    );
    assert!(
        json["dead_code"]["unused_exports"][0]
            .get("introduced")
            .is_none(),
        "gate=all should not annotate per-issue introduced fields without a base snapshot"
    );
}

#[test]
fn audit_gate_cli_flag_overrides_default() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "all",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "--gate all should fail on inherited findings. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(json["attribution"]["gate"].as_str(), Some("all"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0)
    );
    assert_eq!(json["attribution"]["dead_code_inherited"].as_u64(), Some(0));
}

#[test]
fn audit_help_documents_gate() {
    let output = run_fallow_raw(&["audit", "--help"]);
    assert_eq!(output.code, 0, "audit --help should succeed");
    assert!(
        output.stdout.contains("--gate <GATE>"),
        "--help should include --gate, got:\n{}",
        output.stdout
    );
    assert!(
        output.stdout.contains("new-only") && output.stdout.contains("introduced"),
        "--help should document new-only semantics, got:\n{}",
        output.stdout
    );
}

#[test]
fn audit_base_preserves_node_modules_tsconfig_extends_context() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join(".gitignore"), "node_modules\n.fallow\n").unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-rn-alias","main":"src/index.ts","dependencies":{"@react-native/typescript-config":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"extends":"./node_modules/@react-native/typescript-config/tsconfig.json","compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from '@/feature';\nconsole.log(used);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/feature.ts"),
        "export const used = 1;\nexport const legacyUnused = 2;\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    let rn_config = dir.join("node_modules/@react-native/typescript-config");
    fs::create_dir_all(&rn_config).unwrap();
    fs::write(
        rn_config.join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"react-native","moduleResolution":"bundler"}}"#,
    )
    .unwrap();

    fs::write(
        dir.join("src/feature.ts"),
        "export const used = 1;\nexport const legacyUnused = 2;\nexport const introduced = 3;\n",
    )
    .unwrap();
    commit_all(dir, "introduce new export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert!(
        !output.stderr.contains("tsconfig chain")
            && !output.stderr.contains("node_modules directory not found"),
        "audit base worktree should retain installed tsconfig context. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dead_code"]["summary"]["unresolved_imports"].as_u64(),
        Some(0),
        "tsconfig alias should resolve in the current analysis"
    );
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1),
        "only the genuinely new export should be attributed to the changeset"
    );
}

#[test]
fn audit_new_unlisted_dependency_import_site_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-unlisted","main":"src/index.ts","dependencies":{}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler"},"include":["src"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/a.ts"),
        "import leftPad from 'left-pad';\nexport const a = leftPad('a', 2);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { a } from './a';\nconsole.log(a);\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("src/b.ts"),
        "import leftPad from 'left-pad';\nexport const b = leftPad('b', 2);\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { a } from './a';\nimport { b } from './b';\nconsole.log(a, b);\n",
    )
    .unwrap();
    commit_all(dir, "add b");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "new unlisted import site should fail new-only audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1)
    );
    assert_eq!(
        json["dead_code"]["unlisted_dependencies"][0]["introduced"],
        true
    );
}

#[test]
fn audit_empty_catalog_group_changed_manifest_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/app")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-empty-catalog-group","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("packages/app/package.json"),
        r#"{"name":"app","private":true,"main":"src/index.ts","dependencies":{"vue":"catalog:vue3"}}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("packages/app/src")).unwrap();
    fs::write(
        dir.join("packages/app/src/index.ts"),
        "import { ref } from 'vue';\nconsole.log(ref);\n",
    )
    .unwrap();
    fs::write(
        dir.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  vue3:\n    vue: ^3.4.0\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  legacy: {}\n  vue3:\n    old-react: ^17.0.2\n    vue: ^3.4.0\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert_eq!(
        output.code, 0,
        "new warning-level catalog hygiene should not fail audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("warn"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(2)
    );
    assert_eq!(
        json["dead_code"]["unused_catalog_entries"][0]["entry_name"].as_str(),
        Some("old-react")
    );
    assert_eq!(
        json["dead_code"]["unused_catalog_entries"][0]["introduced"],
        true
    );
    assert_eq!(
        json["dead_code"]["empty_catalog_groups"][0]["catalog_name"].as_str(),
        Some("legacy")
    );
    assert_eq!(
        json["dead_code"]["empty_catalog_groups"][0]["introduced"],
        true
    );
}

#[test]
fn audit_invalid_client_export_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-invalid-client-export","private":true,"dependencies":{"next":"15.0.0","react":"19.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nexport default function Page() { return null; }\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // Introduce a server-only export inside the existing "use client" file.
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nexport const metadata = { title: \"Home\" };\nexport default function Page() { return null; }\n",
    )
    .unwrap();

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    assert_eq!(
        output.code, 0,
        "new warning-level invalid client export should not fail audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dead_code"]["invalid_client_exports"][0]["export_name"].as_str(),
        Some("metadata")
    );
    assert_eq!(
        json["dead_code"]["invalid_client_exports"][0]["introduced"],
        true
    );
}

#[test]
fn audit_dependency_location_change_is_introduced() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-dep-move","main":"src/index.ts","devDependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "console.log('hi');\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-dep-move","main":"src/index.ts","dependencies":{"left-pad":"1.0.0"}}"#,
    )
    .unwrap();
    commit_all(dir, "move dependency");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "moving an unused package into dependencies should be introduced. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("fail"));
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(1)
    );
    assert_eq!(
        json["dead_code"]["unused_dependencies"][0]["introduced"],
        true
    );
}

#[test]
fn audit_with_dead_code_baseline_filters_preexisting_issues() {
    let tmp = create_audit_baseline_fixture();
    let dir = tmp.path();
    let baseline_path = dir.join(".fallow-dead-code-baseline.json");

    let git = |args: &[&str]| {
        Command::new("git")
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
            .output()
            .expect("git command failed")
    };
    git(&["checkout", "main"]);
    let save = run_fallow_raw(&[
        "dead-code",
        "--root",
        dir.to_str().unwrap(),
        "--save-baseline",
        baseline_path.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert!(
        save.code == 0 || save.code == 1,
        "save-baseline should not crash, got {}: {}",
        save.code,
        save.stderr
    );
    assert!(
        baseline_path.exists(),
        "baseline file should have been written"
    );
    git(&["checkout", "feature"]);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--dead-code-baseline",
        baseline_path.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "audit with dead-code baseline should pass (no new issues). stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("pass"),
        "verdict should be pass when all pre-existing issues are baselined"
    );
    assert_eq!(
        json["summary"]["dead_code_issues"].as_u64(),
        Some(0),
        "baseline should filter all pre-existing unused exports"
    );
}

#[test]
fn audit_rejects_global_baseline_flag() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "--baseline",
        "anything.json",
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 2,
        "global --baseline on audit should exit 2. stderr: {}",
        output.stderr
    );
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("--dead-code-baseline")
            || combined.contains("--health-baseline")
            || combined.contains("--dupes-baseline"),
        "error should point users at per-analysis flags, got: {combined}"
    );
}

#[test]
fn audit_rejects_global_save_baseline_flag() {
    let tmp = create_audit_baseline_fixture();
    let output = run_fallow_raw(&[
        "--save-baseline",
        "anywhere.json",
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 2,
        "global --save-baseline on audit should exit 2. stderr: {}",
        output.stderr
    );
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("--dead-code-baseline")
            || combined.contains("--health-baseline")
            || combined.contains("--dupes-baseline"),
        "error should point users at per-analysis flags, got: {combined}"
    );
}

#[test]
fn audit_badge_format_exits_2() {
    let dir = create_audit_fixture("badge");
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "badge",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "audit with --format badge should exit 2 (unsupported)"
    );
}

/// `--max-crap` on audit must flow into the health sub-analysis so that a
/// changed file with a high-complexity untested function triggers the
/// failing verdict.
#[test]
fn audit_max_crap_flag_fails_when_threshold_crossed() {
    let dir = create_audit_fixture("crap");

    write_branchy_change(dir.path());

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "1",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 1,
        "audit should fail when --max-crap is crossed. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["verdict"].as_str(),
        Some("fail"),
        "verdict should be fail when CRAP threshold is crossed"
    );
}

#[test]
fn audit_respects_health_threshold_override() {
    let dir = create_audit_fixture("health-threshold-override");
    fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{
  "health": {
    "thresholdOverrides": [
      {
        "files": ["src/index.ts"],
        "functions": ["branchy"],
        "maxCyclomatic": 20,
        "maxCognitive": 20,
        "maxCrap": 100
      }
    ]
  }
}
"#,
    )
    .unwrap();
    write_branchy_change(dir.path());

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "1",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "audit should pass when health override raises local thresholds. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

fn audit_with_env(root: &Path, env: &[(&str, &str)]) -> common::CommandOutput {
    let bin = fallow_bin();
    let mut cmd = Command::new(&bin);
    cmd.args([
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ])
    .env("RUST_LOG", "")
    .env("NO_COLOR", "1");
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Regression test for issue #301. When git invokes hooks (`pre-commit`,
/// `pre-push`), it sets `GIT_INDEX_FILE=.git/index` (relative path) plus
/// related repo-state vars. Before the fix in #301, fallow inherited these
/// into its own git invocations and `git worktree add` failed because the
/// relative index path no longer resolved from the temporary worktree dir.
///
/// The test runs `fallow audit` under each of the ambient repo-state vars
/// individually and asserts the audit succeeds, mirroring the leak shapes a
/// hook subprocess actually sees.
#[test]
fn audit_succeeds_when_ambient_git_env_vars_leak_from_a_hook() {
    let dir = create_audit_fixture("hook_env_leak");
    let root = dir.path();

    let abs_index = root.join(".git/index").to_string_lossy().to_string();
    let cases: &[(&str, &str)] = &[
        ("GIT_INDEX_FILE", ".git/index"),
        ("GIT_INDEX_FILE", abs_index.as_str()),
        ("GIT_DIR", ".git"),
        ("GIT_WORK_TREE", "."),
        ("GIT_OBJECT_DIRECTORY", ".git/objects"),
        ("GIT_COMMON_DIR", ".git"),
        ("GIT_PREFIX", ""),
    ];

    for (key, value) in cases {
        let output = audit_with_env(root, &[(key, value)]);
        assert_eq!(
            output.code, 0,
            "audit must exit 0 with {key}={value:?} set; stderr: {}",
            output.stderr
        );
        let json = parse_json(&output);
        assert!(
            json["verdict"].is_string(),
            "audit JSON should still include a verdict with {key}={value:?} set"
        );
    }
}

#[test]
fn audit_coverage_and_coverage_root_feed_crap_scoring() {
    let dir = create_audit_fixture("coverage-root");
    write_branchy_change(dir.path());

    let without_coverage = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        without_coverage.code, 1,
        "static CRAP estimate should fail before Istanbul coverage is supplied. stderr: {}",
        without_coverage.stderr
    );

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");

    let with_coverage = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--coverage",
        coverage_path.to_str().unwrap(),
        "--coverage-root",
        "/ci/workspace",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        with_coverage.code, 0,
        "Istanbul coverage should lower CRAP below the audit threshold. stderr: {}",
        with_coverage.stderr
    );
    let json = parse_json(&with_coverage);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

#[test]
fn audit_rejects_relative_coverage_root() {
    let dir = create_audit_fixture("coverage-root-relative-rejected");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--coverage-root",
        "src",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "relative --coverage-root should be rejected before audit runs. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

#[test]
fn audit_coverage_relative_path_resolves_against_root_through_base_snapshot() {
    let dir = create_audit_fixture("coverage-relative");
    write_branchy_change(dir.path());

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    let branchy_source = dir.path().join("src/index.ts");
    write_branchy_istanbul_coverage(&coverage_path, &branchy_source.to_string_lossy());

    let with_relative = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--coverage",
        "artifacts/coverage-final.json",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        with_relative.code, 0,
        "relative --coverage must resolve against --root through both the HEAD pass and the base-snapshot recursion. stderr: {}",
        with_relative.stderr
    );
    let json = parse_json(&with_relative);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

/// Write an Istanbul map recording each `(name, line)` function (declared at
/// its 1-based `line` in `coverage_source_path`) with the given fn hit count
/// and no statements: `hits == 0` means measured 0% coverage, anything higher
/// means 100%.
fn write_fns_coverage(
    coverage_path: &std::path::Path,
    coverage_source_path: &str,
    fns: &[(&str, u32)],
    hits: u64,
) {
    fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
    let mut fn_map = serde_json::Map::new();
    let mut fn_hits = serde_json::Map::new();
    for (index, (name, line)) in fns.iter().enumerate() {
        fn_map.insert(
            index.to_string(),
            serde_json::json!({
                "name": name,
                "line": line,
                "decl": {
                    "start": { "line": line, "column": 16 },
                    "end": { "line": line, "column": 23 }
                },
                "loc": {
                    "start": { "line": line, "column": 44 },
                    "end": { "line": line + 8, "column": 1 }
                }
            }),
        );
        fn_hits.insert(index.to_string(), serde_json::json!(hits));
    }
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        coverage_source_path.to_string(),
        serde_json::json!({
            "path": coverage_source_path,
            "statementMap": {},
            "fnMap": fn_map,
            "branchMap": {},
            "s": {},
            "f": fn_hits,
            "b": {}
        }),
    );
    fs::write(coverage_path, serde_json::to_string(&coverage).unwrap()).unwrap();
}

/// Write an Istanbul map recording `branchy` (declared at 1-based `line` in
/// `coverage_source_path`) as never executed, i.e. measured 0% coverage.
fn write_uncovered_branchy_coverage(
    coverage_path: &std::path::Path,
    coverage_source_path: &str,
    line: u32,
) {
    write_fns_coverage(coverage_path, coverage_source_path, &[("branchy", line)], 0);
}

/// The uncovered high-CRAP `branchy` function plus an external test reference,
/// committed as the audit base state. Returns the path to `src/branchy.ts`.
/// The vitest devDependency makes `src/branchy.test.ts` a test entry; without
/// a test-runner plugin the graph estimate is 0% on both sides and the
/// base/head divergence guarded by the #2347 tests never occurs.
fn commit_branchy_with_test_reference(dir: &std::path::Path) -> std::path::PathBuf {
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "devDependencies": {"vitest": "^3.0.0"}}"#,
    )
    .unwrap();
    let branchy_path = dir.join("src/branchy.ts");
    fs::write(
        &branchy_path,
        "export function branchy(n: number): number {\n\
         \x20 if (n < 0) return -1;\n\
         \x20 if (n === 0) return 0;\n\
         \x20 if (n < 10) return 1;\n\
         \x20 if (n < 100) return 2;\n\
         \x20 if (n < 1000) return 3;\n\
         \x20 if (n < 10000) return 4;\n\
         \x20 return 5;\n\
         }\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/branchy.test.ts"),
        "import { branchy } from './branchy';\nbranchy(1);\n",
    )
    .unwrap();
    commit_all(dir, "add branchy with an external test reference");
    branchy_path
}

/// Run `fallow audit --base HEAD~1 --max-crap 10` on `root`, optionally with
/// `--coverage <path>`.
fn run_branchy_audit(
    root: &std::path::Path,
    coverage_path: Option<&std::path::Path>,
) -> common::CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
    ];
    if let Some(coverage_path) = coverage_path {
        args.push("--coverage");
        args.push(coverage_path.to_str().unwrap());
    }
    args.extend(["--format", "json", "--quiet"]);
    run_fallow_raw(&args)
}

/// Assert an estimate-only run passes with `branchy` scored below the CRAP
/// threshold. The coverage variants rely on the estimate and the Istanbul map
/// disagreeing; if fixture drift stopped `branchy.test.ts` from being a test
/// entry, the estimate would hit 0% on both sides, the finding would be
/// inherited either way, and the coverage assertions would pass without
/// exercising the base/head divergence.
fn assert_branchy_below_threshold(output: &common::CommandOutput) {
    assert_eq!(
        output.code, 0,
        "the estimate-only control run must pass. stderr: {}",
        output.stderr
    );
    let json = parse_json(output);
    let has_branchy = json["complexity"]["findings"]
        .as_array()
        .is_some_and(|findings| {
            findings
                .iter()
                .any(|f| f["name"].as_str() == Some("branchy"))
        });
    assert!(
        !has_branchy,
        "the graph estimate must score branchy below the CRAP threshold, or this test no longer exercises the base/head divergence. stdout: {}",
        output.stdout
    );
}

/// Assert the audit passed with the `branchy` CRAP finding scored from
/// Istanbul data and attributed `introduced: false`.
fn assert_branchy_inherited(output: &common::CommandOutput) {
    let json = parse_json(output);
    let findings = json["complexity"]["findings"]
        .as_array()
        .expect("audit JSON should include complexity findings");
    let branchy = findings
        .iter()
        .find(|f| f["name"].as_str() == Some("branchy"))
        .expect("branchy should be reported above the CRAP threshold with 0% measured coverage");
    assert_eq!(
        branchy["introduced"].as_bool(),
        Some(false),
        "branchy is byte-identical on both sides; the base snapshot must score it with the same Istanbul data as HEAD. stderr: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "gate new-only must not fail on the inherited finding. stderr: {}",
        output.stderr
    );
}

/// #2347: a pre-existing high-CRAP function must stay `introduced: false` when
/// `--coverage` is supplied and an unrelated edit touches its file. The base
/// snapshot runs in a temporary worktree, so the Istanbul paths (which point
/// at the HEAD checkout) must be rebased onto that worktree; otherwise the
/// base side falls back to the reachability estimate, scores below threshold,
/// and the unchanged head finding fails `--gate new-only`.
#[test]
fn audit_coverage_keeps_unchanged_function_inherited_through_base_snapshot() {
    let dir = create_audit_fixture("coverage-base-attribution");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let mut source = fs::read_to_string(&branchy_path).unwrap();
    source.push_str("branchy(-1);\n");
    fs::write(&branchy_path, source).unwrap();
    commit_all(dir.path(), "append an unrelated statement below branchy");

    assert_branchy_below_threshold(&run_branchy_audit(dir.path(), None));

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_uncovered_branchy_coverage(&coverage_path, &branchy_path.to_string_lossy(), 1);

    let with_coverage = run_branchy_audit(dir.path(), Some(&coverage_path));
    assert_branchy_inherited(&with_coverage);
}

/// #2347, line-shift variant: the unrelated edit prepends lines ABOVE the
/// function, so the base worktree sees it far from where the HEAD-generated
/// Istanbul map recorded it. The base pass must still match the function
/// (relocated coverage tolerates line drift for unambiguous names) instead of
/// degrading to the reachability estimate and flipping it to introduced.
#[test]
fn audit_coverage_keeps_line_shifted_function_inherited_through_base_snapshot() {
    let dir = create_audit_fixture("coverage-base-line-shift");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let source = fs::read_to_string(&branchy_path).unwrap();
    let padding = "// padding\n".repeat(19);
    fs::write(&branchy_path, format!("{padding}{source}")).unwrap();
    commit_all(dir.path(), "prepend unrelated lines above branchy");

    assert_branchy_below_threshold(&run_branchy_audit(dir.path(), None));

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_uncovered_branchy_coverage(&coverage_path, &branchy_path.to_string_lossy(), 20);

    let with_coverage = run_branchy_audit(dir.path(), Some(&coverage_path));
    assert_branchy_inherited(&with_coverage);
}

/// Failure direction for #2347: rebasing the Istanbul map onto the base
/// worktree must not disable the gate. A genuinely new high-CRAP function in
/// the same 0%-covered file stays `introduced: true` with exit 1, while the
/// pre-existing one stays inherited, both scored from Istanbul data.
#[test]
fn audit_coverage_still_gates_new_high_crap_function() {
    let dir = create_audit_fixture("coverage-gate-new-fn");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let mut source = fs::read_to_string(&branchy_path).unwrap();
    source.push_str(
        "export function freshlyBranchy(n: number): number {\n\
         \x20 if (n < 0) return -1;\n\
         \x20 if (n === 0) return 0;\n\
         \x20 if (n < 10) return 1;\n\
         \x20 if (n < 100) return 2;\n\
         \x20 if (n < 1000) return 3;\n\
         \x20 if (n < 10000) return 4;\n\
         \x20 return 5;\n\
         }\n",
    );
    fs::write(&branchy_path, source).unwrap();
    fs::write(
        dir.path().join("src/branchy.test.ts"),
        "import { branchy, freshlyBranchy } from './branchy';\nbranchy(1);\nfreshlyBranchy(1);\n",
    )
    .unwrap();
    commit_all(dir.path(), "add a new high-complexity function");

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_fns_coverage(
        &coverage_path,
        &branchy_path.to_string_lossy(),
        &[("branchy", 1), ("freshlyBranchy", 10)],
        0,
    );

    let output = run_branchy_audit(dir.path(), Some(&coverage_path));
    assert_eq!(
        output.code, 1,
        "new high-CRAP debt must still fail gate new-only when coverage is supplied. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let findings = json["complexity"]["findings"]
        .as_array()
        .expect("audit JSON should include complexity findings");
    let fresh = findings
        .iter()
        .find(|f| f["name"].as_str() == Some("freshlyBranchy"))
        .expect("the new function should be reported above the CRAP threshold");
    assert_eq!(fresh["introduced"].as_bool(), Some(true));
    assert_eq!(fresh["coverage_source"].as_str(), Some("istanbul"));
    let branchy = findings
        .iter()
        .find(|f| f["name"].as_str() == Some("branchy"))
        .expect("the pre-existing function should still be reported");
    assert_eq!(branchy["introduced"].as_bool(), Some(false));
    assert!(
        json["attribution"]["complexity_introduced"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "attribution should count the new function as introduced: {}",
        json["attribution"]
    );
}

/// #2347 for the zero-flag flow (`vitest --coverage && fallow audit`): the
/// head pass auto-detects `coverage/coverage-final.json`, and the base pass
/// must consume the same map instead of auto-detecting against the base
/// worktree (which never materializes coverage output). The first run also
/// caches a base snapshot without any coverage, so the second run pins that
/// auto-detected coverage participates in the base-snapshot cache key.
#[test]
fn audit_auto_detected_coverage_keeps_unchanged_function_inherited() {
    let dir = create_audit_fixture("coverage-auto-detect-base");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let mut source = fs::read_to_string(&branchy_path).unwrap();
    source.push_str("branchy(-1);\n");
    fs::write(&branchy_path, source).unwrap();
    commit_all(dir.path(), "append an unrelated statement below branchy");

    assert_branchy_below_threshold(&run_branchy_audit(dir.path(), None));

    let coverage_path = dir.path().join("coverage/coverage-final.json");
    write_fns_coverage(
        &coverage_path,
        &branchy_path.to_string_lossy(),
        &[("branchy", 1)],
        1,
    );
    let covered = run_branchy_audit(dir.path(), None);
    assert_eq!(
        covered.code, 0,
        "a fully covered map scores branchy below the CRAP threshold on both sides. stderr: {}",
        covered.stderr
    );

    // Rewriting the map in place (a vitest re-run after a test was removed)
    // must invalidate the cached base snapshot: the auto-detected file's
    // content participates in the base-snapshot cache key exactly like an
    // explicit `--coverage` file.
    write_uncovered_branchy_coverage(&coverage_path, &branchy_path.to_string_lossy(), 1);
    let auto_detected = run_branchy_audit(dir.path(), None);
    assert_branchy_inherited(&auto_detected);
}

#[test]
fn audit_coverage_env_fallback_feeds_crap_scoring() {
    let dir = create_audit_fixture("coverage-env");
    write_branchy_change(dir.path());

    let coverage_path = dir.path().join("artifacts/env-coverage.json");
    let branchy_source = dir.path().join("src/index.ts");
    write_branchy_istanbul_coverage(&coverage_path, &branchy_source.to_string_lossy());

    let without_env = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        without_env.code, 1,
        "static CRAP estimate should fail before FALLOW_COVERAGE is supplied. stderr: {}",
        without_env.stderr
    );

    let output = run_fallow_raw_with_env(
        &[
            "audit",
            "--root",
            dir.path().to_str().unwrap(),
            "--base",
            "HEAD~1",
            "--max-crap",
            "10",
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_COVERAGE", coverage_path.as_path())],
    );
    assert_eq!(
        output.code, 0,
        "FALLOW_COVERAGE should feed audit's health sub-analysis. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
}

/// Write `.fallowrc.json` with the given `health` section body.
fn write_health_config(dir: &std::path::Path, health: &str) {
    fs::write(
        dir.join(".fallowrc.json"),
        format!(r#"{{"health":{health}}}"#),
    )
    .unwrap();
}

/// Run `fallow audit --base HEAD~1 --max-crap 10 --format json --quiet` on
/// `root` with extra arguments and path-typed env vars on the child process.
fn run_gated_audit_with(
    root: &std::path::Path,
    extra_args: &[&str],
    env: &[(&str, &std::path::Path)],
) -> common::CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--max-crap",
        "10",
        "--format",
        "json",
        "--quiet",
    ];
    args.extend_from_slice(extra_args);
    run_fallow_raw_with_env(&args, env)
}

/// #2359: `health.coverage` / `health.coverageRoot` from config must feed the
/// audit health sub-pass exactly like `--coverage` / `--coverage-root`. Audit
/// previously never consulted the config keys and scored CRAP from the
/// reachability estimate while `fallow health` scored from real coverage.
#[test]
fn audit_config_coverage_fallback_matches_cli_coverage_attribution() {
    let dir = create_audit_fixture("coverage-config");
    write_branchy_change(dir.path());
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");

    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/coverage-final.json","coverageRoot":"/ci/workspace"}"#,
    );
    let from_config = run_gated_audit_with(dir.path(), &[], &[]);
    assert_eq!(
        from_config.code, 0,
        "health.coverage should feed audit's health sub-analysis. stderr: {}",
        from_config.stderr
    );
    let config_json = parse_json(&from_config);
    assert_eq!(config_json["verdict"].as_str(), Some("pass"));

    write_health_config(dir.path(), "{}");
    let from_flags = run_gated_audit_with(
        dir.path(),
        &[
            "--coverage",
            coverage_path.to_str().unwrap(),
            "--coverage-root",
            "/ci/workspace",
        ],
        &[],
    );
    assert_eq!(
        from_flags.code, 0,
        "the explicit flags are the reference run. stderr: {}",
        from_flags.stderr
    );
    let flags_json = parse_json(&from_flags);
    assert_eq!(
        config_json["complexity"]["findings"], flags_json["complexity"]["findings"],
        "config-sourced coverage must score the same findings as the flags"
    );
    assert_eq!(
        config_json["attribution"], flags_json["attribution"],
        "config-sourced coverage must attribute findings like the flags"
    );
}

/// #2359 precedence: `--coverage` / `--coverage-root` win over the config
/// keys, and each input resolves independently, so a CLI coverage path pairs
/// with `health.coverageRoot` and a CLI root with `health.coverage`.
#[test]
fn audit_cli_coverage_flags_override_config_coverage() {
    let dir = create_audit_fixture("coverage-config-cli-precedence");
    write_branchy_change(dir.path());
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");
    let stale_path = dir.path().join("artifacts/stale-coverage.json");
    write_branchy_istanbul_coverage(&stale_path, "/ci/workspace/src/other.ts");

    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/stale-coverage.json","coverageRoot":"/ci/workspace"}"#,
    );
    let stale_config = run_gated_audit_with(dir.path(), &[], &[]);
    assert_eq!(
        stale_config.code, 1,
        "a config map without an entry for the function falls back to the estimate. stderr: {}",
        stale_config.stderr
    );
    let cli_coverage = run_gated_audit_with(
        dir.path(),
        &["--coverage", coverage_path.to_str().unwrap()],
        &[],
    );
    assert_eq!(
        cli_coverage.code, 0,
        "--coverage must beat health.coverage while health.coverageRoot still applies. stderr: {}",
        cli_coverage.stderr
    );
    assert_eq!(parse_json(&cli_coverage)["verdict"].as_str(), Some("pass"));

    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/coverage-final.json","coverageRoot":"/wrong/root"}"#,
    );
    let wrong_config_root = run_gated_audit_with(dir.path(), &[], &[]);
    assert_eq!(
        wrong_config_root.code, 1,
        "a wrong config coverage root strips nothing and matches nothing. stderr: {}",
        wrong_config_root.stderr
    );
    let cli_root = run_gated_audit_with(dir.path(), &["--coverage-root", "/ci/workspace"], &[]);
    assert_eq!(
        cli_root.code, 0,
        "--coverage-root must beat health.coverageRoot while health.coverage still applies. stderr: {}",
        cli_root.stderr
    );
    assert_eq!(parse_json(&cli_root)["verdict"].as_str(), Some("pass"));
}

/// #2359 precedence: `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT` win over the
/// config keys, the same order `fallow health` uses. Audit previously read
/// only `FALLOW_COVERAGE`, so the root half also pins that the env root is
/// honored at all.
#[test]
fn audit_env_coverage_overrides_config_coverage() {
    let dir = create_audit_fixture("coverage-config-env-precedence");
    write_branchy_change(dir.path());
    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_branchy_istanbul_coverage(&coverage_path, "/ci/workspace/src/index.ts");
    let stale_path = dir.path().join("artifacts/stale-coverage.json");
    write_branchy_istanbul_coverage(&stale_path, "/ci/workspace/src/other.ts");

    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/stale-coverage.json","coverageRoot":"/ci/workspace"}"#,
    );
    let env_coverage = run_gated_audit_with(
        dir.path(),
        &[],
        &[("FALLOW_COVERAGE", coverage_path.as_path())],
    );
    assert_eq!(
        env_coverage.code, 0,
        "FALLOW_COVERAGE must beat health.coverage while health.coverageRoot still applies. stderr: {}",
        env_coverage.stderr
    );
    assert_eq!(parse_json(&env_coverage)["verdict"].as_str(), Some("pass"));

    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/coverage-final.json","coverageRoot":"/wrong/root"}"#,
    );
    let env_root = run_gated_audit_with(
        dir.path(),
        &[],
        &[("FALLOW_COVERAGE_ROOT", Path::new("/ci/workspace"))],
    );
    assert_eq!(
        env_root.code, 0,
        "FALLOW_COVERAGE_ROOT must beat health.coverageRoot while health.coverage still applies. stderr: {}",
        env_root.stderr
    );
    assert_eq!(parse_json(&env_root)["verdict"].as_str(), Some("pass"));
}

/// #2359 through the #2347 base pass: a config-sourced map is rebased onto
/// the base worktree exactly like `--coverage`, so a pre-existing high-CRAP
/// function stays inherited when an unrelated edit touches its file.
#[test]
fn audit_config_coverage_keeps_unchanged_function_inherited_through_base_snapshot() {
    let dir = create_audit_fixture("coverage-config-base-attribution");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let mut source = fs::read_to_string(&branchy_path).unwrap();
    source.push_str("branchy(-1);\n");
    fs::write(&branchy_path, source).unwrap();
    commit_all(dir.path(), "append an unrelated statement below branchy");

    assert_branchy_below_threshold(&run_branchy_audit(dir.path(), None));

    let coverage_path = dir.path().join("artifacts/coverage-final.json");
    write_uncovered_branchy_coverage(&coverage_path, &branchy_path.to_string_lossy(), 1);
    write_health_config(
        dir.path(),
        r#"{"coverage":"artifacts/coverage-final.json"}"#,
    );

    assert_branchy_inherited(&run_branchy_audit(dir.path(), None));
}

/// #2359 cache key: pointing `health.coverage` at a different map must not
/// reuse the base snapshot computed from the previous map. The first run
/// caches a base pass scored from the covered map; the second must rescore
/// the base side from the uncovered one to keep the finding inherited.
#[test]
fn audit_config_coverage_change_invalidates_cached_base_snapshot() {
    let dir = create_audit_fixture("coverage-config-cache-key");
    let branchy_path = commit_branchy_with_test_reference(dir.path());

    let mut source = fs::read_to_string(&branchy_path).unwrap();
    source.push_str("branchy(-1);\n");
    fs::write(&branchy_path, source).unwrap();
    commit_all(dir.path(), "append an unrelated statement below branchy");

    assert_branchy_below_threshold(&run_branchy_audit(dir.path(), None));

    let covered_path = dir.path().join("artifacts/covered.json");
    write_fns_coverage(
        &covered_path,
        &branchy_path.to_string_lossy(),
        &[("branchy", 1)],
        1,
    );
    write_health_config(dir.path(), r#"{"coverage":"artifacts/covered.json"}"#);
    let covered = run_branchy_audit(dir.path(), None);
    assert_eq!(
        covered.code, 0,
        "a fully covered config map scores branchy below the CRAP threshold on both sides. stderr: {}",
        covered.stderr
    );

    let uncovered_path = dir.path().join("artifacts/uncovered.json");
    write_uncovered_branchy_coverage(&uncovered_path, &branchy_path.to_string_lossy(), 1);
    write_health_config(dir.path(), r#"{"coverage":"artifacts/uncovered.json"}"#);
    assert_branchy_inherited(&run_branchy_audit(dir.path(), None));
}

/// #2359: a `health.coverage` path that does not exist fails audit with the
/// same structured exit 2 as `fallow health` and an explicit `--coverage`.
#[test]
fn audit_missing_config_coverage_is_structured_exit_two() {
    let dir = create_audit_fixture("coverage-config-missing");
    write_branchy_change(dir.path());
    write_health_config(dir.path(), r#"{"coverage":"artifacts/missing.json"}"#);

    let output = run_gated_audit_with(dir.path(), &[], &[]);
    assert_eq!(
        output.code, 2,
        "a missing health.coverage file must fail loudly like --coverage. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.starts_with("coverage:") && message.contains("missing.json"),
        "unexpected error message: {message}"
    );
}

/// #2359: a relative `health.coverageRoot` is rejected before analysis with
/// the same structured exit 2 as a relative `--coverage-root`.
#[test]
fn audit_rejects_relative_config_coverage_root() {
    let dir = create_audit_fixture("coverage-config-root-relative-rejected");
    write_health_config(dir.path(), r#"{"coverageRoot":"src"}"#);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.path().to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 2,
        "relative health.coverageRoot should be rejected before audit runs. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

/// Run `fallow audit` against `root` with string env vars set on the child
/// process. The path-typed `run_fallow_raw_with_env` cannot carry a git ref
/// value, so this builds the command directly.
fn run_audit_string_env(
    root: &std::path::Path,
    extra_args: &[&str],
    env: &[(&str, &str)],
) -> common::CommandOutput {
    let mut cmd = Command::new(fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    common::scrub_coverage_env(&mut cmd);
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.args(["audit", "--root"]);
    cmd.arg(root);
    cmd.args(["--format", "json", "--quiet"]);
    cmd.args(extra_args);
    let output = cmd.output().expect("failed to run fallow binary");
    common::CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// Add a second commit so `HEAD~1` resolves, then return the fixture.
fn audit_fixture_with_two_commits() -> TempDir {
    let tmp = create_audit_fixture("env-base");
    fs::write(
        tmp.path().join("src/utils.ts"),
        "export const used = () => 43;\nexport const unused = () => 0;\n",
    )
    .unwrap();
    commit_all(tmp.path(), "second commit");
    tmp
}

#[test]
fn audit_honors_fallow_audit_base_env_when_no_flag() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(dir.path(), &[], &[("FALLOW_AUDIT_BASE", "HEAD~1")]);

    assert_eq!(
        output.code, 0,
        "audit with FALLOW_AUDIT_BASE should run. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["base_ref"].as_str(),
        Some("HEAD~1"),
        "FALLOW_AUDIT_BASE should set the base ref"
    );
    assert_eq!(
        json["base_description"].as_str(),
        Some("FALLOW_AUDIT_BASE=HEAD~1"),
        "env-set base should carry its provenance"
    );
}

#[test]
fn audit_base_flag_wins_over_fallow_audit_base_env() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(
        dir.path(),
        &["--base", "HEAD"],
        &[("FALLOW_AUDIT_BASE", "HEAD~1")],
    );

    assert_eq!(
        output.code, 0,
        "explicit --base HEAD has no changes, should pass. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["base_ref"].as_str(),
        Some("HEAD"),
        "the --base flag must win over FALLOW_AUDIT_BASE"
    );
    assert!(
        json.get("base_description").is_none() || json["base_description"].is_null(),
        "an explicit --base carries no provenance description"
    );
}

#[test]
fn audit_rejects_malformed_fallow_audit_base_env() {
    let dir = audit_fixture_with_two_commits();
    let output = run_audit_string_env(dir.path(), &[], &[("FALLOW_AUDIT_BASE", "bad;ref")]);

    assert_eq!(
        output.code, 2,
        "a malformed FALLOW_AUDIT_BASE must exit 2, not be silently ignored. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"].as_bool(), Some(true));
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("FALLOW_AUDIT_BASE")),
        "the error should name the offending env var, got: {}",
        json["message"]
    );
}

// Base-reuse predicate characterization tests
//
// These tests pin the behavior of `can_reuse_current_as_base` end-to-end
// through the `fallow audit --gate new-only` path. Each test establishes a
// committed base and a committed head, then asserts on the JSON attribution
// fields `dead_code_introduced` and `dead_code_inherited` to confirm whether
// the reuse predicate correctly skipped the base-snapshot rebuild.
//
// They serve as the safety net for refactors of the underlying helpers
// (for example, batching the per-file `git show` calls).

/// A whitespace-only reformat of a TS file must be treated as equivalent by
/// the tokenizer and allow the base snapshot to be reused. The audit should
/// report zero introduced dead-code findings.
#[test]
fn audit_whitespace_only_change_reports_no_introduced_findings() {
    let dir = create_audit_fixture("reuse-whitespace");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Reformat src/utils.ts with whitespace only (no semantic change).
    fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\n\n\nexport const unused = () => 0;\n",
    )
    .unwrap();
    commit_all(root, "reformat utils");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash on whitespace-only change. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "whitespace-only change must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    // The pre-existing `unused` export should be inherited, not introduced.
    assert!(
        json["attribution"]["dead_code_inherited"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "pre-existing unused export must appear as inherited"
    );
}

/// Adding a genuinely new unused export must be classified as introduced.
#[test]
fn audit_semantic_change_reports_introduced_finding() {
    let dir = create_audit_fixture("reuse-semantic");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Add a new unused export.
    fs::write(
        root.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unused = () => 0;\nexport const extra = 1;\n",
    )
    .unwrap();
    commit_all(root, "add extra unused export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["attribution"]["dead_code_introduced"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "new unused export must be attributed as introduced. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Changing only a Markdown README must not introduce dead-code findings.
/// `is_non_behavioral_doc` classifies `.md` as non-behavioral, so the reuse
/// predicate returns true for a doc-only diff.
#[test]
fn audit_doc_only_change_reports_no_introduced_findings() {
    let dir = create_audit_fixture("reuse-doc");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    fs::write(root.join("README.md"), "# My project\nUpdated docs.\n").unwrap();
    commit_all(root, "update readme");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should not crash on doc-only change. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "doc-only change must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Adding a brand-new TS file with an unused export forces a real base-snapshot
/// computation (the file does not exist in base, so `BaseFileReader::read`
/// returns None and the reuse predicate returns false). The new export should be
/// attributed as introduced.
#[test]
fn audit_new_file_is_treated_as_behavioral() {
    let dir = create_audit_fixture("reuse-newfile");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Add a new file with an unused export; it has no counterpart in base.
    fs::write(
        root.join("src/new.ts"),
        "export const brandNew = 'nobody uses me';\n",
    )
    .unwrap();
    commit_all(root, "add new file with unused export");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0 || output.code == 1,
        "audit should complete successfully even when a new file forces a base-snapshot rebuild. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["attribution"]["dead_code_introduced"]
            .as_u64()
            .is_some_and(|n| n >= 1),
        "new unused export in a new file must be attributed as introduced. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Whitespace-only edits across many `.ts` files in one commit exercise the
/// batched base-file reader: the reuse predicate reads the base version of each
/// changed file sequentially through one `git cat-file --batch` process (the
/// previous implementation spawned one `git show` per file). Each file is
/// token-equivalent to its base, so the reuse check should hold and the audit
/// should introduce zero findings. This pins correctness of multiple
/// sequential reads through a single batch process (trailing-newline
/// consumption, lockstep request/response).
#[test]
fn audit_reuse_check_handles_many_equivalent_files() {
    let dir = create_audit_fixture("reuse-many");
    let root = dir.path();

    // Seed 12 source files that are imported in a chain so each is reachable
    // (no pre-existing unused-file findings), then commit them as the base.
    const FILE_COUNT: usize = 12;
    for i in 0..FILE_COUNT {
        fs::write(
            root.join(format!("src/mod{i}.ts")),
            format!("export const value{i} = {i};\nexport const helper{i} = () => value{i};\n"),
        )
        .unwrap();
    }
    // Wire every module into the import graph via index.ts so none is orphaned.
    use std::fmt::Write as _;
    let mut index = String::from("import { used } from './utils';\nused();\n");
    for i in 0..FILE_COUNT {
        writeln!(index, "import {{ helper{i} }} from './mod{i}';").unwrap();
        writeln!(index, "helper{i}();").unwrap();
    }
    fs::write(root.join("src/index.ts"), &index).unwrap();
    commit_all(root, "seed many modules");

    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Apply whitespace-only edits to every module in one commit. Each file
    // stays token-equivalent to its base, so the reuse predicate must accept
    // all of them across one batch process.
    for i in 0..FILE_COUNT {
        fs::write(
            root.join(format!("src/mod{i}.ts")),
            format!(
                "export const value{i}  =  {i};\n\nexport const helper{i} = ()   => value{i};\n"
            ),
        )
        .unwrap();
    }
    commit_all(root, "whitespace-only edits across modules");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        output.code == 0,
        "audit over many whitespace-only edits should succeed with no introduced findings. code: {}, stderr: {}",
        output.code,
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["attribution"]["dead_code_introduced"].as_u64(),
        Some(0),
        "whitespace-only edits across many files must introduce zero dead-code findings. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
}

/// Changing only `package.json` is neither an analysis-input file nor a
/// non-behavioral doc (`.json` passes neither check), so the reuse predicate
/// treats it as behavioral. The audit must complete and produce a coherent
/// verdict; this test checks exit success rather than a specific attribution
/// count because the JSON change may or may not affect dead-code counts.
#[test]
fn audit_json_only_change_is_behavioral() {
    let dir = create_audit_fixture("reuse-json");
    let root = dir.path();
    let base_sha = {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git rev-parse should succeed");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Remove the unused dependency from package.json: a plausible behavioral change.
    fs::write(
        root.join("package.json"),
        r#"{"name": "audit-test", "main": "src/index.ts", "dependencies": {}}"#,
    )
    .unwrap();
    commit_all(root, "remove unused dep from package.json");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        root.to_str().unwrap(),
        "--base",
        &base_sha,
        "--format",
        "json",
        "--quiet",
    ]);

    // The audit must complete and produce a parseable verdict; it may pass or
    // fail depending on analysis results, but must not crash (exit 2+).
    assert!(
        output.code == 0 || output.code == 1,
        "audit on a package.json-only change must complete without crashing. stderr: {}\nstdout: {}",
        output.stderr,
        output.stdout
    );
    let json = parse_json(&output);
    assert!(
        json.get("verdict").is_some(),
        "audit must produce a verdict field in JSON output. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    // The attribution block must be present.
    assert!(
        json.get("attribution").is_some(),
        "audit must produce an attribution block even when package.json is the only change"
    );
}

/// A Next.js project whose base barrel re-exports only a `"use client"`
/// component. The feature branch adds a server-only re-export to that barrel,
/// turning it into a mixed client/server barrel: the finding is NEW relative to
/// the base, so audit must annotate it `introduced: true`.
fn create_mixed_barrel_audit_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app/components")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-mixed-barrel","dependencies":{"next":"^14.0.0","react":"^18.0.0","server-only":"^0.0.1"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler","jsx":"preserve"},"include":["app"]}"#,
    )
    .unwrap();
    fs::write(
        dir.join("app/components/Button.tsx"),
        "\"use client\";\nexport function Button() {\n  return null;\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("app/components/fetchUser.ts"),
        "import \"server-only\";\nexport function fetchUser() {\n  return { id: 1 };\n}\n",
    )
    .unwrap();
    // Base barrel: client-only re-export, NOT a mixed barrel yet.
    fs::write(
        dir.join("app/components/index.ts"),
        "export { Button } from \"./Button\";\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
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
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    // Feature branch: add the server-only re-export, creating the mix.
    fs::write(
        dir.join("app/components/index.ts"),
        "export { Button } from \"./Button\";\nexport { fetchUser } from \"./fetchUser\";\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "add server-only re-export to barrel",
    ]);

    tmp
}

#[test]
fn audit_annotates_newly_added_mixed_barrel_as_introduced() {
    let tmp = create_mixed_barrel_audit_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    let json = parse_json(&output);
    let barrels = json["dead_code"]["mixed_client_server_barrels"]
        .as_array()
        .expect("dead_code.mixed_client_server_barrels should be an array");
    assert_eq!(
        barrels.len(),
        1,
        "exactly one mixed client/server barrel expected. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    assert_eq!(
        barrels[0]["introduced"], true,
        "the newly-mixed barrel must be annotated introduced: true"
    );
}

/// A Next.js project whose base file has a correctly-positioned leading
/// `"use client"` directive. The feature branch adds an import ABOVE the
/// directive, demoting it to an ordinary expression statement the RSC bundler
/// ignores: the finding is NEW relative to the base, so audit must annotate it
/// `introduced: true`.
fn create_misplaced_directive_audit_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("app")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"audit-misplaced-directive","dependencies":{"next":"^14.0.0","react":"^18.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2022","module":"ESNext","moduleResolution":"bundler","jsx":"preserve"},"include":["app"]}"#,
    )
    .unwrap();
    fs::write(dir.join("app/helper.ts"), "export const helper = 1;\n").unwrap();
    // Base: the directive is correctly positioned at the top of the file.
    fs::write(
        dir.join("app/page.tsx"),
        "\"use client\";\nimport { helper } from \"./helper\";\nexport default function Page() {\n  return helper;\n}\n",
    )
    .unwrap();

    let git = |args: &[&str]| {
        Command::new("git")
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
            .output()
            .expect("git command failed")
    };

    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
    git(&["checkout", "-b", "feature"]);

    // Feature branch: move an import above the directive, demoting it.
    fs::write(
        dir.join("app/page.tsx"),
        "import { helper } from \"./helper\";\n\"use client\";\nexport default function Page() {\n  return helper;\n}\n",
    )
    .unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-m",
        "move import above use client directive",
    ]);

    tmp
}

#[test]
fn audit_annotates_newly_added_misplaced_directive_as_introduced() {
    let tmp = create_misplaced_directive_audit_fixture();
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    let json = parse_json(&output);
    let directives = json["dead_code"]["misplaced_directives"]
        .as_array()
        .expect("dead_code.misplaced_directives should be an array");
    assert_eq!(
        directives.len(),
        1,
        "exactly one misplaced directive expected. full json: {}",
        serde_json::to_string_pretty(&json).unwrap_or_default()
    );
    assert_eq!(
        directives[0]["introduced"], true,
        "the newly-misplaced directive must be annotated introduced: true"
    );
}

// ----------------------------------------------------------------------------
// E5 agent-contract loop (walkthrough guide + walkthrough-file post-validation)
// ----------------------------------------------------------------------------

/// A fixture with two boundary zones (`ui`, `db`) where the diff introduces a
/// new cross-zone edge (ui -> db), so the decision surface emits exactly one
/// real, anchored coupling/boundary decision. The base has no such edge.
fn create_boundary_walkthrough_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src/ui")).unwrap();
    fs::create_dir_all(dir.join("src/db")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "wt-test", "main": "src/ui/page.ts"}"#,
    )
    .unwrap();
    // Boundary config: ui may import only itself (so importing db is a
    // disallowed cross-zone edge).
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{
  "entry": ["src/ui/page.ts"],
  "boundaries": {
    "zones": [
      { "name": "ui", "patterns": ["src/ui/**"] },
      { "name": "db", "patterns": ["src/db/**"] }
    ],
    "rules": [
      { "from": "ui", "allow": [] }
    ]
  }
}"#,
    )
    .unwrap();
    fs::write(dir.join("src/db/conn.ts"), "export const conn = () => 1;\n").unwrap();
    // Base page.ts does NOT import db.
    fs::write(
        dir.join("src/ui/page.ts"),
        "export const render = () => 'hi';\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // HEAD: page.ts now imports db -> a new cross-zone edge ui->db.
    fs::write(
        dir.join("src/ui/page.ts"),
        "import { conn } from '../db/conn';\nexport const render = () => conn();\n",
    )
    .unwrap();
    commit_all(dir, "ui imports db");

    tmp
}

fn run_walkthrough_guide(root: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough-guide",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "walkthrough-guide always exits 0. stderr: {}",
        output.stderr
    );
    parse_json(&output)
}

fn run_walkthrough_file(root: &Path, file: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough-file",
        file.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "walkthrough-file always exits 0. stderr: {}",
        output.stderr
    );
    parse_json(&output)
}

#[test]
fn e5_walkthrough_guide_pins_a_deterministic_snapshot_hash() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    assert_eq!(guide["kind"], "review-walkthrough-guide");
    assert_eq!(guide["command"], "review-walkthrough-guide");
    let hash = guide["graph_snapshot_hash"]
        .as_str()
        .expect("guide pins a graph_snapshot_hash");
    assert!(hash.starts_with("graph:"), "hash is namespaced: {hash}");
    // The digest is graph-derived; the injection note states PR prose is untrusted.
    assert!(
        guide["injection_note"]
            .as_str()
            .unwrap_or_default()
            .contains("untrusted"),
        "injection note documents untrusted PR prose"
    );
    // Re-run on the same tree: the hash is byte-stable (deterministic).
    let again = run_walkthrough_guide(tmp.path());
    assert_eq!(again["graph_snapshot_hash"], guide["graph_snapshot_hash"]);
}

/// A nonexistent `--walkthrough-file` path still refuses the whole payload
/// (nothing is accepted), but the real cause is named on stderr instead of
/// leaving the stale-snapshot refusal as the only, misleading signal.
#[test]
fn e5_unreadable_walkthrough_file_names_the_read_error_on_stderr() {
    let tmp = create_boundary_walkthrough_fixture();
    let missing = tmp.path().join("no_such_agent.json");
    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough-file",
        missing.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "walkthrough-file always exits 0. stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("cannot read walkthrough file"),
        "stderr names the read failure: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("no_such_agent.json"),
        "stderr names the failing path: {}",
        output.stderr
    );
    let validation = parse_json(&output);
    assert_eq!(
        validation["stale"], true,
        "an unreadable file never accepts a judgment"
    );
    assert_eq!(validation["accepted_count"], 0);
}

/// The guide emits per-hunk `change_anchors` from the committed diff, and a
/// judgment citing one (with NO signal_id) is ACCEPTED with `anchor_kind: change`,
/// the weaker region-level anchor. End-to-end through the real binary, so the
/// emitted anchor set equals the set validated on reentry.
#[test]
fn e5_change_anchor_judgment_accepts_anchor_kind_change() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();
    let anchors = guide["change_anchors"]
        .as_array()
        .expect("the guide carries change_anchors");
    assert!(
        !anchors.is_empty(),
        "the page.ts edit must emit at least one change anchor. guide: {}",
        serde_json::to_string_pretty(&guide).unwrap_or_default()
    );
    let anchor_id = anchors[0]["change_anchor"].as_str().unwrap().to_string();
    assert!(anchor_id.starts_with("chg:"), "namespaced: {anchor_id}");

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "change_anchor": anchor_id, "framing": "This region trades a direct import for a seam." }
        ]
    });
    let agent_path = tmp.path().join("agent_change.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["stale"], false, "matching hash is not stale");
    assert_eq!(
        validation["accepted_count"],
        1,
        "the change-anchored judgment accepts. validation: {}",
        serde_json::to_string_pretty(&validation).unwrap_or_default()
    );
    assert_eq!(validation["accepted"][0]["anchor_kind"], "change");
    assert_eq!(validation["accepted"][0]["signal_id"], "");
    assert_eq!(validation["accepted"][0]["deterministic"], false);

    // A fabricated change anchor is rejected (anti-hallucination for the region anchor).
    let bogus = serde_json::json!({
        "graph_snapshot_hash": guide["graph_snapshot_hash"],
        "judgments": [ { "change_anchor": "chg:deadbeefdeadbeef", "framing": "made up" } ]
    });
    fs::write(&agent_path, serde_json::to_string(&bogus).unwrap()).unwrap();
    let rejected = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(rejected["rejected_count"], 1, "fabricated region rejects");
    assert_eq!(rejected["rejected"][0]["reason"], "unknown-change-anchor");
}

/// Done-condition (a): a clean agent JSON citing only emitted signal_ids with
/// the correct snapshot hash is ACCEPTED with zero unanchored findings.
#[test]
fn e5_clean_agent_json_is_accepted_zero_unanchored() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();
    let emitted = guide["digest"]["decisions"]["emitted_signal_ids"]
        .as_array()
        .expect("digest carries the emitted signal_id allowlist");
    assert!(
        !emitted.is_empty(),
        "the boundary change must emit at least one anchored signal. guide: {}",
        serde_json::to_string_pretty(&guide).unwrap_or_default()
    );
    let real_id = emitted[0].as_str().unwrap().to_string();

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "signal_id": real_id, "framing": "Intended coupling.", "concern": "coupling" }
        ]
    });
    // The response artifact is command input, not part of the reviewed change,
    // even if its filename has an otherwise analyzable extension.
    let agent_path = tmp.path().join("agent.ts");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["kind"], "review-walkthrough-validation");
    assert_eq!(validation["stale"], false, "matching hash is not stale");
    assert_eq!(
        validation["accepted_count"], 1,
        "the anchored judgment accepts"
    );
    assert_eq!(validation["rejected_count"], 0, "no rejections");
    assert_eq!(
        validation["unanchored_count"], 0,
        "zero unanchored findings"
    );
    // The framing is fenced as non-deterministic.
    assert_eq!(validation["accepted"][0]["deterministic"], false);
}

/// Done-condition (b): an injected unanchored finding is REJECTED.
#[test]
fn e5_injected_unanchored_signal_is_rejected() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();

    let agent = serde_json::json!({
        "graph_snapshot_hash": hash,
        "judgments": [
            { "signal_id": "sig:deadbeefdeadbeef", "framing": "hallucinated, no graph anchor" }
        ]
    });
    let agent_path = tmp.path().join("agent.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(validation["stale"], false);
    assert_eq!(
        validation["accepted_count"], 0,
        "the fabricated id never accepts"
    );
    assert_eq!(validation["rejected_count"], 1, "it is rejected");
    assert_eq!(validation["rejected"][0]["reason"], "unanchored-signal-id");
}

/// Done-condition (c): stale JSON (old snapshot hash, e.g. the tree moved) is
/// REFUSED.
#[test]
fn e5_stale_snapshot_hash_is_refused() {
    let tmp = create_boundary_walkthrough_fixture();
    let guide = run_walkthrough_guide(tmp.path());
    let emitted = guide["digest"]["decisions"]["emitted_signal_ids"]
        .as_array()
        .unwrap();
    let real_id = emitted[0].as_str().unwrap().to_string();

    // The agent echoes a STALE hash (the tree moved since the guide was emitted),
    // even though it cites a real signal id.
    let agent = serde_json::json!({
        "graph_snapshot_hash": "graph:0000000000000000",
        "judgments": [
            { "signal_id": real_id, "framing": "would be valid, but the snapshot moved" }
        ]
    });
    let agent_path = tmp.path().join("agent.json");
    fs::write(&agent_path, serde_json::to_string(&agent).unwrap()).unwrap();

    let validation = run_walkthrough_file(tmp.path(), &agent_path);
    assert_eq!(
        validation["stale"], true,
        "the old hash is refused as stale"
    );
    assert_eq!(
        validation["accepted_count"], 0,
        "nothing accepts when stale"
    );
    assert_eq!(validation["rejected"][0]["reason"], "stale-snapshot");
}

// ---------------------------------------------------------------------------
// W2 (#347): `fallow review --walkthrough` human/markdown renderer.
// ---------------------------------------------------------------------------

/// Run `fallow review --walkthrough` (default human) and return the raw output.
fn run_walkthrough_human(root: &Path) -> common::CommandOutput {
    run_fallow_raw(&[
        "review",
        "--root",
        root.to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
    ])
}

/// Strip the per-run `analysis_run_id` so two walkthrough-guide JSON envelopes
/// from separate processes can be compared (the id is random telemetry meta, not
/// part of the guide contract).
fn strip_run_id(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(telemetry) = value
        .get_mut("_meta")
        .and_then(|m| m.get_mut("telemetry"))
        .and_then(|t| t.as_object_mut())
    {
        telemetry.remove("analysis_run_id");
    }
    value
}

#[test]
fn w2_walkthrough_human_renders_stages_and_badges() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_walkthrough_human(tmp.path());
    assert_eq!(
        output.code, 0,
        "walkthrough always exits 0. stderr: {}",
        output.stderr
    );
    // The Review Focus header is orientation on stderr.
    assert!(
        output.stderr.contains("Review Focus"),
        "stderr carries the Review Focus header. stderr: {}",
        output.stderr
    );
    // The tour body is on stdout: at least one stage and the changed file row.
    assert!(
        output.stdout.contains("Stage"),
        "stdout carries a stage header. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("page.ts"),
        "the changed page.ts file is in the tour. stdout: {}",
        output.stdout
    );
    // The ui->db boundary decision synthesizes a COUPLING badge.
    assert!(
        output.stdout.contains("COUPLING"),
        "the boundary decision renders a COUPLING badge. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_json_is_byte_identical_to_guide() {
    let tmp = create_boundary_walkthrough_fixture();
    let walkthrough_json = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(walkthrough_json.code, 0, "exits 0");
    let guide_json = run_walkthrough_guide(tmp.path());

    let from_walkthrough = parse_json(&walkthrough_json);
    // Same agent-contract kind + deterministic snapshot hash.
    assert_eq!(from_walkthrough["kind"], "review-walkthrough-guide");
    assert_eq!(
        from_walkthrough["graph_snapshot_hash"],
        guide_json["graph_snapshot_hash"],
    );
    // The whole envelope matches once the random per-run id is stripped: the
    // json branch reuses the one guide JSON path, no second serializer.
    assert_eq!(
        strip_run_id(from_walkthrough),
        strip_run_id(guide_json),
        "--walkthrough --format json must reuse the guide JSON path verbatim",
    );
}

#[test]
fn w2_walkthrough_markdown_renders_plain_paste_artifact() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--format",
        "markdown",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    assert!(
        output.stdout.starts_with("## Fallow Review"),
        "markdown leads with the H2 header. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("### Stage"),
        "markdown has a stage section. stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("`COUPLING`"),
        "badges render as inline code spans. stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains('\u{1b}'),
        "markdown carries no ANSI escapes. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_exits_zero_even_when_verdict_fails() {
    let tmp = create_audit_baseline_fixture();
    fs::write(tmp.path().join("fallow.toml"), "[audit]\ngate = \"all\"\n").unwrap();
    let config = tmp.path().join("fallow.toml");

    // Sanity: the plain audit on this fixture really does fail.
    let audit = run_fallow_raw(&[
        "audit",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--config",
        config.to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(audit.code, 1, "the underlying audit verdict is fail");

    // All three walkthrough render modes stay exit 0 (advisory surface).
    for format in ["human", "markdown", "json"] {
        let output = run_fallow_raw(&[
            "review",
            "--root",
            tmp.path().to_str().unwrap(),
            "--base",
            "main",
            "--config",
            config.to_str().unwrap(),
            "--walkthrough",
            "--format",
            format,
            "--quiet",
        ]);
        assert_eq!(
            output.code, 0,
            "--walkthrough --format {format} must exit 0 on a fail verdict. stderr: {}",
            output.stderr
        );
    }
}

#[test]
fn w2_walkthrough_cleared_panel_collapses_then_expands() {
    let tmp = create_audit_baseline_fixture();
    // Default human render: the Cleared panel is a single collapsed line.
    let collapsed = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main",
        "--walkthrough",
    ]);
    assert_eq!(collapsed.code, 0, "exits 0. stderr: {}", collapsed.stderr);

    // The baseline fixture may or may not de-prioritize a unit; only assert the
    // collapse/expand contract when a Cleared panel is present.
    if collapsed.stdout.contains("Cleared (") {
        assert!(
            collapsed.stdout.contains("--show-cleared"),
            "collapsed Cleared panel points at --show-cleared. stdout: {}",
            collapsed.stdout
        );
        let expanded = run_fallow_raw(&[
            "review",
            "--root",
            tmp.path().to_str().unwrap(),
            "--base",
            "main",
            "--walkthrough",
            "--show-cleared",
        ]);
        assert_eq!(expanded.code, 0, "exits 0");
        assert!(
            !expanded.stdout.contains("pass --show-cleared to expand"),
            "the expanded panel drops the collapse hint. stdout: {}",
            expanded.stdout
        );
    }
}

#[test]
fn w2_walkthrough_viewed_state_round_trips() {
    let tmp = create_boundary_walkthrough_fixture();
    // First render to learn the current snapshot hash.
    let guide = run_walkthrough_guide(tmp.path());
    let hash = guide["graph_snapshot_hash"].as_str().unwrap().to_string();

    // Write a viewed-state ledger keyed on the changed file.
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    let state = serde_json::json!({
        "version": 1,
        "schema": "walkthrough-viewed-marks",
        "graph_snapshot_hash": hash,
        "entries": { "src/ui/page.ts": { "viewed_at": "2026-01-01T00:00:00Z" } }
    });
    fs::write(
        state_dir.join("walkthrough-state.json"),
        serde_json::to_string(&state).unwrap(),
    )
    .unwrap();

    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("viewed"),
        "the matched file renders a viewed mark. stdout: {}",
        output.stdout
    );
}

#[test]
fn w2_walkthrough_stale_viewed_state_is_ignored_not_deleted() {
    let tmp = create_boundary_walkthrough_fixture();
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    let state_path = state_dir.join("walkthrough-state.json");
    // A deliberately WRONG hash: the marks must be ignored on render.
    let state = serde_json::json!({
        "version": 1,
        "schema": "walkthrough-viewed-marks",
        "graph_snapshot_hash": "graph:staleeeeeeeeeeee",
        "entries": { "src/ui/page.ts": { "viewed_at": "2026-01-01T00:00:00Z" } }
    });
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0");
    assert!(
        !output.stdout.contains("\u{2713} viewed"),
        "a stale viewed mark must not render. stdout: {}",
        output.stdout
    );
    // The ledger on disk is NOT deleted (carry-forward).
    assert!(
        state_path.exists(),
        "a stale ledger is carried forward, never deleted"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert!(
        on_disk["entries"]["src/ui/page.ts"].is_object(),
        "the stale entry survives on disk"
    );
}

#[test]
fn w2_walkthrough_missing_and_garbled_state_still_exit_zero() {
    let tmp = create_boundary_walkthrough_fixture();
    // No state file: first-run common case.
    let fresh = run_walkthrough_human(tmp.path());
    assert_eq!(fresh.code, 0, "missing state file still exits 0");
    assert!(
        !fresh.stdout.contains("\u{2713} viewed"),
        "no viewed marks without a ledger"
    );

    // A garbled state file must not hard-error the render.
    let state_dir = tmp.path().join(".fallow");
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(state_dir.join("walkthrough-state.json"), b"{ not json").unwrap();
    let garbled = run_walkthrough_human(tmp.path());
    assert_eq!(garbled.code, 0, "garbled state file still exits 0");
}

#[test]
fn w2_walkthrough_mark_viewed_writes_local_ledger() {
    let tmp = create_boundary_walkthrough_fixture();
    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--mark-viewed",
        "src/ui/page.ts",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    let state_path = tmp.path().join(".fallow/walkthrough-state.json");
    assert!(state_path.exists(), "--mark-viewed writes the ledger");
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["version"], 1);
    assert_eq!(state["schema"], "walkthrough-viewed-marks");
    assert!(
        state["entries"]["src/ui/page.ts"].is_object(),
        "the marked file is recorded. state: {state}"
    );
}

#[test]
fn w2_walkthrough_conflicts_with_guide_and_file() {
    let tmp = create_boundary_walkthrough_fixture();
    // --walkthrough + --walkthrough-guide is a clap arg conflict (usage error,
    // distinct from the exit-0 success path).
    let with_guide = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--walkthrough-guide",
    ]);
    assert_ne!(
        with_guide.code, 0,
        "--walkthrough + --walkthrough-guide is rejected by clap"
    );

    let with_file = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--walkthrough-file",
        "agent.json",
    ]);
    assert_ne!(
        with_file.code, 0,
        "--walkthrough + --walkthrough-file is rejected by clap"
    );
}

/// A fixture whose HEAD diff mixes a load-bearing exported source file (consumed
/// by an UNCHANGED consumer outside the diff), plain source churn, and a
/// NON-source migration file. Exercises the file-accounting + membership fixes:
/// the migration must be surfaced as excluded (not dropped), and no file may
/// appear in both a stage and the Cleared panel.
fn create_mixed_walkthrough_fixture() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("migrations")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "wt-mixed", "main": "src/app.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), r#"{"entry": ["src/app.ts"]}"#).unwrap();

    // The consumer imports the contract; it is NOT touched by the HEAD diff, so
    // `lib.ts` is consumed out-of-diff -> load-bearing.
    fs::write(
        dir.join("src/app.ts"),
        "import { value } from './lib';\nexport const run = () => value();\n",
    )
    .unwrap();
    fs::write(dir.join("src/lib.ts"), "export const value = () => 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");

    // HEAD: change the load-bearing contract, add plain source churn, and add a
    // non-source migration file (which must be surfaced as excluded).
    fs::write(
        dir.join("src/lib.ts"),
        "export const value = () => 2;\nexport const extra = () => 3;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/helper.ts"),
        "export const help = () => 'x';\n",
    )
    .unwrap();
    fs::write(
        dir.join("migrations/0001_init.sql"),
        "CREATE TABLE t (id INTEGER);\n",
    )
    .unwrap();
    commit_all(dir, "change lib, add helper + migration");
    tmp
}

// F1/F2: the rendered Review Focus count reconciles staged + cleared + excluded,
// and the non-source migration file is surfaced as excluded, never dropped.
#[test]
fn w2_walkthrough_surfaces_non_source_and_reconciles_counts() {
    let tmp = create_mixed_walkthrough_fixture();
    let output = run_walkthrough_human(tmp.path());
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    // The non-source migration is surfaced honestly, not silently dropped.
    assert!(
        output.stderr.contains("non-source not reviewed"),
        "the .sql migration must be surfaced as excluded. stderr: {}",
        output.stderr
    );
    // The accounting sub-line names the staged bucket.
    assert!(
        output.stderr.contains("in stages"),
        "the header breakdown names the staged bucket. stderr: {}",
        output.stderr
    );
    // The migration file never appears as a reviewable stage row in the tour body.
    assert!(
        !output.stdout.contains("0001_init.sql"),
        "the non-source migration is not a stage row. stdout: {}",
        output.stdout
    );
}

// F3: a --mark-viewed file is removed from its stage and shown only under
// Cleared (each file in exactly one place: stage XOR cleared).
#[test]
fn w2_walkthrough_viewed_file_collapses_into_cleared_only() {
    let tmp = create_mixed_walkthrough_fixture();
    // Mark the load-bearing file viewed, then render with the Cleared panel open.
    let marked = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--mark-viewed",
        "src/lib.ts",
    ]);
    assert_eq!(
        marked.code, 0,
        "mark-viewed exits 0. stderr: {}",
        marked.stderr
    );

    let output = run_fallow_raw(&[
        "review",
        "--root",
        tmp.path().to_str().unwrap(),
        "--base",
        "main~1",
        "--walkthrough",
        "--show-cleared",
    ]);
    assert_eq!(output.code, 0, "exits 0. stderr: {}", output.stderr);
    let body = &output.stdout;
    let cleared_at = body
        .find("Cleared")
        .unwrap_or_else(|| panic!("cleared panel present. stdout: {body}"));
    let (stage_section, cleared_section) = body.split_at(cleared_at);
    // The viewed file is gone from the stage section and present only in Cleared.
    assert!(
        !stage_section.contains("lib.ts"),
        "the viewed file left its stage. stage section: {stage_section}"
    );
    assert!(
        cleared_section.contains("lib.ts"),
        "the viewed file shows only under Cleared. cleared section: {cleared_section}"
    );
}

/// Build a repo whose `src/old/` directory carries one over-threshold complex
/// function, one unused export, and a cross-file duplicate pair, all committed
/// on `main`. Returns the fixture guard; callers branch and rename.
fn create_rename_attribution_fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src/old")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name": "rename-audit-test", "main": "src/index.ts"}"#,
    )
    .unwrap();

    // Cyclomatic complexity 26 against the default threshold of 20.
    let mut complex_fn =
        String::from("export function complexFn(x: number): number {\n  let out = 0;\n");
    for n in 0..25 {
        use std::fmt::Write as _;
        writeln!(complex_fn, "  if (x > {n}) {{ out += {n}; }}").unwrap();
    }
    complex_fn
        .push_str("  return out;\n}\n\nexport const unusedHelper = () => 'nobody imports me';\n");
    fs::write(dir.join("src/old/impl.ts"), &complex_fn).unwrap();

    let duplicate = "export function sharedBlock(x: number): number {\n\
          const a = x + 1;\n\
          const b = a * 2;\n\
          const c = b - 3;\n\
          const d = c * c;\n\
          const e = d + a;\n\
          const f = e - b;\n\
          const g = f + c;\n\
          const h = g * d;\n\
          const i = h - e;\n\
          return a + b + c + d + e + f + g + h + i;\n\
        }\n";
    fs::write(dir.join("src/old/dupA.ts"), duplicate).unwrap();
    fs::write(dir.join("src/old/dupB.ts"), duplicate).unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "import { complexFn } from './old/impl';\ncomplexFn(1);\n",
    )
    .unwrap();

    git(dir, &["init", "-b", "main"]);
    // Pin LF so the base worktree checkout matches head bytes on Windows
    // runners; clone fingerprints hash the raw fragment text.
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "restructure"]);
    tmp
}

/// Rename `src/old` to `src/renamed` with `git mv` and repoint the importer.
/// The moved files themselves stay byte-identical (R100).
fn rename_old_directory(dir: &Path) {
    git(dir, &["mv", "src/old", "src/renamed"]);
    fs::write(
        dir.join("src/index.ts"),
        "import { complexFn } from './renamed/impl';\ncomplexFn(1);\n",
    )
    .unwrap();
}

fn run_rename_audit(dir: &Path) -> common::CommandOutput {
    run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--gate",
        "new-only",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--dupes-mode",
        "strict",
        "--dupes-min-tokens",
        "10",
        "--dupes-min-lines",
        "3",
    ])
}

#[test]
fn audit_new_only_inherits_findings_across_pure_rename() {
    let tmp = create_rename_attribution_fixture();
    let dir = tmp.path();
    rename_old_directory(dir);
    commit_all(dir, "git mv src/old src/renamed");

    let output = run_rename_audit(dir);

    assert_eq!(
        output.code, 0,
        "a pure git mv must not flip new-only from pass to fail. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["verdict"].as_str(), Some("pass"));
    let attribution = &json["attribution"];
    assert_eq!(
        attribution["complexity_introduced"].as_u64(),
        Some(0),
        "pre-existing complexity on a moved file is inherited: {attribution:#?}"
    );
    assert!(
        attribution["complexity_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the moved over-threshold function stays visible as inherited: {attribution:#?}"
    );
    assert_eq!(
        attribution["dead_code_introduced"].as_u64(),
        Some(0),
        "pre-existing dead code on moved files is inherited: {attribution:#?}"
    );
    assert!(
        attribution["dead_code_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the moved unused export stays visible as inherited: {attribution:#?}"
    );
    assert_eq!(
        attribution["duplication_introduced"].as_u64(),
        Some(0),
        "pre-existing duplication between moved files is inherited: {attribution:#?}"
    );
    assert!(
        attribution["duplication_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the moved clone group stays visible as inherited: {attribution:#?}"
    );
}

#[test]
fn audit_new_only_still_gates_new_finding_in_renamed_file() {
    let tmp = create_rename_attribution_fixture();
    let dir = tmp.path();
    rename_old_directory(dir);

    // Append a genuinely NEW over-threshold function to the moved file. The
    // rename map must only relocate the baseline, not suppress new debt.
    let mut new_fn = fs::read_to_string(dir.join("src/renamed/impl.ts")).unwrap();
    new_fn.push_str("\nexport function freshlyComplex(y: number): number {\n  let out = 0;\n");
    for n in 0..25 {
        use std::fmt::Write as _;
        writeln!(new_fn, "  if (y < {n}) {{ out -= {n}; }}").unwrap();
    }
    new_fn.push_str("  return out;\n}\n");
    fs::write(dir.join("src/renamed/impl.ts"), &new_fn).unwrap();
    commit_all(dir, "rename plus new complexity");

    let output = run_rename_audit(dir);

    assert_eq!(
        output.code, 1,
        "new debt added in a renamed file must still gate. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let attribution = &json["attribution"];
    assert!(
        attribution["complexity_introduced"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the new function attributes as introduced: {attribution:#?}"
    );
    assert!(
        attribution["complexity_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the pre-existing function still attributes as inherited: {attribution:#?}"
    );
}

#[test]
fn audit_new_only_inherits_pre_existing_findings_across_rename_with_edit() {
    let tmp = create_rename_attribution_fixture();
    let dir = tmp.path();
    rename_old_directory(dir);

    // Edit the moved file without introducing any finding: the rename is no
    // longer R100, but the surviving findings must still match their base
    // counterparts under the old path.
    let mut edited = fs::read_to_string(dir.join("src/renamed/impl.ts")).unwrap();
    edited.push_str("\nexport const answer = 42;\n");
    fs::write(dir.join("src/renamed/impl.ts"), &edited).unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { complexFn, answer } from './renamed/impl';\ncomplexFn(answer);\n",
    )
    .unwrap();
    commit_all(dir, "rename plus benign edit");

    let output = run_rename_audit(dir);

    assert_eq!(
        output.code, 0,
        "a rename with a benign edit keeps pre-existing findings inherited. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let attribution = &json["attribution"];
    assert_eq!(
        attribution["complexity_introduced"].as_u64(),
        Some(0),
        "an edited rename still relocates the baseline: {attribution:#?}"
    );
    assert!(
        attribution["complexity_inherited"]
            .as_u64()
            .is_some_and(|count| count >= 1),
        "the surviving function stays inherited: {attribution:#?}"
    );
    assert_eq!(
        attribution["dead_code_introduced"].as_u64(),
        Some(0),
        "no dead code was added by the edit: {attribution:#?}"
    );
}

/// Regression test for #2092: `typeAware.enabled` plus `audit --gate new-only`
/// must not fail when base and head resolve different semantic identities.
/// The tsconfig change between base and head shifts the project config hash,
/// so the audit degrades to syntactic attribution with a warning instead of
/// exiting 2, while a genuinely new unused export still fails the gate.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the regression fixture builds both audit sides and verifies JSON plus CI rendering"
)]
fn audit_new_only_degrades_to_syntactic_attribution_on_type_aware_identity_mismatch() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-type-aware", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"typeAware": {"enabled": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unusedBase = () => 0;\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "feature"]);

    // Change the TypeScript project config so base and head resolve different
    // semantic identities, touch the file with the inherited finding, and add
    // a genuinely new unused export as the gate control.
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true, "noUnusedParameters": true}}"#,
    )
    .unwrap();
    let utils = fs::read_to_string(dir.join("src/utils.ts")).unwrap();
    fs::write(dir.join("src/utils.ts"), format!("{utils}// touched\n")).unwrap();
    fs::write(
        dir.join("src/fresh.ts"),
        "export const usedNew = () => 1;\nexport const unusedNew = () => 2;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nimport { usedNew } from './fresh';\nused();\nusedNew();\n",
    )
    .unwrap();
    commit_all(dir, "change tsconfig and add fresh export");

    let output = common::run_fallow_raw_with_type_aware_sidecar(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(
        output.code, 2,
        "identity mismatch must degrade, not fail the audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    assert_eq!(
        output.code, 1,
        "the genuinely new unused export must still fail the new-only gate. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let warnings = json["_meta"]["type_aware"]["warnings"]
        .as_array()
        .expect("_meta.type_aware.warnings should be an array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|w| w.contains("syntactic attribution"))),
        "degrade warning should name the syntactic fallback: {warnings:#?}"
    );
    let unused_exports = json["dead_code"]["unused_exports"]
        .as_array()
        .expect("dead_code.unused_exports should be an array");
    assert!(
        unused_exports
            .iter()
            .any(|finding| finding["export_name"] == "unusedNew" && finding["introduced"] == true),
        "new export should be attributed as introduced: {unused_exports:#?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|finding| finding["export_name"] == "unusedBase" && finding["introduced"] == false),
        "pre-existing export should stay inherited: {unused_exports:#?}"
    );

    let comment = common::run_fallow_raw_with_type_aware_sidecar(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "pr-comment-gitlab",
        "--quiet",
    ]);
    assert_eq!(
        comment.code, 1,
        "type-aware audit comment should preserve the failing verdict: {}",
        comment.stderr
    );
    assert!(
        comment
            .stdout
            .contains("<!-- fallow-id: fallow-results -->")
    );
    assert!(comment.stdout.contains("unusedNew"), "{}", comment.stdout);
}

/// Regression test for #2102: with `typeAware.enabled`, a diff that only adds
/// a new zero-export file must keep type-aware attribution. The base pass
/// needs no semantic queries (its project-config hash stays deferred) while
/// the head pass runs at least one query (concrete hash); those identities
/// are compatible by design, so the audit must not degrade to syntactic
/// attribution or warn.
#[test]
fn audit_type_aware_new_file_keeps_type_aware_attribution() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-type-aware-new-file", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"typeAware": {"enabled": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(dir.join("src/utils.ts"), "export const used = () => 42;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "feature"]);

    // The diff only adds a zero-export file; its contents do not matter.
    fs::write(dir.join("src/newfile.ts"), "const x = 1;\nvoid x;\n").unwrap();
    commit_all(dir, "add zero-export file");

    let output = common::run_fallow_raw_with_type_aware_sidecar(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(
        output.code, 2,
        "compatible identities must not fail the audit. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let warnings: Vec<String> = json["_meta"]["type_aware"]["warnings"]
        .as_array()
        .map(|warnings| {
            warnings
                .iter()
                .filter_map(|warning| warning.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("syntactic attribution")),
        "a deferred base identity is compatible with a concrete head identity \
and must keep the type-aware comparison: {warnings:#?}"
    );
    assert!(
        json["_meta"]["type_aware"]["identity"].is_object(),
        "head should report its semantic analysis identity"
    );
    let unused_files = json["dead_code"]["unused_files"]
        .as_array()
        .expect("dead_code.unused_files should be an array");
    assert!(
        unused_files
            .iter()
            .any(|finding| finding["path"] == "src/newfile.ts" && finding["introduced"] == true),
        "the new file should be attributed as introduced: {unused_files:#?}"
    );
}

/// Negative control for the audit base-snapshot cache under type-aware
/// analysis (enabled via config, not a CLI flag): the same audit run twice
/// must behave identically. The second run hits the cached base snapshot,
/// which persists the base's semantic identity and pre-refinement syntactic
/// keys, so identities that genuinely match keep matching from cache and the
/// gate never spuriously degrades to syntactic attribution.
#[test]
fn audit_type_aware_base_snapshot_cache_preserves_identity_comparison() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-type-aware-cache", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"typeAware": {"enabled": true}}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { used } from './utils';\nused();\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unusedBase = () => 0;\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    git(dir, &["checkout", "-b", "feature"]);

    // Only existing files change: no tsconfig edit, so base and head resolve
    // the same semantic identity. A genuinely new unused export in an
    // existing file keeps the new-only gate failing on both runs so
    // attribution stays observable.
    fs::write(
        dir.join("src/utils.ts"),
        "export const used = () => 42;\nexport const unusedBase = () => 0;\nexport const unusedNew = () => 2;\n",
    )
    .unwrap();
    commit_all(dir, "add new unused export to an existing file");

    let args = [
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
    ];
    let cold = common::run_fallow_raw_with_type_aware_sidecar(&args);
    let warm = common::run_fallow_raw_with_type_aware_sidecar(&args);

    for (label, output) in [("cold", &cold), ("warm", &warm)] {
        assert_eq!(
            output.code, 1,
            "{label} run must fail the new-only gate on the new unused export only. stdout: {}\nstderr: {}",
            output.stdout, output.stderr
        );
        let json = parse_json(output);
        let warnings: Vec<String> = json["_meta"]["type_aware"]["warnings"]
            .as_array()
            .map(|warnings| {
                warnings
                    .iter()
                    .filter_map(|warning| warning.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("syntactic attribution")),
            "{label} run must not degrade: matching identities must keep the \
type-aware comparison, including from the base-snapshot cache. warnings: {warnings:#?}"
        );
        let unused_exports = json["dead_code"]["unused_exports"]
            .as_array()
            .expect("dead_code.unused_exports should be an array");
        assert!(
            unused_exports
                .iter()
                .any(|finding| finding["export_name"] == "unusedNew"
                    && finding["introduced"] == true),
            "{label} run should attribute the new export as introduced: {unused_exports:#?}"
        );
        assert!(
            unused_exports
                .iter()
                .any(|finding| finding["export_name"] == "unusedBase"
                    && finding["introduced"] == false),
            "{label} run should keep the pre-existing export inherited: {unused_exports:#?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Weakening-signal base/head read semantics (6.F) through the real binary.
// ---------------------------------------------------------------------------

/// A test file deleted at head scans against empty head content and surfaces
/// the removed-tests weakening signal in the review brief, while a file added
/// since base (missing at base) scans against an empty base and must not
/// fabricate any removed-content signal.
#[test]
fn review_brief_weakening_flags_deleted_test_file_not_added_file() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "weakening-fixture", "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "export const run = (): number => 1;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/removed.test.ts"),
        "it('a', () => {});\nit('b', () => {});\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "base");

    fs::remove_file(dir.join("src/removed.test.ts")).unwrap();
    fs::write(dir.join("src/added.test.ts"), "it('new', () => {});\n").unwrap();
    commit_all(dir, "head");

    let output = run_fallow_raw(&[
        "review",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "main~1",
        "--brief",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "the brief always exits 0. stderr: {}",
        output.stderr
    );
    let brief = parse_json(&output);
    let weakening = brief["weakening"]
        .as_array()
        .expect("brief JSON carries a weakening array");
    assert!(
        weakening.iter().any(|signal| {
            signal["file"] == "src/removed.test.ts"
                && signal["kind"] == "test-weakened"
                && signal["evidence"]
                    .as_str()
                    .is_some_and(|evidence| evidence.contains("it( removed"))
        }),
        "the deleted test file yields the removed-tests signal: {weakening:?}"
    );
    assert!(
        weakening
            .iter()
            .all(|signal| signal["file"] != "src/added.test.ts"),
        "a net-new file scans against an empty base and must not fabricate signals: {weakening:?}"
    );
}

/// Write a git repository whose `bun.lockb` blocks override resolution, with
/// one uncommitted file so the audit family has a changeset to report on.
fn bun_lockb_audit_repo(tmp: &TempDir) -> &std::path::Path {
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"issue-2366-audit","private":true,"main":"src/index.ts","overrides":{"ws":"^8.21.0"}}"#,
    )
    .unwrap();
    fs::write(dir.join("bun.lockb"), "").unwrap();
    fs::write(dir.join("src/index.ts"), "export const value = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    fs::write(
        dir.join("src/changed.ts"),
        "export const changed = () => 1;\n",
    )
    .unwrap();
    dir
}

/// Assert the envelope's dead-code section carries exactly one bun.lockb skip
/// diagnostic, root-relative, and that the envelope root carries no array.
fn assert_dead_code_section_carries_bun_lockb_skip(json: &serde_json::Value) {
    let diagnostics = json["dead_code"]["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let skips: Vec<&serde_json::Value> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["kind"] == "bun-lockb-override-resolution-skipped")
        .collect();
    assert_eq!(
        skips.len(),
        1,
        "exactly one bun.lockb skip diagnostic under dead_code: {}",
        json["dead_code"]["workspace_diagnostics"]
    );
    assert_eq!(skips[0]["path"], "package.json");
    assert!(
        json.get("workspace_diagnostics").is_none(),
        "the audit-family root has no diagnostics array of its own: {json}"
    );
}

/// Issue #2366: `fallow audit --format json` carries the analysis-stage
/// workspace diagnostics under `dead_code.workspace_diagnostics[]`, the same
/// `CheckOutput` payload the standalone `dead-code` envelope carries.
/// Preserving analysis-stage entries across the per-analysis config reloads is
/// what lets the audit envelope see them.
#[test]
fn audit_json_dead_code_section_carries_analysis_stage_workspace_diagnostics() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = bun_lockb_audit_repo(&tmp);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().expect("fixture path should be UTF-8"),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert_dead_code_section_carries_bun_lockb_skip(&parse_json(&output));
}

/// Issue #2366: under a per-analysis `production` split the audit family's
/// walks disagree about which files exist, and the last walk to run replaces
/// the process registry's source-discovery set. The dead-code section must
/// still report what the DEAD-CODE walk skipped, from that analysis's own
/// captured list, otherwise `fallow audit --format json` is narrower than the
/// run and narrower than the MCP `audit` tool, which serializes the typed list.
#[test]
fn audit_json_dead_code_section_carries_a_skip_only_the_dead_code_walk_saw() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"issue-2366-audit-split","private":true,"main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"production":{"deadCode":false,"health":true,"dupes":true}}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "export const value = 1;\n").unwrap();
    fs::write(dir.join("src/huge.test.ts"), "// filler\n".repeat(150_000)).unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "initial");
    fs::write(
        dir.join("src/changed.ts"),
        "export const changed = () => 1;\n",
    )
    .unwrap();

    let json = parse_json(&run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().expect("fixture path should be UTF-8"),
        "--base",
        "HEAD",
        "--max-file-size",
        "1",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let diagnostics = json["dead_code"]["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let skips: Vec<&serde_json::Value> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["kind"] == "skipped-large-file")
        .collect();
    assert_eq!(
        skips.len(),
        1,
        "the dead-code section reports what only its own walk skipped: {}",
        json["dead_code"]["workspace_diagnostics"]
    );
    assert_eq!(skips[0]["path"], "src/huge.test.ts");
}

/// Issue #2366: the `audit-brief` envelope shared by `fallow review` and
/// `fallow audit --brief` builds its dead-code section from the same registry,
/// so it is the third carrier that the preserve moves.
#[test]
fn review_json_dead_code_section_carries_analysis_stage_workspace_diagnostics() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = bun_lockb_audit_repo(&tmp);
    let root = dir.to_str().expect("fixture path should be UTF-8");

    for command in [["review"].as_slice(), ["audit", "--brief"].as_slice()] {
        let mut args = command.to_vec();
        args.extend_from_slice(&[
            "--root",
            root,
            "--base",
            "HEAD",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]);
        let output = run_fallow_raw(&args);
        let json = parse_json(&output);
        assert_eq!(
            json["kind"], "audit-brief",
            "{command:?} emits the shared brief envelope: {json}"
        );
        assert_dead_code_section_carries_bun_lockb_skip(&json);
    }
}
