//! Notes about a loaded dead-code baseline that no report states by itself.
//!
//! Each note prints regardless of `--quiet`: it explains findings that the
//! report shows as new, and `--ci` implies `--quiet`.

use fallow_config::ResolvedConfig;
use fallow_types::workspace::WorkspaceDiagnostic;

use super::BaselineIo;
use crate::baseline_gate::LoadedBaselineStaleness;

/// A baseline saved from a narrowed run holds only part of the findings. A
/// wider run that loads it reports the findings outside the saved scope as
/// new, so the run says why before the report does.
///
/// Printed regardless of `--quiet`, like the note for a baseline of another
/// format: no report states this fact, and `--ci` implies `--quiet`.
pub(super) fn warn_about_a_narrower_saved_baseline(
    content: &str,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) {
    let current: Vec<&str> = io
        .scope_reasons
        .iter()
        .map(fallow_output::ScopeReason::as_str)
        .collect();
    let wider: Vec<String> = fallow_engine::baseline::saved_scope_reasons(content)
        .into_iter()
        .filter(|reason| !current.contains(&reason.as_str()))
        .collect();
    if wider.is_empty() {
        return;
    }
    let remedy = if wider.iter().any(|reason| reason == "package-baselines") {
        " Save it again with --no-package-baselines to cover the whole project."
    } else {
        " Save it again from a run without that narrowing to cover the whole project."
    };
    eprintln!(
        "Warning: the baseline {} was saved from a run narrowed by {}, so it lists only part of \
         the findings. This run is wider and can report old findings as new.{remedy}",
        baseline_path.display(),
        wider.join(", "),
    );
}

/// A baseline saved with `node_modules` present and compared without it, or
/// the reverse, can report baseline findings as new. Returns the
/// `baseline-install-state-mismatch` diagnostic for the envelope, and prints
/// its message.
///
/// A legacy baseline does not record its install state, so it gives no note.
pub(super) fn warn_about_another_install_state(
    content: &str,
    baseline_path: &std::path::Path,
    io: &BaselineIo<'_>,
) -> Option<WorkspaceDiagnostic> {
    let diagnostic = fallow_engine::baseline::install_state_diagnostic(
        io.root,
        baseline_path,
        content,
        fallow_config::node_modules_state(io.root),
    )?;
    eprintln!("Warning: {}", diagnostic.message);
    Some(diagnostic)
}

/// The notes that the run adds to `workspace_diagnostics`: the unmatched
/// config patterns, and the install-state note of the loaded baseline.
pub(super) fn run_notes(
    config: &ResolvedConfig,
    reports_dependencies: bool,
    loaded: Option<&LoadedBaselineStaleness>,
) -> Vec<WorkspaceDiagnostic> {
    let mut notes =
        fallow_engine::dead_code::config_pattern_diagnostics(config, reports_dependencies);
    notes.extend(loaded.and_then(|loaded| loaded.install_state.clone()));
    notes
}
