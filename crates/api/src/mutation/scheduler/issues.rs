//! The issues and messages that the scheduler builds for a mutant or a run.

use super::{ScheduleIssue, ScheduledMutant};

/// The issue of a mutant that the dry run hit, while the static map reaches
/// no test of its file.
pub(super) fn static_map_miss_issue(mutant: &ScheduledMutant) -> ScheduleIssue {
    ScheduleIssue {
        code: "static-map-miss",
        message: format!(
            "the dry run hit this mutant in `{}`, but the static map reaches no test of that file; the module graph misses a test that loads it",
            mutant.file
        ),
        mutant_id: Some(mutant.id.clone()),
        stderr_tail: None,
    }
}

/// The longest test name that [`no_covering_test_message`] shows.
const MAX_TEST_NAME_CHARS: usize = 60;
const ELLIPSIS: &str = "...";

/// The `runtime_error` message for a mutant with no covering test left after
/// the dry run of a respawned companion. It gives the count and the first
/// test only: `baseline_failures` holds the full list. The message leaves out
/// the mutant id because the issue carries it.
pub(super) fn no_covering_test_message(dropped: &[String]) -> String {
    let first = dropped.first().map_or_else(String::new, |name| {
        if name.chars().count() <= MAX_TEST_NAME_CHARS {
            return name.clone();
        }
        let kept: String = name
            .chars()
            .take(MAX_TEST_NAME_CHARS - ELLIPSIS.len())
            .collect();
        format!("{kept}{ELLIPSIS}")
    });
    let tests = if dropped.len() == 1 {
        format!("1 test: {first}")
    } else {
        format!("{} tests, first: {first}", dropped.len())
    };
    format!("no covering test passed in the dry run of the restarted companion ({tests})")
}

/// The issue for static test files that the runner does not list. Their
/// tests never run, so a mutant that only they reach stays `no_coverage`.
pub(super) fn undiscovered_issue(files: &[String]) -> Option<ScheduleIssue> {
    (!files.is_empty()).then(|| ScheduleIssue {
        code: "test-files-not-discovered",
        message: {
            let (noun, pronoun) = if files.len() == 1 {
                ("file", "it")
            } else {
                ("files", "they")
            };
            format!(
                "the runner does not list {} static test {noun}, so {pronoun} did not run: {}",
                files.len(),
                files.join(", ")
            )
        },
        mutant_id: None,
        stderr_tail: None,
    })
}
