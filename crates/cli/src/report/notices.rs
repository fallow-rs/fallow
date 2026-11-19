//! The run notices on stderr: one `Note:` line for each notice of a run, in
//! the human output format and without `--quiet`. The machine formats carry
//! the notices in `workspace_diagnostics` and print no note.

use std::path::Path;
use std::sync::Mutex;

use fallow_config::{OutputFormat, PlannedAnalysis, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use rustc_hash::FxHashSet;

/// Print a `Note:` line for each run notice of `diagnostics`. The config
/// loader already warns about a shadowed config file and an unknown security
/// category, so those kinds print no second line.
pub fn print_run_notices(diagnostics: &[WorkspaceDiagnostic], output: OutputFormat, quiet: bool) {
    if quiet || !matches!(output, OutputFormat::Human) {
        return;
    }
    for message in notes(diagnostics) {
        eprintln!("Note: {message}");
    }
}

/// The messages of `diagnostics` that print as a note, without the messages
/// that the process printed before. `fallow watch` resolves the run again on
/// each reload, and the notices of one config do not change between reloads.
fn notes(diagnostics: &[WorkspaceDiagnostic]) -> Vec<&str> {
    static PRINTED: Mutex<Option<FxHashSet<String>>> = Mutex::new(None);
    let mut guard = PRINTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let printed = guard.get_or_insert_with(FxHashSet::default);
    let fresh = diagnostics
        .iter()
        .filter(|diagnostic| note_kind(&diagnostic.kind))
        .map(|diagnostic| diagnostic.message.as_str())
        .filter(|message| printed.insert((*message).to_owned()))
        .collect();
    drop(guard);
    fresh
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn a_notice_prints_once_per_process() {
        let root = Path::new("/project");
        let notice = WorkspaceDiagnostic::new(
            root,
            root.to_path_buf(),
            WorkspaceDiagnosticKind::EnvValueIgnored {
                name: "FALLOW_PRODUCTION_ONCE_PER_PROCESS".to_owned(),
            },
        );
        let notices = std::slice::from_ref(&notice);
        assert_eq!(notes(notices), [notice.message.as_str()]);
        assert!(
            notes(notices).is_empty(),
            "a second resolve of the run, such as a watch reload, prints no repeat"
        );
    }
}
