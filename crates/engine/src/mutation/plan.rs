//! The plan algorithm: select files, instrument them, assign dispositions and
//! rank the planned mutants.

use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fallow_config::EffectKind;
use fallow_mutation::{Instrumented, MutationError, MutationOptions, Operator};
use fallow_security::ProvenanceBindings;
use fallow_types::discover::FileId;
use fallow_types::extract::{FunctionComplexity, ModuleInfo, SinkSite, byte_offset_to_line_col};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};

use super::database_write;
use super::layout::Layout;
use super::{
    FunctionFacts, MutantDisposition, MutantTestReach, MutationPlan, MutationPlanCounts,
    MutationPlanInputs, MutationPlanRequest, MutationScope, PlannedFile, PlannedMutant,
    PruneReason, SkipReason, SkippedFile,
};
use crate::graph::{ModuleGraph, is_declaration_file_path};
use crate::module_graph::{
    ExportTestReach, ExportTestReachPaths, classify_export_test_reach, module_exports,
    relative_key_path,
};
use crate::source::similar_code::{SimilarCodeExtractionLimits, SimilarCodeExtractionSkipReason};

const BOM: char = '\u{feff}';

/// Catalogue effects that reach outside the process. A mutant in a function
/// with such a sink can change files, start processes or send requests.
pub(super) const SIDE_EFFECTS: [EffectKind; 4] = [
    EffectKind::Shell,
    EffectKind::Storage,
    EffectKind::Network,
    EffectKind::Database,
];

/// Callee prefixes of catalogue rows that compute values without IO. The
/// path-traversal rows list `path.join` and `path.resolve` under the storage
/// effect, but these calls only build strings.
const PURE_CALLEE_PREFIXES: [&str; 1] = ["path."];

/// Catalogue rows that carry a network or storage effect but only shape a
/// response or a navigation in the process: a redirect, a header, a cookie,
/// a CORS policy. A test runs them against a stub or a test server, so a
/// mutant there writes nothing outside the process.
pub(super) const RESPONSE_SHAPING_ROWS: [&str; 5] = [
    "header-injection",
    "insecure-cookie",
    "nextjs-open-redirect",
    "open-redirect",
    "permissive-cors",
];

/// Import sources of database clients, ORMs and query builders. A subpath
/// such as `drizzle-orm/libsql` matches its package.
const DATABASE_PACKAGES: [&str; 19] = [
    "@libsql/client",
    "@neondatabase/serverless",
    "@planetscale/database",
    "@prisma/client",
    "@vercel/postgres",
    "better-sqlite3",
    "drizzle-orm",
    "knex",
    "kysely",
    "libsql",
    "mongodb",
    "mongoose",
    "mysql",
    "mysql2",
    "pg",
    "postgres",
    "sequelize",
    "sqlite3",
    "typeorm",
];

/// One file that the scope selects.
struct Candidate<'a> {
    file_id: FileId,
    path: &'a Path,
    rel: String,
    layout: Layout,
    /// `None` when the analysis kept no module facts for the file.
    module: Option<&'a ModuleInfo>,
    added_lines: Option<Vec<u64>>,
}

/// The shared facts of one plan.
struct Context<'a> {
    inputs: &'a MutationPlanInputs<'a>,
    request: &'a MutationPlanRequest,
    graph: &'a ModuleGraph,
    modules: FxHashMap<FileId, &'a ModuleInfo>,
    dead_files: FxHashSet<&'a Path>,
    options: MutationOptions,
    /// The test entry points that reach each selected test-reachable file,
    /// with the `vi.mock` replacement mask of each test applied.
    test_entries: FxHashMap<FileId, Vec<FileId>>,
    /// The test-reachable files that only type tests reach. No runner test
    /// covers them.
    type_test_only: FxHashSet<FileId>,
    /// The files that load a database client package.
    database_files: FxHashSet<FileId>,
}

/// The result for one file.
struct FileOutcome {
    file: Option<PlannedFile>,
    mutants: Vec<RankEntry>,
    skipped: Option<SkippedFile>,
}

/// A mutant with the facts that only the ranking reads.
pub(super) struct RankEntry {
    /// True when the span of the mutant is on an added line of the diff.
    pub(super) in_diff: bool,
    pub(super) planned: PlannedMutant,
}

pub(super) fn build(
    inputs: &MutationPlanInputs<'_>,
    request: &MutationPlanRequest,
) -> MutationPlan {
    let graph = inputs.graph.as_graph();
    let mut context = Context {
        inputs,
        request,
        graph,
        modules: inputs
            .modules
            .iter()
            .map(|module| (module.file_id, module))
            .collect(),
        dead_files: inputs
            .results
            .unused_files
            .iter()
            .map(|finding| finding.file.path.as_path())
            .collect(),
        options: MutationOptions {
            operators: request
                .operators
                .clone()
                .unwrap_or_else(|| Operator::ALL.to_vec()),
            ranges: None,
        },
        test_entries: FxHashMap::default(),
        type_test_only: FxHashSet::default(),
        database_files: database_files(graph),
    };
    let (candidates, unmatched_paths) = select_files(&context);
    let reachable: Vec<FileId> = candidates
        .iter()
        .map(|candidate| candidate.file_id)
        .filter(|&file_id| graph.is_test_reachable(file_id))
        .collect();
    context.test_entries = graph.test_entry_points_reaching(&reachable);
    for (&file_id, entries) in &mut context.test_entries {
        let before = entries.len();
        entries.retain(|&entry| !is_type_test_entry(graph, inputs.root, entry));
        if before > 0 && entries.is_empty() {
            context.type_test_only.insert(file_id);
        }
    }
    let outcomes: Vec<FileOutcome> = candidates
        .par_iter()
        .map(|candidate| plan_file(&context, candidate))
        .collect();

    let mut files = Vec::new();
    let mut mutants = Vec::new();
    let mut skipped_files = Vec::new();
    for outcome in outcomes {
        files.extend(outcome.file);
        mutants.extend(outcome.mutants);
        skipped_files.extend(outcome.skipped);
    }
    rank(&mut mutants, request.max_mutants);
    let mutants: Vec<PlannedMutant> = mutants.into_iter().map(|entry| entry.planned).collect();

    let runnable: FxHashSet<&str> = mutants
        .iter()
        .filter(|entry| is_ranked(entry.disposition))
        .map(|entry| entry.file.as_str())
        .collect();
    files.retain(|file| runnable.contains(file.path.as_str()));
    let test_files: BTreeSet<String> = files
        .iter()
        .flat_map(|file| file.static_test_files.iter().cloned())
        .collect();

    let test_files: Vec<String> = test_files.into_iter().collect();
    let reach = super::reach::test_reach(graph, inputs.root, &test_files);
    MutationPlan {
        files,
        counts: count(&mutants),
        mutants,
        reach_files: reach.files,
        test_file_reach: reach.reach,
        support_files: super::reach::support_files(graph, inputs.root, inputs.setup_files),
        test_files,
        no_test_files: graph
            .test_entry_points
            .iter()
            .all(|&entry| is_type_test_entry(graph, inputs.root, entry)),
        framework_config_checked: inputs.framework_config_exports.is_some(),
        skipped_files,
        unmatched_paths,
    }
}

/// The source files in scope, sorted by root-relative path. Test code, test
/// support, declaration files and files that are not JavaScript, TypeScript,
/// Vue, Svelte or Astro are never selected.
/// Test code is a test entry point of the graph (a file that the runner
/// config makes a test) or a file that the test path rule matches.
/// The second value holds the `Paths` scope entries that select no file.
fn select_files<'a>(context: &Context<'a>) -> (Vec<Candidate<'a>>, Vec<String>) {
    let root = context.inputs.root;
    let mut matched = match &context.request.scope {
        MutationScope::Paths(paths) => vec![false; paths.len()],
        _ => Vec::new(),
    };
    let mut candidates: Vec<Candidate<'a>> = context
        .graph
        .modules
        .iter()
        .filter_map(|node| {
            let rel = relative_key_path(&node.path, root);
            let layout = Layout::from_path(&rel)?;
            if is_declaration_file_path(&node.path)
                || context.graph.test_entry_points.contains(&node.file_id)
                || crate::test_paths::is_test_path_str(root, &rel)
            {
                return None;
            }
            let added_lines = match &context.request.scope {
                MutationScope::ChangedSinceBase(diff) => {
                    let key = diff.key_for(&node.path, root)?;
                    let mut lines: Vec<u64> = diff.added_lines_in(&key)?.iter().copied().collect();
                    lines.sort_unstable();
                    Some(lines)
                }
                MutationScope::All => None,
                MutationScope::Paths(paths) => {
                    let mut selected = false;
                    for (index, path) in paths.iter().enumerate() {
                        if in_path(&node.path, root, path) {
                            matched[index] = true;
                            selected = true;
                        }
                    }
                    if !selected {
                        return None;
                    }
                    None
                }
            };
            Some(Candidate {
                file_id: node.file_id,
                path: &node.path,
                rel,
                layout,
                module: context.modules.get(&node.file_id).copied(),
                added_lines,
            })
        })
        .collect();
    candidates.sort_by(|left, right| left.rel.cmp(&right.rel));
    let mut unmatched: Vec<String> = match &context.request.scope {
        MutationScope::Paths(paths) => paths
            .iter()
            .zip(&matched)
            .filter(|(_, matched)| !**matched)
            .map(|(path, _)| path.to_string_lossy().replace('\\', "/"))
            .collect(),
        _ => Vec::new(),
    };
    unmatched.sort();
    unmatched.dedup();
    (candidates, unmatched)
}

fn in_path(file: &Path, root: &Path, selected: &Path) -> bool {
    let selected: PathBuf = if selected.is_absolute() {
        selected.to_path_buf()
    } else {
        root.join(selected)
    };
    let file = dunce::simplified(file);
    file.starts_with(dunce::simplified(&selected))
}

fn plan_file(context: &Context<'_>, candidate: &Candidate<'_>) -> FileOutcome {
    let skipped = |reason| FileOutcome {
        file: None,
        mutants: Vec::new(),
        skipped: Some(SkippedFile {
            path: candidate.rel.clone(),
            reason,
        }),
    };
    if context.request.skip_component_files && matches!(candidate.layout, Layout::Component(_)) {
        return skipped(SkipReason::RunnerCannotLoad);
    }
    let Some(module) = candidate.module else {
        return skipped(SkipReason::NotAnalyzed);
    };
    let Ok(bytes) = std::fs::read(candidate.path) else {
        return skipped(SkipReason::Unreadable);
    };
    let Ok(raw) = std::str::from_utf8(&bytes) else {
        return skipped(SkipReason::Unreadable);
    };
    let source = raw.strip_prefix(BOM).unwrap_or(raw);
    if xxhash_rust::xxh3::xxh3_64(source.as_bytes()) != module.content_hash {
        return skipped(SkipReason::Stale);
    }
    let mut options = context.options.clone();
    options.ranges = candidate
        .added_lines
        .as_deref()
        .map(|lines| added_line_ranges(source, lines));
    let instrumented = match candidate
        .layout
        .instrument(source, &candidate.rel, &options)
    {
        Ok(instrumented) => instrumented,
        Err(MutationError::Parse { .. }) => return skipped(SkipReason::Parse),
        Err(MutationError::ReservedName { .. }) => return skipped(SkipReason::ReservedName),
        Err(MutationError::TooLarge) => return skipped(SkipReason::TooLarge),
    };
    let covered = context.graph.is_test_reachable(candidate.file_id)
        && !context.type_test_only.contains(&candidate.file_id);
    let facts = FileFacts::new(context, candidate, module, source);
    let mut mutants = assign(candidate, &facts, covered, instrumented.mutants.clone());
    attach_test_reach(context, candidate, source, covered, &mut mutants);
    let static_test_files = if covered {
        static_test_files(context, candidate.file_id)
    } else {
        Vec::new()
    };
    FileOutcome {
        file: Some(planned_file(
            candidate,
            &bytes,
            instrumented,
            static_test_files,
        )),
        mutants,
        skipped: None,
    }
}

fn planned_file(
    candidate: &Candidate<'_>,
    bytes: &[u8],
    instrumented: Instrumented,
    static_test_files: Vec<String>,
) -> PlannedFile {
    PlannedFile {
        path: candidate.rel.clone(),
        source_sha256: lowercase_hex(&Sha256::digest(bytes)),
        instrumented_code: instrumented.code,
        static_test_files,
    }
}

pub(super) fn lowercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// The byte ranges of the added lines. A range covers the line text and its
/// line break, and adjacent lines merge into one range.
fn added_line_ranges(source: &str, lines: &[u64]) -> Vec<(u32, u32)> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for &line in lines {
        let Some(index) = usize::try_from(line)
            .ok()
            .and_then(|line| line.checked_sub(1))
        else {
            continue;
        };
        let Some(&start) = starts.get(index) else {
            continue;
        };
        let end = starts.get(index + 1).copied().unwrap_or(source.len());
        let (Ok(start), Ok(end)) = (u32::try_from(start), u32::try_from(end)) else {
            continue;
        };
        match ranges.last_mut() {
            Some(last) if last.1 == start => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }
    ranges
}

/// The per-file facts that decide a disposition.
struct FileFacts<'a> {
    file_prune: Option<PruneReason>,
    framework_regions: Vec<(u32, u32)>,
    sink_starts: Vec<u32>,
    top_level_sink: bool,
    complexity: FxHashMap<(u32, u32), &'a FunctionComplexity>,
    line_offsets: &'a [u32],
    added_lines: Option<&'a [u64]>,
}

impl<'a> FileFacts<'a> {
    fn new(
        context: &Context<'_>,
        candidate: &'a Candidate<'a>,
        module: &'a ModuleInfo,
        source: &str,
    ) -> Self {
        let file_prune = if is_generated(candidate.path, source) {
            Some(PruneReason::Generated)
        } else if context.dead_files.contains(candidate.path) {
            Some(PruneReason::DeadCode)
        } else {
            None
        };
        let framework_regions = context
            .inputs
            .framework_config_exports
            .and_then(|exports| {
                let names = exports.exports_for(&candidate.rel)?;
                let config_file = exports.is_config_file(&candidate.rel);
                Some(framework_regions(
                    module,
                    names,
                    config_file,
                    source,
                    candidate.layout,
                ))
            })
            .unwrap_or_default();
        // A sink in a component template does not run when the module
        // loads, so only the sinks in the script blocks count.
        let script_spans = candidate.layout.script_spans(source);
        let in_script = |start: u32| {
            script_spans
                .as_ref()
                .is_none_or(|spans| spans.iter().any(|&(from, to)| from <= start && start < to))
        };
        let sink_starts = if context.request.allow_side_effects {
            Vec::new()
        } else {
            let mut starts: Vec<u32> = module
                .security_sinks
                .iter()
                .filter(|site| is_side_effect_sink(module, site))
                .map(|site| site.span_start)
                .collect();
            if has_database_reach(context, candidate.file_id) {
                let direct = context.database_files.contains(&candidate.file_id);
                starts.extend(database_write::starts(source, candidate.layout, direct));
            }
            starts.retain(|&start| in_script(start));
            starts
        };
        let top_level_sink =
            !sink_starts.is_empty() && has_top_level_site(&sink_starts, source, candidate.layout);
        Self {
            file_prune,
            framework_regions,
            sink_starts,
            top_level_sink,
            complexity: module
                .complexity
                .iter()
                .map(|function| ((function.line, function.col), function))
                .collect(),
            line_offsets: &module.line_offsets,
            added_lines: candidate.added_lines.as_deref(),
        }
    }

    fn function(&self, function: &fallow_mutation::FunctionRef) -> FunctionFacts {
        let (line, col) = byte_offset_to_line_col(self.line_offsets, function.span.0);
        let end = function.span.1.saturating_sub(1).max(function.span.0);
        let end_line = byte_offset_to_line_col(self.line_offsets, end).0;
        let measured = self.complexity.get(&(line, col));
        let in_diff = self.span_in_diff(function.span);
        FunctionFacts {
            name: measured
                .map(|function| function.name.clone())
                .or_else(|| function.name.clone()),
            span: function.span,
            line,
            end_line,
            cyclomatic: measured.map(|function| function.cyclomatic),
            in_diff,
        }
    }

    /// Whether the byte span is on an added line of the diff. The check does
    /// not need an enclosing function, so a top-level mutant on an added line
    /// is in the diff too.
    fn span_in_diff(&self, span: (u32, u32)) -> bool {
        let Some(lines) = self.added_lines else {
            return false;
        };
        let first = byte_offset_to_line_col(self.line_offsets, span.0).0;
        let end = span.1.saturating_sub(1).max(span.0);
        let last = byte_offset_to_line_col(self.line_offsets, end).0;
        has_line_between(lines, first, last)
    }

    fn disposition(&self, mutant: &fallow_mutation::Mutant, covered: bool) -> MutantDisposition {
        if let Some(reason) = self.file_prune {
            return MutantDisposition::Pruned(reason);
        }
        let function = mutant
            .enclosing_function
            .as_ref()
            .map(|function| function.span);
        let in_config = self.framework_regions.iter().any(|&region| {
            contains(region, mutant.span) && function.is_none_or(|span| !contains(region, span))
        });
        if in_config {
            return MutantDisposition::Pruned(PruneReason::FrameworkConfig);
        }
        // A top-level sink runs on each import of the module, so every
        // top-level mutant can change what it writes.
        let calls_sink = function.map_or(self.top_level_sink, |span| {
            self.sink_starts
                .iter()
                .any(|&start| span.0 <= start && start < span.1)
        });
        if calls_sink {
            return MutantDisposition::Pruned(PruneReason::SideEffectSink);
        }
        if covered {
            MutantDisposition::Planned
        } else {
            MutantDisposition::NoCoverage
        }
    }
}

/// Whether the sorted `lines` hold a line from `first` to `last`, both
/// included.
pub(super) fn has_line_between(lines: &[u64], first: u32, last: u32) -> bool {
    let index = lines.partition_point(|&line| line < u64::from(first));
    lines
        .get(index)
        .is_some_and(|&line| line <= u64::from(last))
}

const fn contains(outer: (u32, u32), inner: (u32, u32)) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1
}

fn assign(
    candidate: &Candidate<'_>,
    facts: &FileFacts<'_>,
    covered: bool,
    mutants: Vec<fallow_mutation::Mutant>,
) -> Vec<RankEntry> {
    mutants
        .into_iter()
        .map(|mutant| RankEntry {
            in_diff: facts.span_in_diff(mutant.span),
            planned: PlannedMutant {
                disposition: facts.disposition(&mutant, covered),
                function: mutant
                    .enclosing_function
                    .as_ref()
                    .map(|function| facts.function(function)),
                file: candidate.rel.clone(),
                rank: None,
                test_reach: None,
                mutant,
            },
        })
        .collect()
}

/// Sets the test reach of each mutant that no static rule prunes. The value
/// exports of the file are classified once, with the same test-file rule as
/// the test reach of `audit`.
fn attach_test_reach(
    context: &Context<'_>,
    candidate: &Candidate<'_>,
    source: &str,
    covered: bool,
    mutants: &mut [RankEntry],
) {
    if mutants
        .iter()
        .all(|entry| matches!(entry.planned.disposition, MutantDisposition::Pruned(_)))
    {
        return;
    }
    // No test that runs loads a file that only type tests reach.
    if context.type_test_only.contains(&candidate.file_id) {
        for entry in mutants
            .iter_mut()
            .filter(|entry| !matches!(entry.planned.disposition, MutantDisposition::Pruned(_)))
        {
            entry.planned.test_reach = Some(MutantTestReach {
                reach: ExportTestReach::NoneFile,
                export_name: None,
                tests: Vec::new(),
                export_test_files: Vec::new(),
                mocked_targets: Vec::new(),
            });
        }
        return;
    }
    let graph = context.graph;
    let root = context.inputs.root;
    let mut is_test_file = |file_id: FileId| -> bool {
        graph.test_entry_points.contains(&file_id)
            || graph.modules.get(file_id.0 as usize).is_some_and(|node| {
                crate::test_paths::is_test_code_path_str(root, &relative_key_path(&node.path, root))
            })
    };
    let exports: Vec<ExportTestReachPaths> = module_exports(graph, root, candidate.file_id, source)
        .iter()
        .filter_map(|export| classify_export_test_reach(graph, root, export, &mut is_test_file))
        .collect();
    let file_reach = file_test_reach(graph, covered, &exports);
    for entry in mutants
        .iter_mut()
        .filter(|entry| !matches!(entry.planned.disposition, MutantDisposition::Pruned(_)))
    {
        let line = entry.planned.mutant.line;
        let reach = exports
            .iter()
            .find(|export| {
                let (first, last) = export.declaration_lines;
                first <= line && line <= last
            })
            .map_or_else(
                || file_reach.clone(),
                |export| MutantTestReach {
                    reach: export.reach,
                    export_name: Some(export.export_name.clone()),
                    tests: mocked_only_files(export.reach, &export.tests),
                    export_test_files: export.tests.clone(),
                    mocked_targets: mocked_only_files(export.reach, &export.mocked_targets),
                },
            );
        entry.planned.test_reach = Some(reach);
    }
}

/// The test reach of the code outside each value export declaration.
fn file_test_reach(
    graph: &ModuleGraph,
    covered: bool,
    exports: &[ExportTestReachPaths],
) -> MutantTestReach {
    let mut reach = MutantTestReach {
        reach: ExportTestReach::NoneFile,
        export_name: None,
        tests: Vec::new(),
        export_test_files: Vec::new(),
        mocked_targets: Vec::new(),
    };
    if graph.test_entry_points.is_empty() {
        return reach;
    }
    if covered {
        reach.reach = ExportTestReach::None;
        return reach;
    }
    let mut tests = BTreeSet::new();
    let mut mocked_targets = BTreeSet::new();
    for export in exports
        .iter()
        .filter(|export| export.reach == ExportTestReach::MockedOnly)
    {
        reach.reach = ExportTestReach::MockedOnly;
        tests.extend(export.tests.iter().cloned());
        mocked_targets.extend(export.mocked_targets.iter().cloned());
    }
    reach.tests = tests.into_iter().collect();
    reach.mocked_targets = mocked_targets.into_iter().collect();
    reach
}

/// The files of a `mocked-only` class. The other classes list no files.
fn mocked_only_files(reach: ExportTestReach, files: &[String]) -> Vec<String> {
    if reach == ExportTestReach::MockedOnly {
        files.to_vec()
    } else {
        Vec::new()
    }
}

/// Generated code, by the same path and header rule that similar-code
/// extraction applies. The extraction returns before it parses a generated
/// source, so only a non-generated file pays for a parse.
fn is_generated(path: &Path, source: &str) -> bool {
    let limits = SimilarCodeExtractionLimits {
        max_functions: 0,
        max_source_bytes_per_function: 0,
        max_total_source_bytes: 0,
    };
    crate::source::similar_code::extract(path, source, limits)
        .skipped
        .iter()
        .any(|skip| skip.reason == SimilarCodeExtractionSkipReason::GeneratedSource)
}

/// A captured sink site that matches a catalogue row with an IO effect.
///
/// A row with an import provenance matches only when the module can import
/// that source, so a local `get(object, key)` does not match the `https.get`
/// row. The modules are released, so an identifier that no ES import and no
/// export binds still matches: it can be a `require()` binding.
fn is_side_effect_sink(module: &ModuleInfo, site: &SinkSite) -> bool {
    if PURE_CALLEE_PREFIXES
        .iter()
        .any(|prefix| site.callee_path.starts_with(prefix))
    {
        return false;
    }
    fallow_security::catalogue()
        .matchers()
        .iter()
        .any(|matcher| {
            matcher.sink_shape == site.sink_shape
                && SIDE_EFFECTS.contains(&matcher.effect)
                && !RESPONSE_SHAPING_ROWS.contains(&matcher.id.as_str())
                && matcher.first_matching_pattern(&site.callee_path).is_some()
                && matcher.provenance_satisfied(
                    module,
                    &site.callee_path,
                    ProvenanceBindings::ReleasedModule,
                )
        })
}

/// Whether the module loads a database client package, or loads a project
/// module that loads one. One hop covers the common layout where a `db`
/// module creates the client and the other modules import it. A type-only
/// import counts too: a function that takes the client as a parameter often
/// imports only its type.
fn has_database_reach(context: &Context<'_>, file_id: FileId) -> bool {
    context.database_files.contains(&file_id)
        || context
            .graph
            .edges_for(file_id)
            .iter()
            .any(|target| context.database_files.contains(target))
}

/// The files that load a database client package with an ES import, a
/// `require()` call or an `import()` call. The graph keeps these facts after
/// the release of the module payload, which drains `require_calls` and
/// `dynamic_imports`.
fn database_files(graph: &ModuleGraph) -> FxHashSet<FileId> {
    graph
        .package_usage
        .iter()
        .filter(|(package, _)| {
            DATABASE_PACKAGES
                .iter()
                .any(|database| fallow_security::import_source_matches(package, database))
        })
        .flat_map(|(_, users)| users.iter().copied())
        .collect()
}

/// Whether one of the offsets lies outside every function and arrow
/// function of the source. The function kinds match the enclosing functions
/// that `fallow-mutation` reports.
fn has_top_level_site(starts: &[u32], source: &str, layout: Layout) -> bool {
    use oxc_ast::ast::{ArrowFunctionExpression, Function};
    use oxc_ast_visit::Visit;
    use oxc_syntax::scope::ScopeFlags;

    #[derive(Default)]
    struct Spans(Vec<(u32, u32)>);
    impl<'a> Visit<'a> for Spans {
        fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
            self.0.push((it.span.start, it.span.end));
            oxc_ast_visit::walk::walk_function(self, it, flags);
        }
        fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
            self.0.push((it.span.start, it.span.end));
            oxc_ast_visit::walk::walk_arrow_function_expression(self, it);
        }
    }

    let (text, kind) = layout.parse_view(source);
    fallow_extract::parse_bounded_or_default(&text, kind.source_type(), |parsed| {
        let mut spans = Spans::default();
        spans.visit_program(&parsed.program);
        starts.iter().any(|&start| {
            !spans
                .0
                .iter()
                .any(|&(from, to)| from <= start && start < to)
        })
    })
}

/// The top-level statements that declare one of the framework config
/// exports of the module. In a tool config file, a top-level
/// `module.exports = ...` statement is one of them too.
fn framework_regions(
    module: &ModuleInfo,
    names: &BTreeSet<String>,
    config_file: bool,
    source: &str,
    layout: Layout,
) -> Vec<(u32, u32)> {
    use oxc_span::GetSpan;

    let starts: Vec<u32> = module
        .exports
        .iter()
        .filter(|export| {
            names
                .iter()
                .any(|name| name == "*" || export.name.matches_str(name))
        })
        .map(|export| export.span.start)
        .collect();
    let commonjs_default = config_file && module.has_cjs_exports;
    if starts.is_empty() && !commonjs_default {
        return Vec::new();
    }
    let (text, kind) = layout.parse_view(source);
    fallow_extract::parse_bounded_or_default(&text, kind.source_type(), |parsed| {
        parsed
            .program
            .body
            .iter()
            .filter(|statement| {
                let span = statement.span();
                starts
                    .iter()
                    .any(|&offset| span.start <= offset && offset < span.end)
                    || (commonjs_default && assigns_module_exports(statement))
            })
            .map(|statement| {
                let span = statement.span();
                (span.start, span.end)
            })
            .collect()
    })
}

/// True for a top-level `module.exports = ...` statement. A CommonJS config
/// file sets its config this way, so it counts as the default export.
fn assigns_module_exports(statement: &oxc_ast::ast::Statement<'_>) -> bool {
    use oxc_ast::ast::{AssignmentTarget, Expression, Statement};

    let Statement::ExpressionStatement(statement) = statement else {
        return false;
    };
    let Expression::AssignmentExpression(assignment) = &statement.expression else {
        return false;
    };
    let AssignmentTarget::StaticMemberExpression(member) = &assignment.left else {
        return false;
    };
    member.property.name == "exports"
        && matches!(&member.object, Expression::Identifier(object) if object.name == "module")
}

/// The test entry points that reach the file, sorted by root-relative path.
/// A test that replaces the file, or a module on each import path to it, with
/// `vi.mock` does not reach it.
fn static_test_files(context: &Context<'_>, file_id: FileId) -> Vec<String> {
    let graph = context.graph;
    let tests: BTreeSet<String> = context
        .test_entries
        .get(&file_id)
        .into_iter()
        .flatten()
        .filter_map(|&entry| graph.modules.get(entry.0 as usize))
        .map(|node| relative_key_path(&node.path, context.inputs.root))
        .collect();
    tests.into_iter().collect()
}

/// File-name markers of a type test (`index.test-d.ts`, `a.spec-d.ts`).
const TYPE_TEST_FILE_MARKERS: &[&str] = &[".test-d.", ".spec-d."];

/// The directory of `tsd` type tests.
const TYPE_TEST_DIR_NAME: &str = "test-d";

/// The extensions of a type test below [`TYPE_TEST_DIR_NAME`]. `tsd` reads
/// TypeScript files only.
const TYPE_TEST_DIR_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".mts", ".cts"];

/// True when the test entry point is a type test (`tsd`, `expect-type`, the
/// Vitest type-check files). A compiler checks a type test, and no test
/// runner executes it, so it never covers a mutant.
fn is_type_test_entry(graph: &ModuleGraph, root: &Path, entry: FileId) -> bool {
    graph
        .modules
        .get(entry.0 as usize)
        .is_some_and(|node| is_type_test_path(&relative_key_path(&node.path, root)))
}

fn is_type_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut segments = lower.split('/').rev();
    let name = segments.next().unwrap_or_default();
    TYPE_TEST_FILE_MARKERS
        .iter()
        .any(|marker| name.contains(marker))
        || (TYPE_TEST_DIR_EXTENSIONS
            .iter()
            .any(|extension| name.ends_with(extension))
            && segments.any(|segment| segment == TYPE_TEST_DIR_NAME))
}

const fn is_ranked(disposition: MutantDisposition) -> bool {
    matches!(
        disposition,
        MutantDisposition::Planned | MutantDisposition::Pending
    )
}

/// Orders the mutants and gives each planned mutant its rank. The order is
/// diff membership of the mutant first, then the cyclomatic complexity of the function,
/// then file, span and id. The `max_mutants` limit applies after the order.
/// Unranked mutants follow in file, span and id order.
pub(super) fn rank(mutants: &mut [RankEntry], max_mutants: Option<usize>) {
    mutants.sort_by(|left, right| {
        let key = |entry: &RankEntry| {
            let function = entry.planned.function.as_ref();
            (
                !is_ranked(entry.planned.disposition),
                !entry.in_diff,
                Reverse(
                    function
                        .and_then(|function| function.cyclomatic)
                        .unwrap_or(0),
                ),
            )
        };
        let order = key(left).cmp(&key(right));
        let (left, right) = (&left.planned, &right.planned);
        order
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.mutant.span.cmp(&right.mutant.span))
            .then_with(|| left.mutant.id.cmp(&right.mutant.id))
    });
    let limit = max_mutants.unwrap_or(usize::MAX);
    for (index, entry) in mutants
        .iter_mut()
        .map(|entry| &mut entry.planned)
        .take_while(|entry| is_ranked(entry.disposition))
        .enumerate()
    {
        entry.rank = u32::try_from(index + 1).ok();
        if index >= limit {
            entry.disposition = MutantDisposition::Pending;
        }
    }
}

fn count(mutants: &[PlannedMutant]) -> MutationPlanCounts {
    let mut counts = MutationPlanCounts {
        total: mutants.len(),
        ..MutationPlanCounts::default()
    };
    for entry in mutants {
        match entry.disposition {
            MutantDisposition::Planned => counts.planned += 1,
            MutantDisposition::Pending => counts.pending += 1,
            MutantDisposition::NoCoverage => counts.no_coverage += 1,
            MutantDisposition::Pruned(PruneReason::DeadCode) => counts.pruned.dead_code += 1,
            MutantDisposition::Pruned(PruneReason::Generated) => counts.pruned.generated += 1,
            MutantDisposition::Pruned(PruneReason::SideEffectSink) => {
                counts.pruned.side_effect_sink += 1;
            }
            MutantDisposition::Pruned(PruneReason::FrameworkConfig) => {
                counts.pruned.framework_config += 1;
            }
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_tests_are_named_by_marker_or_test_d_directory() {
        for path in [
            "test/types/index.test-d.ts",
            "src/a.spec-d.tsx",
            "test-d/index.ts",
            "packages/x/Test-D/a.ts",
        ] {
            assert!(is_type_test_path(path), "{path}");
        }
        for path in [
            "test/a.test.ts",
            "src/test-data.ts",
            "test-d.ts",
            "src/a-test-d.ts",
            "test-d/helper.js",
        ] {
            assert!(!is_type_test_path(path), "{path}");
        }
    }

    /// `(line, top_level, in_diff)` for each mutant of the source when line 1
    /// is the only added line.
    fn diff_membership(source: &str) -> Vec<(u32, bool, bool)> {
        let path = Path::new("/project/src/limit.ts");
        let layout = Layout::from_path("src/limit.ts").expect("a TypeScript path");
        let instrumented = layout
            .instrument(source, "src/limit.ts", &MutationOptions::default())
            .expect("the source parses");
        let line_offsets = fallow_types::extract::compute_line_offsets(source);
        let added_lines = vec![1];
        let candidate = Candidate {
            file_id: FileId(0),
            path,
            rel: "src/limit.ts".to_owned(),
            layout,
            module: None,
            added_lines: Some(added_lines.clone()),
        };
        let facts = FileFacts {
            file_prune: None,
            framework_regions: Vec::new(),
            sink_starts: Vec::new(),
            top_level_sink: false,
            complexity: FxHashMap::default(),
            line_offsets: &line_offsets,
            added_lines: Some(&added_lines),
        };
        assign(&candidate, &facts, true, instrumented.mutants)
            .into_iter()
            .map(|entry| {
                (
                    entry.planned.mutant.line,
                    entry.planned.function.is_none(),
                    entry.in_diff,
                )
            })
            .collect()
    }

    #[test]
    fn a_top_level_mutant_on_an_added_line_is_in_the_diff() {
        let entries = diff_membership(
            "export const limit = 60 * 5;\n\
             export function clamp(value: number): number {\n\
             \x20 return value > limit ? limit : value;\n\
             }\n",
        );
        let top_level_on_added: Vec<bool> = entries
            .iter()
            .filter(|&&(line, top_level, _)| line == 1 && top_level)
            .map(|&(_, _, in_diff)| in_diff)
            .collect();
        assert!(!top_level_on_added.is_empty(), "{entries:?}");
        assert!(
            top_level_on_added.iter().all(|&in_diff| in_diff),
            "{entries:?}"
        );
        let in_function_off_diff: Vec<bool> = entries
            .iter()
            .filter(|&&(line, top_level, _)| line == 3 && !top_level)
            .map(|&(_, _, in_diff)| in_diff)
            .collect();
        assert!(!in_function_off_diff.is_empty(), "{entries:?}");
        assert!(
            in_function_off_diff.iter().all(|&in_diff| !in_diff),
            "{entries:?}"
        );
    }
}
