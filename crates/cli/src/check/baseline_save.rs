//! Write a dead-code baseline file.

use super::*;

/// Serialize the current results to a baseline file, creating parent dirs.
pub(super) fn save_baseline_file(
    results: &fallow_types::results::AnalysisResults,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) -> Result<(), ExitCode> {
    if let Some(refusal) = fallow_engine::baseline::refuse_baseline_kind_overwrite(
        baseline_path,
        fallow_engine::baseline::BaselineKind::DeadCode,
    ) {
        return Err(emit_error(&refusal, 2, io.output));
    }
    let baseline_data =
        BaselineData::from_results_with_identity(results, io.root, io.analysis_identity.clone())
            .with_scope_reasons(io.scope_reasons)
            .with_node_modules(fallow_config::node_modules_state(io.root));
    let mut json = serde_json::to_string_pretty(&baseline_data)
        .map_err(|e| emit_error(&format!("failed to serialize baseline: {e}"), 2, io.output))?;
    json.push('\n');
    match fallow_engine::write_guard::write_file(
        baseline_path,
        json.as_bytes(),
        fallow_engine::write_guard::WriteTarget::Path,
    ) {
        Ok(()) => {}
        Err(e) if e.is_directory() => {
            return Err(emit_error(
                &format!("failed to create baseline directory: {e}"),
                2,
                io.output,
            ));
        }
        Err(e) => {
            return Err(emit_error(
                &format!("failed to save baseline: {e}"),
                2,
                io.output,
            ));
        }
    }
    if !io.quiet {
        eprintln!("Baseline saved to {}", baseline_path.display());
    }
    // The package map comes from the config and applies to every run, so a
    // saved baseline is partial unless the run turned the map off. A later run
    // that edits an unchanged package then reports its old findings as new.
    if io
        .scope_reasons
        .contains(fallow_output::ScopeReason::PackageBaselines)
    {
        eprintln!(
            "Warning: workspaces.changedSince narrowed this run, so the baseline {} holds only the \
             findings in changed files of the mapped packages. Save it with \
             --no-package-baselines to cover the whole project.",
            baseline_path.display()
        );
    }
    Ok(())
}
