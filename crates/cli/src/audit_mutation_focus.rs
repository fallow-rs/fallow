//! `fallow audit --brief --mutation-report <file>`: the surviving mutants of a
//! saved `fallow mutation` report on the added lines of the diff, counted by
//! file for the `mutation` component of the focus score.
//!
//! The join reads the report only; it runs no test. A report file whose
//! SHA-256 digest differs from the current source file is stale, so its
//! mutants do not count.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::Deserialize;

use crate::audit_focus::FocusFileCount;
use crate::mutation_inspect::{read_report_bytes, report_input_error, sha256_hex};

/// The status token of a surviving mutant.
const SURVIVED: &str = "survived";

/// The fields of a saved `fallow mutation` report that the join reads.
/// Unknown fields are ignored, so a report of a later minor version still
/// reads.
#[derive(Debug, Deserialize)]
struct SavedRun {
    #[serde(default)]
    kind: Option<String>,
    mutants: Vec<SavedMutant>,
    #[serde(default)]
    files: Vec<SavedFile>,
}

/// The status stays an optional string, so a missing status or a status of a
/// later version reads as "not a survivor" instead of a parse error.
#[derive(Debug, Deserialize)]
struct SavedMutant {
    file: String,
    line: u32,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SavedFile {
    path: String,
    source_sha256: String,
}

/// A parsed mutation report: the surviving mutants and the source digest of
/// each report file.
#[derive(Debug, Default)]
pub struct AuditMutationReport {
    survivors: Vec<(String, u32)>,
    digests: FxHashMap<String, String>,
}

/// Reads and parses a saved `fallow mutation` report.
///
/// # Errors
///
/// Returns an error with exit code 2 when the file cannot be read, is too
/// large, does not parse, or is not a `mutation` report.
pub fn read_audit_mutation_report(
    path: &Path,
) -> Result<AuditMutationReport, fallow_api::ProgrammaticError> {
    parse_audit_mutation_report(&read_report_bytes(path)?)
}

fn parse_audit_mutation_report(
    bytes: &[u8],
) -> Result<AuditMutationReport, fallow_api::ProgrammaticError> {
    let run: SavedRun = serde_json::from_slice(bytes).map_err(|error| {
        report_input_error(format!(
            "the file is not a `fallow mutation --format json` report: {error}"
        ))
    })?;
    if let Some(kind) = run.kind.as_deref()
        && kind != "mutation"
    {
        return Err(report_input_error(format!(
            "the file is a `{kind}` document, not a `fallow mutation` run report"
        )));
    }
    Ok(AuditMutationReport {
        survivors: run
            .mutants
            .into_iter()
            .filter(|mutant| mutant.status.as_deref() == Some(SURVIVED))
            .map(|mutant| (mutant.file, mutant.line))
            .collect(),
        digests: run
            .files
            .into_iter()
            .map(|file| (file.path, file.source_sha256))
            .collect(),
    })
}

impl AuditMutationReport {
    /// The surviving mutants on added lines, counted by root-relative file
    /// and sorted by file. A file without a digest in the report, a missing
    /// file, and a file whose source changed after the run add nothing.
    #[must_use]
    pub fn survivors_on_added_lines(
        &self,
        root: &Path,
        diff_index: Option<&fallow_output::DiffIndex>,
    ) -> Vec<FocusFileCount> {
        let Some(diff_index) = diff_index else {
            return Vec::new();
        };
        let mut fresh: FxHashMap<&str, bool> = FxHashMap::default();
        let mut counts: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for (file, line) in &self.survivors {
            let key = diff_index.key_for_root_relative(file);
            if !diff_index.line_is_added(&key, u64::from(*line)) {
                continue;
            }
            let is_fresh = *fresh
                .entry(file.as_str())
                .or_insert_with(|| self.source_matches(root, file));
            if is_fresh {
                *counts.entry(file.as_str()).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .map(|(file, count)| FocusFileCount {
                file: file.to_string(),
                count,
            })
            .collect()
    }

    /// [`Self::survivors_on_added_lines`] for the brief, with a note on stderr
    /// (unless `quiet`) when no file of the report exists under `root`.
    #[must_use]
    pub(crate) fn brief_survivors(
        &self,
        root: &Path,
        quiet: bool,
        diff_index: Option<&fallow_output::DiffIndex>,
    ) -> Vec<FocusFileCount> {
        if !quiet && !self.matches_root(root) {
            eprintln!(
                "Note: no file of the --mutation-report file exists under {}. Pass the \
                 report of a mutation run at the same root.",
                root.display()
            );
        }
        self.survivors_on_added_lines(root, diff_index)
    }

    /// Whether a file of the report exists under `root`. A report of another
    /// root, for example of one package of a monorepo, matches no file.
    #[must_use]
    pub fn matches_root(&self, root: &Path) -> bool {
        self.digests.is_empty() || self.digests.keys().any(|file| root.join(file).is_file())
    }

    fn source_matches(&self, root: &Path, file: &str) -> bool {
        let Some(expected) = self.digests.get(file) else {
            return false;
        };
        std::fs::read(root.join(file)).is_ok_and(|bytes| sha256_hex(&bytes) == *expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/src/a.ts b/src/a.ts\n\
--- a/src/a.ts\n\
+++ b/src/a.ts\n\
@@ -1,1 +1,3 @@\n\
 const a = 1;\n\
+const b = a ?? 2;\n\
+const c = b > 1;\n";

    fn report(root: &Path, source: &str, digest_override: Option<&str>) -> Vec<u8> {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.ts"), source).unwrap();
        let digest = digest_override.map_or_else(|| sha256_hex(source.as_bytes()), str::to_string);
        serde_json::to_vec(&serde_json::json!({
            "kind": "mutation",
            "mutants": [
                {"file": "src/a.ts", "line": 1, "status": "survived"},
                {"file": "src/a.ts", "line": 2, "status": "survived"},
                {"file": "src/a.ts", "line": 3, "status": "survived"},
                {"file": "src/a.ts", "line": 3, "status": "killed"},
                {"file": "src/a.ts", "line": 3, "status": "a-later-status"},
            ],
            "files": [{"path": "src/a.ts", "source_sha256": digest}],
        }))
        .unwrap()
    }

    const SOURCE: &str = "const a = 1;\nconst b = a ?? 2;\nconst c = b > 1;\n";

    #[test]
    fn counts_survivors_on_added_lines_only() {
        let dir = tempfile::tempdir().unwrap();
        let parsed = parse_audit_mutation_report(&report(dir.path(), SOURCE, None)).unwrap();
        let diff = fallow_output::DiffIndex::from_unified_diff(DIFF);
        assert_eq!(
            parsed.survivors_on_added_lines(dir.path(), Some(&diff)),
            vec![FocusFileCount {
                file: "src/a.ts".to_string(),
                count: 2,
            }]
        );
    }

    #[test]
    fn a_stale_report_file_adds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let parsed =
            parse_audit_mutation_report(&report(dir.path(), SOURCE, Some("0000"))).unwrap();
        let diff = fallow_output::DiffIndex::from_unified_diff(DIFF);
        assert!(
            parsed
                .survivors_on_added_lines(dir.path(), Some(&diff))
                .is_empty()
        );
    }

    #[test]
    fn no_diff_adds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let parsed = parse_audit_mutation_report(&report(dir.path(), SOURCE, None)).unwrap();
        assert!(parsed.survivors_on_added_lines(dir.path(), None).is_empty());
    }

    #[test]
    fn a_mutant_without_a_status_reads_and_does_not_count() {
        let parsed = parse_audit_mutation_report(
            br#"{"kind":"mutation","mutants":[{"file":"src/a.ts","line":2}],"files":[]}"#,
        )
        .unwrap();
        assert!(parsed.survivors.is_empty());
    }

    #[test]
    fn a_report_of_another_root_does_not_match_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let parsed = parse_audit_mutation_report(&report(dir.path(), SOURCE, None)).unwrap();
        assert!(parsed.matches_root(dir.path()));
        let other = tempfile::tempdir().unwrap();
        assert!(!parsed.matches_root(other.path()));
    }

    #[test]
    fn refuses_a_document_of_another_kind() {
        let error =
            parse_audit_mutation_report(br#"{"kind":"mutation-plan","mutants":[]}"#).unwrap_err();
        assert!(error.message.contains("mutation-plan"), "{}", error.message);
    }

    #[test]
    fn refuses_a_file_that_is_not_a_report() {
        assert!(parse_audit_mutation_report(b"{}").is_err());
    }
}
