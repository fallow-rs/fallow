//! Cooperative cancellation of the programmatic analysis path.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{
    AnalysisOptions, CombinedOptions, DeadCodeOptions, FeatureFlagsOptions, ProgrammaticError,
};

use super::{combined::run_combined, dead_code::run_dead_code, feature_flags::run_feature_flags};

/// Files per generated project. Large enough that one full analysis takes long
/// enough to cancel a measurable way into it, small enough to stay a unit test.
const GENERATED_FILES: usize = 400;

fn generated_project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"api-cancellation","main":"src/index.ts"}"#,
    )
    .expect("package.json");

    let mut index = String::new();
    for module in 0..GENERATED_FILES {
        let _ = writeln!(index, "import './mod{module}';");
        let mut source = String::new();
        let _ = writeln!(
            source,
            "import {{ helper0 as upstream }} from './mod{}';",
            module.saturating_sub(1)
        );
        for symbol in 0..20 {
            let _ = writeln!(
                source,
                "export const helper{symbol} = (input: number): number => {{\n  \
                 if (input > {symbol}) {{\n    return input * {symbol} + upstream;\n  }}\n  \
                 return input - {symbol};\n}};"
            );
        }
        std::fs::write(src.join(format!("mod{module}.ts")), source).expect("module");
    }
    index.push_str("export const entry = 1;\nconsole.log(entry);\n");
    std::fs::write(src.join("index.ts"), index).expect("entry");
    project
}

fn uncached_analysis(root: &Path, cancellation: Option<Arc<AtomicBool>>) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        // Both timed runs must do the same work: a shared on-disk parse cache
        // would make whichever run comes second look artificially fast.
        no_cache: true,
        cancellation,
        ..AnalysisOptions::default()
    }
}

fn dead_code_options(root: &Path, cancellation: Option<Arc<AtomicBool>>) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: uncached_analysis(root, cancellation),
        ..DeadCodeOptions::default()
    }
}

/// What a run that never started reports. Anything else means the analysis was
/// already under way when the token flipped.
const ENTRY_GUARD_STOP: &str = "analysis was cancelled before config load and file discovery";

fn assert_cancelled(error: &ProgrammaticError) {
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_CANCELLED"),
        "cancellation must carry its own code, got {error:?}"
    );
    assert_eq!(error.exit_code, 2);
}

/// Cancelling before the call short-circuits it. The assertion that matters is
/// the shape of the result: an empty report would read downstream as a project
/// with nothing wrong with it.
#[test]
fn a_cancelled_dead_code_run_fails_instead_of_reporting_an_empty_project() {
    let project = super::tests::dead_code_project();
    let root = project.path();

    let completed = run_dead_code(&dead_code_options(root, None)).expect("baseline analysis");
    assert!(
        !completed.results().unused_exports.is_empty(),
        "the fixture must have findings, so an empty report would be a plausible wrong answer"
    );

    let token = Arc::new(AtomicBool::new(true));
    let error = run_dead_code(&dead_code_options(root, Some(token)))
        .expect_err("a cancelled analysis must not return a report");
    assert_cancelled(&error);
    assert_eq!(
        error.message, ENTRY_GUARD_STOP,
        "a token set before the call must stop the run before it loads anything"
    );
}

#[test]
fn a_cancelled_combined_run_fails_instead_of_reporting_empty_sections() {
    let project = super::tests::dead_code_project();
    let token = Arc::new(AtomicBool::new(true));
    let error = run_combined(&CombinedOptions {
        analysis: uncached_analysis(project.path(), Some(token)),
        dead_code: true,
        duplication: true,
        health: true,
        ..CombinedOptions::default()
    })
    .expect_err("a cancelled combined analysis must not return sections");
    assert_cancelled(&error);
}

/// Cancelling an analysis that is already running stops it at a boundary it
/// had not reached, rather than letting it finish and discarding the result.
///
/// The assertion is the boundary the error names, not how long the call took.
/// Elapsed time is not evidence that work stopped: a run that completed and
/// then reported a cancellation would return just as quickly. How much work a
/// stop actually skips is asserted where a counter for it exists, in
/// `fallow_engine`'s
/// `a_cancelled_parse_stops_partway_and_leaves_no_truncated_cache_behind`,
/// which cancels a parse mid-flight and asserts on the module count.
#[test]
fn cancelling_a_running_analysis_stops_it_at_a_boundary_it_had_not_reached() {
    let project = generated_project();
    let root = project.path();

    // Warm the filesystem before either timed run, so the comparison measures
    // cancellation and not page-cache misses.
    run_dead_code(&dead_code_options(root, None)).expect("warm-up analysis");

    let started = Instant::now();
    let completed = run_dead_code(&dead_code_options(root, None)).expect("baseline analysis");
    let full_run = started.elapsed();
    assert!(
        !completed.results().unused_exports.is_empty(),
        "the generated project must produce findings"
    );
    assert!(
        full_run >= Duration::from_millis(50),
        "the fixture is too small to observe a stop in: {full_run:?}"
    );

    let token = Arc::new(AtomicBool::new(false));
    let (cancelled_at_tx, cancelled_at_rx) = std::sync::mpsc::channel();
    let watchdog = {
        let token = Arc::clone(&token);
        // A tenth of the way in: far enough that the run is really under way,
        // early enough that finishing anyway would be unmistakable.
        let delay = full_run / 10;
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            token.store(true, Ordering::SeqCst);
            let _ = cancelled_at_tx.send(Instant::now());
        })
    };

    let error = run_dead_code(&dead_code_options(root, Some(token)))
        .expect_err("a cancelled analysis must not return a report");
    watchdog.join().expect("watchdog thread");
    cancelled_at_rx
        .recv()
        .expect("watchdog reports when it cancelled");

    assert_cancelled(&error);
    assert_ne!(
        error.message, ENTRY_GUARD_STOP,
        "the run must have been under way when it was cancelled, not stopped at the entry guard"
    );
    assert!(
        error.message.starts_with("analysis was cancelled before "),
        "the error must name the boundary the run stopped at: {}",
        error.message
    );
}

/// `featureFlags` is Api-backed in Code Mode and spends its time in the parse
/// loop, so the token has to reach the loop rather than only guard the entry.
/// A run cancelled while it is under way must name a boundary past that guard.
#[test]
fn a_cancelled_feature_flags_run_stops_past_its_entry_guard() {
    let project = generated_project();
    let root = project.path();

    let flags_options = |cancellation: Option<Arc<AtomicBool>>| FeatureFlagsOptions {
        analysis: uncached_analysis(root, cancellation),
        top: None,
    };

    run_feature_flags(&flags_options(None)).expect("warm-up scan");
    let started = Instant::now();
    run_feature_flags(&flags_options(None)).expect("baseline scan");
    let full_run = started.elapsed();
    assert!(
        full_run >= Duration::from_millis(50),
        "the fixture is too small to observe a stop in: {full_run:?}"
    );

    let token = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let token = Arc::clone(&token);
        let delay = full_run / 10;
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            token.store(true, Ordering::SeqCst);
        })
    };
    let error = run_feature_flags(&flags_options(Some(token)))
        .expect_err("a cancelled scan must not return flags");
    watchdog.join().expect("watchdog thread");

    assert_cancelled(&error);
    assert_ne!(
        error.message, ENTRY_GUARD_STOP,
        "the scan must have been under way when it was cancelled, not stopped at the entry guard"
    );
}
