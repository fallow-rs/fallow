//! Hooks for the save-to-publish lab bench in `crates/benchmarks`.
//!
//! This module is not a stable API. It drives the same analysis, diagnostic
//! build and publish plan as a server run, without a client connection. It
//! models a push client with no open documents, so every planned publish is a
//! message that the client receives.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use ls_types::Uri;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::{BlockingAnalysisInput, SharedSessionStore, run_blocking_analysis};
use crate::diagnostic_filter::attach_changed_since_data;
use crate::document_state::VersionSnapshot;
use crate::publish::{DiagnosticCache, PublishContext, plan_clears, plan_new_diagnostics};
use crate::session_store::EditorSessionStore;

/// The counts of one save in the lab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavePublishCounts {
    /// Files with at least one diagnostic after the save.
    pub files_with_diagnostics: usize,
    /// `textDocument/publishDiagnostics` messages that the save sends.
    pub publishes: usize,
    /// Project sessions that loaded the config and walked the project.
    pub sessions_loaded: usize,
    /// Files parsed from source.
    pub modules_parsed: usize,
    /// Files served from the persisted parse cache.
    pub disk_cache_hits: usize,
}

/// One editor session in the lab. It keeps the pull cache, the previous URI
/// set and the project sessions across saves, as the server does.
pub struct SavePublishLab {
    root: PathBuf,
    cache: DiagnosticCache,
    previous_uris: FxHashSet<Uri>,
    sessions: SharedSessionStore,
}

impl SavePublishLab {
    /// Start a lab for the project at `root`. It keeps the project sessions
    /// between saves, as the server does for a client that reports file
    /// changes.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self::with_session_reuse(root, true)
    }

    /// Start a lab that keeps the project sessions between saves only when
    /// `reuse` is true. With `reuse` false, each save loads a new session,
    /// as the server does with `FALLOW_LSP_REUSE_SESSION=0`.
    #[must_use]
    pub fn with_session_reuse(root: &Path, reuse: bool) -> Self {
        Self {
            root: crate::path_utils::canonicalize_for_lsp(root),
            cache: DiagnosticCache::default(),
            previous_uris: FxHashSet::default(),
            sessions: Arc::new(std::sync::Mutex::new(EditorSessionStore::new(reuse))),
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
            run_cancellation: Arc::new(AtomicBool::new(false)),
            sessions: Arc::clone(&self.sessions),
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
            sessions_loaded: output.parse_work.sessions_loaded,
            modules_parsed: output.parse_work.parse.modules_parsed,
            disk_cache_hits: output.parse_work.parse.disk_cache_hits,
        })
    }
}

/// Map `(line, byte column)` positions in the file at `path` to UTF-16
/// columns with one position mapper, as one diagnostic build does. Returns
/// the sum of the columns, so the caller can check the result.
#[must_use]
pub fn map_utf16_columns(path: &Path, positions: &[(u32, u32)]) -> u64 {
    let mut mapper = crate::position::PositionMapper::default();
    positions
        .iter()
        .map(|&(line, col)| u64::from(mapper.utf16_col(path, line, col)))
        .sum()
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
                sessions_loaded: 1,
                modules_parsed: MODULE_COUNT + 1,
                disk_cache_hits: 0,
            }
        );
    }

    #[test]
    fn noop_save_publishes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        let counts = lab.save().expect("second save succeeds");

        assert_eq!(counts.files_with_diagnostics, MODULE_COUNT);
        assert_eq!(
            counts.publishes, 0,
            "no diagnostic changed, so nothing is sent"
        );
    }

    #[test]
    fn one_changed_file_publishes_only_that_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        std::fs::write(
            dir.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const unused1 = 1;\nexport const added = 1;\n",
        )
        .expect("add an unused export");
        let counts = lab.save().expect("second save succeeds");

        assert_eq!(counts.publishes, 1);
    }

    // Unix only: other platforms expose no inode change time, so a kept
    // session parses again on every save and the parse counts differ.
    #[cfg(unix)]
    #[test]
    fn a_second_save_parses_only_the_changed_file_in_the_kept_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        std::fs::write(
            dir.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const unused1 = 1;\nexport const added = 1;\n",
        )
        .expect("add an unused export");
        let counts = lab.save().expect("second save succeeds");

        assert_eq!(
            (
                counts.sessions_loaded,
                counts.modules_parsed,
                counts.disk_cache_hits
            ),
            (0, 1, 0),
            "the kept session loads no config and reads no parse cache"
        );
    }

    // Unix only: other platforms expose no inode change time, so a kept
    // session parses again on every save and the parse counts differ.
    #[cfg(unix)]
    #[test]
    fn a_noop_save_parses_nothing_in_the_kept_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        let counts = lab.save().expect("second save succeeds");

        assert_eq!(
            (
                counts.sessions_loaded,
                counts.modules_parsed,
                counts.disk_cache_hits
            ),
            (0, 0, 0)
        );
    }

    #[test]
    fn without_reuse_each_save_loads_a_session_and_reads_the_parse_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::with_session_reuse(dir.path(), false);
        lab.save().expect("first save succeeds");

        let counts = lab.save().expect("second save succeeds");

        assert_eq!(
            (
                counts.sessions_loaded,
                counts.modules_parsed,
                counts.disk_cache_hits
            ),
            (1, 0, MODULE_COUNT + 1)
        );
    }

    #[test]
    fn a_deleted_file_leaves_no_diagnostics_in_the_kept_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        std::fs::remove_file(dir.path().join("src/module2.ts")).expect("delete a module");
        std::fs::write(
            dir.path().join("src/index.ts"),
            "import { used0 } from \"./module0\";\nimport { used1 } from \"./module1\";\nconsole.log(used0, used1);\n",
        )
        .expect("drop the import of the deleted module");
        let counts = lab.save().expect("second save succeeds");

        assert_eq!(counts.sessions_loaded, 0);
        assert_eq!(
            counts.files_with_diagnostics,
            MODULE_COUNT - 1,
            "the deleted module has no findings left"
        );
        assert_eq!(
            counts.publishes, 1,
            "a clear goes out for the deleted module"
        );
    }

    #[test]
    fn a_created_file_gets_diagnostics_in_the_kept_session() {
        let dir = tempfile::tempdir().expect("temp dir");
        write_lab_fixture(dir.path());
        let mut lab = SavePublishLab::new(dir.path());
        lab.save().expect("first save succeeds");

        std::fs::write(
            dir.path().join("src/orphan.ts"),
            "export const orphan = 1;\n",
        )
        .expect("create an unreachable file");
        let counts = lab.save().expect("second save succeeds");

        assert_eq!(counts.sessions_loaded, 0);
        assert_eq!(counts.files_with_diagnostics, MODULE_COUNT + 1);
        assert_eq!(counts.publishes, 1);
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
        assert_eq!(
            counts.publishes, 1,
            "only the file that lost its findings needs a publish, which is empty",
        );
    }
}
