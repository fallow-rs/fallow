//! Churn-file validation for health analysis.

use std::process::ExitCode;

use crate::error::emit_error;

use super::HealthExecutionOptions;

/// Validate an explicit `--churn-file` up front so a malformed import is a loud
/// hard error rather than a silent hotspot skip. Runs before the pipeline, and
/// only when churn would actually be consumed (`--hotspots` / `--targets`;
/// `--ownership` is subsumed because the dispatch layer sets `hotspots =
/// hotspots || ownership` before building `HealthExecutionOptions`), so an
/// inert `--churn-file` on a non-churn run is not penalized.
///
/// The file is re-read in `hotspots::fetch_churn_data`; the duplicate read is
/// negligible for realistic churn files and bounded by `MAX_CHURN_EVENTS`.
pub fn validate_health_churn_file(opts: &HealthExecutionOptions<'_>) -> Result<(), ExitCode> {
    if let Some(churn_file) = opts.churn_file
        && (opts.hotspots || opts.targets)
    {
        let resolved = super::scoring::resolve_relative_to_root(churn_file, Some(opts.root));
        crate::analyze_churn_from_file(&resolved, opts.root)
            .map_err(|e| emit_error(&e, 2, opts.output))?;
    }
    Ok(())
}
