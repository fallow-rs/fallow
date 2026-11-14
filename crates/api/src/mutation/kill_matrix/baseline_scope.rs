//! The baseline failures that can affect one `--kill-matrix-for` target.
//!
//! A failing test gives no kill evidence, so it cannot make a target test
//! look redundant. It can share state with a target test, though. A failure
//! therefore affects a target when it shares a test or a test file with the
//! target, or when it covers, kills, or ran without a mutant that a target
//! test covers or kills.

use std::collections::BTreeSet;

use fallow_output::{MutantResult, MutationBaselineFailure};
use rustc_hash::{FxHashMap, FxHashSet};

/// The baseline failures that affect each target.
pub struct Scope<'a> {
    baseline: &'a [MutationBaselineFailure],
    results: &'a [MutantResult],
    test_files: &'a FxHashMap<String, String>,
}

impl<'a> Scope<'a> {
    pub fn new(
        baseline: &'a [MutationBaselineFailure],
        results: &'a [MutantResult],
        test_files: &'a FxHashMap<String, String>,
    ) -> Self {
        Self {
            baseline,
            results,
            test_files,
        }
    }

    /// The names of the failures that affect a target with `tests`, sorted:
    /// the test id, or the file of a file error. `None` when no failure
    /// affects the target.
    pub fn failures(&self, tests: &[String]) -> Option<Vec<String>> {
        if self.baseline.is_empty() {
            return None;
        }
        let tests: FxHashSet<&str> = tests.iter().map(String::as_str).collect();
        let files: FxHashSet<&str> = tests
            .iter()
            .filter_map(|test| self.test_files.get(*test))
            .filter(|file| !file.is_empty())
            .map(String::as_str)
            .collect();
        let mutants: Vec<&MutantResult> = self
            .results
            .iter()
            .filter(|result| touches(result, |test| tests.contains(test)))
            .collect();
        let mut names = BTreeSet::new();
        let mut affected = false;
        for failure in self.baseline {
            let shares_file = failure
                .file
                .as_deref()
                .is_some_and(|file| files.contains(file));
            // An error with no test and no file can come from each file.
            let unscoped = failure.test_id.is_none() && failure.file.is_none();
            let shares_mutant = failure.test_id.as_deref().is_some_and(|id| {
                tests.contains(id)
                    || mutants.iter().any(|result| {
                        touches(result, |test| test == id)
                            || failure.mutant_ids.contains(&result.location.mutant_id)
                    })
            });
            if !(shares_file || unscoped || shares_mutant) {
                continue;
            }
            affected = true;
            names.extend(failure.test_id.as_ref().or(failure.file.as_ref()).cloned());
        }
        affected.then(|| names.into_iter().collect())
    }
}

/// True when a covering or killing test of `result` matches `test`.
fn touches(result: &MutantResult, test: impl Fn(&str) -> bool) -> bool {
    result
        .covering_tests
        .iter()
        .chain(&result.killing_tests)
        .any(|id| test(id))
}
