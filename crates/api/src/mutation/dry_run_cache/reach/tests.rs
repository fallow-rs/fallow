use std::collections::{BTreeMap, BTreeSet};

use fallow_engine::mutation::{SourceKind, function_probe, module_probe, outline, probe_key};
use rustc_hash::FxHashMap;

use super::{KeepReason, ReachFacts, ReachStore};
use crate::mutation::scheduler::ShareProbes;

const TEST: &str = "src/a.test.ts";
const A: &str = "src/a.ts";
const B: &str = "src/b.ts";
const INDEX: &str = "src/index.ts";
const ENV: &str = "env-1";

const A_SOURCE: &str = "export const a = (x: number): number => (x > 0 ? x + 1 : x - 1);\n";
const B_SOURCE: &str =
    "export const b = (x: number): number => x * 2;\nexport function c() { return 1; }\n";
const INDEX_SOURCE: &str = "export * from './a';\nexport * from './b';\n";

/// The project of one run: the source of each probed file, the mutant
/// spans, and files without probes on disk.
struct Run {
    sources: BTreeMap<&'static str, String>,
    mutants: FxHashMap<String, Vec<(u32, u32)>>,
    dir: tempfile::TempDir,
}

impl Run {
    fn new() -> Self {
        let sources = [(A, A_SOURCE), (B, B_SOURCE), (INDEX, INDEX_SOURCE)]
            .into_iter()
            .map(|(path, source)| (path, source.to_owned()))
            .collect();
        Self {
            sources,
            mutants: FxHashMap::default(),
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn edit(&mut self, path: &'static str, source: &str) -> &mut Self {
        self.sources.insert(path, source.to_owned());
        self
    }

    /// Adds a mutant at the first `text` of `path`.
    fn mutant(&mut self, path: &'static str, text: &str) -> &mut Self {
        let start = self.sources[path].find(text).unwrap() as u32;
        self.mutants
            .entry(path.to_owned())
            .or_default()
            .push((start, start + text.len() as u32));
        self
    }

    fn facts(&self, extra_reach: &[&str]) -> ReachFacts {
        let probed = self
            .sources
            .iter()
            .map(|(path, source)| ((*path).to_owned(), outline(source, SourceKind::Ts).unwrap()))
            .collect();
        let mut files: Vec<String> = self.sources.keys().map(|path| (*path).to_owned()).collect();
        files.extend(extra_reach.iter().map(|path| (*path).to_owned()));
        let reach = std::iter::once((TEST.to_owned(), files)).collect();
        ReachFacts::from_parts(self.dir.path(), probed, reach, self.mutants.clone())
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
}

/// The probe ids of a test file that loads the barrel and both files, and
/// calls `b` only.
fn calls_b() -> ShareProbes {
    share(
        &[
            module_probe(&probe_key(INDEX)),
            module_probe(&probe_key(A)),
            module_probe(&probe_key(B)),
        ],
        &[function_probe(&probe_key(B), 0)],
    )
}

fn share(statics: &[String], entered: &[String]) -> ShareProbes {
    ShareProbes {
        files: vec![TEST.to_owned()],
        statics: statics.iter().cloned().collect(),
        entered: std::iter::once((TEST.to_owned(), entered.iter().cloned().collect())).collect(),
        failed: BTreeSet::new(),
    }
}

fn recorded(run: &Run, probes: &ShareProbes, extra_reach: &[&str]) -> ReachStore {
    let mut store = ReachStore::default();
    let count = store.record(
        &run.facts(extra_reach),
        std::slice::from_ref(probes),
        true,
        |_| ENV.to_owned(),
        1,
    );
    assert_eq!(count, 1);
    store
}

fn decide(store: &ReachStore, run: &Run, extra_reach: &[&str]) -> Result<(), KeepReason> {
    store.decide(&run.facts(extra_reach), TEST, ENV)
}

#[test]
fn a_test_file_without_a_record_stays() {
    let run = Run::new();
    assert_eq!(
        ReachStore::default().decide(&run.facts(&[]), TEST, ENV),
        Err(KeepReason::NoRecord)
    );
}

#[test]
fn an_unchanged_project_drops_the_file_when_no_mutant_is_in_entered_code() {
    let mut run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    assert_eq!(decide(&store, &run, &[]), Ok(()));
    run.mutant(A, "x + 1");
    assert_eq!(
        decide(&store, &run, &[]),
        Ok(()),
        "a body that the file never entered"
    );
    run.mutant(B, "x * 2");
    assert_eq!(
        decide(&store, &run, &[]),
        Err(KeepReason::MutantInEnteredFunction)
    );
}

#[test]
fn a_change_of_a_body_that_the_file_never_entered_drops_the_file() {
    let mut run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    run.edit(
        A,
        "export const a = (x: number): number => (x >= 0 ? x + 2 : x - 1);\n",
    )
    .mutant(A, "x + 2");
    assert_eq!(decide(&store, &run, &[]), Ok(()));
}

#[test]
fn a_change_of_an_entered_body_keeps_the_file() {
    let mut run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    run.edit(
        B,
        "export const b = (x: number): number => x * 3;\nexport function c() { return 1; }\n",
    );
    assert_eq!(
        decide(&store, &run, &[]),
        Err(KeepReason::ChangedEnteredFunction)
    );
}

#[test]
fn code_outside_the_bodies_of_a_loaded_file_keeps_the_file() {
    let mut run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    // A new parameter: arity checks read it without a call.
    run.edit(
        A,
        "export const a = (x: number, y = 1): number => (x > 0 ? x + 1 : x - 1);\n",
    );
    assert_eq!(decide(&store, &run, &[]), Err(KeepReason::ChangedSkeleton));
}

#[test]
fn a_change_of_a_file_that_the_test_file_never_loaded_drops_the_file() {
    let mut run = Run::new();
    let probes = share(
        &[module_probe(&probe_key(B))],
        &[function_probe(&probe_key(B), 0)],
    );
    let store = recorded(&run, &probes, &[]);
    run.edit(
        A,
        "import './side';\nexport const a = (x: number): number => x;\n",
    )
    .mutant(A, "x");
    assert_eq!(decide(&store, &run, &[]), Ok(()));
}

#[test]
fn a_mutant_in_the_skeleton_of_a_loaded_file_keeps_the_file() {
    let mut run = Run::new();
    run.edit(
        B,
        "export const LIMIT = 2 + 1;\nexport const b = (x: number): number => x * 2;\n",
    );
    let store = recorded(&run, &calls_b(), &[]);
    run.mutant(B, "2 + 1");
    assert_eq!(
        decide(&store, &run, &[]),
        Err(KeepReason::MutantInLoadedCode)
    );
}

#[test]
fn the_probes_outside_each_test_belong_to_each_file_of_the_share() {
    let mut run = Run::new();
    // The share entered `a` while a module loaded, so the test file entered
    // it too.
    let probes = share(
        &[
            module_probe(&probe_key(A)),
            function_probe(&probe_key(A), 0),
        ],
        &[],
    );
    let store = recorded(&run, &probes, &[]);
    run.mutant(A, "x + 1");
    assert_eq!(
        decide(&store, &run, &[]),
        Err(KeepReason::MutantInEnteredFunction)
    );
}

#[test]
fn another_environment_keeps_the_file() {
    let run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    assert_eq!(
        store.decide(&run.facts(&[]), TEST, "env-2"),
        Err(KeepReason::Environment)
    );
}

#[test]
fn a_new_reached_file_keeps_the_file() {
    let run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    run.write("src/new.ts", "export const n = 1;\n");
    assert_eq!(
        decide(&store, &run, &["src/new.ts"]),
        Err(KeepReason::NewReachFile)
    );
}

#[test]
fn a_changed_file_without_probes_keeps_the_file() {
    let run = Run::new();
    run.write("src/data.json", "{\"a\": 1}");
    let store = recorded(&run, &calls_b(), &["src/data.json"]);
    assert_eq!(decide(&store, &run, &["src/data.json"]), Ok(()));
    run.write("src/data.json", "{\"a\": 2}");
    assert_eq!(
        decide(&store, &run, &["src/data.json"]),
        Err(KeepReason::ChangedFileWithoutProbes)
    );
}

#[test]
fn a_failed_or_incomplete_dry_run_writes_no_record() {
    let run = Run::new();
    let mut failed = calls_b();
    failed.failed.insert(TEST.to_owned());
    let mut store = ReachStore::default();
    let facts = run.facts(&[]);
    assert_eq!(
        store.record(&facts, &[failed], true, |_| ENV.to_owned(), 1),
        0
    );
    assert_eq!(store.decide(&facts, TEST, ENV), Err(KeepReason::NoRecord));
    assert_eq!(
        store.record(&facts, &[calls_b()], false, |_| ENV.to_owned(), 1),
        0
    );
    assert_eq!(store.decide(&facts, TEST, ENV), Err(KeepReason::NoRecord));
}

#[test]
fn a_failed_dry_run_removes_an_earlier_record() {
    let run = Run::new();
    let mut store = recorded(&run, &calls_b(), &[]);
    let mut failed = calls_b();
    failed.failed.insert(TEST.to_owned());
    store.record(&run.facts(&[]), &[failed], true, |_| ENV.to_owned(), 2);
    assert_eq!(decide(&store, &run, &[]), Err(KeepReason::NoRecord));
}

#[test]
fn a_dropped_file_keeps_its_record_for_the_next_change() {
    let mut run = Run::new();
    let store = recorded(&run, &calls_b(), &[]);
    run.edit(
        A,
        "export const a = (x: number): number => (x > 0 ? x + 3 : x - 1);\n",
    );
    assert_eq!(decide(&store, &run, &[]), Ok(()));
    // A second change on top of the first: the record still holds the
    // facts of the recorded run, so the rule compares with them.
    run.edit(
        A,
        "export const a = (x: number): number => (x > 0 ? x + 4 : x - 1);\n",
    );
    assert_eq!(decide(&store, &run, &[]), Ok(()));
    run.edit(
        B,
        "export const b = (x: number): number => x * 5;\nexport function c() { return 1; }\n",
    );
    assert_eq!(
        decide(&store, &run, &[]),
        Err(KeepReason::ChangedEnteredFunction)
    );
}

#[test]
fn prune_keeps_only_the_parts_that_an_entry_names() {
    let run = Run::new();
    let mut store = recorded(&run, &calls_b(), &[]);
    let mut other = calls_b();
    other.files = vec!["src/b.test.ts".to_owned()];
    other.entered = BTreeMap::new();
    other.statics.clear();
    let facts = run.facts(&[]);
    store.record(&facts, &[other], true, |_| ENV.to_owned(), 2);
    store.prune(1);
    assert_eq!(store.tests.len(), 1);
    assert!(store.tests.contains_key("src/b.test.ts"));
    assert_eq!(store.statics.len(), 1);
    assert_eq!(decide(&store, &run, &[]), Err(KeepReason::NoRecord));
}
