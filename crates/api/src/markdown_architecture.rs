//! Render findings together with an optional architecture inventory.
use super::*;

/// Build markdown output for analysis results.
pub fn build_markdown(results: &AnalysisResults, root: &Path) -> String {
    let total = results.total_issues();
    let mut out = results
        .shared_state
        .as_ref()
        .map(|inventory| fallow_output::shared_state::markdown(inventory, root))
        .unwrap_or_default();

    if total == 0 {
        out.push_str("## Fallow: no issues found\n");
        if health_signal_count(results) == 0 {
            push_cascade_note(&mut out, results.cascade_hidden);
            return out;
        }
        out.push('\n');
    } else {
        let _ = write!(out, "## Fallow: {total} issue{} found\n\n", plural(total));
    }

    push_markdown_sections(&mut out, results, root);
    push_cascade_note(&mut out, results.cascade_hidden);
    out
}
