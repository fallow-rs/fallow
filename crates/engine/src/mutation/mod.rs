//! The static half of mutation testing: select, map, prune and rank mutants.
//!
//! The plan reads only facts that the dead-code analysis already has: the
//! module graph with its test reachability (including the `vi.mock`
//! replacement masks), the dead-code results, the function complexity of each
//! module, and the captured security sink sites. It reads the source text of
//! each selected file one time to instrument it, and it holds the
//! instrumented code in memory. It never writes a file and never runs user
//! code.
//!
//! Each mutant gets one disposition:
//!
//! - `planned`: a test file reaches the file, and no static rule prunes the
//!   mutant. The runner executes these mutants in rank order.
//! - `pending`: a planned mutant after the `max_mutants` limit.
//! - `no-coverage`: no test file reaches the file in the graph. A test that
//!   replaces the module with `vi.mock` does not reach it.
//! - `pruned`: a static rule removes the mutant (see [`PruneReason`](crate::mutation::PruneReason)).

mod layout;
mod plan;
mod reach;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

pub use fallow_mutation::{
    FunctionRef, Mutant, MutantId, MutationError, Operator, RESERVED_PREFIX, SourceKind,
};
pub use fallow_output::DiffIndex;
pub use layout::{instrument_source, is_component_file};

use crate::EngineError;
use crate::module_graph::{ExportTestReach, RetainedModuleGraph};
use crate::session::AnalysisSession;
use crate::source::ModuleInfo;
use fallow_types::results::AnalysisResults;

/// The runner config modules at the project root. Each test run loads them,
/// so they and the files that they reach through static imports are in
/// [`MutationPlan::support_files`].
pub const RUNNER_CONFIG_MODULES: [&str; 18] = [
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
    "vite.config.ts",
    "vite.config.mts",
    "vite.config.cts",
    "vite.config.js",
    "vite.config.mjs",
    "vite.config.cjs",
    "vitest.workspace.ts",
    "vitest.workspace.mts",
    "vitest.workspace.cts",
    "vitest.workspace.js",
    "vitest.workspace.mjs",
    "vitest.workspace.cjs",
];

/// The files and lines that a plan mutates.
#[derive(Debug, Clone)]
pub enum MutationScope {
    /// The added lines of a diff against the base ref. Build the index with
    /// the existing diff machinery (`changed_files::try_get_changed_diff`,
    /// `DiffIndex::from_unified_diff` and `diff_source::place_diff`). Only
    /// mutants that overlap an added line are kept.
    ChangedSinceBase(DiffIndex),
    /// Each source file of the project.
    All,
    /// The files at these paths, or below these directories. A relative path
    /// is relative to the project root.
    Paths(Vec<PathBuf>),
}

/// What to plan.
#[derive(Debug, Clone)]
pub struct MutationPlanRequest {
    /// The files and lines to mutate.
    pub scope: MutationScope,
    /// The operators to use. `None` uses each operator.
    pub operators: Option<Vec<Operator>>,
    /// Keep the mutants in functions that call a file system, child process,
    /// network or database sink. The default prunes them, because a mutant
    /// can turn a guarded side effect into an unguarded one.
    pub allow_side_effects: bool,
    /// The maximum number of planned mutants. The mutants after this limit,
    /// in rank order, become `pending`.
    pub max_mutants: Option<usize>,
    /// Leave each Vue, Svelte and Astro component file out of the plan,
    /// with the skip reason [`SkipReason::RunnerCannotLoad`]. Set it when
    /// the test runner has no loader for a component file.
    pub skip_component_files: bool,
}

impl MutationPlanRequest {
    /// A request with the default options for `scope`.
    #[must_use]
    pub const fn new(scope: MutationScope) -> Self {
        Self {
            scope,
            operators: None,
            allow_side_effects: false,
            max_mutants: None,
            skip_component_files: false,
        }
    }
}

/// Framework config exports, by root-relative file path.
///
/// A framework plugin marks some exports as used by the framework, for
/// example `export const revalidate = 60` in a Next.js page. A mutant in such
/// an export changes framework config, not tested logic, so the plan prunes
/// it. Mutants inside a function in the export are not pruned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameworkConfigExports {
    files: BTreeMap<String, BTreeSet<String>>,
}

impl FrameworkConfigExports {
    /// Records that the framework uses `export` of the root-relative `file`.
    pub fn insert(&mut self, file: impl Into<String>, export: impl Into<String>) {
        self.files
            .entry(file.into())
            .or_default()
            .insert(export.into());
    }

    /// The framework config export names of one root-relative file.
    #[must_use]
    pub fn exports_for(&self, file: &str) -> Option<&BTreeSet<String>> {
        self.files.get(file)
    }
}

/// The analysis facts that a plan reads.
#[derive(Clone, Copy)]
pub struct MutationPlanInputs<'a> {
    /// The project root.
    pub root: &'a Path,
    /// The module graph of the analysis.
    pub graph: &'a RetainedModuleGraph,
    /// The parsed modules, with complexity.
    pub modules: &'a [ModuleInfo],
    /// The dead-code results of the analysis.
    pub results: &'a AnalysisResults,
    /// The framework config exports. `None` when the caller has no plugin
    /// facts. The plan then prunes no `framework-config` mutants and sets
    /// [`MutationPlan::framework_config_checked`] to false.
    pub framework_config_exports: Option<&'a FrameworkConfigExports>,
    /// The setup files of the test runner config, absolute or relative to
    /// the root, with or without an extension. They fill
    /// [`MutationPlan::support_files`], with the runner config modules at the
    /// root.
    pub setup_files: &'a [PathBuf],
}

/// Why a static rule removes a mutant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PruneReason {
    /// The file is unused: no entry point reaches it.
    DeadCode,
    /// The file is generated code.
    Generated,
    /// The enclosing function calls a file system, child process, network or
    /// database sink of the security catalogue. A mutant outside each
    /// function gets this reason when the module calls such a sink outside
    /// each function, because that sink runs on each import of the module.
    /// A sink that only shapes a response or a navigation (a redirect, a
    /// header, a cookie, a CORS policy) does not count.
    SideEffectSink,
    /// The mutant is in an export that a framework reads as config.
    FrameworkConfig,
}

/// The status of a mutant in a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(tag = "status", content = "reason", rename_all = "kebab-case")]
pub enum MutantDisposition {
    /// The runner executes the mutant.
    Planned,
    /// A planned mutant after the `max_mutants` limit.
    Pending,
    /// No test file reaches the file of the mutant.
    NoCoverage,
    /// A static rule removes the mutant.
    Pruned(PruneReason),
}

/// The facts of the function that contains a mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunctionFacts {
    /// The function name. The complexity name wins over the syntax name, so
    /// that the name is the same as in `health` output.
    pub name: Option<String>,
    /// The byte span of the function.
    pub span: (u32, u32),
    /// The 1-based first line of the function.
    pub line: u32,
    /// The 1-based last line of the function.
    pub end_line: u32,
    /// The cyclomatic complexity, when the analysis measured the function.
    pub cyclomatic: Option<u16>,
    /// True when an added line of the diff is in the function.
    pub in_diff: bool,
}

/// One mutant of the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedMutant {
    /// The mutant.
    pub mutant: Mutant,
    /// The root-relative path of the file, with forward slashes.
    pub file: String,
    /// The 1-based execution rank of a planned or pending mutant.
    pub rank: Option<u32>,
    /// The status of the mutant.
    pub disposition: MutantDisposition,
    /// The function that contains the mutant.
    pub function: Option<FunctionFacts>,
    /// How tests reach the code of the mutant in the module graph. `None`
    /// for a pruned mutant.
    pub test_reach: Option<MutantTestReach>,
}

/// How tests reach the code of one mutant, from the module graph.
///
/// A mutant in the top-level statement that declares a value export gets the
/// class of that export. A mutant outside each such statement gets the class
/// of its file: `none-file` when no test loads the file, `mocked-only` when no
/// test loads the file and a mock explains an export of the file, and `none`
/// when a test loads the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutantTestReach {
    /// The reach class.
    pub reach: ExportTestReach,
    /// The export whose declaration holds the mutant. `None` for a mutant
    /// outside each value export declaration.
    pub export_name: Option<String>,
    /// For `mocked-only`: the test files whose references a mock removed,
    /// sorted.
    pub tests: Vec<String>,
    /// For `mocked-only`: the mocked modules on the reference paths, sorted.
    pub mocked_targets: Vec<String>,
}

/// One file that the runner loads in its instrumented form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// The SHA-256 digest of the file bytes, as lowercase hex.
    pub source_sha256: String,
    /// The instrumented source text.
    pub instrumented_code: String,
    /// The test files that reach the file through imports, sorted. A test
    /// that replaces the file, or a module on each import path to it, with
    /// `vi.mock` is not in the list.
    pub static_test_files: Vec<String>,
}

/// Why the plan did not instrument a selected file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// The file could not be read.
    Unreadable,
    /// The file changed after the analysis parsed it.
    Stale,
    /// The file does not parse.
    Parse,
    /// The file declares a name that the instrumentation reserves.
    ReservedName,
    /// The file is larger than 4 GiB.
    TooLarge,
    /// The module graph has the file, but the analysis kept no module facts
    /// for it, for example because extraction could not read it.
    NotAnalyzed,
    /// The test runner cannot load the file: a component file under a runner
    /// with no component loader.
    RunnerCannotLoad,
}

/// A selected file that the plan did not instrument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    /// The root-relative path, with forward slashes.
    pub path: String,
    /// Why the plan skipped the file.
    pub reason: SkipReason,
}

/// The number of pruned mutants for each reason.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PruneCounts {
    /// Mutants in unused files.
    pub dead_code: usize,
    /// Mutants in generated files.
    pub generated: usize,
    /// Mutants in functions that call a side-effect sink, and top-level
    /// mutants of a module with a top-level side-effect sink.
    pub side_effect_sink: usize,
    /// Mutants in framework config exports.
    pub framework_config: usize,
}

/// The number of mutants for each disposition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct MutationPlanCounts {
    /// Each mutant of the plan.
    pub total: usize,
    /// Planned mutants.
    pub planned: usize,
    /// Planned mutants after the `max_mutants` limit.
    pub pending: usize,
    /// Mutants that no test file reaches.
    pub no_coverage: usize,
    /// Pruned mutants, by reason.
    pub pruned: PruneCounts,
}

/// The static mutation plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationPlan {
    /// The files with a planned or pending mutant, sorted by path.
    pub files: Vec<PlannedFile>,
    /// Each mutant. Ranked mutants come first in rank order, then the other
    /// mutants by file, span and id.
    pub mutants: Vec<PlannedMutant>,
    /// The union of the static test files of `files`, sorted.
    pub test_files: Vec<String>,
    /// The files that the test files reach through static imports, sorted.
    /// [`MutationPlan::test_file_reach`] indexes into this list.
    #[serde(skip)]
    pub reach_files: Vec<String>,
    /// The files that each test file of `test_files` reaches through static
    /// imports, as sorted indices into `reach_files`. The list leaves out the
    /// test file itself and ignores `vi.mock`, so it is a superset of the
    /// files that the test loads. The result cache of a run keys on the
    /// content of these files.
    #[serde(skip)]
    pub test_file_reach: BTreeMap<String, Vec<u32>>,
    /// The setup files of the test runner, the runner config modules at the
    /// root ([`RUNNER_CONFIG_MODULES`]), and the files that they reach through
    /// static imports, sorted. Each test loads them, so the result cache keys
    /// on their content for each mutant.
    #[serde(skip)]
    pub support_files: Vec<String>,
    /// The number of mutants for each disposition.
    pub counts: MutationPlanCounts,
    /// True when the project has no test files. Each mutant is then
    /// `no-coverage`, and a report shows this one fact instead of one line
    /// for each mutant.
    pub no_test_files: bool,
    /// True when the plan applied the `framework-config` prune rule. False
    /// when the caller gave no framework config exports, so that a mutant in
    /// such an export can be `planned`.
    pub framework_config_checked: bool,
    /// The selected files that the plan did not instrument, sorted by path.
    pub skipped_files: Vec<SkippedFile>,
    /// The paths of a `Paths` scope that select no source file, as given and
    /// with forward slashes, sorted. A path can select no file because it
    /// does not exist, or because it names only test code, declaration files
    /// or files outside the module graph.
    pub unmatched_paths: Vec<String>,
}

impl MutationPlan {
    /// The files that `test_file` reaches through static imports, sorted.
    /// Empty for a file that is not a test file of the plan.
    pub fn test_file_reach_paths<'a>(&'a self, test_file: &str) -> impl Iterator<Item = &'a str> {
        self.test_file_reach
            .get(test_file)
            .into_iter()
            .flatten()
            .filter_map(|&index| self.reach_files.get(index as usize))
            .map(String::as_str)
    }
}

/// An error from [`plan_mutations_for_session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationPlanError {
    /// The analysis failed.
    Analysis(EngineError),
    /// The analysis did not keep an artifact that the plan reads.
    MissingArtifact(&'static str),
    /// The plugin stage failed, so the plan cannot read the framework config
    /// exports.
    Plugins(String),
}

impl std::fmt::Display for MutationPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Analysis(error) => write!(f, "the analysis failed: {error}"),
            Self::MissingArtifact(name) => {
                write!(
                    f,
                    "the analysis did not keep the {name} for the mutation plan"
                )
            }
            Self::Plugins(message) => write!(f, "the plugin stage failed: {message}"),
        }
    }
}

impl std::error::Error for MutationPlanError {}

impl From<EngineError> for MutationPlanError {
    fn from(error: EngineError) -> Self {
        Self::Analysis(error)
    }
}

/// Builds the mutation plan from the facts of one analysis.
#[must_use]
pub fn plan_mutations(
    inputs: &MutationPlanInputs<'_>,
    request: &MutationPlanRequest,
) -> MutationPlan {
    plan::build(inputs, request)
}

/// Runs the dead-code analysis of `session` with complexity and the graph,
/// then builds the mutation plan.
///
/// The framework config exports come from the plugin used-export rules of
/// the session, the same rules that the unused-export analysis reads. The
/// plugin stage runs one more time for them.
///
/// # Errors
///
/// Returns an error when the analysis or the plugin stage fails.
pub fn plan_mutations_for_session(
    session: &AnalysisSession,
    request: &MutationPlanRequest,
) -> Result<MutationPlan, MutationPlanError> {
    plan_mutations_for_session_with(session, request, |_| false)
}

/// Like [`plan_mutations_for_session`], but `skip_components` reads the
/// first plan and decides if the component files leave the plan. The test
/// runner can depend on the test files of the plan, so the caller decides
/// after the first plan. When the first plan has a component mutant and
/// `skip_components` returns true, the plan is built again from the same
/// analysis with [`MutationPlanRequest::skip_component_files`] set.
///
/// # Errors
///
/// Returns an error when the analysis or the plugin stage fails.
pub fn plan_mutations_for_session_with(
    session: &AnalysisSession,
    request: &MutationPlanRequest,
    skip_components: impl FnOnce(&MutationPlan) -> bool,
) -> Result<MutationPlan, MutationPlanError> {
    let artifacts = session.analyze_dead_code_with_artifacts(true, true)?;
    let graph = artifacts
        .graph
        .as_ref()
        .ok_or(MutationPlanError::MissingArtifact("module graph"))?;
    let modules = artifacts
        .modules
        .as_deref()
        .ok_or(MutationPlanError::MissingArtifact("parsed modules"))?;
    let inventory = crate::list_inventory::collect_listing_inventory(session, false)
        .map_err(|error| MutationPlanError::Plugins(error.message().to_owned()))?;
    let framework_config_exports = framework_config_exports(session, graph, &inventory);
    let setup_files = inventory.plugins.backend().setup_files();
    let inputs = MutationPlanInputs {
        root: session.root(),
        graph,
        modules,
        results: &artifacts.results,
        framework_config_exports: Some(&framework_config_exports),
        setup_files: &setup_files,
    };
    let plan = plan_mutations(&inputs, request);
    let has_component = plan
        .mutants
        .iter()
        .any(|mutant| is_component_file(&mutant.file));
    if request.skip_component_files || !has_component || !skip_components(&plan) {
        return Ok(plan);
    }
    let request = MutationPlanRequest {
        skip_component_files: true,
        ..request.clone()
    };
    Ok(plan_mutations(&inputs, &request))
}

/// The plugin used-export names of each module of the graph.
fn framework_config_exports(
    session: &AnalysisSession,
    graph: &RetainedModuleGraph,
    inventory: &crate::list_inventory::ListingInventory,
) -> FrameworkConfigExports {
    let names = inventory.plugins.backend().used_export_names();
    let mut exports = FrameworkConfigExports::default();
    for node in &graph.as_graph().modules {
        let rel = crate::module_graph::relative_key_path(&node.path, session.root());
        for name in names.names_for(&rel) {
            exports.insert(rel.clone(), name);
        }
    }
    exports
}
