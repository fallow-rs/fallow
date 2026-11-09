//! Platform oracle: the platform form of a checkout must not change findings.
//!
//! A Windows checkout differs from a Unix checkout in two ways. The files can
//! have CRLF line endings, and each path has backslash separators below a
//! drive root. Findings, finding ids, clone fingerprints and baselines must
//! stay the same in each form, so that a baseline that a Windows developer
//! saves matches the findings of a Linux CI runner.
//!
//! - `oracle` runs the CLI on a generated project with LF line endings and on
//!   the same project with CRLF or mixed line endings. The outputs of
//!   `dead-code`, `dupes`, `health` and `audit` must be equal, and a baseline
//!   that one form saves must give the same result in the other form.
//! - `seams` and `health_rows` feed backslash and drive-root paths into the
//!   code that turns a path into a key: display paths, finding ids, baseline
//!   keys and grouped health rows. A test host on macOS or Linux cannot change
//!   the real path separator, so these tests call that code directly.
//!
//! Pull request CI runs fixed seeds. The daily `Oracle Seeds` workflow runs
//! `oracle::seed_sweep_keeps_findings_identical` over
//! `FALLOW_PLATFORM_SEEDS` project seeds from `FALLOW_PLATFORM_SEED_START`.

use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

mod health_rows;
mod oracle;
mod project;
mod seams;

/// The seam tests run this fixed seed, so pull request CI checks the same
/// cases on each run.
const SEED: u64 = 0x0050_1A7F_0000_0001;
const CASES: u32 = 64;

/// The proptest settings of the seam tests.
pub fn proptest_config() -> ProptestConfig {
    ProptestConfig {
        cases: CASES,
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
        ..ProptestConfig::default()
    }
}

/// The path segments below the root, ending in a source file.
pub fn tail() -> impl Strategy<Value = Vec<String>> {
    (
        prop::collection::vec("[a-z][a-z0-9_-]{0,6}", 0..4),
        "[a-z][a-z0-9_-]{0,6}",
        prop_oneof![Just("ts"), Just("tsx"), Just("js"), Just("vue")],
    )
        .prop_map(|(mut dirs, stem, extension)| {
            dirs.push(format!("{stem}.{extension}"));
            dirs
        })
}
