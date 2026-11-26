//! Fix JSON output contract.

use fallow_types::workspace::WorkspaceDiagnostic;
use serde::Serialize;
use serde_json::Value;

/// A diagnostic that withholds at least one fix of the run, with the step
/// that unblocks the fix.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct IncompleteAnalysisDiagnostic {
    /// The diagnostic, in the shape of a `workspace_diagnostics[]` entry.
    #[serde(flatten)]
    pub diagnostic: WorkspaceDiagnostic,
    /// The config change or command that lets the run analyze the path, so
    /// that the withheld fixes become available.
    pub action: String,
}

/// Inputs for building `fallow fix --format json`.
#[derive(Clone, Copy)]
pub struct FixJsonOutputInput<'a> {
    /// True when `--dry-run` was passed and no files were modified.
    pub dry_run: bool,
    /// Per-fix entries as prepared by the fix planner.
    pub fixes: &'a [Value],
    /// Fixes skipped because file content changed since analysis.
    pub skipped_content_changed: usize,
    /// Fixes skipped because the file mixes line endings.
    pub skipped_mixed_line_endings: usize,
    /// Export removals skipped because confidence was below the threshold.
    pub skipped_low_confidence_exports: usize,
    /// Dependency removals withheld because the finding carried a
    /// reachability caveat.
    pub skipped_low_confidence_dependencies: usize,
    /// Enum- and class-member removals withheld because the finding carried a
    /// reachability caveat.
    pub skipped_low_confidence_members: usize,
    /// Export removals withheld because a re-export that the run keeps still
    /// names the export.
    pub skipped_kept_re_exports: usize,
    /// Export removals withheld because the same `export` keyword also
    /// exports a name that the run keeps.
    pub skipped_shared_declarations: usize,
    /// Export removals withheld in production mode because a file outside
    /// the production scope imports the export.
    pub skipped_test_importers: usize,
    /// The diagnostics that can withhold the
    /// `low_confidence_incomplete_analysis` entries of `fixes`.
    pub incomplete_analysis_diagnostics: &'a [IncompleteAnalysisDiagnostic],
}

/// JSON root emitted by `fallow fix --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow fix --format json"))]
pub struct FixJsonOutput<'a> {
    /// True when `--dry-run` was passed and no files were modified.
    pub dry_run: bool,
    /// Per-fix entries, applied and skipped alike.
    pub fixes: &'a [Value],
    /// Number of entries in `fixes` whose `applied` flag is true.
    pub total_fixed: usize,
    /// User-facing skipped entries; excludes plan-level skip diagnostics,
    /// which are reported through the dedicated `skipped_*` counters.
    pub skipped: usize,
    /// Fixes skipped because file content changed since analysis.
    pub skipped_content_changed: usize,
    /// Fixes skipped because the file mixes line endings.
    pub skipped_mixed_line_endings: usize,
    /// Export removals skipped because confidence was below the threshold.
    pub skipped_low_confidence_exports: usize,
    /// Dependency removals withheld because the finding carried a
    /// reachability caveat. Counted separately from the export withholding
    /// so the two names stay honest; both are intentional and neither moves
    /// the exit code.
    pub skipped_low_confidence_dependencies: usize,
    /// Enum- and class-member removals withheld because the finding carried a
    /// reachability caveat. A third counter for the same reason the second
    /// one exists: a member is neither an export nor a dependency, and
    /// folding it into either name would make that name lie. Intentional,
    /// exit-code-neutral, and `0` on a run that read every file it found.
    pub skipped_low_confidence_members: usize,
    /// Export removals withheld because a re-export that the run keeps still
    /// names the export: the removal would leave that re-export pointing at a
    /// declaration that is no longer exported. Each withheld export is a
    /// `remove_export` entry with `skip_reason: "kept_re_export"`, so this
    /// counts exports, not files. Intentional and exit-code-neutral.
    pub skipped_kept_re_exports: usize,
    /// Export removals withheld because the same `export` keyword also
    /// exports a name that the run keeps, as in `export const { a, b } = x;`
    /// with only `b` unused: the removal of the keyword would remove the
    /// export of `a` too. Each withheld export is a `remove_export` entry
    /// with `skip_reason: "shared_declaration_in_use"`, so this counts
    /// exports, not files. Intentional and exit-code-neutral.
    pub skipped_shared_declarations: usize,
    /// Export removals withheld in production mode because a reachable test,
    /// story, fixture or other file outside the production scope imports the
    /// export: the removal would break that import. Each withheld export is a
    /// `remove_export` entry with `skip_reason: "test_importers"`, so this
    /// counts exports, not files. Intentional and exit-code-neutral.
    pub skipped_test_importers: usize,
    /// The diagnostics that can withhold the entries of `fixes` with
    /// `skip_reason: "low_confidence_incomplete_analysis"`: each source file
    /// or hidden folder that the run did not fully analyze, with the step
    /// that unblocks the fix. Files that the run never read come first, then
    /// parse failures, each group sorted by path. A diagnostic that degrades
    /// the analysis but can withhold no fix (for example
    /// `undeclared-workspace`) is not listed. Omitted when no fix is withheld
    /// for this reason.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub incomplete_analysis_diagnostics: &'a [IncompleteAnalysisDiagnostic],
}

/// Count fix entries whose `applied` flag is true.
#[must_use]
pub fn count_applied_fixes(fixes: &[Value]) -> usize {
    fixes
        .iter()
        .filter(|fix| fix.get("applied").and_then(Value::as_bool).unwrap_or(false))
        .count()
}

/// Count user-facing skipped entries, excluding plan-level skip diagnostics.
#[must_use]
pub fn count_reported_fix_skips(fixes: &[Value]) -> usize {
    fixes
        .iter()
        .filter(|fix| {
            let is_skipped = fix.get("skipped").and_then(Value::as_bool).unwrap_or(false);
            let reason = fix.get("skip_reason").and_then(Value::as_str);
            let is_plan_skip = matches!(
                reason,
                Some(
                    "content_changed"
                        | "mixed_line_endings"
                        | "low_confidence_off_graph"
                        | "low_confidence_unresolved_imports"
                        | "low_confidence_incomplete_analysis"
                        | "kept_re_export"
                        | "shared_declaration_in_use"
                        | "generated_source"
                        | "test_importers"
                )
            );
            is_skipped && !is_plan_skip
        })
        .count()
}

/// Build the typed fix JSON root.
#[must_use]
pub fn build_fix_json_output(input: FixJsonOutputInput<'_>) -> FixJsonOutput<'_> {
    FixJsonOutput {
        dry_run: input.dry_run,
        fixes: input.fixes,
        total_fixed: count_applied_fixes(input.fixes),
        skipped: count_reported_fix_skips(input.fixes),
        skipped_content_changed: input.skipped_content_changed,
        skipped_mixed_line_endings: input.skipped_mixed_line_endings,
        skipped_low_confidence_exports: input.skipped_low_confidence_exports,
        skipped_low_confidence_dependencies: input.skipped_low_confidence_dependencies,
        skipped_low_confidence_members: input.skipped_low_confidence_members,
        skipped_kept_re_exports: input.skipped_kept_re_exports,
        skipped_shared_declarations: input.skipped_shared_declarations,
        skipped_test_importers: input.skipped_test_importers,
        incomplete_analysis_diagnostics: input.incomplete_analysis_diagnostics,
    }
}

/// Serialize the typed fix JSON root.
///
/// # Errors
///
/// Returns a serde error when a fix entry cannot be converted to JSON.
pub fn serialize_fix_json_output(
    input: FixJsonOutputInput<'_>,
) -> Result<Value, serde_json::Error> {
    serde_json::to_value(build_fix_json_output(input))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fix_output_counts_applied_and_user_skips() {
        let fixes = vec![
            json!({"applied": true}),
            json!({"applied": false, "skipped": true, "skip_reason": "manual"}),
            json!({"skipped": true, "skip_reason": "content_changed"}),
            json!({"skipped": true, "skip_reason": "low_confidence_unresolved_imports"}),
            json!({
                "type": "remove_dependency",
                "skipped": true,
                "skip_reason": "low_confidence_incomplete_analysis",
            }),
            json!({
                "type": "remove_enum_member",
                "skipped": true,
                "skip_reason": "low_confidence_incomplete_analysis",
            }),
            json!({
                "type": "remove_export",
                "applied": false,
                "skipped": true,
                "skip_reason": "kept_re_export",
            }),
            json!({
                "type": "remove_export",
                "applied": false,
                "skipped": true,
                "skip_reason": "shared_declaration_in_use",
            }),
        ];

        let output = build_fix_json_output(FixJsonOutputInput {
            dry_run: true,
            fixes: &fixes,
            skipped_content_changed: 1,
            skipped_mixed_line_endings: 2,
            skipped_low_confidence_exports: 3,
            skipped_low_confidence_dependencies: 4,
            skipped_low_confidence_members: 5,
            skipped_kept_re_exports: 6,
            skipped_shared_declarations: 7,
            skipped_test_importers: 3,
            incomplete_analysis_diagnostics: &[],
        });

        assert!(output.dry_run);
        assert_eq!(output.total_fixed, 1);
        assert_eq!(output.skipped, 1);
        assert_eq!(output.skipped_content_changed, 1);
        assert_eq!(output.skipped_mixed_line_endings, 2);
        assert_eq!(output.skipped_low_confidence_exports, 3);
        assert_eq!(
            output.skipped, 1,
            "a withheld dependency belongs to its own counter, not the generic skip count"
        );
        assert_eq!(output.skipped_low_confidence_dependencies, 4);
        assert_eq!(
            output.skipped, 1,
            "a withheld enum member belongs to its own counter too"
        );
        assert_eq!(output.skipped_low_confidence_members, 5);
        assert_eq!(
            output.skipped, 1,
            "an export held back by a kept re-export belongs to its own counter"
        );
        assert_eq!(output.skipped_kept_re_exports, 6);
        assert_eq!(
            output.skipped, 1,
            "an export held back by its shared declaration belongs to its own counter"
        );
        assert_eq!(output.skipped_shared_declarations, 7);
        assert_eq!(output.skipped_test_importers, 3);
    }

    #[test]
    fn fix_output_serializes_expected_root_keys() {
        let fixes = vec![json!({"type": "unused-export", "applied": true})];
        let value = serialize_fix_json_output(FixJsonOutputInput {
            dry_run: false,
            fixes: &fixes,
            skipped_content_changed: 0,
            skipped_mixed_line_endings: 0,
            skipped_low_confidence_exports: 0,
            skipped_low_confidence_dependencies: 0,
            skipped_low_confidence_members: 0,
            skipped_kept_re_exports: 0,
            skipped_shared_declarations: 0,
            skipped_test_importers: 0,
            incomplete_analysis_diagnostics: &[],
        })
        .expect("fix output serializes");

        assert_eq!(value["dry_run"], false);
        assert_eq!(value["total_fixed"], 1);
        assert_eq!(value["skipped"], 0);
        assert_eq!(value["skipped_kept_re_exports"], 0);
        assert_eq!(value["skipped_shared_declarations"], 0);
        assert_eq!(value["skipped_test_importers"], 0);
        assert_eq!(value["fixes"][0]["type"], "unused-export");
    }
}
