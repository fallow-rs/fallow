//! The install state that a dead-code baseline records.
//!
//! Findings that need installed packages (dependency, unlisted,
//! unresolved-import and script-binary findings) differ between a run with
//! `node_modules` and a run without it. A baseline saved in one state and
//! compared in the other can then report baseline findings as new. The
//! baseline records the state at save time, and a run in the other state gets
//! a `baseline-install-state-mismatch` diagnostic.
//!
//! The state stays advisory. It sits next to the semantic identity inside
//! `analysis_identity`, but it is not part of
//! [`SemanticAnalysisIdentity`], so a difference never stops the comparison.

use std::path::Path;

use fallow_types::semantic::SemanticAnalysisIdentity;
use fallow_types::workspace::{NodeModulesState, WorkspaceDiagnostic, WorkspaceDiagnosticKind};

use super::BaselineData;

/// The `analysis_identity` object of a dead-code baseline file.
///
/// The semantic identity is flattened, so the file keeps its shape and a
/// legacy file without `node_modules` still loads. An older fallow that reads
/// a new file ignores the extra key.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct BaselineAnalysisIdentity {
    #[serde(flatten)]
    pub(super) semantic: SemanticAnalysisIdentity,
    /// The install state when the baseline was saved. `None` for a file that
    /// predates this key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) node_modules: Option<NodeModulesState>,
}

impl From<SemanticAnalysisIdentity> for BaselineAnalysisIdentity {
    fn from(semantic: SemanticAnalysisIdentity) -> Self {
        Self {
            semantic,
            node_modules: None,
        }
    }
}

impl BaselineData {
    /// Record the install state of the saving run.
    #[must_use]
    pub const fn with_node_modules(mut self, state: NodeModulesState) -> Self {
        self.analysis_identity.node_modules = Some(state);
        self
    }
}

/// The install state a baseline file recorded when it was saved.
///
/// `None` for a legacy file, a file of another format, or a file that is not
/// JSON. Reads the raw file, so it also answers for a file that the dead-code
/// format does not load.
#[must_use]
pub fn saved_node_modules_state(json: &str) -> Option<NodeModulesState> {
    let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let state = value.get("analysis_identity")?.get("node_modules")?;
    serde_json::from_value(state.clone()).ok()
}

/// The `baseline-install-state-mismatch` diagnostic for a loaded baseline, or
/// `None` when the saved state is unknown or equals `run`.
///
/// The baseline path is canonicalized, so it shows root-relative when `root`
/// is a canonical path and the caller passed another spelling of it.
#[must_use]
pub fn install_state_diagnostic(
    root: &Path,
    baseline_path: &Path,
    json: &str,
    run: NodeModulesState,
) -> Option<WorkspaceDiagnostic> {
    let saved = saved_node_modules_state(json)?;
    (saved != run).then(|| {
        WorkspaceDiagnostic::new(
            root,
            dunce::canonicalize(baseline_path).unwrap_or_else(|_| baseline_path.to_path_buf()),
            WorkspaceDiagnosticKind::BaselineInstallStateMismatch {
                baseline_node_modules: saved,
                run_node_modules: run,
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::AnalysisResults;

    fn saved(state: Option<NodeModulesState>) -> String {
        let data = BaselineData::from_results(&AnalysisResults::default(), Path::new("/p"));
        let data = match state {
            Some(state) => data.with_node_modules(state),
            None => data,
        };
        serde_json::to_string(&data).expect("serializes")
    }

    #[test]
    fn a_saved_baseline_records_the_install_state_inside_the_analysis_identity() {
        let json = saved(Some(NodeModulesState::Present));
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["analysis_identity"]["node_modules"], "present");
        assert_eq!(value["analysis_identity"]["mode"], "syntactic");
        assert_eq!(
            saved_node_modules_state(&json),
            Some(NodeModulesState::Present)
        );
        let loaded: BaselineData = serde_json::from_str(&json).expect("loads");
        assert_eq!(
            loaded.analysis_identity(),
            &SemanticAnalysisIdentity::syntactic()
        );
    }

    #[test]
    fn a_legacy_baseline_has_no_install_state_and_still_loads() {
        let json = saved(None);
        assert!(!json.contains("node_modules"));
        assert_eq!(saved_node_modules_state(&json), None);
        assert!(serde_json::from_str::<BaselineData>(&json).is_ok());
        let without_identity = r#"{"unused_files":[],"unused_exports":[],"unused_types":[],
            "unused_dependencies":[],"unused_dev_dependencies":[]}"#;
        assert!(serde_json::from_str::<BaselineData>(without_identity).is_ok());
        assert_eq!(saved_node_modules_state(without_identity), None);
        assert_eq!(saved_node_modules_state("not json"), None);
    }

    /// An older fallow reads `analysis_identity` straight into the semantic
    /// identity, so the extra key must not break that read.
    #[test]
    fn the_semantic_identity_ignores_the_install_state_key() {
        let json = saved(Some(NodeModulesState::Missing));
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let identity: SemanticAnalysisIdentity =
            serde_json::from_value(value["analysis_identity"].clone()).expect("older read");
        assert_eq!(identity, SemanticAnalysisIdentity::syntactic());
    }

    #[test]
    fn only_a_known_and_different_state_gives_a_diagnostic() {
        let root = Path::new("/p");
        let path = root.join("fallow-baseline.json");
        let present = saved(Some(NodeModulesState::Present));
        assert!(
            install_state_diagnostic(root, &path, &present, NodeModulesState::Present).is_none()
        );
        assert!(
            install_state_diagnostic(root, &path, &saved(None), NodeModulesState::Missing)
                .is_none()
        );
        let diagnostic = install_state_diagnostic(root, &path, &present, NodeModulesState::Missing)
            .expect("mismatch");
        assert_eq!(
            diagnostic.kind,
            WorkspaceDiagnosticKind::BaselineInstallStateMismatch {
                baseline_node_modules: NodeModulesState::Present,
                run_node_modules: NodeModulesState::Missing,
            }
        );
        assert!(diagnostic.degrades_analysis);
    }
}
