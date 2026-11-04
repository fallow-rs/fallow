//! The facts of the dry run that the scheduler reads.

use super::*;

/// The facts of the dry run that the scheduler uses.
pub(super) struct DryRunFacts {
    /// Each passing test that is not in an excluded file, with its duration.
    pub(super) passing: BTreeMap<String, (String, f64)>,
    pub(super) covering: FxHashMap<String, BTreeSet<String>>,
    pub(super) static_ids: FxHashSet<String>,
    pub(super) baseline: Vec<BaselineFailure>,
    /// The setup time of each test file outside its tests.
    pub(super) file_setup: BTreeMap<String, f64>,
}

impl DryRunFacts {
    /// The setup time of the distinct test files of `tests`.
    pub(super) fn setup_of(&self, tests: &[String]) -> f64 {
        let files: BTreeSet<&str> = tests
            .iter()
            .filter_map(|test| self.passing.get(test).map(|(file, _)| file.as_str()))
            .collect();
        files
            .into_iter()
            .filter_map(|file| self.file_setup.get(file))
            .sum()
    }
}

pub(super) fn read_dry_run(
    plan: &SchedulePlan,
    caps: Capabilities,
    result: &DryRunResult,
) -> Result<DryRunFacts, ScheduleError> {
    let reported = result
        .hits
        .values()
        .flatten()
        .chain(&result.static_hits)
        .chain(
            result
                .file_hits
                .iter()
                .flat_map(|hits| hits.values().flatten()),
        );
    for id in reported {
        if !plan.known_ids.contains(id) {
            return Err(ScheduleError::UnknownHit(id.clone()));
        }
    }
    let mut baseline = Vec::new();
    let mut excluded_files = FxHashSet::default();
    let mut global_errors = Vec::new();
    for error in &result.unhandled_errors {
        match &error.file {
            Some(file) => {
                excluded_files.insert(file.clone());
                baseline.push(BaselineFailure {
                    file: Some(file.clone()),
                    test_id: None,
                    message: Some(error.message.clone()),
                    mutant_ids: Vec::new(),
                });
            }
            None => global_errors.push(error.message.clone()),
        }
    }
    if !global_errors.is_empty() {
        return Err(ScheduleError::Baseline(global_errors));
    }
    let mut passing = BTreeMap::new();
    for test in &result.tests {
        match test.status {
            TestStatus::Fail => baseline.push(BaselineFailure {
                file: Some(test.file.clone()),
                test_id: Some(test.test_id.clone()),
                message: test.message.as_deref().and_then(bounded_test_error),
                mutant_ids: Vec::new(),
            }),
            TestStatus::Pass if !excluded_files.contains(&test.file) => {
                passing.insert(
                    test.test_id.clone(),
                    (test.file.clone(), test.duration_ms.unwrap_or(0.0)),
                );
            }
            _ => {}
        }
    }
    baseline.sort();
    let mut covering: FxHashMap<String, BTreeSet<String>> = FxHashMap::default();
    if caps.per_test_attribution {
        for (test, ids) in &result.hits {
            if !passing.contains_key(test) {
                continue;
            }
            for id in ids {
                covering.entry(id.clone()).or_default().insert(test.clone());
            }
        }
    } else if let Some(file_hits) = &result.file_hits {
        for (file, ids) in file_hits {
            let tests: Vec<&String> = passing
                .iter()
                .filter(|(_, (test_file, _))| test_file == file)
                .map(|(test, _)| test)
                .collect();
            for id in ids {
                covering
                    .entry(id.clone())
                    .or_default()
                    .extend(tests.iter().map(|test| (*test).clone()));
            }
        }
    }
    let static_ids = result.static_hits.iter().cloned().collect();
    Ok(DryRunFacts {
        passing,
        covering,
        static_ids,
        baseline,
        file_setup: result.file_setup_ms.clone(),
    })
}

/// The covering tests of `mutant` from the dry run. A static mutant runs
/// each passing test of the dry-run files that load its module, and also the
/// tests that hit it at run time (for example through a dynamic `import()`).
/// A static map miss has no static test file, so it runs each passing
/// dry-run test.
pub(super) fn covering_tests(
    plan: &SchedulePlan,
    facts: &DryRunFacts,
    dry_files: &[String],
    mutant: &ScheduledMutant,
    static_mutant: bool,
    map_miss: bool,
) -> BTreeSet<String> {
    if !static_mutant {
        return facts.covering.get(&mutant.id).cloned().unwrap_or_default();
    }
    let files: FxHashSet<&String> = plan
        .file_test_files
        .get(&mutant.file)
        .into_iter()
        .flatten()
        .filter(|file| dry_files.contains(file))
        .collect();
    let mut tests: BTreeSet<String> = facts
        .passing
        .iter()
        .filter(|(_, (file, _))| map_miss || files.contains(file))
        .map(|(test, _)| test.clone())
        .collect();
    if let Some(runtime) = facts.covering.get(&mutant.id) {
        tests.extend(runtime.iter().cloned());
    }
    tests
}
