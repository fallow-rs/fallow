use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::super::protocol::{Capabilities, InitOptions, TestStatus};
use super::super::result_cache::KeyFiles;
use super::super::scheduler::StoredTest;
use super::*;

const TEST: &str = "test/a.test.ts";

/// A project with two source files, one test file that reaches `src/a.ts`
/// and `test/helper.ts`, a setup file, a lockfile and a runner config.
fn project(dir: &Path) {
    for (path, text) in [
        ("src/a.ts", "export const a = 1 + 1;\n"),
        ("src/other.ts", "export const other = 3;\n"),
        ("test/helper.ts", "export const exact = true;\n"),
        (TEST, "import { a } from '../src/a';\n"),
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

fn result_cache(root: &Path) -> Arc<ResultCache> {
    Arc::new(ResultCache::keys_only(
        &root.join(".fallow/mutation"),
        root,
        "vitest",
        KeyFiles {
            reach_files: vec!["src/a.ts".to_owned(), "test/helper.ts".to_owned()],
            test_file_reach: BTreeMap::from([(TEST.to_owned(), vec![0, 1])]),
            support_files: vec!["test/setup.ts".to_owned()],
            runner_projects: Vec::new(),
        },
    ))
}

fn init(env: &[(&str, &str)], test_files: &[&str]) -> InitRequest {
    InitRequest {
        root: "/project".to_owned(),
        runner: "vitest".to_owned(),
        config_path: None,
        run_dir: "/cache/run-1".to_owned(),
        path_map: BTreeMap::new(),
        options: InitOptions {
            test_files: test_files.iter().map(|&file| file.to_owned()).collect(),
            env: env
                .iter()
                .map(|&(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            ..InitOptions::default()
        },
    }
}

/// The parts of one run that the tests vary.
struct Run<'a> {
    instrumented: [(&'a str, &'a str); 2],
    operators: Option<&'a [String]>,
    init: InitRequest,
    ready: Ready,
}

impl Default for Run<'_> {
    fn default() -> Self {
        Self {
            instrumented: [
                ("src/a.ts", "export const a = __m(1) ? 1 - 1 : 1 + 1;\n"),
                ("src/other.ts", "export const other = 3;\n"),
            ],
            operators: None,
            init: init(&[], &[]),
            ready: ready("4.1.0", "v26.13.0"),
        }
    }
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

fn open(root: &Path, run: &Run<'_>) -> DryRunCache {
    let files: Vec<RunFile<'_>> = run
        .instrumented
        .iter()
        .map(|&(path, code)| RunFile { path, code })
        .collect();
    DryRunCache::open(
        &root.join(".fallow/mutation"),
        result_cache(root),
        &files,
        run.operators,
        &run.init,
    )
}

fn key(root: &Path, run: &Run<'_>) -> String {
    open(root, run).key(&run.ready, TEST)
}

fn edit(root: &Path, path: &str) {
    let file = root.join(path);
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str("// changed\n");
    fs::write(file, text).unwrap();
}

fn facts() -> StoredDryRun {
    StoredDryRun {
        tests: vec![StoredTest {
            test_id: format!("{TEST}::adds"),
            duration_ms: Some(3.0),
            status: TestStatus::Pass,
            index: None,
        }],
        hits: BTreeMap::from([(format!("{TEST}::adds"), vec!["m1".to_owned()])]),
        setup_ms: Some(1.5),
        ..StoredDryRun::default()
    }
}

#[test]
fn the_key_is_stable_when_no_key_part_changes() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let first = key(dir.path(), &Run::default());
    assert_eq!(first, key(dir.path(), &Run::default()));
    edit(dir.path(), "src/other.ts");
    let scoped = Run {
        init: init(&[], &[TEST, "test/b.test.ts"]),
        instrumented: [
            ("src/a.ts", "export const a = __m(1) ? 1 - 1 : 1 + 1;\n"),
            ("src/other.ts", "export const other = __m(2) ? 2 : 3;\n"),
        ],
        ..Run::default()
    };
    assert_eq!(
        first,
        key(dir.path(), &scoped),
        "a source file outside the reach, its instrumented code, and the test file selection are not key parts"
    );
}

#[test]
fn each_input_of_the_dry_run_of_a_file_changes_its_key() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let first = key(dir.path(), &Run::default());
    let operators = ["arithmetic-operator".to_owned()];
    let runs = [
        (
            "the instrumented code of a reached source file",
            Run {
                instrumented: [
                    ("src/a.ts", "export const a = __m(9) ? 1 * 1 : 1 + 1;\n"),
                    ("src/other.ts", "export const other = 3;\n"),
                ],
                ..Run::default()
            },
        ),
        (
            "the operators",
            Run {
                operators: Some(&operators),
                ..Run::default()
            },
        ),
        (
            "the runner environment",
            Run {
                init: init(&[("TZ", "UTC")], &[]),
                ..Run::default()
            },
        ),
        (
            "the project root",
            Run {
                init: InitRequest {
                    root: "/moved/project".to_owned(),
                    ..init(&[], &[])
                },
                ..Run::default()
            },
        ),
        (
            "the runner version",
            Run {
                ready: ready("4.1.1", "v26.13.0"),
                ..Run::default()
            },
        ),
        (
            "the runtime version",
            Run {
                ready: ready("4.1.0", "v26.14.0"),
                ..Run::default()
            },
        ),
    ];
    for (part, run) in runs {
        assert_ne!(first, key(dir.path(), &run), "{part}");
    }
    for path in [
        TEST,
        "test/helper.ts",
        "src/a.ts",
        "test/setup.ts",
        "package-lock.json",
        "vitest.config.ts",
    ] {
        edit(dir.path(), path);
        assert_ne!(first, key(dir.path(), &Run::default()), "{path}");
    }
}

#[test]
fn a_saved_entry_is_found_under_its_key_only() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let run = Run::default();
    let cache = open(dir.path(), &run);
    let key = cache.key(&run.ready, TEST);
    let stored = cache.record(&[DryRunEntry {
        test_file: TEST.to_owned(),
        key: key.clone(),
        facts: facts(),
    }]);
    assert_eq!(stored, 1);
    cache.save().unwrap();

    let reopened = open(dir.path(), &run);
    assert_eq!(reopened.get(TEST, &key), Some(facts()));
    assert_eq!(reopened.get(TEST, "other key"), None);
    assert_eq!(reopened.get("test/b.test.ts", &key), None);
}

#[test]
fn a_file_of_another_format_or_a_broken_file_gives_an_empty_cache() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let run = Run::default();
    let cache = open(dir.path(), &run);
    let key = cache.key(&run.ready, TEST);
    cache.record(&[DryRunEntry {
        test_file: TEST.to_owned(),
        key: key.clone(),
        facts: facts(),
    }]);
    cache.save().unwrap();
    let path = dir.path().join(".fallow/mutation").join(DRY_RUN_CACHE_FILE);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace(
            &format!("\"format_version\":{FORMAT_VERSION}"),
            "\"format_version\":0",
        ),
    )
    .unwrap();
    assert_eq!(open(dir.path(), &run).get(TEST, &key), None);
    fs::write(&path, "{").unwrap();
    assert_eq!(open(dir.path(), &run).get(TEST, &key), None);
}

#[test]
fn the_least_recently_used_entries_go_above_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let run = Run::default();
    let entry = |file: &str| DryRunEntry {
        test_file: file.to_owned(),
        key: "key".to_owned(),
        facts: facts(),
    };
    let mut cache = open(dir.path(), &run);
    cache.max_entries = 2;
    cache.record(&[entry("a"), entry("b")]);
    cache.save().unwrap();
    let mut cache = open(dir.path(), &run);
    cache.max_entries = 2;
    assert!(cache.get("a", "key").is_some(), "a run uses `a`");
    cache.record(&[entry("c")]);
    cache.save().unwrap();
    let cache = open(dir.path(), &run);
    assert!(cache.get("a", "key").is_some());
    assert!(cache.get("b", "key").is_none(), "`b` is the oldest entry");
    assert!(cache.get("c", "key").is_some());
}
