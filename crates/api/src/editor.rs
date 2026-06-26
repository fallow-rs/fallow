//! Editor-facing analysis contracts shared by LSP and future editor adapters.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

pub use fallow_engine::changed_files::{
    ChangedFilesError, resolve_git_toplevel, try_get_changed_files_with_toplevel,
};

pub type EditorAnalysisResults = fallow_engine::AnalysisResults;
pub type EditorAnalysisSession = fallow_engine::AnalysisSession;
pub type EditorDeadCodeAnalysisOutput = fallow_engine::DeadCodeAnalysisOutput;
pub type EditorDuplicationReport = fallow_engine::DuplicationReport;
pub type EditorProjectAnalysisOutput = fallow_engine::ProjectAnalysisOutput;

/// Dead-code and duplication output shaped for editor integrations.
#[derive(Debug, Default)]
pub struct EditorAnalysisOutput {
    pub results: EditorAnalysisResults,
    pub duplication: EditorDuplicationReport,
}

impl EditorAnalysisOutput {
    #[must_use]
    pub const fn new(results: EditorAnalysisResults, duplication: EditorDuplicationReport) -> Self {
        Self {
            results,
            duplication,
        }
    }

    #[must_use]
    pub fn from_project_output(output: EditorProjectAnalysisOutput) -> Self {
        Self::new(output.dead_code.results, output.duplication)
    }

    pub fn merge_project_output(&mut self, output: EditorProjectAnalysisOutput) {
        self.merge_results(output.dead_code.results);
        self.merge_duplication(output.duplication);
    }

    pub fn merge_results(&mut self, source: EditorAnalysisResults) {
        self.results.merge_into(source);
    }

    pub fn merge_duplication(&mut self, source: EditorDuplicationReport) {
        self.duplication.clone_groups.extend(source.clone_groups);
        self.duplication
            .clone_families
            .extend(source.clone_families);
        self.duplication
            .mirrored_directories
            .extend(source.mirrored_directories);
        self.duplication.stats.clone_groups += source.stats.clone_groups;
        self.duplication.stats.clone_instances += source.stats.clone_instances;
        self.duplication.stats.total_files += source.stats.total_files;
        self.duplication.stats.files_with_clones += source.stats.files_with_clones;
        self.duplication.stats.total_lines += source.stats.total_lines;
        self.duplication.stats.duplicated_lines += source.stats.duplicated_lines;
        self.duplication.stats.total_tokens += source.stats.total_tokens;
        self.duplication.stats.duplicated_tokens += source.stats.duplicated_tokens;
        self.duplication.stats.clone_groups_below_min_occurrences +=
            source.stats.clone_groups_below_min_occurrences;
        self.duplication.stats.duplication_percentage = if self.duplication.stats.total_lines > 0 {
            (self.duplication.stats.duplicated_lines as f64
                / self.duplication.stats.total_lines as f64)
                * 100.0
        } else {
            0.0
        };
    }

    pub fn filter_by_changed_files(&mut self, changed_files: &FxHashSet<PathBuf>, root: &Path) {
        fallow_engine::changed_files::filter_results_by_changed_files(
            &mut self.results,
            changed_files,
        );
        fallow_engine::changed_files::filter_duplication_by_changed_files(
            &mut self.duplication,
            changed_files,
            root,
        );
    }

    pub fn filter_by_changed_since(
        &mut self,
        root: &Path,
        toplevel: &Path,
        git_ref: &str,
    ) -> Result<usize, ChangedFilesError> {
        let changed = try_get_changed_files_with_toplevel(root, toplevel, git_ref)?;
        let changed_count = changed.len();
        self.filter_by_changed_files(&changed, root);
        Ok(changed_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fallow_engine::duplicates::{CloneGroup, CloneInstance, DuplicationStats};

    #[test]
    fn merges_duplication_stats_and_recomputes_percentage() {
        let mut output = EditorAnalysisOutput {
            duplication: EditorDuplicationReport {
                clone_groups: vec![CloneGroup {
                    instances: vec![CloneInstance {
                        file: PathBuf::from("src/a.ts"),
                        start_line: 1,
                        end_line: 4,
                        start_col: 0,
                        end_col: 10,
                        fragment: "const a = 1;".to_string(),
                    }],
                    token_count: 8,
                    line_count: 4,
                }],
                clone_families: Vec::new(),
                mirrored_directories: Vec::new(),
                stats: DuplicationStats {
                    clone_groups: 1,
                    clone_instances: 1,
                    total_files: 1,
                    files_with_clones: 1,
                    total_lines: 20,
                    duplicated_lines: 4,
                    total_tokens: 80,
                    duplicated_tokens: 8,
                    duplication_percentage: 20.0,
                    clone_groups_below_min_occurrences: 1,
                },
            },
            ..Default::default()
        };

        output.merge_duplication(EditorDuplicationReport {
            clone_groups: Vec::new(),
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats {
                clone_groups: 0,
                clone_instances: 0,
                total_files: 1,
                files_with_clones: 0,
                total_lines: 30,
                duplicated_lines: 6,
                total_tokens: 120,
                duplicated_tokens: 12,
                duplication_percentage: 20.0,
                clone_groups_below_min_occurrences: 2,
            },
        });

        assert_eq!(output.duplication.stats.total_lines, 50);
        assert_eq!(output.duplication.stats.duplicated_lines, 10);
        assert_eq!(
            output.duplication.stats.clone_groups_below_min_occurrences,
            3
        );
        assert!((output.duplication.stats.duplication_percentage - 20.0).abs() < f64::EPSILON);
    }
}
