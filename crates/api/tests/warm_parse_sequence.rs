//! The parse work of the typed tool sequence that the MCP server runs:
//! `run_dead_code`, `run_duplication`, `run_health` and `run_trace_file`,
//! then the same sequence again.
//!
//! The store is process-wide, so this file is its own test binary and holds
//! one test. A store that keeps nothing counts the parse work of the path
//! without a warm store: each session parses through the persisted cache.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep fixture setup concise"
)]

use std::path::Path;
use std::sync::Arc;

use fallow_api::warm_parse::{self, WarmParseCounts, WarmParseLimits, WarmParseStore};
use fallow_api::{
    AnalysisOptions, ComplexityOptions, DeadCodeOptions, DuplicationOptions, TraceFileOptions,
    run_dead_code, run_duplication, run_health, run_trace_file,
};

const SOURCE_FILES: usize = 4;

fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir().expect("temp dir");
    let root = project.path();
    std::fs::create_dir(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"warm-parse-sequence","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './utils';\nimport { other } from './other';\nused(other);\n",
    )
    .expect("index");
    std::fs::write(
        root.join("src/utils.ts"),
        "export const used = (value: number): number => value;\nexport const unused = 1;\n",
    )
    .expect("utils");
    std::fs::write(root.join("src/other.ts"), "export const other = 2;\n").expect("other");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 3;\n").expect("orphan");
    project
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        ..AnalysisOptions::default()
    }
}

fn run_sequence(root: &Path) {
    run_dead_code(&DeadCodeOptions {
        analysis: analysis(root),
        ..DeadCodeOptions::default()
    })
    .expect("dead code");
    run_duplication(&DuplicationOptions {
        analysis: analysis(root),
        ..DuplicationOptions::default()
    })
    .expect("duplication");
    run_health(&ComplexityOptions {
        analysis: analysis(root),
        ..ComplexityOptions::default()
    })
    .expect("health");
    run_trace_file(&TraceFileOptions {
        analysis: analysis(root),
        file: "src/index.ts".to_string(),
    })
    .expect("trace file");
}

fn counts_for(limits: WarmParseLimits) -> WarmParseCounts {
    let project = project();
    let store = Arc::new(WarmParseStore::new(limits));
    warm_parse::install(Some(Arc::clone(&store)));
    run_sequence(project.path());
    run_sequence(project.path());
    warm_parse::install(None);
    store.counts()
}

#[test]
fn a_repeated_tool_sequence_parses_each_file_once_with_a_warm_store() {
    let without_reuse = counts_for(WarmParseLimits {
        max_entries: 0,
        ..WarmParseLimits::default()
    });
    let warm = counts_for(WarmParseLimits::default());

    assert_eq!(
        without_reuse,
        WarmParseCounts {
            parse_runs: 6,
            modules_parsed: SOURCE_FILES,
            disk_cache_hits: 5 * SOURCE_FILES,
            modules_reused: 0,
        },
        "without reuse, each dead-code, health and trace session reads the persisted cache"
    );
    assert_eq!(
        warm,
        WarmParseCounts {
            parse_runs: 1,
            modules_parsed: SOURCE_FILES,
            disk_cache_hits: 0,
            modules_reused: 5 * SOURCE_FILES,
        },
        "with the store, only the first session parses"
    );
}
