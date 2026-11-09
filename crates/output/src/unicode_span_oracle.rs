//! Unicode oracle for SARIF region columns.
//!
//! The oracle puts a comment with astral characters, combining marks and
//! Unicode whitespace before a finding. A SARIF run without `columnKind`
//! counts `startColumn` in UTF-16 code units (SARIF 2.1.0, section 3.14.17),
//! so the column must point at the finding when a consumer slices the line in
//! UTF-16 code units (issue #4405). The fingerprint must keep the byte column,
//! so that alerts in code scanning keep their identity.
//!
//! The case count and the seed come from `PROPTEST_CASES` and
//! `PROPTEST_RNG_SEED`. Without `PROPTEST_RNG_SEED` the seed is fixed, so
//! each pull request run checks the same cases.

use std::path::Path;

use fallow_config::RulesConfig;
use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationReport};
use fallow_types::output_dead_code::UnusedExportFinding;
use fallow_types::results::{AnalysisResults, UnusedExport};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use serde_json::Value;

use crate::{
    SARIF_FINGERPRINT_KEY, build_dead_code_sarif, build_duplication_sarif,
    sarif_finding_fingerprint,
};

const DEFAULT_SEED: u64 = 0x0001_f600;

/// Comment text: ASCII, two-byte, three-byte and astral characters, a
/// combining mark, a zero-width joiner sequence and Unicode whitespace.
const TEXT: &[&str] = &[
    "a",
    " ",
    "\u{e9}",
    "e\u{301}",
    "\u{65e5}",
    "\u{1f600}",
    "\u{1f469}\u{200d}\u{1f4bb}",
    "\u{1d49c}",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
];

const NAME: &str = "unused";

fn config() -> ProptestConfig {
    let mut config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(DEFAULT_SEED);
    }
    config
}

fn text() -> BoxedStrategy<String> {
    prop::collection::vec(prop::sample::select(TEXT), 0..6)
        .prop_map(|parts| parts.concat())
        .boxed()
}

fn rule(id: &str, description: &str, level: &str) -> Value {
    serde_json::json!({
        "id": id,
        "shortDescription": { "text": description },
        "defaultConfiguration": { "level": level }
    })
}

/// Write `src/a.ts` with one line, and return the line and the 0-based byte
/// column of [`NAME`] on it.
fn write_source(root: &Path, comment: &str) -> (String, u32) {
    let line = format!("/* {comment} */ export const {NAME} = 1;");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/a.ts"), format!("{line}\n")).expect("write source");
    let byte_col = line.rfind(NAME).expect("name on line");
    (line, u32::try_from(byte_col).expect("short line"))
}

/// The text at the 1-based UTF-16 `column` of `line`, as a SARIF consumer
/// reads it.
fn text_at_utf16_column(line: &str, column: u64) -> String {
    let units: Vec<u16> = line.encode_utf16().collect();
    let start = usize::try_from(column - 1).expect("small column");
    String::from_utf16(units.get(start..).expect("column on line")).expect("valid UTF-16")
}

fn region(result: &Value) -> (u64, u64) {
    let region = &result["locations"][0]["physicalLocation"]["region"];
    (
        region["startLine"].as_u64().expect("startLine"),
        region["startColumn"].as_u64().expect("startColumn"),
    )
}

fn first_result(sarif: &Value) -> &Value {
    let results = sarif["runs"][0]["results"].as_array().expect("results");
    assert_eq!(results.len(), 1, "{sarif:#}");
    assert!(sarif["runs"][0].get("columnKind").is_none());
    &results[0]
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn dead_code_columns_are_utf16_columns(comment in text()) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();
        let (line, byte_col) = write_source(root, &comment);
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/a.ts"),
            export_name: NAME.to_owned(),
            is_type_only: false,
            line: 1,
            col: byte_col,
            span_start: byte_col,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));

        let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &rule);
        let result = first_result(&sarif);
        let (start_line, column) = region(result);

        prop_assert_eq!(start_line, 1);
        prop_assert!(text_at_utf16_column(&line, column).starts_with(NAME), "{line:?} at {column}");
        let byte_fingerprint =
            sarif_finding_fingerprint("fallow/unused-export", "src/a.ts", &line, byte_col + 1);
        prop_assert_eq!(
            result["partialFingerprints"][SARIF_FINGERPRINT_KEY].as_str(),
            Some(byte_fingerprint.as_str()),
            "the fingerprint keeps the byte column"
        );
    }

    #[test]
    fn clone_columns_are_utf16_columns(comment in text()) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path();
        let (line, byte_col) = write_source(root, &comment);
        let instance = CloneInstance {
            file: root.join("src/a.ts"),
            start_line: 1,
            end_line: 1,
            start_col: byte_col as usize,
            end_col: line.len(),
            fragment: String::new(),
            is_symlink: false,
        };
        let report = DuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![instance],
                token_count: 10,
                line_count: 1,
                similarity: None,
            }],
            ..DuplicationReport::default()
        };

        let sarif = build_duplication_sarif(&report, root, &rule);
        let (_, column) = region(first_result(&sarif));

        prop_assert!(text_at_utf16_column(&line, column).starts_with(NAME), "{line:?} at {column}");
    }
}
