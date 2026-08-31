//! Per-file branching totals for the audit's base-versus-head comparison.
//!
//! Deliberately independent of the health findings path. That path drops every
//! unit below the configured threshold and skips suppressed units entirely, so
//! aggregating there would let one `fallow-ignore-next-line complexity` comment
//! remove a unit's branches from the total. These totals are threshold-blind and
//! suppression-blind by construction.

use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::{FileBranching, ModuleInfo};
use rustc_hash::FxHashMap;
use std::path::PathBuf;

/// Branching totals per absolute file path.
///
/// Empty when the run did not request complexity, because `ModuleInfo`
/// carries no units in that case and an all-zero map would be
/// indistinguishable from a project with no branching.
pub type BranchingByFile = FxHashMap<PathBuf, FileBranching>;

/// Aggregate every analyzed module into per-file branching totals.
///
/// Files whose module holds no units are omitted rather than recorded as zero:
/// a file with no functions contributes nothing to either term, and omitting it
/// keeps the payload proportional to the code that has units.
#[must_use]
pub fn branching_by_file(files: &[DiscoveredFile], modules: &[ModuleInfo]) -> BranchingByFile {
    let mut by_file = BranchingByFile::default();
    for module in modules {
        if module.complexity.is_empty() {
            continue;
        }
        let Some(file) = files.get(module.file_id.0 as usize) else {
            continue;
        };
        let totals = FileBranching::from_units(&module.complexity);
        if totals.functions == 0 {
            continue;
        }
        by_file.insert(file.path.clone(), totals);
    }
    by_file
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::discover::FileId;
    use fallow_types::extract::{
        ComplexityContribution, ComplexityContributionKind, ComplexityMetric, FunctionComplexity,
    };

    fn file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 0,
        }
    }

    fn unit(name: &str, cyclomatic: u16, branches: u16) -> FunctionComplexity {
        FunctionComplexity {
            name: name.to_string(),
            is_private_member: false,
            line: 1,
            col: 0,
            cyclomatic,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: std::iter::repeat_n(
                ComplexityContribution {
                    line: 1,
                    col: 0,
                    metric: ComplexityMetric::Cyclomatic,
                    kind: ComplexityContributionKind::If,
                    weight: 1,
                    nesting: 0,
                },
                usize::from(branches),
            )
            .collect(),
        }
    }

    fn module(file_id: u32, units: Vec<FunctionComplexity>) -> ModuleInfo {
        ModuleInfo {
            complexity: units,
            ..ModuleInfo::empty(FileId(file_id))
        }
    }

    #[test]
    fn aggregates_each_module_under_its_path() {
        let files = vec![file(0, "/p/a.ts"), file(1, "/p/b.ts")];
        let modules = vec![
            module(0, vec![unit("one", 3, 2), unit("two", 2, 1)]),
            module(1, vec![unit("three", 1, 0)]),
        ];

        let by_file = branching_by_file(&files, &modules);

        let a = by_file[&PathBuf::from("/p/a.ts")];
        assert_eq!(a.branch_points, 3);
        assert_eq!(a.functions, 2);
        assert_eq!(a.peak_cyclomatic, 3);
        assert_eq!(a.implied_cyclomatic(), 5, "2 functions carrying 3 branches");

        let b = by_file[&PathBuf::from("/p/b.ts")];
        assert_eq!(b.branch_points, 0);
        assert_eq!(b.functions, 1);
    }

    #[test]
    fn omits_modules_with_no_units() {
        let files = vec![file(0, "/p/a.ts")];
        let modules = vec![module(0, Vec::new())];
        assert!(branching_by_file(&files, &modules).is_empty());
    }

    #[test]
    fn omits_modules_whose_only_units_are_synthetic() {
        let files = vec![file(0, "/p/a.vue")];
        let modules = vec![module(0, vec![unit("<template>", 4, 3)])];
        assert!(
            branching_by_file(&files, &modules).is_empty(),
            "a synthetic template unit is excluded, so the file has nothing to report"
        );
    }

    #[test]
    fn skips_a_module_whose_file_id_is_out_of_range() {
        let files = vec![file(0, "/p/a.ts")];
        let modules = vec![module(7, vec![unit("one", 2, 1)])];
        assert!(branching_by_file(&files, &modules).is_empty());
    }
}
