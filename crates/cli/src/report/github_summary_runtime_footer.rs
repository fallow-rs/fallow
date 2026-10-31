//! Footer of the runtime coverage section in the health step summary.

use serde_json::Value;

use super::health_thresholds_footer;
use crate::report::github::num;

/// Port of the footer branch of `summary-health.jq`. A positive-only capture
/// has null tracked and unhit totals, so it shows the positive evidence only.
pub(super) fn health_runtime_footer(
    env: &Value,
    complex: usize,
    prod: usize,
    hot: usize,
    runtime: &Value,
) -> String {
    if complex > 0 {
        return health_thresholds_footer(env);
    }
    if prod > 0 {
        let summary = runtime.get("summary").cloned().unwrap_or(Value::Null);
        if summary.get("capture_policy").and_then(Value::as_str) == Some("positive_only") {
            return format!(
                "\n\nPartial capture: **{}** functions with positive execution evidence, **{}** untracked",
                num(&summary, "functions_hit"),
                num(&summary, "functions_untracked"),
            );
        }
        return format!(
            "\n\n**{}** tracked functions, **{}** hit, **{}** unhit, **{}** untracked",
            num(&summary, "functions_tracked"),
            num(&summary, "functions_hit"),
            num(&summary, "functions_unhit"),
            num(&summary, "functions_untracked"),
        );
    }
    format!(
        "\n\nObserved **{hot}** hot path{} in runtime coverage.",
        if hot == 1 { "" } else { "s" },
    )
}
