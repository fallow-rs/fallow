use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::super::cache::{BaseKeyRequest, audit_base_snapshot_cache_key};
use super::super::{AuditOptions, AuditTypeAwareOptions, audit_review_benchmark_options};
use super::BaseRunInputs;
use fallow_config::{CommandKind, EnvSnapshot, RunInputs, SelectionInput, Surface};
use fallow_engine::resolved_run::{ResolvedRun, resolve_run};
use fallow_types::sourced::{Given, Origin};

const EMPTY_V8_DUMP: &str = r#"{"result":[]}"#;

use fallow_engine::git_fixture::fixture_git as git;

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

/// The head run of `fallow audit`. `env` is the whole environment of the
/// run, so the process environment does not reach the test.
fn head_run(root: &Path, env: &[(&str, &str)]) -> ResolvedRun {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::from_pairs(env, Some(root)),
        CommandKind::Audit,
        SelectionInput::Lists {
            only: Vec::new(),
            skip: Vec::new(),
            origin: Origin::Flag("--only"),
        },
    );
    inputs.root = Some(Given {
        value: root.to_path_buf(),
        origin: Origin::Flag("--root"),
    });
    inputs.threads = Some(Given {
        value: 1,
        origin: Origin::Flag("--threads"),
    });
    resolve_run(&inputs).expect("head run")
}

fn key_hash(opts: &AuditOptions<'_>) -> u64 {
    key_hash_with(
        opts,
        &head_run(opts.root, &[]),
        AuditTypeAwareOptions::default(),
    )
}

fn key_hash_with(
    opts: &AuditOptions<'_>,
    head: &ResolvedRun,
    type_aware: AuditTypeAwareOptions<'_>,
) -> u64 {
    optional_key_hash(opts, head, type_aware).expect("key should exist")
}

/// The hash of the base key, or `None` when the run uses no base cache.
fn optional_key_hash(
    opts: &AuditOptions<'_>,
    head: &ResolvedRun,
    type_aware: AuditTypeAwareOptions<'_>,
) -> Option<u64> {
    let base_run = BaseRunInputs::resolve(opts);
    audit_base_snapshot_cache_key(&BaseKeyRequest {
        opts,
        base_run: &base_run,
        base_ref: "HEAD",
        changed_files: &FxHashSet::default(),
        workspace_patterns: &[],
        head,
        type_aware,
    })
    .expect("key should compute")
    .map(|key| key.hash)
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

#[test]
fn a_saved_baseline_path_does_not_change_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let without = key_hash(&opts);

    for path in ["a.json", "b.json"] {
        let path = Path::new(path);
        let mut dead_code = options(root, &config_path, &cache_dir);
        dead_code.dead_code_baseline = Some(path);
        let mut health = options(root, &config_path, &cache_dir);
        health.health_baseline = Some(path);
        let mut dupes = options(root, &config_path, &cache_dir);
        dupes.dupes_baseline = Some(path);
        for with in [&dead_code, &health, &dupes] {
            assert_eq!(
                key_hash(with),
                without,
                "a saved-baseline path must not change the base key"
            );
        }
    }
}

fn type_aware_off() -> AuditTypeAwareOptions<'static> {
    AuditTypeAwareOptions {
        enabled: Some(false),
        ..AuditTypeAwareOptions::default()
    }
}

#[test]
fn the_type_aware_flag_changes_the_base_key_of_a_type_aware_config() {
    let tmp = repo(&[(".fallowrc.json", r#"{"typeAware":{"enabled":true}}"#)]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let head = head_run(root, &[]);

    assert_ne!(
        key_hash_with(&opts, &head, AuditTypeAwareOptions::default()),
        key_hash_with(&opts, &head, type_aware_off()),
        "--no-type-aware must change the base key of a type-aware config"
    );
}

#[test]
fn a_flag_that_keeps_the_selection_keeps_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let head = head_run(root, &[]);

    assert_eq!(
        key_hash_with(&opts, &head, AuditTypeAwareOptions::default()),
        key_hash_with(&opts, &head, type_aware_off()),
        "two syntactic runs must share the base key"
    );
}

#[test]
fn the_type_aware_environment_changes_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let hash = |env: &[(&str, &str)]| {
        key_hash_with(
            &opts,
            &head_run(root, env),
            AuditTypeAwareOptions::default(),
        )
    };

    let syntactic = hash(&[]);
    let type_aware = hash(&[("FALLOW_TYPE_AWARE", "1")]);
    let with_project = hash(&[
        ("FALLOW_TYPE_AWARE", "1"),
        ("FALLOW_TYPE_AWARE_PROJECTS", "tsconfig.app.json"),
    ]);

    assert_ne!(
        syntactic, type_aware,
        "FALLOW_TYPE_AWARE must change the base key"
    );
    assert_ne!(
        with_project, syntactic,
        "FALLOW_TYPE_AWARE_PROJECTS must change the base key"
    );
    assert_ne!(
        with_project, type_aware,
        "FALLOW_TYPE_AWARE_PROJECTS must change the base key"
    );
}

#[test]
fn the_audit_type_aware_key_changes_the_base_key() {
    let tmp = repo(&[]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let head = head_run(root, &[]);
    let audit_key = AuditTypeAwareOptions {
        config_default: Some(true),
        ..AuditTypeAwareOptions::default()
    };

    assert_ne!(
        key_hash_with(&opts, &head, AuditTypeAwareOptions::default()),
        key_hash_with(&opts, &head, audit_key),
        "audit.typeAware must change the base key"
    );
}

#[test]
fn an_unusable_type_aware_variable_turns_off_the_base_cache() {
    let tmp = repo(&[]);
    let root = tmp.path();
    let config_path = None;
    let cache_dir = root.join(".fallow/cache");
    let opts = options(root, &config_path, &cache_dir);
    let head = head_run(root, &[("FALLOW_TYPE_AWARE", "maybe")]);

    assert_eq!(
        optional_key_hash(&opts, &head, AuditTypeAwareOptions::default()),
        None,
        "a run with an unusable FALLOW_TYPE_AWARE value must not read a base snapshot"
    );
}
