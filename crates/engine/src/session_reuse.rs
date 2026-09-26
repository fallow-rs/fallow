//! Parse reuse for a session that lives across several analysis runs.
//!
//! An editor keeps one session per project root. Between two runs, only a few
//! files change. The session then parses those files again and keeps the other
//! modules in memory, so a run does not read the persisted parse cache again.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;
use fallow_types::source_fingerprint::SourceFingerprint;

/// The most files that one incremental parse handles. A larger change set,
/// such as a branch switch, takes the full parse path. That path can serve
/// files from the persisted parse cache and writes that cache back.
pub const MAX_INCREMENTAL_REPARSE_FILES: usize = 256;

/// The parse work that a session did since it was created.
///
/// The counts cover the parses that go through the module cache of the
/// session. They grow over the life of the session, so a caller takes the
/// difference of two snapshots to get the work of one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionParseCounts {
    /// Files parsed from source.
    pub modules_parsed: usize,
    /// Files served from the persisted parse cache.
    pub disk_cache_hits: usize,
    /// Files served from the in-memory modules of the session.
    pub modules_reused: usize,
}

impl SessionParseCounts {
    /// The work between an earlier snapshot and this one.
    #[must_use]
    pub const fn since(self, earlier: Self) -> Self {
        Self {
            modules_parsed: self.modules_parsed.saturating_sub(earlier.modules_parsed),
            disk_cache_hits: self.disk_cache_hits.saturating_sub(earlier.disk_cache_hits),
            modules_reused: self.modules_reused.saturating_sub(earlier.modules_reused),
        }
    }
}

/// Thread-safe counters behind [`SessionParseCounts`].
#[derive(Debug, Default)]
pub struct ParseCountCells {
    modules_parsed: AtomicUsize,
    disk_cache_hits: AtomicUsize,
    modules_reused: AtomicUsize,
}

impl ParseCountCells {
    pub fn record(&self, counts: SessionParseCounts) {
        self.modules_parsed
            .fetch_add(counts.modules_parsed, Ordering::Relaxed);
        self.disk_cache_hits
            .fetch_add(counts.disk_cache_hits, Ordering::Relaxed);
        self.modules_reused
            .fetch_add(counts.modules_reused, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SessionParseCounts {
        SessionParseCounts {
            modules_parsed: self.modules_parsed.load(Ordering::Relaxed),
            disk_cache_hits: self.disk_cache_hits.load(Ordering::Relaxed),
            modules_reused: self.modules_reused.load(Ordering::Relaxed),
        }
    }
}

/// The positions whose fingerprint changed. `None` when the two lists do not
/// describe the same files, because then the positions do not line up.
pub fn changed_file_indices(
    previous: &[SourceFingerprint],
    current: &[SourceFingerprint],
) -> Option<Vec<usize>> {
    if previous.len() != current.len() {
        return None;
    }
    Some(
        previous
            .iter()
            .zip(current)
            .enumerate()
            .filter_map(|(index, (before, after))| (before != after).then_some(index))
            .collect(),
    )
}

/// Put the modules of the parsed files in place of their old modules.
///
/// `modules` is in file order, one module for each file that could be read.
/// `reparsed` names the files that were parsed again, and `fresh` holds their
/// new modules in file order. A file that could not be read has no module, so
/// a file can gain or lose its module here.
///
/// When no other owner shares `modules` and each parsed file keeps exactly
/// one module, the modules are replaced in place. Otherwise the function
/// builds a new module list.
pub fn merge_reparsed_modules(
    modules: &mut Arc<[ModuleInfo]>,
    reparsed: &[FileId],
    fresh: Vec<ModuleInfo>,
) {
    let fresh = match replace_in_place(modules, reparsed, fresh) {
        Ok(()) => return,
        Err(fresh) => fresh,
    };
    let mut merged = Vec::with_capacity(modules.len() + fresh.len());
    let mut fresh = fresh.into_iter().peekable();
    for module in modules.iter() {
        while let Some(next) = fresh.next_if(|next| next.file_id.0 < module.file_id.0) {
            merged.push(next);
        }
        if !is_reparsed(reparsed, module.file_id) {
            merged.push(module.clone());
        }
    }
    merged.extend(fresh);
    *modules = merged.into();
}

/// `reparsed` is sorted by file id.
fn is_reparsed(reparsed: &[FileId], file_id: FileId) -> bool {
    reparsed
        .binary_search_by_key(&file_id.0, |reparsed| reparsed.0)
        .is_ok()
}

/// Replace the old modules in place. Returns the fresh modules when that is
/// not possible.
fn replace_in_place(
    modules: &mut Arc<[ModuleInfo]>,
    reparsed: &[FileId],
    fresh: Vec<ModuleInfo>,
) -> Result<(), Vec<ModuleInfo>> {
    let old_count = modules
        .iter()
        .filter(|module| is_reparsed(reparsed, module.file_id))
        .count();
    if old_count != fresh.len() {
        return Err(fresh);
    }
    let Some(slots) = Arc::get_mut(modules) else {
        return Err(fresh);
    };
    let positions: Option<Vec<usize>> = fresh
        .iter()
        .map(|module| {
            slots
                .binary_search_by_key(&module.file_id.0, |slot| slot.file_id.0)
                .ok()
        })
        .collect();
    let Some(positions) = positions else {
        return Err(fresh);
    };
    for (position, module) in positions.into_iter().zip(fresh) {
        slots[position] = module;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(file_id: u32, content_hash: u64) -> ModuleInfo {
        let mut module = fallow_extract::parse_source_to_module(
            FileId(file_id),
            std::path::Path::new("module.ts"),
            "export const value = 1;\n",
            content_hash,
            false,
        );
        module.content_hash = content_hash;
        module
    }

    fn ids_and_hashes(modules: &[ModuleInfo]) -> Vec<(u32, u64)> {
        modules
            .iter()
            .map(|module| (module.file_id.0, module.content_hash))
            .collect()
    }

    #[test]
    fn changed_file_indices_lists_the_moved_fingerprints() {
        let before = [
            SourceFingerprint::new(1, 10),
            SourceFingerprint::new(2, 20),
            SourceFingerprint::new(3, 30),
        ];
        let after = [
            SourceFingerprint::new(1, 10),
            SourceFingerprint::new(9, 21),
            SourceFingerprint::new(3, 30),
        ];

        assert_eq!(changed_file_indices(&before, &after), Some(vec![1]));
        assert_eq!(changed_file_indices(&before, &after[..2]), None);
    }

    #[test]
    fn an_unshared_module_list_is_updated_in_place() {
        let mut modules: Arc<[ModuleInfo]> = vec![module(0, 1), module(1, 1), module(2, 1)].into();
        let before = Arc::as_ptr(&modules).cast::<ModuleInfo>();

        merge_reparsed_modules(&mut modules, &[FileId(1)], vec![module(1, 2)]);

        assert_eq!(ids_and_hashes(&modules), vec![(0, 1), (1, 2), (2, 1)]);
        assert_eq!(
            Arc::as_ptr(&modules).cast::<ModuleInfo>(),
            before,
            "no other owner holds the list, so the update needs no copy"
        );
    }

    #[test]
    fn a_shared_module_list_is_copied_and_the_other_owner_keeps_the_old_modules() {
        let mut modules: Arc<[ModuleInfo]> = vec![module(0, 1), module(1, 1)].into();
        let other_owner = Arc::clone(&modules);

        merge_reparsed_modules(&mut modules, &[FileId(0)], vec![module(0, 2)]);

        assert_eq!(ids_and_hashes(&modules), vec![(0, 2), (1, 1)]);
        assert_eq!(ids_and_hashes(&other_owner), vec![(0, 1), (1, 1)]);
    }

    #[test]
    fn a_file_can_gain_or_lose_its_module() {
        let mut modules: Arc<[ModuleInfo]> = vec![module(0, 1), module(2, 1)].into();

        merge_reparsed_modules(&mut modules, &[FileId(1), FileId(2)], vec![module(1, 2)]);

        assert_eq!(
            ids_and_hashes(&modules),
            vec![(0, 1), (1, 2)],
            "file 1 became readable and file 2 did not, so the list follows the files"
        );
    }

    const MODULE_COUNT: usize = 4;

    /// A project with an entry that imports each module. Each module has one
    /// unused export.
    fn write_project(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("src")).expect("create source directory");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"session-reuse","private":true,"main":"src/index.ts"}"#,
        )
        .expect("write package");
        let mut entry = String::new();
        for index in 0..MODULE_COUNT {
            use std::fmt::Write as _;
            std::fs::write(
                root.join(format!("src/module{index}.ts")),
                format!(
                    "export const used{index} = {index};\nexport const unused{index} = {index};\n"
                ),
            )
            .expect("write module");
            write!(
                entry,
                "import {{ used{index} }} from './module{index}';\nconsole.log(used{index});\n"
            )
            .expect("write to a String");
        }
        std::fs::write(root.join("src/index.ts"), entry).expect("write entry");
    }

    fn unused_export_names(session: &crate::session::AnalysisSession) -> Vec<String> {
        let mut names: Vec<String> = session
            .analyze_dead_code()
            .expect("analysis succeeds")
            .results
            .unused_exports
            .iter()
            .map(|finding| finding.export.export_name.clone())
            .collect();
        names.sort();
        names
    }

    fn file_count(session: &crate::session::AnalysisSession) -> usize {
        session.files().len()
    }

    #[test]
    fn a_warm_session_parses_only_the_changed_file() {
        let project = tempfile::tempdir().expect("project");
        write_project(project.path());
        let session = crate::session::AnalysisSession::load_default(project.path());
        unused_export_names(&session);
        let first = session.parse_counts();

        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const unused1 = 1;\nexport const added = 1;\n",
        )
        .expect("add an unused export");
        let names = unused_export_names(&session);

        assert!(names.contains(&"added".to_string()), "{names:?}");
        assert_eq!(
            session.parse_counts().since(first),
            SessionParseCounts {
                modules_parsed: 1,
                disk_cache_hits: 0,
                modules_reused: MODULE_COUNT,
            },
            "only the changed file is parsed, and the persisted cache is not read"
        );
    }

    #[test]
    fn a_refreshed_session_sees_created_and_deleted_files() {
        let project = tempfile::tempdir().expect("project");
        write_project(project.path());
        let mut session = crate::session::AnalysisSession::load_default(project.path());
        unused_export_names(&session);

        assert!(!session.refresh_discovery(), "nothing changed on disk");
        std::fs::remove_file(project.path().join("src/module3.ts")).expect("delete module");
        std::fs::write(
            project.path().join("src/index.ts"),
            "import { used0 } from './module0';\nconsole.log(used0);\n",
        )
        .expect("drop the imports");
        std::fs::write(
            project.path().join("src/created.ts"),
            "export const fresh = 1;\n",
        )
        .expect("create a file");

        assert!(session.refresh_discovery(), "the file set changed");
        assert_eq!(file_count(&session), MODULE_COUNT + 1);
        let unused_files: Vec<String> = session
            .analyze_dead_code()
            .expect("analysis succeeds")
            .results
            .unused_files
            .iter()
            .map(|finding| {
                finding
                    .file
                    .path
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            unused_files.contains(&"created.ts".to_string()),
            "the created file is analyzed: {unused_files:?}"
        );
        assert!(
            !unused_files.contains(&"module3.ts".to_string()),
            "the deleted file leaves no finding: {unused_files:?}"
        );
    }

    #[test]
    fn a_flush_stores_the_reparsed_module_for_the_next_session() {
        let project = tempfile::tempdir().expect("project");
        write_project(project.path());
        let session = crate::session::AnalysisSession::load_default(project.path());
        unused_export_names(&session);
        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const changed = 1;\n",
        )
        .expect("change a module");
        unused_export_names(&session);
        session.flush_parse_cache();
        drop(session);

        let next = crate::session::AnalysisSession::load_default(project.path());
        let names = unused_export_names(&next);

        assert!(names.contains(&"changed".to_string()), "{names:?}");
        assert_eq!(
            next.parse_counts().modules_parsed,
            0,
            "the persisted cache holds the module of the incremental parse"
        );
    }

    #[test]
    fn a_flush_never_stores_an_older_module_under_a_newer_fingerprint() {
        let project = tempfile::tempdir().expect("project");
        write_project(project.path());
        let session = crate::session::AnalysisSession::load_default(project.path());
        unused_export_names(&session);
        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const first = 1;\n",
        )
        .expect("first change");
        unused_export_names(&session);
        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = 1;\nexport const secondChange = 1;\n",
        )
        .expect("second change, not analyzed");
        session.flush_parse_cache();
        drop(session);

        let next = crate::session::AnalysisSession::load_default(project.path());
        let names = unused_export_names(&next);

        assert!(names.contains(&"secondChange".to_string()), "{names:?}");
        assert!(!names.contains(&"first".to_string()), "{names:?}");
    }

    /// The read failures and parse degradations of the project, as file
    /// names with a kind.
    fn source_diagnostics(session: &crate::session::AnalysisSession) -> Vec<String> {
        use fallow_types::workspace::WorkspaceDiagnosticKind;
        let mut found: Vec<String> = session
            .current_workspace_diagnostics()
            .into_iter()
            .filter_map(|diagnostic| {
                let kind = match diagnostic.kind {
                    WorkspaceDiagnosticKind::SourceReadFailure { .. } => "read",
                    WorkspaceDiagnosticKind::SourceParseDegraded { .. } => "parse",
                    _ => return None,
                };
                let name = diagnostic.path.file_name()?.to_string_lossy().into_owned();
                Some(format!("{kind}:{name}"))
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn an_incremental_parse_keeps_the_source_diagnostics_of_the_project_current() {
        let project = tempfile::tempdir().expect("project");
        write_project(project.path());
        let session = crate::session::AnalysisSession::load_default(project.path());
        unused_export_names(&session);
        assert!(source_diagnostics(&session).is_empty());

        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = ;\n",
        )
        .expect("break the syntax");
        std::fs::write(project.path().join("src/module2.ts"), [0xff, 0xfe, 0x00])
            .expect("invalid UTF-8");
        unused_export_names(&session);
        assert_eq!(
            source_diagnostics(&session),
            ["parse:module1.ts", "read:module2.ts"],
            "the incremental parse reports the new problems"
        );

        std::fs::write(
            project.path().join("src/module1.ts"),
            "export const used1 = 1;\n",
        )
        .expect("fix the syntax");
        unused_export_names(&session);
        assert_eq!(
            source_diagnostics(&session),
            ["read:module2.ts"],
            "the fixed file loses its entry, and the unchanged file keeps its entry"
        );
        assert_eq!(session.parse_counts().disk_cache_hits, 0, "no full parse");
    }

    #[test]
    fn counts_since_an_earlier_snapshot_give_the_work_of_one_run() {
        let cells = ParseCountCells::default();
        cells.record(SessionParseCounts {
            modules_parsed: 3,
            disk_cache_hits: 0,
            modules_reused: 0,
        });
        let first = cells.snapshot();
        cells.record(SessionParseCounts {
            modules_parsed: 1,
            disk_cache_hits: 0,
            modules_reused: 2,
        });

        assert_eq!(
            cells.snapshot().since(first),
            SessionParseCounts {
                modules_parsed: 1,
                disk_cache_hits: 0,
                modules_reused: 2,
            }
        );
    }
}
