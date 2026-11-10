//! The merge of a `--recheck` run with the earlier report. The run plans
//! only the files of the selected mutants, and it prunes each other mutant
//! of those files as `not-selected`. The merged report gives such a mutant
//! its earlier result, and it keeps the mutants of each file outside the
//! run. Then the summary describes the full set of the earlier report.
//!
//! An earlier result applies only while the file has the digest of the
//! earlier report. A file whose source changed loses its earlier results.
//! An earlier kill also needs each test file of its killing tests. The merge
//! does not run a carried mutant again, so it cannot see a test that a test
//! file lost or that a change made weaker.

use std::collections::BTreeSet;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;

use crate::mutation_cli::tasks::test_file_of;

use fallow_output::{
    MutantResult, MutantStatus, MutationFunctionResult, MutationPlanFile, MutationReport,
    MutationStatusCounts, MutationSummary,
};

use crate::mutation_inspect::{report_relative_path, sha256_hex};

/// The prune reason of a mutant that the recheck did not select.
const NOT_SELECTED: &str = "not-selected";

/// The parts of the earlier report that the merge reads. Each mutant and
/// function parses on its own, so one entry of an older format does not
/// stop the merge of the others.
#[derive(Debug, Default, Deserialize)]
pub struct Previous {
    #[serde(default)]
    mutants: Vec<serde_json::Value>,
    #[serde(default)]
    functions: Vec<serde_json::Value>,
    #[serde(default)]
    files: Vec<PreviousFile>,
}

/// One file of the earlier report. A report of an older version can lack
/// `static_test_files`.
#[derive(Debug, Deserialize)]
struct PreviousFile {
    path: String,
    source_sha256: String,
    #[serde(default)]
    static_test_files: Vec<String>,
}

impl Previous {
    /// Parses the merge parts of the earlier report. A report that does not
    /// parse gives an empty merge, so the run report stays as it is.
    pub fn parse(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_default()
    }
}

/// What the merge did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Merged {
    /// The mutants with a result from the earlier report.
    pub carried: usize,
    /// The files outside the run whose earlier results do not apply,
    /// sorted: the source changed, or the earlier report has no digest or
    /// no readable entry for the file.
    pub stale_files: Vec<String>,
}

/// Merges `previous` into the run `report` of a recheck. `selected` holds
/// the ids that the recheck ran, and `root` is the project root of the
/// report paths. The summary and the function results then count the merged
/// mutants. The kill matrix stays the evidence of this run.
pub fn merge(
    report: &mut MutationReport,
    previous: &Previous,
    selected: &FxHashSet<&str>,
    root: &Path,
) -> Merged {
    let Earlier {
        mut inside,
        outside,
        outside_files,
        stale_files,
    } = sort_earlier(report, previous, selected, root);
    let mut carried = 0;
    for index in 0..report.mutants.len() {
        let mutant = &report.mutants[index];
        if !is_not_selected(mutant) {
            continue;
        }
        let Some(earlier) = inside.remove(&mutant.location.mutant_id) else {
            continue;
        };
        move_in_function(&mut report.functions, mutant, earlier.status);
        report.mutants[index] = earlier;
        carried += 1;
    }
    carried += outside.len();
    add_outside_files(report, previous, outside, &outside_files);
    report.summary = MutationSummary::from_results(&report.mutants);
    Merged {
        carried,
        stale_files,
    }
}

/// Whether `mutant` is `pruned` because a recheck did not select it. Such
/// an entry of the earlier report has no result to give.
fn is_not_selected(mutant: &MutantResult) -> bool {
    mutant.status == MutantStatus::Pruned && mutant.prune_reason.as_deref() == Some(NOT_SELECTED)
}

/// The earlier results that apply to the merged report.
struct Earlier {
    /// The results of the not selected mutants of the run files, by id.
    inside: FxHashMap<String, MutantResult>,
    /// The results of the mutants of the unchanged files outside the run.
    outside: Vec<MutantResult>,
    /// The unchanged files outside the run.
    outside_files: BTreeSet<String>,
    /// The files outside the run whose earlier results do not apply, sorted.
    stale_files: Vec<String>,
}

/// Sorts the not selected mutants of `previous` by their file: a file of the
/// run with the digest of the earlier report, an unchanged file outside the
/// run, or a stale file.
fn sort_earlier(
    report: &MutationReport,
    previous: &Previous,
    selected: &FxHashSet<&str>,
    root: &Path,
) -> Earlier {
    let earlier_digest: FxHashMap<&str, &str> = previous
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.source_sha256.as_str()))
        .collect();
    let run_digest: FxHashMap<&str, &str> = report
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.source_sha256.as_str()))
        .collect();
    let static_tests: FxHashMap<&str, &[String]> = previous
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.static_test_files.as_slice()))
        .collect();
    let (parsed, unreadable) = parse_mutants(previous);
    let mut inside = FxHashMap::default();
    let mut outside = Vec::new();
    let mut applies_by_file: FxHashMap<String, bool> = FxHashMap::default();
    for mut mutant in parsed {
        if selected.contains(mutant.location.mutant_id.as_str()) {
            continue;
        }
        let file = mutant.location.file.as_str();
        let digest = earlier_digest.get(file).copied();
        let tests = static_tests.get(file).copied().unwrap_or_default();
        let kill_holds = killing_test_files_exist(root, &mutant, tests);
        mutant.carried = true;
        if let Some(&current) = run_digest.get(file) {
            if digest == Some(current) && kill_holds && !is_not_selected(&mutant) {
                inside.insert(mutant.location.mutant_id.clone(), mutant);
            }
            continue;
        }
        let applies = applies_by_file
            .entry(file.to_owned())
            .or_insert_with_key(|file| {
                !unreadable.contains(file)
                    && digest.is_some()
                    && current_digest(root, file).as_deref() == digest
            });
        *applies &= kill_holds;
        if *applies {
            outside.push(mutant);
        }
    }
    outside.retain(|mutant| {
        applies_by_file
            .get(&mutant.location.file)
            .copied()
            .unwrap_or_default()
    });
    let mut outside_files = BTreeSet::new();
    let mut stale_files = BTreeSet::new();
    for (file, applies) in applies_by_file {
        if applies {
            outside_files.insert(file);
        } else {
            stale_files.insert(file);
        }
    }
    Earlier {
        inside,
        outside,
        outside_files,
        stale_files: stale_files.into_iter().collect(),
    }
}

/// The mutants of `previous` that parse, and the files with an entry that
/// does not parse. Such a file keeps no earlier result outside the run, so
/// the report never shows a part of the file.
fn parse_mutants(previous: &Previous) -> (Vec<MutantResult>, FxHashSet<String>) {
    let mut parsed = Vec::new();
    let mut unreadable = FxHashSet::default();
    for value in &previous.mutants {
        match MutantResult::deserialize(value) {
            Ok(mutant) => parsed.push(mutant),
            Err(_) => {
                if let Some(file) = value.get("file").and_then(serde_json::Value::as_str) {
                    unreadable.insert(file.to_owned());
                }
            }
        }
    }
    (parsed, unreadable)
}

/// Adds the earlier mutants, function results and digests of the unchanged
/// files outside the run to `report`.
fn add_outside_files(
    report: &mut MutationReport,
    previous: &Previous,
    outside: Vec<MutantResult>,
    outside_files: &BTreeSet<String>,
) {
    let start = report.functions.len();
    report.functions.extend(
        previous
            .functions
            .iter()
            .filter_map(|value| MutationFunctionResult::deserialize(value).ok())
            .filter(|function| outside_files.contains(&function.file)),
    );
    recount_functions(&mut report.functions[start..], &outside);
    report
        .functions
        .sort_by(|left, right| (&left.file, left.line).cmp(&(&right.file, right.line)));
    report.files.extend(
        previous
            .files
            .iter()
            .filter(|file| outside_files.contains(&file.path))
            .map(|file| MutationPlanFile {
                path: file.path.clone(),
                source_sha256: file.source_sha256.clone(),
                static_test_files: file.static_test_files.clone(),
            }),
    );
    report
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    report.mutants.extend(outside);
}

/// Whether each test file of the killing tests of a `killed` or `timeout`
/// mutant still exists under `root`. A test id without a known test file
/// does not count. `static_test_files` are the static test files of the
/// file of the mutant.
fn killing_test_files_exist(
    root: &Path,
    mutant: &MutantResult,
    static_test_files: &[String],
) -> bool {
    if !matches!(mutant.status, MutantStatus::Killed | MutantStatus::Timeout) {
        return true;
    }
    mutant
        .killed_by
        .iter()
        .chain(&mutant.killing_tests)
        .filter_map(|id| test_file_of(id, static_test_files))
        .all(|file| report_relative_path(&file).is_ok_and(|relative| root.join(relative).is_file()))
}

/// The current SHA-256 digest of the report path `file`. `None` for a path
/// that leaves the root or a file that cannot be read.
fn current_digest(root: &Path, file: &str) -> Option<String> {
    let relative = report_relative_path(file).ok()?;
    std::fs::read(root.join(relative))
        .ok()
        .map(|bytes| sha256_hex(&bytes))
}

/// Counts the earlier function results of the files outside the run again
/// from their carried `mutants`, so each count agrees with the mutants of
/// the report.
fn recount_functions(functions: &mut [MutationFunctionResult], mutants: &[MutantResult]) {
    for function in functions.iter_mut() {
        function.counts = MutationStatusCounts::default();
        function.survived.clear();
    }
    for mutant in mutants {
        let location = &mutant.location;
        if let Some(group) = functions.iter_mut().find(|group| in_group(group, mutant)) {
            group.counts.add(mutant.status);
            if mutant.status == MutantStatus::Survived {
                group.survived.push(location.mutant_id.clone());
            }
        }
    }
    for function in functions.iter_mut() {
        function.score = function.counts.score();
    }
}

/// Whether `mutant` is in the function `group`: the same file, first line
/// and function name.
fn in_group(group: &MutationFunctionResult, mutant: &MutantResult) -> bool {
    let location = &mutant.location;
    (group.file.as_str(), group.line, group.name.as_deref())
        == (
            location.file.as_str(),
            location.function_line,
            location.function.as_deref(),
        )
}

/// Moves one `not-selected` mutant of its function group to `status`. The
/// group of a mutant has its file, the first line and the name of its
/// function. Two functions with the same name on one line share that key,
/// so the move takes the first such group that still counts a pruned
/// mutant.
fn move_in_function(
    functions: &mut [MutationFunctionResult],
    mutant: &MutantResult,
    status: MutantStatus,
) {
    let Some(group) = functions
        .iter_mut()
        .find(|group| in_group(group, mutant) && group.counts.pruned > 0)
    else {
        return;
    };
    group.counts.remove(MutantStatus::Pruned);
    group.counts.add(status);
    if status == MutantStatus::Survived {
        group.survived.push(mutant.location.mutant_id.clone());
    }
    group.score = group.counts.score();
}
