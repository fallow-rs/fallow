//! The run notices on stderr: one `Note:` line for each notice of a run, in
//! the human output format and without `--quiet`. The machine formats carry
//! the notices in `workspace_diagnostics` and print no note.

use std::path::Path;

use fallow_config::{OutputFormat, PlannedAnalysis, WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Print a `Note:` line for each run notice of `diagnostics`. The config
/// loader already warns about a shadowed config file and an unknown security
/// category, so those kinds print no second line.
pub fn print_run_notices(diagnostics: &[WorkspaceDiagnostic], output: OutputFormat, quiet: bool) {
    if quiet || !matches!(output, OutputFormat::Human) {
        return;
    }
    for diagnostic in diagnostics {
        if note_kind(&diagnostic.kind) {
            eprintln!("Note: {}", diagnostic.message);
        }
    }
}

const fn note_kind(kind: &WorkspaceDiagnosticKind) -> bool {
    kind.is_run_notice()
        && !matches!(
            kind,
            WorkspaceDiagnosticKind::ConfigFileShadowed { .. }
                | WorkspaceDiagnosticKind::UnknownSecurityCategory { .. }
        )
}

/// The reason of an inert flag.
#[derive(Debug, Clone, Copy)]
pub enum InertReason {
    /// The config key turns the analysis off.
    OffByConfig(&'static str),
    /// The command does not read the flag.
    NotReadByCommand,
}

/// Record that `flag` had no effect on `analysis` of the run at `root`: the
/// notice goes into the workspace diagnostics of the root, which every
/// analysis of the run folds into its output, and prints as a note.
pub fn record_inert_flag(
    root: &Path,
    flag: &str,
    analysis: PlannedAnalysis,
    reason: InertReason,
    output: OutputFormat,
    quiet: bool,
) {
    let (reason, key) = match reason {
        InertReason::OffByConfig(key) => ("off-by-config", Some(key.to_owned())),
        InertReason::NotReadByCommand => ("not-read-by-command", None),
    };
    let notice = WorkspaceDiagnostic::new(
        root,
        root.to_path_buf(),
        WorkspaceDiagnosticKind::InertFlag {
            flag: flag.to_owned(),
            analysis: analysis.wire_name().to_owned(),
            reason: reason.to_owned(),
            key,
        },
    )
    .into_root_relative(root);
    print_run_notices(std::slice::from_ref(&notice), output, quiet);
    fallow_config::append_workspace_diagnostics(root, vec![notice]);
}

/// The run notices of an audit, for the root `workspace_diagnostics` of its
/// JSON envelope. The sub-analysis sections carry the other diagnostics.
pub fn audit_run_notices(result: &crate::audit::AuditResult) -> Vec<WorkspaceDiagnostic> {
    let sections = [
        result
            .check
            .as_ref()
            .map(|check| check.workspace_diagnostics.as_slice()),
        result
            .dupes
            .as_ref()
            .map(|dupes| dupes.workspace_diagnostics.as_slice()),
        result
            .health
            .as_ref()
            .map(|health| health.workspace_diagnostics.as_slice()),
    ];
    fallow_api::audit_run_notices(sections.into_iter().flatten())
}
