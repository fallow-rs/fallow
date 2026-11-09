//! Feed Windows path forms and CRLF text into the code that turns a path or a
//! fragment into a key: display paths, finding ids, and baselines.
//!
//! On Windows the walker joins each discovered path onto the project root,
//! so a finding path is `<root>\src\a.ts` and the part below the root uses
//! backslashes. A test host on macOS or Linux splits a path only on `/`. The
//! tests model the Windows form as a root that the host can split (`/work`
//! or the drive root `C:/work`) and a tail below it with backslashes. Each key
//! must be the same as for the tail with forward slashes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fallow_engine::baseline::{
    BaselineData, DeadCodeBaselineOutcome, DuplicationBaselineData, apply_dead_code_baseline,
    filter_new_clone_groups,
};
use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationReport};
use fallow_types::identity::stamp_dead_code_finding_ids;
use fallow_types::output_dead_code::{UnusedExportFinding, UnusedFileFinding};
use fallow_types::path_util::display_relative;
use fallow_types::results::{AnalysisResults, UnusedExport, UnusedFile};
use fallow_types::semantic::SemanticAnalysisIdentity;
use proptest::prelude::*;

use super::{proptest_config, tail};

/// The project roots that the host can split: a POSIX root and a drive root.
fn root() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("/work/repo".to_owned()),
        Just("C:/work/repo".to_owned())
    ]
}

/// `root` joined with `segments`, with `/` or with `\` below the root.
fn path_in_form(root: &str, segments: &[String], backslash: bool) -> PathBuf {
    let separator = if backslash { "\\" } else { "/" };
    PathBuf::from(format!("{root}/{}", segments.join(separator)))
}

fn dead_code_results(root: &str, files: &[Vec<String>], backslash: bool) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    for (index, segments) in files.iter().enumerate() {
        let path = path_in_form(root, segments, backslash);
        if index % 2 == 0 {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile { path }));
        } else {
            results
                .unused_exports
                .push(UnusedExportFinding::with_actions(UnusedExport {
                    path,
                    export_name: format!("unused{index}"),
                    is_type_only: false,
                    line: 1,
                    col: 13,
                    span_start: 13,
                    is_re_export: false,
                    deprecated: false,
                    deprecated_reason: None,
                }));
        }
    }
    results
}

fn finding_ids(results: &AnalysisResults) -> Vec<Option<String>> {
    results
        .unused_files
        .iter()
        .map(|finding| finding.finding_id.clone())
        .chain(
            results
                .unused_exports
                .iter()
                .map(|finding| finding.finding_id.clone()),
        )
        .collect()
}

fn dead_code_baseline(results: &AnalysisResults, root: &Path) -> String {
    serde_json::to_string(&BaselineData::from_results_with_identity(
        results,
        root,
        SemanticAnalysisIdentity::syntactic(),
    ))
    .expect("serialize baseline")
}

/// Apply `baseline` to `results` and return the number of findings left.
fn left_after_baseline(mut results: AnalysisResults, baseline: &str, root: &Path) -> usize {
    let outcome = apply_dead_code_baseline(
        &mut results,
        baseline,
        root,
        &SemanticAnalysisIdentity::syntactic(),
        false,
    )
    .expect("apply baseline");
    assert!(
        matches!(outcome, DeadCodeBaselineOutcome::Applied { .. }),
        "the file is not a dead-code baseline"
    );
    results.total_issues()
}

/// A function that the clone detector tokenizes, `lines` statements long.
fn clone_fragment(lines: usize, crlf: bool) -> String {
    let mut text =
        String::from("export function copy(items: number[]): number {\n  let sum = 0;\n");
    for line in 0..lines {
        let _ = writeln!(text, "  sum += items[{line}] * {line};");
    }
    text.push_str("  return sum;\n}");
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

fn clone_report(
    root: &str,
    files: &[Vec<String>],
    backslash: bool,
    lines: usize,
    crlf: bool,
) -> DuplicationReport {
    let instances = files
        .iter()
        .map(|segments| CloneInstance {
            file: path_in_form(root, segments, backslash),
            start_line: 1,
            end_line: lines + 4,
            start_col: 0,
            end_col: 1,
            fragment: clone_fragment(lines, crlf),
            is_symlink: false,
        })
        .collect();
    DuplicationReport {
        clone_groups: vec![CloneGroup {
            instances,
            token_count: 20 + lines * 9,
            line_count: lines + 4,
            similarity: None,
        }],
        ..DuplicationReport::default()
    }
}

/// Distinct file paths, so the findings and the clone instances do not merge.
fn distinct_files(range: std::ops::Range<usize>) -> impl Strategy<Value = Vec<Vec<String>>> {
    prop::collection::vec(tail(), range).prop_map(|mut files| {
        files.sort();
        files.dedup();
        files
    })
}

proptest! {
    #![proptest_config(proptest_config())]

    /// Each report shows a root-relative path with `/`.
    #[test]
    fn display_paths_ignore_the_separator(root in root(), segments in tail()) {
        let expected = segments.join("/");
        for backslash in [false, true] {
            let path = path_in_form(&root, &segments, backslash);
            prop_assert_eq!(display_relative(Path::new(&root), &path), expected.clone());
        }
    }

    /// Finding ids, baseline files and the baseline filter of `dead-code`
    /// give the same result for each separator form.
    #[test]
    fn dead_code_ids_and_baselines_ignore_the_separator(
        root in root(),
        files in distinct_files(1..6),
    ) {
        let root_path = Path::new(&root);
        let mut slash = dead_code_results(&root, &files, false);
        let mut backslash = dead_code_results(&root, &files, true);
        stamp_dead_code_finding_ids(&mut slash, root_path);
        stamp_dead_code_finding_ids(&mut backslash, root_path);
        prop_assert_eq!(finding_ids(&slash), finding_ids(&backslash));

        let slash_baseline = dead_code_baseline(&slash, root_path);
        let backslash_baseline = dead_code_baseline(&backslash, root_path);
        prop_assert_eq!(&slash_baseline, &backslash_baseline);
        prop_assert_eq!(left_after_baseline(backslash, &slash_baseline, root_path), 0);
        prop_assert_eq!(left_after_baseline(slash, &backslash_baseline, root_path), 0);
    }

    /// A `dupes` baseline that one form saves is the same file as the
    /// baseline of the other form, and it removes each clone group of the
    /// other form.
    #[test]
    fn dupes_baselines_ignore_the_separator_and_line_endings(
        root in root(),
        files in distinct_files(2..5),
        lines in 3usize..8,
    ) {
        prop_assume!(files.len() >= 2);
        let root_path = Path::new(&root);
        let unix = clone_report(&root, &files, false, lines, false);
        let windows = clone_report(&root, &files, true, lines, true);
        let unix_baseline = DuplicationBaselineData::from_report(&unix, root_path);
        let windows_baseline = DuplicationBaselineData::from_report(&windows, root_path);
        prop_assert_eq!(
            serde_json::to_string(&unix_baseline).expect("serialize baseline"),
            serde_json::to_string(&windows_baseline).expect("serialize baseline")
        );
        let left = filter_new_clone_groups(windows, &unix_baseline, root_path);
        prop_assert!(left.clone_groups.is_empty(), "{} clone groups stay", left.clone_groups.len());
        let left = filter_new_clone_groups(unix, &windows_baseline, root_path);
        prop_assert!(left.clone_groups.is_empty(), "{} clone groups stay", left.clone_groups.len());
    }
}

/// The CodeClimate fingerprint of a clone instance hashes the first 64
/// characters of the fragment without `\r`, so the same clone keeps its
/// fingerprint with CRLF line endings. The input is one clone group of two
/// instances whose fragment has a line break in the first 64 characters,
/// once with `\n` and once with `\r\n`.
#[test]
fn codeclimate_clone_fingerprints_ignore_line_endings() {
    let files = vec![vec!["a.ts".to_owned()], vec!["b.ts".to_owned()]];
    let fingerprints = |crlf: bool| -> Vec<String> {
        let report = clone_report("/work/repo", &files, false, 3, crlf);
        fallow_api::build_duplication_codeclimate(&report, Path::new("/work/repo"))
            .into_iter()
            .map(|issue| issue.fingerprint)
            .collect()
    };
    assert_eq!(fingerprints(false), fingerprints(true));
}
