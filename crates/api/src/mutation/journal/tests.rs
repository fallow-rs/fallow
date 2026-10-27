use super::*;
use crate::mutation::result_cache::KeyFiles;

fn header() -> JournalHeader {
    JournalHeader {
        journal_format: JOURNAL_FORMAT,
        fallow_version: "2.0.0".to_owned(),
        runner: "vitest".to_owned(),
        runtime: "node 24.1.0".to_owned(),
        run_dir: "0123456789abcdef0123".to_owned(),
        runner_config: None,
        scope: "all".to_owned(),
        operators: None,
        allow_side_effects: false,
    }
}

fn keys(dir: &Path) -> Arc<ResultCache> {
    Arc::new(ResultCache::keys_only(
        dir,
        dir,
        "vitest",
        KeyFiles::default(),
    ))
}

fn open(dir: &Path, header: &JournalHeader, resume: bool) -> Result<Opened, JournalMismatch> {
    Journal::open(dir, header, keys(dir), resume)
}

fn killed(test: &str) -> CachedResult {
    CachedResult {
        status: MutantStatus::Killed,
        killed_by: Some(test.to_owned()),
        duration_ms: Some(5),
    }
}

fn lines(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(Journal::path(dir))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_run_without_resume_starts_a_new_journal_and_appends_each_result() {
    let dir = tempfile::tempdir().unwrap();
    let opened = open(dir.path(), &header(), false).unwrap();
    opened.journal.append("m1", "k1", &killed("t1"));
    assert!(opened.note.is_none());
    assert_eq!(lines(dir.path()).len(), 2);

    let next = open(dir.path(), &header(), false).unwrap();
    assert_eq!(
        lines(dir.path()),
        vec![serde_json::to_string(&header()).unwrap()],
        "a run without --resume replaces the journal"
    );
    assert_eq!(next.journal.resumed("m1", "k1"), None);
}

#[test]
fn a_resume_reads_the_final_results_and_ignores_a_cut_last_line() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(dir.path(), &header(), false).unwrap();
    first.journal.append("m1", "k1", &killed("t1"));
    first.journal.append(
        "m2",
        "k2",
        &CachedResult {
            status: MutantStatus::Survived,
            killed_by: None,
            duration_ms: Some(9),
        },
    );
    first.journal.append(
        "m3",
        "k3",
        &CachedResult {
            status: MutantStatus::Timeout,
            killed_by: None,
            duration_ms: None,
        },
    );
    drop(first);
    let mut file = OpenOptions::new()
        .append(true)
        .open(Journal::path(dir.path()))
        .unwrap();
    file.write_all(br#"{"id":"m4","key":"k4","sta"#).unwrap();
    drop(file);

    let resumed = open(dir.path(), &header(), true).unwrap();
    assert!(resumed.note.is_none());
    let journal = resumed.journal;
    assert_eq!(journal.resumed("m1", "k1"), Some(killed("t1")));
    assert_eq!(
        journal.resumed("m2", "k2").map(|result| result.status),
        Some(MutantStatus::Survived)
    );
    assert_eq!(journal.resumed("m3", "k3"), None, "a timeout is not final");
    assert_eq!(journal.resumed("m4", "k4"), None, "a cut line is ignored");

    journal.append("m5", "k5", &killed("t5"));
    let after = lines(dir.path());
    assert_eq!(after.len(), 4, "{after:#?}");
    for line in &after[1..] {
        assert!(serde_json::from_str::<Line>(line).is_ok(), "{line}");
    }
}

#[test]
fn a_changed_key_is_rejected_and_counted_once() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(dir.path(), &header(), false).unwrap();
    first.journal.append("m1", "k1", &killed("t1"));
    drop(first);

    let journal = open(dir.path(), &header(), true).unwrap().journal;
    assert_eq!(journal.resumed("m1", "another-key"), None);
    assert_eq!(journal.resumed("m1", "another-key"), None);
    assert_eq!(journal.rejected(), 1);
    assert_eq!(journal.resumed("m1", "k1"), Some(killed("t1")));
}

#[test]
fn a_resume_with_another_fingerprint_names_each_input_that_differs() {
    let dir = tempfile::tempdir().unwrap();
    drop(open(dir.path(), &header(), false).unwrap());

    let mut changed = header();
    changed.runtime = "node 22.18.0".to_owned();
    changed.run_dir = "fedcba9876543210fedc".to_owned();
    changed.operators = Some(vec!["arithmetic-operator".to_owned()]);
    let Err(mismatch) = open(dir.path(), &changed, true) else {
        panic!("a different fingerprint is refused");
    };
    assert_eq!(
        mismatch.differences,
        vec![
            "runtime (journal: node 24.1.0, this run: node 22.18.0)".to_owned(),
            "planned source and instrumentation (journal: 0123456789ab, this run: fedcba987654)"
                .to_owned(),
            "operators (journal: each operator, this run: arithmetic-operator)".to_owned(),
        ]
    );
    assert_eq!(
        lines(dir.path()),
        vec![serde_json::to_string(&header()).unwrap()],
        "a refused resume keeps the journal"
    );
}

#[test]
fn a_resume_without_a_usable_journal_starts_from_the_beginning_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let opened = open(dir.path(), &header(), true).unwrap();
    assert_eq!(
        opened.note.as_deref(),
        Some("the run started from the beginning, because no journal exists")
    );
    assert_eq!(lines(dir.path()).len(), 1, "the run starts its own journal");

    std::fs::write(Journal::path(dir.path()), "{\"journal_for").unwrap();
    let opened = open(dir.path(), &header(), true).unwrap();
    assert_eq!(
        opened.note.as_deref(),
        Some("the run started from the beginning, because the journal has no valid header")
    );
}

#[test]
fn a_failed_write_is_reported_and_never_stops_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("file");
    std::fs::write(&blocked, "").unwrap();
    let opened = open(&blocked, &header(), false).unwrap();
    opened.journal.append("m1", "k1", &killed("t1"));
    assert!(
        opened
            .journal
            .write_error()
            .is_some_and(|error| error.contains("cannot write")),
        "{:?}",
        opened.journal.write_error()
    );
}
