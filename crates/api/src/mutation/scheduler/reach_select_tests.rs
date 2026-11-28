//! The reach selection of a run: probe ids, the dropped files, and the
//! check run.

use super::shard_tests::{FILES, dry_of, dry_runs, shard_plan, shard_spawner, verdicts};
use super::*;

const PROBE_ONLY: &str = "src/probe-only.ts";

/// A lookup that drops the files of its list.
#[derive(Debug)]
struct Drops(Vec<&'static str>);

impl ReachLookup for Drops {
    fn select(&self, _: &Ready, dry_files: &[String]) -> ReachSelection {
        ReachSelection {
            disabled: None,
            dropped: dry_files
                .iter()
                .filter(|file| self.0.contains(&file.as_str()))
                .cloned()
                .collect(),
            kept: BTreeMap::new(),
        }
    }
}

/// A store with no stored facts, so each file dry-runs and gives an entry.
#[derive(Debug, Default)]
struct Keeps;

impl DryRunLookup for Keeps {
    fn key(&self, _: &Ready, test_file: &str) -> String {
        test_file.to_owned()
    }

    fn get(&self, _: &str, _: &str) -> Option<StoredDryRun> {
        None
    }
}

/// The dry run of `files` with probe ids: each test enters function 0 of
/// the probe-only file, and a module load outside each test gives its
/// module probe.
fn probed_dry(files: &[String]) -> DryRunResult {
    let mut dry = dry_of(files, None);
    for hits in dry.hits.values_mut() {
        hits.push("rkey.0".to_owned());
    }
    dry.static_hits.push("rkey.m".to_owned());
    dry.unloaded_paths.push(PROBE_ONLY.to_owned());
    dry
}

fn probe_spawner() -> FakeSpawner {
    let plain = shard_spawner(None);
    FakeSpawner::new(move |index, request| match request {
        Request::DryRun { test_files } => Ok(Response::DryRun(probed_dry(test_files))),
        other => (plain.script)(index, other),
    })
}

fn reach_config(dropped: Vec<&'static str>, check: bool) -> ScheduleConfig {
    let mut config = config(1);
    config.reach = Some(ReachConfig {
        lookup: Arc::new(Drops(dropped)),
        check,
        probe_only: std::iter::once(PROBE_ONLY.to_owned()).collect(),
    });
    config.init.path_map = ["src/a.ts", PROBE_ONLY]
        .into_iter()
        .map(|path| (path.to_owned(), format!("run/{path}")))
        .collect();
    config
}

fn reach_plan(ids: &[&str]) -> SchedulePlan {
    SchedulePlan {
        reaching_test_files: BTreeMap::from([(
            "src/a.ts".to_owned(),
            vec!["test/a.test.ts".to_owned(), "test/b.test.ts".to_owned()],
        )]),
        ..shard_plan(ids)
    }
}

const IDS: [&str; 4] = ["m0", "m1", "m2", "m3"];

#[test]
fn probe_ids_leave_the_mutant_facts_as_without_probes() {
    let plain = run_schedule(&shard_spawner(None), &reach_plan(&IDS), &config(1)).unwrap();
    let probed = run_schedule(
        &probe_spawner(),
        &reach_plan(&IDS),
        &reach_config(vec![], false),
    )
    .expect("a probe id is no unknown hit");
    assert_eq!(verdicts(&probed), verdicts(&plain));
    assert!(!probed.unloaded_paths.iter().any(|path| path == PROBE_ONLY));
    let reach = probed.reach.expect("reach outcome");
    let share = &reach.shares[0];
    assert_eq!(share.statics, BTreeSet::from(["rkey.m".to_owned()]));
    assert!(share.entered["test/a.test.ts"].contains("rkey.0"));
    assert!(share.failed.is_empty());
}

#[test]
fn a_dropped_file_does_not_dry_run_and_a_check_run_keeps_it() {
    let dry_files = |check: bool| {
        let spawner = probe_spawner();
        let outcome = run_schedule(
            &spawner,
            &reach_plan(&IDS),
            &reach_config(vec!["test/b.test.ts"], check),
        )
        .unwrap();
        let files: Vec<String> = dry_runs(&spawner)
            .into_iter()
            .flat_map(|(_, files)| files)
            .collect();
        (files, outcome)
    };
    let (narrowed, outcome) = dry_files(false);
    assert!(!narrowed.contains(&"test/b.test.ts".to_owned()));
    assert_eq!(
        outcome.reach.unwrap().selection.dropped,
        vec!["test/b.test.ts".to_owned()]
    );
    let (checked, outcome) = dry_files(true);
    assert_eq!(checked.len(), FILES.len());
    assert!(outcome.reach.unwrap().check);
}

#[test]
fn a_probe_only_file_is_no_loaded_path_of_a_stored_entry() {
    let mut config = reach_config(vec![], false);
    config.dry_runs = Some(Arc::new(Keeps) as Arc<dyn DryRunLookup>);
    let outcome = run_schedule(&probe_spawner(), &reach_plan(&IDS), &config).unwrap();
    assert_eq!(outcome.dry_run_entries.len(), FILES.len());
    for entry in &outcome.dry_run_entries {
        assert!(
            !entry
                .facts
                .loaded_paths
                .iter()
                .any(|path| path == PROBE_ONLY),
            "{entry:?}"
        );
    }
    // `c` reaches no source file of the plan, so no fact binds it to the
    // other files.
    let c = outcome
        .dry_run_entries
        .iter()
        .find(|entry| entry.test_file == "test/c.test.ts")
        .unwrap();
    assert!(c.facts.bound.is_empty(), "{c:?}");
}
