//! Fix JSON output contract.

use serde::Serialize;
use serde_json::Value;

/// Inputs for building `fallow fix --format json`.
#[derive(Clone, Copy)]
pub struct FixJsonOutputInput<'a> {
    pub dry_run: bool,
    pub fixes: &'a [Value],
    pub skipped_content_changed: usize,
    pub skipped_mixed_line_endings: usize,
    pub skipped_low_confidence_exports: usize,
}

/// JSON root emitted by `fallow fix --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow fix --format json"))]
pub struct FixJsonOutput<'a> {
    pub dry_run: bool,
    pub fixes: &'a [Value],
    pub total_fixed: usize,
    pub skipped: usize,
    pub skipped_content_changed: usize,
    pub skipped_mixed_line_endings: usize,
    pub skipped_low_confidence_exports: usize,
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
        ];

        let output = build_fix_json_output(FixJsonOutputInput {
            dry_run: true,
            fixes: &fixes,
            skipped_content_changed: 1,
            skipped_mixed_line_endings: 2,
            skipped_low_confidence_exports: 3,
        });

        assert!(output.dry_run);
        assert_eq!(output.total_fixed, 1);
        assert_eq!(output.skipped, 1);
        assert_eq!(output.skipped_content_changed, 1);
        assert_eq!(output.skipped_mixed_line_endings, 2);
        assert_eq!(output.skipped_low_confidence_exports, 3);
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
        })
        .expect("fix output serializes");

        assert_eq!(value["dry_run"], false);
        assert_eq!(value["total_fixed"], 1);
        assert_eq!(value["skipped"], 0);
        assert_eq!(value["fixes"][0]["type"], "unused-export");
    }
}
