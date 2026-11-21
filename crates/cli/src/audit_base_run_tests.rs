use std::path::{Path, PathBuf};
use std::process::Command;

use rustc_hash::FxHashSet;

use super::super::cache::{BaseKeyRequest, audit_base_snapshot_cache_key};
use super::super::{AuditOptions, audit_review_benchmark_options};
use super::BaseRunInputs;

const EMPTY_V8_DUMP: &str = r#"{"result":[]}"#;

fn git(dir: &Path, args: &[&str]) {
    let output = fallow_engine::changed_files::clear_ambient_git_env(&mut Command::new("git"))
        .args(args)
        .current_dir(dir)
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
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir should be created");
    }
    std::fs::write(path, contents).expect("file should be written");
}

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"base-identity","main":"src/index.ts"}"#,
    );
    write(root, "src/index.ts", "export const used = 1;\n");
    for (relative, contents) in files {
        write(root, relative, contents);
    }
    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    tmp
}

#[expect(
    clippy::ref_option,
    reason = "AuditOptions.config_path is &Option<PathBuf>"
)]
fn options<'a>(
    root: &'a Path,
    config_path: &'a Option<PathBuf>,
    cache_dir: &'a Path,
) -> AuditOptions<'a> {
    let mut opts = audit_review_benchmark_options(root, config_path, cache_dir, 1);
    opts.no_cache = false;
    opts.brief = false;
    opts.changed_since = Some("HEAD");
    opts.production_dead_code = None;
    opts.production_health = None;
    opts.production_dupes = None;
    opts
}

fn key_hash(opts: &AuditOptions<'_>) -> u64 {
    let base_run = BaseRunInputs::resolve(opts);
    audit_base_snapshot_cache_key(&BaseKeyRequest {
        opts,
        base_run: &base_run,
        base_ref: "HEAD",
        changed_files: &FxHashSet::default(),
        workspace_patterns: &[],
    })
    .expect("key should compute")
    .expect("key should exist")
    .hash
}

#[test]
fn a_changed_v8_dump_changes_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    write(root, "v8/coverage-1.json", EMPTY_V8_DUMP);
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let mut opts = options(root, &config_path, &cache_dir);
    opts.coverage = Some(Path::new("v8"));

    let first = key_hash(&opts);
    write(root, "v8/coverage-1.json", r#"{"result":[], "x": 1}"#);
    let changed = key_hash(&opts);
    write(root, "v8/coverage-2.json", EMPTY_V8_DUMP);
    let added = key_hash(&opts);
    std::fs::remove_file(root.join("v8/coverage-2.json")).expect("dump should be removed");
    let removed = key_hash(&opts);

    assert_ne!(first, changed, "a changed dump must change the base key");
    assert_ne!(changed, added, "an added dump must change the base key");
    assert_eq!(
        removed, changed,
        "a removed dump must give the key of the remaining dumps"
    );
}

#[test]
fn an_istanbul_map_in_the_directory_hides_the_dumps_from_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    write(root, "cov/coverage-final.json", "{}");
    write(root, "cov/coverage-1.json", EMPTY_V8_DUMP);
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let mut opts = options(root, &config_path, &cache_dir);
    opts.coverage = Some(Path::new("cov"));

    let first = key_hash(&opts);
    write(root, "cov/coverage-1.json", r#"{"result":[], "x": 1}"#);
    let dump_changed = key_hash(&opts);
    write(root, "cov/coverage-final.json", r#"{"x": 1}"#);
    let map_changed = key_hash(&opts);

    assert_eq!(
        first, dump_changed,
        "the reader ignores the dumps next to an Istanbul map"
    );
    assert_ne!(
        dump_changed, map_changed,
        "a changed Istanbul map must change the base key"
    );
}

#[test]
fn the_base_key_reads_the_coverage_of_the_base_options() {
    let tmp = repo(&[]);
    let root = tmp.path();
    write(root, "coverage/coverage-final.json", "{}");
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);

    let first = key_hash(&opts);
    write(root, "coverage/coverage-final.json", r#"{"x": 1}"#);
    let changed = key_hash(&opts);

    assert_ne!(
        first, changed,
        "an auto-detected coverage map must change the base key"
    );
}
