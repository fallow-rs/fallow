//! Hooks for the save-to-publish lab bench in `crates/benchmarks`.
//!
//! This module is not a stable API. It drives the same analysis, diagnostic
//! build and publish plan as a server run, without a client connection. It
//! models a push client with no open documents, so every planned publish is a
//! message that the client receives.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ls_types::{Diagnostic, Uri};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::{BlockingAnalysisInput, run_blocking_analysis};
use crate::diagnostic_filter::attach_changed_since_data;
use crate::document_state::VersionSnapshot;
use crate::publish::{PublishContext, plan_clears, plan_new_diagnostics};

/// The counts of one save in the lab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePublishCounts {
    /// Files with at least one diagnostic after the save.
    pub files_with_diagnostics: usize,
    /// `textDocument/publishDiagnostics` messages that the save sends.
    pub publishes: usize,
}

/// One editor session in the lab. It keeps the pull cache and the previous
/// URI set across saves, as the server does.
pub struct SavePublishLab {
    root: PathBuf,
    cache: FxHashMap<Uri, Vec<Diagnostic>>,
    previous_uris: FxHashSet<Uri>,
}

impl SavePublishLab {
    /// Start a session for the project at `root`.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: crate::path_utils::canonicalize_for_lsp(root),
            cache: FxHashMap::default(),
            previous_uris: FxHashSet::default(),
        }
    }

    /// Run the work of one save: analysis, diagnostic build, publish plan.
    ///
    /// # Errors
    ///
    /// Returns the analysis error message when the project analysis fails.
    pub fn save(&mut self) -> Result<SavePublishCounts, String> {
        let input = BlockingAnalysisInput {
            project_roots: vec![self.root.clone()],
            config_path: None,
            allow_remote_extends: false,
            duplication_options: None,
            production_override: None,
            inline_complexity_enabled: false,
            type_aware_options: None,
            type_aware_sessions: Arc::default(),
            type_aware_changes: fallow_api::TypeAwareFileChanges::default(),
            root: self.root.clone(),
            toplevel: None,
            changed_since: None,
            cancellation: Arc::new(AtomicBool::new(false)),
        };
        let output = run_blocking_analysis(&input).map_err(|error| error.to_string())?;
        let mut diagnostics_by_file =
            crate::diagnostics::build_diagnostics(crate::diagnostics::DiagnosticInput::new(
                &output.analysis.results,
                &output.analysis.duplication,
                &self.root,
            ));
        attach_changed_since_data(&mut diagnostics_by_file, None);
        let files_with_diagnostics = diagnostics_by_file.len();

        let disabled = FxHashSet::default();
        let snapshot = VersionSnapshot::default();
        let live_documents = FxHashMap::default();
        let context = PublishContext {
            disabled: &disabled,
            snapshot: &snapshot,
            live_documents: &live_documents,
        };
        let plan = plan_new_diagnostics(&mut self.cache, diagnostics_by_file, &context);
        let mut new_uris = plan.new_uris;
        let clears = plan_clears(
            &mut self.cache,
            &self.previous_uris,
            &mut new_uris,
            &context,
        );
        self.previous_uris = new_uris;
        Ok(SavePublishCounts {
            files_with_diagnostics,
            publishes: plan.publishes.len() + clears.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    const MODULE_COUNT: usize = 3;

    fn write_lab_fixture(root: &Path) {
        std::fs::create_dir_all(root.join("src")).expect("create src dir");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"lsp-lab","private":true,"main":"src/index.ts"}"#,
        )
        .expect("write package");
        let mut entry = String::new();
        for index in 0..MODULE_COUNT {
            std::fs::write(
                root.join(format!("src/module{index}.ts")),
                format!(
                    "export const used{index} = {index};\nexport const unused{index} = {index};\n"
                ),
            )
            .expect("write module");
            writeln!(
                entry,
                "import {{ used{index} }} from \"./module{index}\";\nconsole.log(used{index});"
            )
            .expect("write to a String");
        }
        std::fs::write(root.join("src/index.ts"), entry).expect("write entry");
    }

    #[test]
    fn first_save_publishes_every_file_with_findings() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());

        let counts = lab.save().expect("lab save succeeds");

        assert_eq!(
            counts,
            SavePublishCounts {
                files_with_diagnostics: MODULE_COUNT,
                publishes: MODULE_COUNT,
            }
        );
    }

    #[test]
    fn removed_findings_publish_a_clear() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        std::fs::write(
            dir.path().join("src/module0.ts"),
            "export const used0 = 0;\n",
        )
        .expect("remove the unused export");
        let counts = lab.save().expect("second save succeeds");

        assert_eq!(counts.files_with_diagnostics, MODULE_COUNT - 1);
        assert!(
            counts.publishes >= 1,
            "the file that lost its findings needs an empty publish",
        );
    }
}
