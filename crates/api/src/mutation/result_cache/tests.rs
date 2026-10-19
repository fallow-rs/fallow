use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use rustc_hash::FxHashMap;

use super::super::protocol::Capabilities;
use super::super::scheduler::{MutantOutcome, ScheduleTimings};
use super::*;

/// A project with one source file, one test file, one file that the test
/// reaches, one setup file, a lockfile and a runner config.
fn project(dir: &Path) {
    for (path, text) in [
        ("src/a.ts", "export const a = 1 + 1;\n"),
        ("src/dep.ts", "export const dep = 2;\n"),
        ("src/unrelated.ts", "export const other = 3;\n"),
        ("test/a.test.ts", "import { a } from '../src/a';\n"),
        ("test/setup.ts", "globalThis.ready = true;\n"),
        ("package-lock.json", "{\"lockfileVersion\":3}\n"),
        ("vitest.config.ts", "export default {};\n"),
        ("package.json", "{\"name\":\"fixture\"}\n"),
    ] {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
}

fn key_files() -> KeyFiles {
    KeyFiles {
        reach_files: vec!["src/a.ts".to_owned(), "src/dep.ts".to_owned()],
        test_file_reach: BTreeMap::from([("test/a.test.ts".to_owned(), vec![0, 1])]),
        support_files: vec!["test/setup.ts".to_owned()],
    }
}

fn open(root: &Path) -> ResultCache {
    ResultCache::open(&root.join(".fallow/mutation"), root, "vitest", key_files())
}

fn ready(runner: &str, runtime: &str) -> Ready {
    Ready {
        runner_version: runner.to_owned(),
        runtime_version: runtime.to_owned(),
        test_files: Vec::new(),
        capabilities: Capabilities::default(),
        notes: Vec::new(),
    }
}

fn mutant(id: &str) -> ScheduledMutant {
    ScheduledMutant {
        id: id.to_owned(),
        file: "src/a.ts".to_owned(),
    }
}

/// The key of the standard query against the current files.
fn key(root: &Path) -> String {
    open(root).key(
        &ready("5.0.4", "v24.0.0"),
        &mutant("m1"),
        &["test/a.test.ts::adds".to_owned()],
        &["test/a.test.ts".to_owned()],
    )
}

fn edit(root: &Path, path: &str) {
    let file = root.join(path);
    let mut text = fs::read_to_string(&file).unwrap_or_default();
    text.push_str("// changed\n");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

#[test]
fn the_key_is_stable_when_no_key_part_changes() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let first = key(dir.path());
    assert_eq!(first, key(dir.path()), "a new cache gives the same key");
    edit(dir.path(), "src/unrelated.ts");
    assert_eq!(
        first,
        key(dir.path()),
        "a file outside the key parts does not change the key"
    );
}

#[test]
fn a_file_change_in_each_key_part_changes_the_key() {
    for (part, path) in [
        ("the mutated source file", "src/a.ts"),
        ("a covering test file", "test/a.test.ts"),
        ("a file that a covering test reaches", "src/dep.ts"),
        ("a runner setup file", "test/setup.ts"),
        ("the lockfile", "package-lock.json"),
        ("the runner config", "vitest.config.ts"),
        ("the package manifest", "package.json"),
        ("a new runner config file", "vite.config.mts"),
        ("a new lockfile", "pnpm-lock.yaml"),
        ("a new bun test config file", "bunfig.toml"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path());
        let before = key(dir.path());
        edit(dir.path(), path);
        assert_ne!(before, key(dir.path()), "{part} ({path})");
    }
}

#[test]
fn a_lockfile_in_a_parent_directory_is_a_key_part() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("packages/app");
    project(&root);
    fs::remove_file(root.join("package-lock.json")).unwrap();
    fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();
    let before = key(&root);
    edit(dir.path(), "pnpm-lock.yaml");
    assert_ne!(before, key(&root), "the workspace lockfile above the root");
}

fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// A tsconfig chain: the root config extends a relative base (with a
/// comment and without the `.json` extension), the base extends a package
/// config, and an array entry extends a second relative config.
fn tsconfig_chain(root: &Path) {
    write(
        root,
        "tsconfig.json",
        "{\n  // the shared options\n  \"extends\": [\"./configs/base\", \"./configs/paths.json\"],\n}\n",
    );
    write(
        root,
        "configs/base.json",
        "{ \"extends\": \"@acme/tsconfig/strict.json\" }\n",
    );
    write(root, "configs/paths.json", "{ \"compilerOptions\": {} }\n");
    write(
        root,
        "node_modules/@acme/tsconfig/strict.json",
        "{ \"extends\": \"./base.json\", \"compilerOptions\": { \"strict\": true } }\n",
    );
    write(
        root,
        "node_modules/@acme/tsconfig/base.json",
        "{ \"compilerOptions\": {} }\n",
    );
}

#[test]
fn a_change_to_a_config_that_tsconfig_extends_changes_the_key() {
    for path in [
        "configs/base.json",
        "configs/paths.json",
        "node_modules/@acme/tsconfig/strict.json",
        "node_modules/@acme/tsconfig/base.json",
    ] {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path());
        tsconfig_chain(dir.path());
        let before = key(dir.path());
        edit(dir.path(), path);
        assert_ne!(before, key(dir.path()), "{path}");
    }
}

#[test]
fn a_tsconfig_extends_cycle_ends() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    write(
        dir.path(),
        "tsconfig.json",
        "{ \"extends\": \"./a.json\" }\n",
    );
    write(
        dir.path(),
        "a.json",
        "{ \"extends\": \"./tsconfig.json\" }\n",
    );
    let before = key(dir.path());
    edit(dir.path(), "a.json");
    assert_ne!(before, key(dir.path()));
}

#[test]
fn the_runner_name_changes_the_key() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let tests = ["test/a.test.ts::adds".to_owned()];
    let files = ["test/a.test.ts".to_owned()];
    let key = |runner: &str| {
        ResultCache::open(
            &dir.path().join(".fallow/mutation"),
            dir.path(),
            runner,
            key_files(),
        )
        .key(&ready("24.0.0", "24.0.0"), &mutant("m1"), &tests, &files)
    };
    assert_ne!(
        key("vitest"),
        key("node-test"),
        "a result of one runner never answers a run of another runner"
    );
}

#[test]
fn each_query_part_changes_the_key() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let cache = open(dir.path());
    let tests = ["test/a.test.ts::adds".to_owned()];
    let files = ["test/a.test.ts".to_owned()];
    let base = cache.key(&ready("5.0.4", "v24.0.0"), &mutant("m1"), &tests, &files);
    let other_tests = [
        "test/a.test.ts::adds".to_owned(),
        "test/a.test.ts::subtracts".to_owned(),
    ];
    let variants = [
        (
            "the mutant id",
            cache.key(&ready("5.0.4", "v24.0.0"), &mutant("m2"), &tests, &files),
        ),
        (
            "the covering tests",
            cache.key(
                &ready("5.0.4", "v24.0.0"),
                &mutant("m1"),
                &other_tests,
                &files,
            ),
        ),
        (
            "the covering test files",
            cache.key(
                &ready("5.0.4", "v24.0.0"),
                &mutant("m1"),
                &tests,
                &["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()],
            ),
        ),
        (
            "the runner version",
            cache.key(&ready("5.0.5", "v24.0.0"), &mutant("m1"), &tests, &files),
        ),
        (
            "the runtime version",
            cache.key(&ready("5.0.4", "v24.1.0"), &mutant("m1"), &tests, &files),
        ),
    ];
    for (part, variant) in variants {
        assert_ne!(base, variant, "{part}");
    }
    let reversed = [other_tests[1].clone(), other_tests[0].clone()];
    assert_eq!(
        cache.key(
            &ready("5.0.4", "v24.0.0"),
            &mutant("m1"),
            &other_tests,
            &files
        ),
        cache.key(&ready("5.0.4", "v24.0.0"), &mutant("m1"), &reversed, &files),
        "the test order does not change the key"
    );
}

fn outcome(results: &[(&str, MutantStatus, bool)], keys: &[(&str, &str)]) -> ScheduleOutcome {
    ScheduleOutcome {
        ready: None,
        results: results
            .iter()
            .map(|&(id, status, cached)| {
                (
                    id.to_owned(),
                    MutantOutcome {
                        status,
                        killed_by: (status == MutantStatus::Killed)
                            .then(|| "test/a.test.ts::adds".to_owned()),
                        covering_tests: vec!["test/a.test.ts::adds".to_owned()],
                        static_mutant: false,
                        duration_ms: Some(7),
                        retried: false,
                        cached,
                    },
                )
            })
            .collect(),
        baseline_failures: Vec::new(),
        unloaded_paths: Vec::new(),
        undiscovered_test_files: Vec::new(),
        issues: Vec::new(),
        unretried: Vec::new(),
        timings: ScheduleTimings::default(),
        workers: 1,
        cache_keys: keys
            .iter()
            .map(|&(id, key)| (id.to_owned(), key.to_owned()))
            .collect::<FxHashMap<_, _>>(),
        promoted: Vec::new(),
    }
}

#[test]
fn killed_and_survived_results_are_stored_with_the_not_keyed_limit() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let cache = open(dir.path());
    let mut run = outcome(
        &[
            ("killed", MutantStatus::Killed, false),
            ("survived", MutantStatus::Survived, false),
            ("unretried", MutantStatus::Survived, false),
            ("timeout", MutantStatus::Timeout, false),
            ("flaky", MutantStatus::Flaky, false),
            ("error", MutantStatus::RuntimeError, false),
        ],
        &[
            ("killed", "k1"),
            ("survived", "k2"),
            ("unretried", "k3"),
            ("timeout", "k4"),
            ("flaky", "k5"),
            ("error", "k6"),
        ],
    );
    run.unretried = vec!["unretried".to_owned()];
    assert_eq!(cache.record(&run), CacheCounts { hits: 0, stored: 2 });
    cache.save().unwrap();

    let file: serde_json::Value = serde_json::from_slice(
        &fs::read(dir.path().join(".fallow/mutation/results.json")).unwrap(),
    )
    .unwrap();
    let entries = file["entries"].as_object().unwrap();
    assert_eq!(
        entries.keys().collect::<Vec<_>>(),
        ["killed", "survived"],
        "a timeout, a flaky result, a runtime error and an unretried survivor are not stored"
    );
    assert_eq!(
        entries["killed"]["not_keyed"],
        serde_json::json!(["dynamic-imports", "file-reads"])
    );

    let reopened = open(dir.path());
    let hit = reopened.get("killed", "k1").expect("a hit");
    assert_eq!(hit.status, MutantStatus::Killed);
    assert_eq!(hit.killed_by.as_deref(), Some("test/a.test.ts::adds"));
    assert_eq!(hit.duration_ms, Some(7));
    assert!(
        reopened.get("killed", "other key").is_none(),
        "a key change is a miss"
    );
    assert!(reopened.get("unknown", "k1").is_none());
}

#[test]
fn a_cached_result_is_not_stored_again() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let cache = open(dir.path());
    let run = outcome(&[("hit", MutantStatus::Killed, true)], &[]);
    assert_eq!(cache.record(&run), CacheCounts { hits: 1, stored: 0 });
}

#[test]
fn a_damaged_or_foreign_cache_file_gives_an_empty_cache() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let file = dir.path().join(".fallow/mutation/results.json");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    for text in [
        "not json",
        r#"{"format_version":99,"generation":1,"entries":{"m1":{"key":"k","status":"killed","not_keyed":[],"generation":1}}}"#,
        r#"{"format_version":1,"generation":1,"entries":{"m1":{"key":"k","status":"timeout","not_keyed":[],"generation":1}}}"#,
    ] {
        fs::write(&file, text).unwrap();
        assert!(open(dir.path()).get("m1", "k").is_none(), "{text}");
    }
}

#[test]
fn the_least_recently_used_entries_leave_above_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let mut cache = open(dir.path());
    cache.max_entries = 2;
    cache.record(&outcome(
        &[
            ("old", MutantStatus::Killed, false),
            ("used", MutantStatus::Killed, false),
        ],
        &[("old", "k1"), ("used", "k2")],
    ));
    cache.save().unwrap();

    let mut cache = open(dir.path());
    cache.max_entries = 2;
    assert!(cache.get("used", "k2").is_some());
    cache.record(&outcome(
        &[("new", MutantStatus::Survived, false)],
        &[("new", "k3")],
    ));
    cache.save().unwrap();

    let cache = open(dir.path());
    assert!(cache.get("old", "k1").is_none(), "the oldest entry left");
    assert!(cache.get("used", "k2").is_some(), "a hit keeps an entry");
    assert!(cache.get("new", "k3").is_some());
}
