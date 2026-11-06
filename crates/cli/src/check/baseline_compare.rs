//! Loads a dead-code baseline and compares it with the current results.

use super::*;

/// Load a baseline file, filter out matched issues, and return this run's view
/// of the loaded baseline.
///
/// `fallow_engine::baseline::apply_dead_code_baseline` reads the file and
/// removes the findings; this function owns the file read, the messages and
/// the record for the `recheck-baseline` next step.
pub(super) fn load_and_compare_baseline(
    results: &mut fallow_types::results::AnalysisResults,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) -> Result<LoadedBaselineStaleness, ExitCode> {
    use fallow_engine::baseline::{
        DeadCodeBaselineError, DeadCodeBaselineOutcome, apply_dead_code_baseline,
    };

    let content = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read baseline: {e}"), 2, io.output))?;
    baseline_notes::warn_about_a_narrower_saved_baseline(&content, baseline_path, io);
    let comparison =
        baseline_notes::compare_with_saved_baseline(&content, io.scope_reasons, io.filters);
    let install_state =
        baseline_notes::warn_about_another_install_state(&content, baseline_path, io);
    let outcome = apply_dead_code_baseline(
        results,
        &content,
        &io.config.root,
        io.analysis_identity,
        !comparison.reasons.is_empty(),
    )
    .map_err(|err| match err {
        DeadCodeBaselineError::Parse(message) => emit_error(
            &format!("failed to parse baseline: {message}"),
            2,
            io.output,
        ),
        DeadCodeBaselineError::IncompatibleIdentity(fields) => {
            let type_aware_flag = if matches!(
                io.analysis_identity.mode,
                fallow_types::semantic::SemanticAnalysisMode::TypeAware
            ) {
                " --type-aware"
            } else {
                ""
            };
            emit_error(
                &format!(
                    "baseline analysis identity is incompatible in: {}. Regenerate it with: fallow {}{type_aware_flag} --save-baseline {}",
                    fields.join(", "),
                    io.command,
                    baseline_path.display(),
                ),
                2,
                io.output,
            )
        }
    })?;
    let (staleness, unrecognised_format, saved_by, legacy_keys) = match outcome {
        DeadCodeBaselineOutcome::Applied {
            staleness,
            legacy_keys,
        } => (staleness, false, None, legacy_keys),
        DeadCodeBaselineOutcome::NotDeadCode {
            staleness,
            saved_by,
        } => (staleness, true, saved_by, false),
    };
    if !io.quiet {
        eprintln!("Comparing against baseline: {}", baseline_path.display());
        if legacy_keys && matches!(io.output, OutputFormat::Human) {
            eprintln!(
                "Note: {} uses the old baseline key format. Run --save-baseline to rewrite it without line numbers.",
                baseline_path.display()
            );
        }
        if !unrecognised_format {
            baseline_notes::warn_on_staleness(staleness, baseline_path, comparison.saved_mode);
        }
    }
    if unrecognised_format {
        baseline_notes::note_unrecognised_file(baseline_path, saved_by, io);
    }
    crate::output_runtime::set_loaded_baseline(crate::output_runtime::LoadedBaselineRecheck {
        command: io.command,
        path: baseline_path.display().to_string(),
        baseline_entries: staleness.entries,
        scope_reasons: comparison.reasons,
        saved_mode: comparison.saved_mode,
    });
    Ok(LoadedBaselineStaleness {
        staleness,
        path: baseline_path.to_path_buf(),
        scope_reasons: comparison.reasons,
        saved_mode: comparison.saved_mode,
        uncovered_categories: comparison.uncovered_categories,
        unrecognised_format,
        saved_by,
        legacy_keys,
        install_state,
    })
}
