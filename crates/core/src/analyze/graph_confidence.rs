//! Attach `reachability_caveats` to the dead-code findings a file this run
//! never fully analyzed can distort.
//!
//! A run can fail to see a source file's imports in five ways, and every one
//! of them is already recorded in `workspace_diagnostics[]`: the file did not
//! parse cleanly (`source-parse-degraded`), it could not be read
//! (`source-read-failure`), or discovery skipped it before reading it
//! (`skipped-large-file`, `skipped-minified-file`, `skipped-source-dotdir`).
//! All five have the same consequence. An import the run never saw credits
//! nothing, so the module it pointed at can surface as a confident
//! `unused-file` or `unused-export` finding carrying a `delete-file` or
//! `remove-export` action.
//!
//! Those signals stay report-only: oxc emits recoverable errors for valid
//! syntax newer than the parser, and the size guard exists precisely so a
//! multi-megabyte generated file cannot blow up a run, so gating findings on
//! either would mute real results project-wide. But the diagnostic sits at the
//! top of the envelope while the finding it distorts sits far away in
//! `unused_files[]`, and nothing connects the two. This pass carries the
//! caveat to the finding.
//!
//! What can actually be computed. The imports a run never read cannot be
//! attributed to a target: there is no record of them anywhere, so an exact
//! "this unseen file would have imported that path" link does not exist
//! without reading and parsing the source. Three things ARE computable from
//! data the run already holds, and each covers one route an unseen import
//! takes into a verdict:
//!
//! 1. The finding's own file was not fully analyzed, so the export list read
//!    by the "is any export of this file referenced from a reachable module"
//!    test may be truncated.
//! 2. Some module that is observed reachable parsed degraded, so the import
//!    graph a reachability verdict rests on is incomplete.
//! 3. Some file was not analyzed at all, which is enough to distort any
//!    verdict in the run.
//!
//! The last two are run-level conditions, not per-finding ones, and the wire
//! documentation says so. The degraded-parse one discriminates: if every
//! degraded module is itself unreachable, no missing edge attributable to a
//! degraded parse can change a reachability verdict, so no caveat is emitted.
//! That holds because the FIRST missing edge along any entry-point path
//! leaves from a module whose every predecessor edge was observed, so that
//! module is observed reachable, and it is degraded precisely because the
//! edge went missing.
//!
//! A file the run never read gets NO such narrowing, and must not: it has no
//! module and no graph node, so its reachability is not observable at all. A
//! 6 MB file the size guard skipped may be the entry point's only importer of
//! the module now reported unused, and nothing in the run can rule that out.
//! [`WorkspaceDiagnosticKind::source_never_analyzed`][never] is the single place that
//! decides which diagnostics belong to that class; a new skip kind inherits
//! this caveat, and the `fallow fix` withholding that follows it, by being
//! classified there.
//!
//! [never]: fallow_types::workspace::WorkspaceDiagnosticKind::source_never_analyzed
//!
//! The dependency arrays do NOT get the reachability narrowing either, and
//! must not: a package is reported unused when NO module imports its
//! specifier, and fallow counts an import from an unreachable module
//! (verified: an orphan file importing a package keeps that package off
//! `unused_dependencies[]`). So any unseen source anywhere can hide the import
//! that would have credited the package, and every dependency finding in an
//! incomplete run carries the caveat.
//!
//! One consequence is deliberate and worth stating plainly: because the
//! run-level caveat lands on every reachability finding, a single unread file
//! withholds `fallow fix`'s export removals across the WHOLE project,
//! including in files that parsed perfectly. That is the honest answer rather
//! than a coarse one. A hidden import can point anywhere, so there is no
//! subset of findings the unseen file provably cannot reach; narrowing to,
//! say, the unread file's own directory would be a guess presented as
//! evidence. The finding is still reported, the caveat says exactly why the
//! write was withheld, and resolving the diagnostic restores every removal.
//!
//! Nothing here suppresses, filters, reorders, or downgrades a finding. It
//! does reach every mutation surface: `fallow fix`, the LSP quick fix, the MCP
//! fix tools, and the `auto_fixable` flag an agent plans against all ask
//! `MutationEvidence::may_auto_apply_mutation`, which is true exactly when the
//! finding carries no caveat. That single predicate is why widening the class
//! here widens the protection everywhere without touching a consumer, and why
//! the write below goes through `set_reachability_caveats` rather than
//! assigning the field: the setter enforces the gate on the finding's actions
//! in the same call.
//!
//! Members join reachability findings and dependency findings as a third
//! shape. `unused_enum_members[]` does not rest on reachability at all: member
//! usage is collected by walking the member accesses of every module the run
//! parsed, reachable or not, so a member whose only reference lives in an
//! unread file reads as unused exactly like an export does, and the
//! reachability narrowing above does not apply to it. `unused_class_members[]`
//! is the SAME verdict off the SAME walk (`collect_direct_member_accesses`
//! populates one `accessed_members` map that both arrays are scanned against;
//! only the bucket the finding lands in differs by `MemberKind`), so it takes
//! the member rule unchanged rather than one of its own. It reached this list
//! late, on the argument that its removal starts withheld and only the
//! type-aware sidecar opens it. That argument was about `fallow fix`, and the
//! review formats never asked: they render a literal one-click ```suggestion```
//! block for a class member on `rule_id` alone, so an unread file could ship a
//! committable deletion with nothing on the comment saying the evidence was
//! incomplete. `unused_store_members[]` exposes no mutation on any surface
//! (no `Fix` action, no LSP code action, and
//! `store_members_never_offer_unverified_line_deletions` pins the absent
//! review suggestion), so it has nothing to withhold. It is here anyway,
//! because the caveat is a disclosure before it is a gate: a reader deciding
//! by hand whether to delete a store member deserves the same hint as the
//! eight arrays beside it, and a run that reports eight caveated findings
//! and one bare one reads as though the bare one were better evidenced.
//! `unused_types[]` rests
//! on exactly the same reachability test as `unused_exports[]` and gets
//! exactly the same caveats; a type export and a value export in the same file
//! must not render with different confidence when the LSP offers the same
//! quick fix for both.

use std::path::Path;

use rustc_hash::FxHashSet;

use fallow_types::output_dead_code::{CaveatedFinding, ReachabilityCaveat};
use fallow_types::workspace::WorkspaceDiagnostic;

use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::AnalysisResults;

/// The caveats every dependency finding of an incomplete run carries.
/// Reachability does not narrow this one: an import from an unreachable module
/// still credits a package, so any unseen import can hide it.
/// `IncompleteFileAnalysis` never applies, because the file a dependency
/// finding names is a `package.json` rather than a parsed source module.
const DEPENDENCY_CAVEATS: [ReachabilityCaveat; 1] = [ReachabilityCaveat::IncompleteImportGraph];

/// The incomplete-analysis facts of one run, resolved to paths once so the
/// annotation pass is a lookup per finding rather than a scan per finding.
pub(super) struct GraphConfidenceContext<'a> {
    /// Paths whose own extraction is incomplete: a module that parsed with
    /// errors, or a file a diagnostic says was never analyzed. Only the
    /// entries that name a discovered source file can ever match a finding;
    /// a skipped-directory path is carried for uniformity and matches nothing.
    incomplete_paths: FxHashSet<&'a Path>,
    /// Whether some import edge this run's verdicts rest on may be missing.
    graph_incomplete: bool,
}

impl<'a> GraphConfidenceContext<'a> {
    /// Collect the incompletely analyzed files of this run and whether the
    /// import graph can be missing an edge.
    ///
    /// `modules` carries the per-file parse error count and the graph supplies
    /// the path and reachability flag for the same `FileId`, which is what lets
    /// the degraded-parse leg narrow on reachability. `diagnostics` is the run's
    /// workspace-diagnostic list, the ONLY record of a file that was never read:
    /// it has no `ModuleInfo` and no graph node to ask.
    pub(super) fn new(
        graph: &'a ModuleGraph,
        modules: &[ModuleInfo],
        diagnostics: &'a [WorkspaceDiagnostic],
    ) -> Self {
        let mut incomplete_paths = FxHashSet::default();
        let mut graph_incomplete = false;
        for module in modules {
            if module.parse_error_count == 0 {
                continue;
            }
            let Some(node) = graph.modules.get(module.file_id.0 as usize) else {
                continue;
            };
            incomplete_paths.insert(node.path.as_path());
            graph_incomplete = graph_incomplete || node.is_reachable() || node.is_entry_point();
        }
        for diagnostic in diagnostics {
            if !diagnostic.kind.source_never_analyzed() {
                continue;
            }
            incomplete_paths.insert(diagnostic.path.as_path());
            graph_incomplete = true;
        }
        Self {
            incomplete_paths,
            graph_incomplete,
        }
    }

    /// Whether this run analyzed every file it discovered, in which case the
    /// annotation pass has nothing to do and every finding stays byte-identical.
    fn is_clean(&self) -> bool {
        self.incomplete_paths.is_empty() && !self.graph_incomplete
    }

    /// The caveats that apply to a reachability finding reported on `path`.
    /// Returned in enum declaration order, so the result is sorted and
    /// deduplicated by construction.
    fn caveats_for(&self, path: &Path) -> Vec<ReachabilityCaveat> {
        let mut caveats = Vec::new();
        if self.incomplete_paths.contains(path) {
            caveats.push(ReachabilityCaveat::IncompleteFileAnalysis);
        }
        if self.graph_incomplete {
            caveats.push(ReachabilityCaveat::IncompleteImportGraph);
        }
        caveats
    }

    /// The caveats on a MEMBER verdict reported in `path`.
    ///
    /// Members do not rest on reachability, so the narrowing [`Self::new`]
    /// applies to the degraded-parse leg of `graph_incomplete` must not be
    /// reused here. `collect_direct_member_accesses` walks the member accesses
    /// of every module the run resolved with no reachability filter at all, so
    /// an access inside an UNREACHABLE degraded module credits the member
    /// exactly as one inside a reachable module does. The condition is
    /// therefore "any module of this run was incompletely analyzed", the same
    /// one the dependency arrays use, plus the own-file leg a dependency
    /// finding cannot have because its path is a `package.json`.
    ///
    /// Enum members and class members share this rule because they share the
    /// walk: one `accessed_members` map is built once and both kinds are
    /// scanned against it, so nothing about a class member narrows further
    /// than an enum member does.
    fn member_caveats_for(&self, path: &Path) -> Vec<ReachabilityCaveat> {
        let mut caveats = Vec::new();
        if self.incomplete_paths.contains(path) {
            caveats.push(ReachabilityCaveat::IncompleteFileAnalysis);
        }
        caveats.push(ReachabilityCaveat::IncompleteImportGraph);
        caveats
    }

    /// Stamp the caveats onto the verdicts a lost import edge can distort.
    /// A complete run returns without touching a finding.
    ///
    /// Every write goes through [`CaveatedFinding::set_reachability_caveats`],
    /// which is also what withholds the finding's mutating actions, so a new
    /// array added to this loop inherits the gate and one left out of it is a
    /// finding with no caveat rather than a caveated finding with a live
    /// auto-fix.
    pub(super) fn annotate(&self, results: &mut AnalysisResults) {
        if self.is_clean() {
            return;
        }
        for finding in &mut results.unused_files {
            let caveats = self.caveats_for(&finding.file.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_exports {
            let caveats = self.caveats_for(&finding.export.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_types {
            let caveats = self.caveats_for(&finding.export.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_enum_members {
            let caveats = self.member_caveats_for(&finding.member.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_class_members {
            let caveats = self.member_caveats_for(&finding.member.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_store_members {
            let caveats = self.member_caveats_for(&finding.member.path);
            finding.set_reachability_caveats(caveats);
        }
        for finding in &mut results.unused_dependencies {
            finding.set_reachability_caveats(DEPENDENCY_CAVEATS.to_vec());
        }
        for finding in &mut results.unused_dev_dependencies {
            finding.set_reachability_caveats(DEPENDENCY_CAVEATS.to_vec());
        }
        for finding in &mut results.unused_optional_dependencies {
            finding.set_reachability_caveats(DEPENDENCY_CAVEATS.to_vec());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use crate::resolve::ResolvedModule;
    use crate::results::{UnusedDependency, UnusedExport, UnusedFile, UnusedMember};
    use fallow_types::extract::MemberKind;
    use fallow_types::output::IssueAction;
    use fallow_types::output_dead_code::{
        UnusedClassMemberFinding, UnusedDependencyFinding, UnusedEnumMemberFinding,
        UnusedExportFinding, UnusedFileFinding, UnusedStoreMemberFinding, UnusedTypeFinding,
    };
    use fallow_types::results::DependencyLocation;
    use fallow_types::workspace::WorkspaceDiagnosticKind;
    use std::path::PathBuf;

    const ROOT: &str = "/p";
    const INDEX: &str = "/p/src/index.ts";
    const HELPER: &str = "/p/src/helper.ts";
    const ORPHAN: &str = "/p/src/orphan.ts";

    /// A three-file graph whose only entry point is `index.ts`; nothing imports
    /// anything, so `helper.ts` and `orphan.ts` are both unreachable.
    fn graph() -> ModuleGraph {
        let paths = [INDEX, HELPER, ORPHAN];
        let files: Vec<DiscoveredFile> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).expect("test file count fits u32")),
                path: PathBuf::from(path),
                size_bytes: 0,
            })
            .collect();
        let entry_points = vec![EntryPoint {
            path: PathBuf::from(INDEX),
            source: EntryPointSource::ManualEntry,
        }];
        let resolved: Vec<ResolvedModule> = files
            .iter()
            .map(|file| ResolvedModule {
                file_id: file.id,
                path: file.path.clone(),
                ..Default::default()
            })
            .collect();
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    /// `ModuleInfo` values carrying the given parse error count per `FileId`.
    fn modules(error_counts: [u32; 3]) -> Vec<ModuleInfo> {
        error_counts
            .into_iter()
            .enumerate()
            .map(|(index, errors)| ModuleInfo {
                parse_error_count: errors,
                ..ModuleInfo::empty(FileId(
                    u32::try_from(index).expect("test file count fits u32"),
                ))
            })
            .collect()
    }

    fn unused(paths: &[&str]) -> AnalysisResults {
        AnalysisResults {
            unused_files: paths
                .iter()
                .map(|path| {
                    UnusedFileFinding::with_actions(UnusedFile {
                        path: PathBuf::from(path),
                    })
                })
                .collect(),
            ..AnalysisResults::default()
        }
    }

    /// One `unused_enum_members[]` finding for `Color.Blue`, declared in a file
    /// that parsed perfectly. The narrowing that applies to reachability
    /// verdicts must not reach it.
    fn with_unused_enum_member(mut results: AnalysisResults) -> AnalysisResults {
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from(INDEX),
                parent_name: "Color".to_string(),
                member_name: "Blue".to_string(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 2,
            }));
        results
    }

    /// One `unused_class_members[]` finding for `Widget.onlyUsedInBigFile`,
    /// declared in the same perfectly-parsed file as the enum member above so
    /// the two can be compared directly.
    fn with_unused_class_member(mut results: AnalysisResults) -> AnalysisResults {
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from(INDEX),
                parent_name: "Widget".to_string(),
                member_name: "onlyUsedInBigFile".to_string(),
                kind: MemberKind::ClassMethod,
                line: 6,
                col: 2,
            }));
        results
    }

    /// One `unused_store_members[]` finding, declared in the same
    /// perfectly-parsed file as the class member above so the two can be
    /// compared directly.
    fn with_unused_store_member(mut results: AnalysisResults) -> AnalysisResults {
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from(INDEX),
                parent_name: "useCounterStore".to_string(),
                member_name: "onlyUsedInBigFile".to_string(),
                kind: MemberKind::StoreMember,
                line: 7,
                col: 2,
            }));
        results
    }

    /// One `unused_exports[]` and one `unused_types[]` finding in the same
    /// file, so the two can be compared directly.
    fn with_unused_export_and_type(mut results: AnalysisResults) -> AnalysisResults {
        let export = |name: &str| UnusedExport {
            path: PathBuf::from(HELPER),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        };
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(export("helper")));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(export("Shape")));
        results
    }

    /// One `unused_dependencies[]` finding for `lodash`, on top of whatever
    /// unused files the caller asked for.
    fn with_unused_dependency(mut results: AnalysisResults) -> AnalysisResults {
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/p/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
    }

    #[test]
    fn a_run_that_parsed_cleanly_flags_nothing() {
        let graph = graph();
        let mut results = unused(&[HELPER, ORPHAN]);

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &[]).annotate(&mut results);

        assert!(
            results
                .unused_files
                .iter()
                .all(|finding| finding.reachability_caveats.is_empty()),
            "a healthy project must carry no marker at all"
        );
    }

    #[test]
    fn a_degraded_reachable_module_flags_the_findings_its_lost_imports_could_reach() {
        let graph = graph();
        let mut results = unused(&[HELPER, ORPHAN]);

        // The entry file is the one that failed to parse: the imports it never
        // credited are exactly why the other two read as unused.
        GraphConfidenceContext::new(&graph, &modules([3, 0, 0]), &[]).annotate(&mut results);

        for finding in &results.unused_files {
            assert_eq!(
                finding.reachability_caveats,
                vec![ReachabilityCaveat::IncompleteImportGraph],
                "{} should carry the incomplete-graph caveat",
                finding.file.path.display()
            );
        }
    }

    #[test]
    fn a_degraded_unreachable_module_flags_only_itself() {
        let graph = graph();
        let mut results = unused(&[HELPER, ORPHAN]);

        // Only `helper.ts` degraded, and it is unreachable: no missing edge of
        // its can change any verdict but its own, whose export list is short.
        GraphConfidenceContext::new(&graph, &modules([0, 2, 0]), &[]).annotate(&mut results);

        let helper = results
            .unused_files
            .iter()
            .find(|finding| finding.file.path.ends_with("helper.ts"))
            .expect("helper finding present");
        assert_eq!(
            helper.reachability_caveats,
            vec![ReachabilityCaveat::IncompleteFileAnalysis],
            "the degraded file's own truncated export list is the only caveat"
        );

        let orphan = results
            .unused_files
            .iter()
            .find(|finding| finding.file.path.ends_with("orphan.ts"))
            .expect("orphan finding present");
        assert!(
            orphan.reachability_caveats.is_empty(),
            "an unreachable degraded module must not cast a caveat over the whole run"
        );
    }
    #[test]
    fn a_clean_run_leaves_a_dependency_finding_unmarked() {
        let graph = graph();
        let mut results = with_unused_dependency(unused(&[]));

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &[]).annotate(&mut results);

        assert!(
            results.unused_dependencies[0]
                .reachability_caveats
                .is_empty(),
            "a project that parses cleanly must keep the previous wire shape"
        );
    }

    /// The reachability narrowing must NOT be applied to a dependency verdict.
    /// A package is reported unused when no module imports its specifier, and
    /// an import from an unreachable module still credits it, so a degraded
    /// parse in an unreachable file can hide the import that would have kept
    /// the package. `fallow fix` reads this to withhold `remove-dependency`,
    /// the most destructive write it has.
    #[test]
    fn an_unreachable_degraded_module_still_caveats_every_dependency() {
        let graph = graph();
        let mut results = with_unused_dependency(unused(&[HELPER, ORPHAN]));

        GraphConfidenceContext::new(&graph, &modules([0, 2, 0]), &[]).annotate(&mut results);

        assert_eq!(
            results.unused_dependencies[0].reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "an unreachable degraded module can still hide a package import"
        );
        assert!(
            results
                .unused_files
                .iter()
                .find(|finding| finding.file.path.ends_with("orphan.ts"))
                .expect("orphan finding present")
                .reachability_caveats
                .is_empty(),
            "the dependency widening must not leak into the reachability verdicts"
        );
    }

    /// The finding's own file is a `package.json`, never a parsed module, so
    /// the per-file caveat cannot apply however the run degraded.
    #[test]
    fn a_dependency_finding_never_claims_its_own_file_parsed_degraded() {
        let graph = graph();
        let mut results = with_unused_dependency(unused(&[]));

        GraphConfidenceContext::new(&graph, &modules([3, 2, 1]), &[]).annotate(&mut results);

        assert!(
            !results.unused_dependencies[0]
                .reachability_caveats
                .contains(&ReachabilityCaveat::IncompleteFileAnalysis),
            "package.json is not a parsed source module"
        );
    }

    /// Every diagnostic kind the run never read a file for. Each is
    /// constructed once here and driven through the same assertions, so the
    /// protection is a property of the class rather than of the size skip that
    /// exposed the hole.
    fn never_analyzed_kinds() -> Vec<WorkspaceDiagnosticKind> {
        vec![
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
            WorkspaceDiagnosticKind::SkippedMinifiedFile {
                size_bytes: 2 * 1024 * 1024,
            },
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: "permission denied".to_owned(),
            },
        ]
    }

    fn diagnostic(path: &str, kind: WorkspaceDiagnosticKind) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(Path::new(ROOT), PathBuf::from(path), kind)
    }

    /// The hole this pass was reopened by. A 6 MB file whose first line is
    /// `import { needed } from './lib'` is skipped before it is ever read, so
    /// `lib.ts` reads as unused and `fix` offers to delete it or strip its
    /// export. Nothing about that file is in `modules` or in the graph, so the
    /// diagnostic list is the only evidence the run has.
    ///
    /// The assertion is written over the whole class, not over the size skip:
    /// classifying a new kind in
    /// `WorkspaceDiagnosticKind::source_never_analyzed` is the ONLY wiring its
    /// findings need to inherit the caveat, and the `fallow fix` withholding
    /// follows the caveat rather than its cause.
    #[test]
    fn every_never_analyzed_file_caveats_every_reachability_verdict() {
        for kind in never_analyzed_kinds() {
            let id = kind.id();
            assert!(
                kind.source_never_analyzed(),
                "{id} must be classified as a file the run never analyzed"
            );

            let graph = graph();
            let mut results = with_unused_dependency(unused(&[HELPER, ORPHAN]));
            let diagnostics = vec![diagnostic("/p/src/huge.ts", kind)];

            GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
                .annotate(&mut results);

            for finding in &results.unused_files {
                assert_eq!(
                    finding.reachability_caveats,
                    vec![ReachabilityCaveat::IncompleteImportGraph],
                    "{id}: {} rests on an import graph missing an unread file's edges",
                    finding.file.path.display()
                );
            }
            assert_eq!(
                results.unused_dependencies[0].reachability_caveats,
                vec![ReachabilityCaveat::IncompleteImportGraph],
                "{id}: an unread file can hide the import that credits a package"
            );
        }
    }

    /// A file the run never read has no graph node, so the reachability
    /// narrowing that keeps a caveat off a verdict when every degraded module
    /// is unreachable cannot be evaluated for it. The skipped file may be the
    /// entry point's only importer of the module now reported unused, and the
    /// run holds nothing that rules that out.
    #[test]
    fn an_unread_file_is_never_narrowed_away_by_reachability() {
        let graph = graph();
        let mut results = unused(&[ORPHAN]);
        let diagnostics = vec![diagnostic(
            "/p/src/huge.ts",
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        assert_eq!(
            results.unused_files[0].reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "no degraded module is reachable, yet the unread file still taints the verdict"
        );
    }

    /// A read failure is the one kind of the class whose file is still
    /// discovered, so it can itself be reported unused. Its own extraction is
    /// empty, which is the per-finding caveat, on top of the run-level one.
    #[test]
    fn an_unreadable_file_reported_unused_carries_its_own_caveat() {
        let graph = graph();
        let mut results = unused(&[HELPER, ORPHAN]);
        let diagnostics = vec![diagnostic(
            HELPER,
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: "permission denied".to_owned(),
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        let helper = results
            .unused_files
            .iter()
            .find(|finding| finding.file.path.ends_with("helper.ts"))
            .expect("helper finding present");
        assert_eq!(
            helper.reachability_caveats,
            vec![
                ReachabilityCaveat::IncompleteFileAnalysis,
                ReachabilityCaveat::IncompleteImportGraph,
            ],
            "nothing was extracted from a file that could not be read"
        );
    }

    /// Issue #2638: a built-in ignore pattern that removed generated output is
    /// designed behavior, not a degraded run. Raising a caveat here would fire
    /// on every project with a non-gitignored `dist/` or `coverage/` and make
    /// `fallow fix` withhold `delete-file` and `remove-export` project-wide.
    #[test]
    fn a_built_in_ignore_exclusion_flags_nothing() {
        let graph = graph();
        let mut results = with_unused_dependency(unused(&[HELPER, ORPHAN]));
        let diagnostics = vec![diagnostic(
            "/p/packages/web/build",
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/build/**".to_owned(),
                file_count: 12,
                directory_count: 1,
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        assert!(
            results
                .unused_files
                .iter()
                .all(|finding| finding.reachability_caveats.is_empty())
                && results.unused_dependencies[0]
                    .reachability_caveats
                    .is_empty(),
            "generated output the product is designed to exclude raises no caveat"
        );
    }

    /// A diagnostic outside the class must keep a healthy run byte-identical.
    /// `node-modules-missing` fires on every uninstalled tree and
    /// `boundaries-not-configured` fires on every project that never opted in,
    /// so treating either as an unseen import would caveat almost every run.
    #[test]
    fn a_diagnostic_outside_the_class_flags_nothing() {
        let graph = graph();
        let mut results = with_unused_dependency(unused(&[HELPER, ORPHAN]));
        let diagnostics = vec![
            diagnostic(
                "/p/node_modules",
                WorkspaceDiagnosticKind::NodeModulesMissing,
            ),
            diagnostic("/p", WorkspaceDiagnosticKind::BoundariesNotConfigured),
        ];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        assert!(
            results
                .unused_files
                .iter()
                .all(|finding| finding.reachability_caveats.is_empty())
                && results.unused_dependencies[0]
                    .reachability_caveats
                    .is_empty(),
            "only a file the run failed to read may raise a caveat"
        );
    }

    /// A member's usage comes from walking the member accesses of every module
    /// the run PARSED, with no reachability filter, so a member whose only
    /// reference lives in a file the run never read reads as unused exactly
    /// like an export does. Measured: `fix --yes` deleted `Color.Blue` while a
    /// size-skipped file still referenced it.
    ///
    /// The caveat is what withholds that deletion, so this asserts the caveat
    /// AND the action it gates, in one place: the enum member's own file
    /// parsed perfectly here, which is exactly the case where an
    /// own-file-only rule would have missed it.
    #[test]
    fn an_unread_file_caveats_an_enum_member_verdict_and_withholds_its_removal() {
        let graph = graph();
        let mut results = with_unused_enum_member(unused(&[]));
        let diagnostics = vec![diagnostic(
            "/p/src/huge.ts",
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        let member = &results.unused_enum_members[0];
        assert_eq!(
            member.reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "the unread file may hold the only access to this member"
        );
        assert!(
            !member.actions.iter().any(IssueAction::is_auto_fixable),
            "the remove-enum-member action must not advertise itself as applicable"
        );
    }

    /// An unreachable module that parsed degraded cannot change a reachability
    /// verdict, so `unused_files[]` stays clean. It CAN hold the member access
    /// that credits an enum member, because member collection ignores
    /// reachability entirely. Pinning the divergence keeps a future refactor
    /// from reusing the reachability narrowing for members.
    #[test]
    fn a_degraded_unreachable_module_caveats_a_member_but_not_a_file() {
        let graph = graph();
        let mut results = with_unused_class_member(with_unused_enum_member(unused(&[ORPHAN])));

        // `helper.ts` (index 1) is unreachable and parsed with errors.
        GraphConfidenceContext::new(&graph, &modules([0, 2, 0]), &[]).annotate(&mut results);

        assert!(
            results.unused_files[0].reachability_caveats.is_empty(),
            "an unreachable degraded module cannot change a reachability verdict"
        );
        assert_eq!(
            results.unused_enum_members[0].reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "it can still hold the member access that credits this member"
        );
        assert_eq!(
            results.unused_class_members[0].reachability_caveats,
            results.unused_enum_members[0].reachability_caveats,
            "a class member is the same verdict off the same access walk, so it must not \
             render with more confidence than an enum member in the same file"
        );
    }

    /// The measured blocker, at the layer that decides it: `src/big.ts` is the
    /// only caller of `Widget.onlyUsedInBigFile` and the size guard skipped it,
    /// so the member reads as unused solely because of a file the run never
    /// opened. Verified against a release binary: with the guard raised the
    /// finding disappears entirely.
    ///
    /// A store member offers no mutation on any surface: no `Fix` action, no
    /// LSP code action, and `store_members_never_offer_unverified_line_deletions`
    /// pins the absent review suggestion. So this asserts disclosure only, and
    /// asserts the absence of a mutation as the reason the disclosure stands
    /// alone. If a store-member mutation is ever added, this test fails and the
    /// gate has to be reasoned about rather than inherited.
    #[test]
    fn an_unread_file_caveats_a_store_member_verdict_with_no_mutation_to_withhold() {
        let graph = graph();
        let mut results = with_unused_store_member(unused(&[]));
        let diagnostics = vec![diagnostic(
            "/p/src/big.ts",
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        let member = &results.unused_store_members[0];
        assert_eq!(
            member.reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "the unread file may hold the only access to this store member"
        );
        assert!(
            !member.actions.iter().any(IssueAction::is_auto_fixable),
            "a store member must not advertise an applicable mutation"
        );
    }

    /// A clean run leaves the new array bare, so the caveat marks incomplete
    /// evidence rather than becoming decoration on every report.
    #[test]
    fn a_clean_run_leaves_a_store_member_uncaveated() {
        let graph = graph();
        let mut results = with_unused_store_member(unused(&[]));

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &[]).annotate(&mut results);

        assert!(
            results.unused_store_members[0]
                .reachability_caveats
                .is_empty()
        );
    }

    /// The caveat is what stops the review formats from rendering a one-click
    /// deletion for it, so this asserts the caveat AND that no action on the
    /// finding advertises itself as applicable.
    #[test]
    fn an_unread_file_caveats_a_class_member_verdict_and_withholds_its_removal() {
        let graph = graph();
        let mut results = with_unused_class_member(unused(&[]));
        let diagnostics = vec![diagnostic(
            "/p/src/big.ts",
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        let member = &results.unused_class_members[0];
        assert_eq!(
            member.reachability_caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "the unread file may hold the only call to this member"
        );
        assert!(
            !member.actions.iter().any(IssueAction::is_auto_fixable),
            "the remove-class-member action must not advertise itself as applicable"
        );
    }

    /// A type export rests on exactly the reachability test a value export
    /// does, and the LSP offers the same remove-the-`export`-keyword quick fix
    /// for both, so the two must not render with different confidence.
    #[test]
    fn a_type_export_carries_the_same_caveat_as_a_value_export() {
        let graph = graph();
        let mut results = with_unused_export_and_type(unused(&[]));
        let diagnostics = vec![diagnostic(
            "/p/src/huge.ts",
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
        )];

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &diagnostics)
            .annotate(&mut results);

        assert_eq!(
            results.unused_types[0].reachability_caveats,
            results.unused_exports[0].reachability_caveats,
            "an unused type and an unused export in the same file must agree"
        );
        assert!(
            !results.unused_types[0]
                .actions
                .iter()
                .any(IssueAction::is_auto_fixable),
            "and the type's removal is withheld the same way"
        );
    }

    /// The whole mechanism is opt-in on evidence: a run that read every file it
    /// discovered must keep every array byte-identical and every fix live.
    #[test]
    fn a_clean_run_leaves_the_new_arrays_untouched() {
        let graph = graph();
        let mut results = with_unused_enum_member(with_unused_export_and_type(unused(&[ORPHAN])));

        GraphConfidenceContext::new(&graph, &modules([0, 0, 0]), &[]).annotate(&mut results);

        assert!(
            results.unused_enum_members[0]
                .reachability_caveats
                .is_empty()
                && results.unused_types[0].reachability_caveats.is_empty(),
            "a complete run stamps nothing"
        );
        assert!(
            results.unused_enum_members[0]
                .actions
                .iter()
                .any(IssueAction::is_auto_fixable),
            "and withholds nothing"
        );
    }
}
