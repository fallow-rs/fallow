//! Dead-code result helpers exposed through the engine boundary.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_config::{
    ResolvedConfig, RulesConfig, Severity, WorkspaceDiagnostic, WorkspaceDiagnosticKind,
};
use fallow_types::discover::StableFileKey;

pub use crate::results::{
    AnalysisResults, DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    DeadCodeAnalysisWithHashes, derive_security_severity, enable_security_rules,
    resolve_security_finding_severity, security_catalogue_title, security_rule_id,
    security_rules_can_error,
};

pub use crate::effective_severity::{
    RuleSeverity, SeveritySource, apply_effective_severities, findings_without_severity,
    promote_effective_warns,
};

use crate::{
    EngineResult, change_scope::ChangeScope,
    session::analyze_dead_code_with_parse_result_from_config, source::ModuleInfo,
};

/// Run dead-code analysis from pre-parsed modules.
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
pub(crate) fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    analyze_dead_code_with_parse_result_from_config(config, modules)
}

/// `workspace_diagnostics[]` entries for the config patterns that matched
/// nothing in the latest dead-code pass over `config`.
///
/// One entry per unmatched `ignoreFindings` pattern and, when
/// `reports_dependencies` is true, one per unmatched `ignoreDependencies`
/// glob, in config order. A surface passes `reports_dependencies = false` when
/// its run does not report dependency findings (an issue-type filter without
/// the dependency types, or a file scope), because a dependency glob is then
/// not relevant to what the run shows.
///
/// The CLI, the programmatic API and the MCP typed path all build their
/// envelope from this one function, and the human note reads the same
/// entries, so every output states the same patterns.
#[must_use]
pub fn config_pattern_diagnostics(
    config: &ResolvedConfig,
    reports_dependencies: bool,
) -> Vec<WorkspaceDiagnostic> {
    let dependency_globs = if reports_dependencies && dependency_rules_on(&config.rules) {
        config.ignore_dependencies.unmatched_globs()
    } else {
        Vec::new()
    };
    let finding_patterns = config.ignore_findings.unmatched_patterns();
    finding_patterns
        .into_iter()
        .map(
            |pattern| WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched {
                pattern: pattern.to_owned(),
            },
        )
        .chain(dependency_globs.into_iter().map(|pattern| {
            WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
                pattern: pattern.to_owned(),
            }
        }))
        .map(|kind| {
            WorkspaceDiagnostic::new(&config.root, config.root.clone(), kind)
                .into_root_relative(&config.root)
        })
        .collect()
}

/// Whether at least one rule that `ignoreDependencies` controls is on. With
/// every such rule off, the run reports no dependency finding at all.
fn dependency_rules_on(rules: &RulesConfig) -> bool {
    [
        rules.unused_dependencies,
        rules.unused_dev_dependencies,
        rules.unused_optional_dependencies,
        rules.unlisted_dependencies,
        rules.type_only_dependencies,
        rules.test_only_dependencies,
        rules.dev_dependencies_in_production,
    ]
    .into_iter()
    .any(|severity| severity != Severity::Off)
}

/// Write a stable `finding_id` onto every dead-code finding in `results`.
///
/// Every producer calls this on the full result set, before the workspace,
/// scope, changed-file, ignore, baseline and rule filters. A filter then never
/// changes the id of a finding that stays in the report.
pub fn stamp_finding_ids(results: &mut AnalysisResults, root: &Path) {
    fallow_types::identity::stamp_dead_code_finding_ids(results, root);
}

/// Give a `finding_id` to each dead-code finding that has none, and keep the
/// existing ids.
///
/// Type-aware refinement adds findings after the scope filters ran. A full
/// restamp there would compute tiebreak suffixes over the filtered set.
pub fn stamp_missing_finding_ids(results: &mut AnalysisResults, root: &Path) {
    fallow_types::identity::stamp_missing_dead_code_finding_ids(results, root);
}

/// A validated `--finding-id` request: the ids a run reports, in request order.
///
/// The filter runs after every other filter and after the baseline, so it
/// narrows what the run would otherwise report. The ids themselves are
/// stamped on the full result set before any filter, so a filter never
/// changes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingIdFilter {
    requested: Vec<String>,
    set: FxHashSet<String>,
}

impl FindingIdFilter {
    /// Validate the requested ids and drop duplicates. Returns `Ok(None)`
    /// when `values` is empty.
    ///
    /// # Errors
    ///
    /// Returns a message that names the first value that is not a current
    /// dead-code finding id (`dc1:<rule>:<16 hex digits>` with an optional
    /// `~<k>` suffix). A typo must never read as "the finding is gone".
    pub fn parse<S: AsRef<str>>(values: &[S]) -> Result<Option<Self>, String> {
        if values.is_empty() {
            return Ok(None);
        }
        let mut requested = Vec::with_capacity(values.len());
        let mut set = FxHashSet::default();
        for value in values {
            let id = value.as_ref().trim();
            if !fallow_types::identity::is_dead_code_finding_id(id) {
                return Err(format!(
                    "invalid finding id '{id}': expected {}:<rule>:<16 hex digits>, \
                     optionally with a ~<k> suffix, as printed in the finding_id field",
                    fallow_types::identity::DEAD_CODE_ID_SCHEME
                ));
            }
            if set.insert(id.to_owned()) {
                requested.push(id.to_owned());
            }
        }
        Ok(Some(Self { requested, set }))
    }

    /// The requested ids that a finding in `results` carries. Reads only.
    #[must_use]
    pub fn present(&self, results: &mut AnalysisResults) -> FxHashSet<String> {
        fallow_types::identity::present_dead_code_finding_ids(results, &self.set)
    }

    /// Keep only the requested findings and build the query answer.
    ///
    /// `filtered` holds the requested ids that the analysis found and a filter
    /// of this run removed. `run_reasons` are the options of this run that
    /// can hide a finding without a fix. `rule-off` is added when the rule of
    /// a missing id is `off` in `config`.
    pub fn apply(
        &self,
        results: &mut AnalysisResults,
        config: &ResolvedConfig,
        filtered: &FxHashSet<String>,
        run_reasons: impl IntoIterator<Item = fallow_output::FindingIdQueryReason>,
    ) -> fallow_output::FindingIdQuery {
        let found = fallow_types::identity::retain_dead_code_findings_by_id(results, &self.set);
        let rule_off = self
            .requested
            .iter()
            .filter(|id| !found.contains(*id))
            .any(|id| finding_id_rule_is_off(id, config));
        fallow_output::FindingIdQuery::new(
            self.requested.clone(),
            |id| found.contains(id),
            |id| filtered.contains(id),
            run_reasons
                .into_iter()
                .chain(rule_off.then_some(fallow_output::FindingIdQueryReason::RuleOff)),
            analysis_fingerprint(config),
        )
    }
}

/// Evidence for a finding-id answer, collected around the filter stages of
/// one run.
///
/// A requested id that is present before a filter stage and absent after it
/// was hidden by this run, not fixed. Those ids end in `filtered`. A stage
/// that is analysis (type-aware refinement) stays outside every stage, so a
/// finding it removes counts as gone.
#[derive(Debug, Clone)]
pub struct FindingIdTrace {
    filter: FindingIdFilter,
    before_stage: FxHashSet<String>,
    filtered: FxHashSet<String>,
}

impl FindingIdTrace {
    /// Start the first filter stage on the full result set.
    #[must_use]
    pub fn start(filter: FindingIdFilter, results: &mut AnalysisResults) -> Self {
        let before_stage = filter.present(results);
        Self {
            filter,
            before_stage,
            filtered: FxHashSet::default(),
        }
    }

    /// Start a filter stage after work that is not a filter.
    pub fn start_stage(&mut self, results: &mut AnalysisResults) {
        self.before_stage = self.filter.present(results);
    }

    /// End a filter stage: the requested ids it removed count as filtered.
    pub fn end_stage(&mut self, results: &mut AnalysisResults) {
        let after = self.filter.present(results);
        self.filtered
            .extend(self.before_stage.drain().filter(|id| !after.contains(id)));
    }

    /// Apply the filter and build the answer. See [`FindingIdFilter::apply`].
    pub fn finish(
        self,
        results: &mut AnalysisResults,
        config: &ResolvedConfig,
        run_reasons: impl IntoIterator<Item = fallow_output::FindingIdQueryReason>,
    ) -> fallow_output::FindingIdQuery {
        self.filter
            .apply(results, config, &self.filtered, run_reasons)
    }
}

/// The version prefix of an analysis fingerprint. A change to the hash inputs
/// moves it, so an old fingerprint never equals a new one.
const ANALYSIS_FINGERPRINT_SCHEME: &str = "af1";

/// Ignore files that discovery reads in each directory it walks.
const IGNORE_FILE_NAMES: &[&str] = &[".gitignore", ".ignore"];

/// Non-source files that import resolution and entry-point discovery read,
/// in every directory: manifests and TypeScript or JavaScript project files.
/// The built-in and external plugin config patterns are added to these.
const RESOLUTION_FILE_GLOBS: &[&str] =
    &["**/package.json", "**/tsconfig*.json", "**/jsconfig*.json"];

/// The maximum depth of a followed tsconfig `extends` chain.
const MAX_EXTENDS_DEPTH: usize = 8;

/// A stable hash of every input, other than the source files, that decides
/// which dead-code findings a run of `config` reports.
///
/// The inputs:
/// - the fallow version;
/// - the detection config digest (merged user config after `extends`,
///   external plugins, rule packs);
/// - the settings that a surface changes after resolution: production mode,
///   `includeEntryExports`, the effective rules, the type-aware mode,
///   requirement and project list, the file size limit;
/// - the root-relative path and content of the repository ignore files, the
///   `package.json` files, the `tsconfig*.json` and `jsconfig*.json` files and
///   the `extends` files they name, and every file that matches a built-in or
///   external plugin config pattern.
///
/// File content is normalized (CRLF to LF, trailing newlines removed) and the
/// entries are sorted, so two checkouts of one commit give the same value on
/// every platform. Known exclusions: the global git excludes file and other
/// machine environment outside the `FALLOW_*` variables.
#[must_use]
pub fn analysis_fingerprint(config: &ResolvedConfig) -> String {
    analysis_fingerprint_for_version(config, env!("CARGO_PKG_VERSION"))
}

/// [`analysis_fingerprint`] for an explicit fallow version.
#[must_use]
pub fn analysis_fingerprint_for_version(config: &ResolvedConfig, version: &str) -> String {
    let rules = serde_json::to_string(&config.rules).unwrap_or_default();
    let projects: Vec<String> = config
        .type_aware
        .projects
        .iter()
        .map(|project| root_relative_text(&config.root, project))
        .collect();
    let type_aware = format!(
        "{}:{}:{}",
        config.type_aware.enabled,
        serde_json::to_string(&config.type_aware.require).unwrap_or_default(),
        projects.join("|")
    );
    let max_file_size = config
        .max_file_size_bytes
        .map_or_else(|| "none".to_owned(), |bytes| bytes.to_string());
    let input_files = input_files_digest(config);
    let hash = fallow_types::identity::fnv1a64_parts(&[
        ANALYSIS_FINGERPRINT_SCHEME,
        version,
        &config.detection_config_digest,
        if config.production {
            "production"
        } else {
            "all"
        },
        if config.include_entry_exports {
            "entry-exports"
        } else {
            "no-entry-exports"
        },
        &rules,
        &type_aware,
        &max_file_size,
        &input_files,
    ]);
    format!("{ANALYSIS_FINGERPRINT_SCHEME}:{hash}")
}

/// `path` relative to `root` with forward slashes when it is inside the
/// root, else the text as given.
fn root_relative_text(root: &Path, path: &str) -> String {
    Path::new(path).strip_prefix(root).map_or_else(
        |_| path.replace('\\', "/"),
        |relative| StableFileKey::from_relative(relative).as_str().to_owned(),
    )
}

/// Normalize file text before it is hashed: CRLF becomes LF and trailing
/// newlines are removed, so a checkout with `core.autocrlf` hashes the same
/// as one without it.
fn normalized_text(content: &[u8]) -> String {
    String::from_utf8_lossy(content)
        .replace("\r\n", "\n")
        .trim_end_matches('\n')
        .to_owned()
}

/// The walker for the fingerprint inputs.
///
/// It honors the repository `.gitignore`, `.ignore` and `.git/info/exclude`
/// files, but never the global git excludes file of the machine: that file
/// would prune directories on one machine and not on another. It skips hidden
/// directories (the fallow cache lives there) and `node_modules`.
fn fingerprint_walk_builder(root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            if !is_dir || entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            !name.starts_with('.') && name != "node_modules"
        });
    builder
}

/// The globs of the non-source files the analysis reads to resolve imports
/// and entry points, each also tried under `**/`.
fn resolution_file_globs(config: &ResolvedConfig) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    let external = config
        .external_plugins
        .iter()
        .flat_map(|plugin| plugin.config_patterns.iter().map(String::as_str));
    let patterns = crate::core_backend::builtin_config_patterns()
        .into_iter()
        .chain(external)
        .chain(RESOLUTION_FILE_GLOBS.iter().copied());
    for pattern in patterns {
        let anywhere = if pattern.starts_with("**/") {
            pattern.to_owned()
        } else {
            format!("**/{pattern}")
        };
        for candidate in [pattern.to_owned(), anywhere] {
            if let Ok(glob) = globset::Glob::new(&candidate) {
                builder.add(glob);
            }
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}

fn is_project_config_name(name: &str) -> bool {
    (name.starts_with("tsconfig") || name.starts_with("jsconfig"))
        && std::path::Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

/// A hash over the root-relative path and normalized content of each
/// fingerprint input file. See [`analysis_fingerprint`].
fn input_files_digest(config: &ResolvedConfig) -> String {
    let root = config.root.as_path();
    let globs = resolution_file_globs(config);
    let mut files: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    if let Ok(content) = std::fs::read(root.join(".git/info/exclude")) {
        files.insert(".git/info/exclude".to_owned(), normalized_text(&content));
    }
    let mut project_configs: Vec<PathBuf> = Vec::new();
    for entry in fingerprint_walk_builder(root).build().flatten() {
        if entry.file_type().is_none_or(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        let name = entry.file_name().to_string_lossy();
        let is_ignore_file = IGNORE_FILE_NAMES.contains(&name.as_ref());
        if !is_ignore_file && !globs.is_match(relative) {
            continue;
        }
        if config.ignore_patterns.is_match(relative) {
            continue;
        }
        if let Ok(content) = std::fs::read(entry.path()) {
            let key = StableFileKey::from_relative(relative).as_str().to_owned();
            files.insert(key, normalized_text(&content));
            if is_project_config_name(&name) {
                project_configs.push(entry.path().to_path_buf());
            }
        }
    }
    for project_config in project_configs {
        add_extends_chain(root, &project_config, &mut files);
    }
    let parts: Vec<&str> = files
        .iter()
        .flat_map(|(path, content)| [path.as_str(), content.as_str()])
        .collect();
    fallow_types::identity::fnv1a64_parts(&parts)
}

/// Follow the `extends` chain of one tsconfig or jsconfig file and add each
/// file it names, also a file the walk did not see: one in a hidden
/// directory, outside the root, or in `node_modules`.
fn add_extends_chain(
    root: &Path,
    project_config: &Path,
    files: &mut std::collections::BTreeMap<String, String>,
) {
    let mut seen: FxHashSet<PathBuf> = FxHashSet::default();
    let mut frontier: Vec<(PathBuf, usize)> = vec![(project_config.to_path_buf(), 0)];
    while let Some((current, depth)) = frontier.pop() {
        if depth >= MAX_EXTENDS_DEPTH || !seen.insert(current.clone()) {
            continue;
        }
        for target in read_extends(&current).unwrap_or_default() {
            let Some(next) = resolve_extends_target(root, &current, &target) else {
                continue;
            };
            if let Ok(content) = std::fs::read(&next) {
                files.insert(extends_key(root, &next), normalized_text(&content));
                frontier.push((next, depth + 1));
            }
        }
    }
}

/// The `extends` targets of a tsconfig or jsconfig file: one string or an
/// array of strings.
fn read_extends(path: &Path) -> Option<Vec<String>> {
    let content = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = fallow_config::jsonc::parse_to_value(&content).ok()?;
    match value.get("extends")? {
        serde_json::Value::String(target) => Some(vec![target.clone()]),
        serde_json::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

/// The file an `extends` target names: a relative path from the extending
/// file, or a package path under the root `node_modules`.
fn resolve_extends_target(root: &Path, from: &Path, target: &str) -> Option<PathBuf> {
    let base = if target.starts_with('.') || Path::new(target).is_absolute() {
        from.parent()?.join(target)
    } else {
        root.join("node_modules").join(target)
    };
    let candidates = [
        base.clone(),
        base.with_extension("json"),
        base.join("tsconfig.json"),
    ];
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// The hash key of an `extends` file: root-relative when inside the root,
/// else `extends:` plus the file name, which carries no machine path.
fn extends_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| {
            format!(
                "extends:{}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        },
        |relative| StableFileKey::from_relative(relative).as_str().to_owned(),
    )
}

/// Whether the rule of `id` is `off` in the top-level rules or in any
/// override. An override is file-scoped and the id carries no path, so any
/// override that turns the rule off counts.
fn finding_id_rule_is_off(id: &str, config: &ResolvedConfig) -> bool {
    let Some(kind) = id
        .split(':')
        .nth(1)
        .and_then(fallow_types::suppress::IssueKind::parse)
    else {
        return false;
    };
    let off = |rules: &RulesConfig| rules.severity_for_kind(kind) == Severity::Off;
    off(&config.rules)
        || config.overrides.iter().any(|entry| {
            let mut rules = config.rules.clone();
            rules.apply_partial(&entry.rules);
            off(&rules)
        })
}

/// Scope dead-code results to the union of the given workspace roots.
///
/// The full cross-workspace graph is still built before this helper runs, so
/// cross-package imports are resolved. Only reported findings are narrowed.
pub fn filter_to_workspaces(results: &mut AnalysisResults, ws_roots: &[PathBuf]) {
    let any_under = |path: &Path| ws_roots.iter().any(|root| path.starts_with(root));
    let pkg_jsons = ws_roots
        .iter()
        .map(|root| root.join("package.json"))
        .collect::<Vec<_>>();
    let in_pkg_jsons = |path: &Path| pkg_jsons.iter().any(|pkg| path == pkg);

    filter_workspace_source_findings(results, &any_under);
    filter_workspace_dependency_findings(results, &any_under, &in_pkg_jsons);
    filter_workspace_graph_findings(results, &any_under);
    filter_workspace_policy_findings(results, &any_under);
}

/// The scope of one dead-code run, as the surface resolved it.
///
/// Every field is optional. A field that is `None` does not narrow the run.
#[derive(Debug, Clone, Copy)]
pub struct DeadCodeScope<'a> {
    /// `--workspace`, `--changed-workspaces` and a positional path: the union
    /// of these roots.
    pub workspace_roots: Option<&'a [PathBuf]>,
    /// The resolved change scope: a global changed-file set or the
    /// configured package baselines.
    pub changes: Option<&'a ChangeScope>,
    /// A unified diff, with the root that finding paths resolve against.
    pub diff: Option<(&'a fallow_output::DiffIndex, &'a Path)>,
    /// `--file`: the only files to report. Dependency findings are dropped,
    /// because a file list does not own a manifest.
    pub files: Option<&'a FxHashSet<PathBuf>>,
}

/// Narrow dead-code results to the scope of the run.
///
/// The CLI, the programmatic API and the MCP typed path call this one function,
/// so a scope narrows the same way on every surface. The filters run in this
/// order: workspace roots, changed files, the diff, the file list. Then the
/// configured `ignoreFindings` patterns run again, because the scope filters
/// remove owners from a finding with several owners (`duplicate_exports`). A
/// finding that only ignored owners hold after the scope is hidden, as the
/// "hidden only when every owner matches" rule says.
pub fn apply_scope(
    results: &mut AnalysisResults,
    scope: &DeadCodeScope<'_>,
    config: &ResolvedConfig,
) {
    if let Some(roots) = scope.workspace_roots {
        filter_to_workspaces(results, roots);
    }
    if let Some(changes) = scope.changes {
        changes.retain_dead_code(results);
    }
    if let Some((diff, root)) = scope.diff {
        crate::diff_scope::filter_dead_code_by_diff(results, diff, root);
    }
    if let Some(files) = scope.files {
        filter_by_changed_files(results, files);
        clear_dependency_findings(results);
    }
    filter_configured_ignored_findings(results, config);
}

fn clear_dependency_findings(results: &mut AnalysisResults) {
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
    results.dev_dependencies_in_production.clear();
}

/// Scope dead-code results to findings affected by changed files.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_by_changed_files(results: &mut AnalysisResults, changed_files: &FxHashSet<PathBuf>) {
    crate::changed_files::filter_results_by_changed_files(results, changed_files);
}

/// Apply configured source-owned finding exclusions to an analysis result.
///
/// Analysis stages that append findings after the engine pipeline, such as
/// type-aware reconciliation, must call this before exposing their final
/// result.
pub fn filter_configured_ignored_findings(results: &mut AnalysisResults, config: &ResolvedConfig) {
    if config.ignore_findings.is_empty() {
        return;
    }

    results.remove_ignored_dead_code_findings(|path| {
        let key = if path.is_absolute() {
            let Ok(relative) = path.strip_prefix(&config.root) else {
                return false;
            };
            StableFileKey::from_relative(relative)
        } else {
            StableFileKey::from_relative(path)
        };
        config.ignore_findings.is_ignored(key.as_str())
    });
}

fn filter_workspace_source_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_files
        .retain(|finding| any_under(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| any_under(&finding.export.path));
    results
        .unused_types
        .retain(|finding| any_under(&finding.export.path));
    results
        .private_type_leaks
        .retain(|finding| any_under(&finding.leak.path));
    results
        .deprecated_exports_in_use
        .retain(|finding| any_under(&finding.export.path));
    results
        .unused_enum_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_class_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_store_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unprovided_injects
        .retain(|finding| any_under(&finding.inject.path));
    results
        .unrendered_components
        .retain(|finding| any_under(&finding.component.path));
    results
        .unused_component_props
        .retain(|finding| any_under(&finding.prop.path));
    results
        .absent_component_props
        .retain(|finding| any_under(&finding.prop.path));
    results
        .unused_component_emits
        .retain(|finding| any_under(&finding.emit.path));
    results
        .unused_component_inputs
        .retain(|finding| any_under(&finding.input.path));
    results
        .unused_component_outputs
        .retain(|finding| any_under(&finding.output.path));
    results
        .unused_svelte_events
        .retain(|finding| any_under(&finding.event.path));
    results
        .unused_server_actions
        .retain(|finding| any_under(&finding.action.path));
    results
        .unused_load_data_keys
        .retain(|finding| any_under(&finding.key.path));
    results
        .unresolved_imports
        .retain(|finding| any_under(&finding.import.path));
}

fn filter_workspace_dependency_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
    in_pkg_jsons: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_dev_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_optional_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .type_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .test_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .dev_dependencies_in_production
        .retain(|finding| in_pkg_jsons(&finding.dep.path));

    results.unlisted_dependencies.retain(|finding| {
        finding
            .dep
            .imported_from
            .iter()
            .any(|source| any_under(&source.path))
    });
    results.unused_dependency_overrides.clear();
    results.misconfigured_dependency_overrides.clear();
}

fn filter_workspace_graph_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    for duplicate in &mut results.duplicate_exports {
        duplicate
            .export
            .locations
            .retain(|location| any_under(&location.path));
    }
    results
        .duplicate_exports
        .retain(|duplicate| duplicate.export.locations.len() >= 2);

    results
        .circular_dependencies
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));

    results
        .re_export_cycles
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));

    results
        .package_cycles
        .retain(|cycle| cycle.cycle.edges.iter().any(|edge| any_under(&edge.path)));
}

fn filter_workspace_policy_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .boundary_violations
        .retain(|finding| any_under(&finding.violation.from_path));
    results
        .boundary_coverage_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .boundary_call_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .policy_violations
        .retain(|finding| any_under(&finding.violation.path));

    results
        .stale_suppressions
        .retain(|finding| any_under(&finding.path));

    results
        .security_findings
        .retain(|finding| any_under(&finding.path));
    results
        .security_unresolved_callee_diagnostics
        .retain(|finding| any_under(&finding.path));

    results.unused_catalog_entries.clear();
    results.empty_catalog_groups.clear();
    results
        .unresolved_catalog_references
        .retain(|finding| any_under(&finding.reference.path));

    results
        .invalid_client_exports
        .retain(|finding| any_under(&finding.export.path));

    results
        .mixed_client_server_barrels
        .retain(|finding| any_under(&finding.barrel.path));

    results
        .misplaced_directives
        .retain(|finding| any_under(&finding.directive_site.path));

    results
        .route_collisions
        .retain(|finding| any_under(&finding.collision.path));

    results
        .dynamic_segment_name_conflicts
        .retain(|finding| any_under(&finding.conflict.path));
}

/// Remove findings whose effective severity is `Off` from an analysis result.
///
/// Every surface that reports findings runs this pass: the `check` command
/// (which also serves `dead-code` and the CLI audit), the editor analysis path
/// behind inline diagnostics and the sidebar, and the programmatic runtime
/// behind the MCP tools, the decision surface and the Node bindings. Each of
/// them runs it at the same two points, once over the freshly analyzed set and
/// once after type-aware reconciliation, because reconciliation can append
/// findings. The pass removes findings and writes the gate severity of each
/// finding that stays, so the second run is idempotent when nothing was
/// appended.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types. Circular dependencies resolve against every file in
/// the cycle. Non-file-scoped issues (unused deps, unlisted deps, duplicate
/// exports) use the base rules only.
pub fn apply_rule_severities(results: &mut AnalysisResults, config: &ResolvedConfig) {
    let rules = &config.rules;
    let has_overrides = !config.overrides.is_empty();

    if has_overrides {
        apply_file_override_rules(results, config);
        apply_boundary_override_rules(results, config);
    } else {
        apply_base_file_rules(results, rules);
    }

    apply_base_collection_rules(results, rules);
    apply_effective_severities(results, config);
}

fn apply_base_collection_rules(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unused_dependencies == Severity::Off {
        results.unused_dependencies.clear();
    }
    if rules.unused_dev_dependencies == Severity::Off {
        results.unused_dev_dependencies.clear();
    }
    if rules.unused_optional_dependencies == Severity::Off {
        results.unused_optional_dependencies.clear();
    }
    if rules.unlisted_dependencies == Severity::Off {
        results.unlisted_dependencies.clear();
    }
    if rules.duplicate_exports == Severity::Off {
        results.duplicate_exports.clear();
    }
    if rules.type_only_dependencies == Severity::Off {
        results.type_only_dependencies.clear();
    }
    if rules.test_only_dependencies == Severity::Off {
        results.test_only_dependencies.clear();
    }
    if rules.dev_dependencies_in_production == Severity::Off {
        results.dev_dependencies_in_production.clear();
    }
    if rules.circular_dependencies == Severity::Off {
        results.circular_dependencies.clear();
    }
    if rules.re_export_cycle == Severity::Off {
        results.re_export_cycles.clear();
    }
    if rules.package_cycle == Severity::Off {
        results.package_cycles.clear();
    }
    if rules.boundary_violation == Severity::Off {
        results.boundary_violations.clear();
        results.boundary_coverage_violations.clear();
        results.boundary_call_violations.clear();
    }
    if rules.policy_violation == Severity::Off {
        results.policy_violations.clear();
    }
    if rules.unused_catalog_entries == Severity::Off {
        results.unused_catalog_entries.clear();
    }
    if rules.empty_catalog_groups == Severity::Off {
        results.empty_catalog_groups.clear();
    }
    if rules.unresolved_catalog_references == Severity::Off {
        results.unresolved_catalog_references.clear();
    }
    if rules.unused_dependency_overrides == Severity::Off {
        results.unused_dependency_overrides.clear();
    }
    if rules.misconfigured_dependency_overrides == Severity::Off {
        results.misconfigured_dependency_overrides.clear();
    }
}

fn apply_file_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    apply_dead_code_override_rules(results, config);
    apply_catalog_override_rules(results, config);
    apply_framework_override_rules(results, config);
    apply_circular_override_rules(results, config);
}

fn apply_dead_code_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    apply_core_dead_code_override_rules(results, config);
    apply_component_dead_code_override_rules(results, config);
}

/// Retain core (non-component) dead-code findings whose per-file rule is not Off.
fn apply_core_dead_code_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results
        .unused_files
        .retain(|f| config.resolve_rules_for_path(&f.file.path).unused_files != Severity::Off);
    results
        .unused_exports
        .retain(|e| config.resolve_rules_for_path(&e.export.path).unused_exports != Severity::Off);
    results
        .unused_types
        .retain(|e| config.resolve_rules_for_path(&e.export.path).unused_types != Severity::Off);
    results.private_type_leaks.retain(|e| {
        config
            .resolve_rules_for_path(&e.leak.path)
            .private_type_leaks
            != Severity::Off
    });
    results.deprecated_exports_in_use.retain(|e| {
        config
            .resolve_rules_for_path(&e.export.path)
            .deprecated_exports_in_use
            != Severity::Off
    });
    results.unused_enum_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_enum_members
            != Severity::Off
    });
    results.unused_class_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_class_members
            != Severity::Off
    });
    results.unused_store_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_store_members
            != Severity::Off
    });
    results.unprovided_injects.retain(|f| {
        config
            .resolve_rules_for_path(&f.inject.path)
            .unprovided_injects
            != Severity::Off
    });
    results.unresolved_imports.retain(|i| {
        config
            .resolve_rules_for_path(&i.import.path)
            .unresolved_imports
            != Severity::Off
    });
}

/// Retain component-shaped dead-code findings whose per-file rule is not Off.
fn apply_component_dead_code_override_rules(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
) {
    results.unrendered_components.retain(|c| {
        config
            .resolve_rules_for_path(&c.component.path)
            .unrendered_components
            != Severity::Off
    });
    results.unused_component_props.retain(|p| {
        config
            .resolve_rules_for_path(&p.prop.path)
            .unused_component_props
            != Severity::Off
    });
    results.absent_component_props.retain(|p| {
        config
            .resolve_rules_for_path(&p.prop.path)
            .absent_component_props
            != Severity::Off
    });
    results.unused_component_emits.retain(|e| {
        config
            .resolve_rules_for_path(&e.emit.path)
            .unused_component_emits
            != Severity::Off
    });
    results.unused_component_inputs.retain(|i| {
        config
            .resolve_rules_for_path(&i.input.path)
            .unused_component_inputs
            != Severity::Off
    });
    results.unused_component_outputs.retain(|o| {
        config
            .resolve_rules_for_path(&o.output.path)
            .unused_component_outputs
            != Severity::Off
    });
    results.unused_svelte_events.retain(|e| {
        config
            .resolve_rules_for_path(&e.event.path)
            .unused_svelte_events
            != Severity::Off
    });
    results.unused_server_actions.retain(|a| {
        config
            .resolve_rules_for_path(&a.action.path)
            .unused_server_actions
            != Severity::Off
    });
    results.unused_load_data_keys.retain(|k| {
        config
            .resolve_rules_for_path(&k.key.path)
            .unused_load_data_keys
            != Severity::Off
    });
}

fn apply_catalog_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.stale_suppressions.retain(|s| {
        let rules = config.resolve_rules_for_path(&s.path);
        if s.missing_reason {
            rules.require_suppression_reason != Severity::Off
        } else {
            rules.stale_suppressions != Severity::Off
        }
    });
    results.unresolved_catalog_references.retain(|r| {
        config
            .resolve_rules_for_path(&r.reference.path)
            .unresolved_catalog_references
            != Severity::Off
    });
    results.empty_catalog_groups.retain(|g| {
        config
            .resolve_rules_for_path(&g.group.path)
            .empty_catalog_groups
            != Severity::Off
    });
    results.unused_dependency_overrides.retain(|o| {
        config
            .resolve_rules_for_path(&o.entry.path)
            .unused_dependency_overrides
            != Severity::Off
    });
    results.misconfigured_dependency_overrides.retain(|o| {
        config
            .resolve_rules_for_path(&o.entry.path)
            .misconfigured_dependency_overrides
            != Severity::Off
    });
}

fn apply_framework_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.invalid_client_exports.retain(|e| {
        config
            .resolve_rules_for_path(&e.export.path)
            .invalid_client_export
            != Severity::Off
    });
    results.mixed_client_server_barrels.retain(|b| {
        config
            .resolve_rules_for_path(&b.barrel.path)
            .mixed_client_server_barrel
            != Severity::Off
    });
    results.misplaced_directives.retain(|d| {
        config
            .resolve_rules_for_path(&d.directive_site.path)
            .misplaced_directive
            != Severity::Off
    });
    results.route_collisions.retain(|c| {
        config
            .resolve_rules_for_path(&c.collision.path)
            .route_collision
            != Severity::Off
    });
    results.dynamic_segment_name_conflicts.retain(|c| {
        config
            .resolve_rules_for_path(&c.conflict.path)
            .dynamic_segment_name_conflict
            != Severity::Off
    });
}

fn apply_circular_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.circular_dependencies.retain(|c| {
        c.cycle
            .files
            .iter()
            .any(|path| config.resolve_rules_for_path(path).circular_dependencies != Severity::Off)
    });
}

fn apply_base_file_rules(results: &mut AnalysisResults, rules: &RulesConfig) {
    clear_base_core_dead_code(results, rules);
    clear_base_component_dead_code(results, rules);
    clear_base_suppression_and_framework(results, rules);
}

/// Clear core (non-component) dead-code findings whose base rule is Off.
fn clear_base_core_dead_code(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unused_files == Severity::Off {
        results.unused_files.clear();
    }
    if rules.unused_exports == Severity::Off {
        results.unused_exports.clear();
    }
    if rules.unused_types == Severity::Off {
        results.unused_types.clear();
    }
    if rules.private_type_leaks == Severity::Off {
        results.private_type_leaks.clear();
    }
    if rules.deprecated_exports_in_use == Severity::Off {
        results.deprecated_exports_in_use.clear();
    }
    if rules.unused_enum_members == Severity::Off {
        results.unused_enum_members.clear();
    }
    if rules.unused_class_members == Severity::Off {
        results.unused_class_members.clear();
    }
    if rules.unused_store_members == Severity::Off {
        results.unused_store_members.clear();
    }
    if rules.unprovided_injects == Severity::Off {
        results.unprovided_injects.clear();
    }
    if rules.unresolved_imports == Severity::Off {
        results.unresolved_imports.clear();
    }
}

/// Clear component-shaped dead-code findings whose base rule is Off.
fn clear_base_component_dead_code(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unrendered_components == Severity::Off {
        results.unrendered_components.clear();
    }
    if rules.unused_component_props == Severity::Off {
        results.unused_component_props.clear();
    }
    if rules.absent_component_props == Severity::Off {
        results.absent_component_props.clear();
    }
    if rules.unused_component_emits == Severity::Off {
        results.unused_component_emits.clear();
    }
    if rules.unused_component_inputs == Severity::Off {
        results.unused_component_inputs.clear();
    }
    if rules.unused_component_outputs == Severity::Off {
        results.unused_component_outputs.clear();
    }
    if rules.unused_svelte_events == Severity::Off {
        results.unused_svelte_events.clear();
    }
    if rules.unused_server_actions == Severity::Off {
        results.unused_server_actions.clear();
    }
    if rules.unused_load_data_keys == Severity::Off {
        results.unused_load_data_keys.clear();
    }
}

/// Apply base stale-suppression retention and clear framework findings whose
/// base rule is Off.
fn clear_base_suppression_and_framework(results: &mut AnalysisResults, rules: &RulesConfig) {
    results.stale_suppressions.retain(|s| {
        if s.missing_reason {
            rules.require_suppression_reason != Severity::Off
        } else {
            rules.stale_suppressions != Severity::Off
        }
    });
    if rules.invalid_client_export == Severity::Off {
        results.invalid_client_exports.clear();
    }
    if rules.mixed_client_server_barrel == Severity::Off {
        results.mixed_client_server_barrels.clear();
    }
    if rules.misplaced_directive == Severity::Off {
        results.misplaced_directives.clear();
    }
    if rules.route_collision == Severity::Off {
        results.route_collisions.clear();
    }
    if rules.dynamic_segment_name_conflict == Severity::Off {
        results.dynamic_segment_name_conflicts.clear();
    }
}

fn apply_boundary_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.boundary_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.from_path)
            .boundary_violation
            != Severity::Off
    });
    results.boundary_coverage_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            != Severity::Off
    });
    results.boundary_call_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            != Severity::Off
    });
    results.policy_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .policy_violation
            != Severity::Off
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use fallow_types::output_dead_code::{
        BoundaryViolationFinding, CircularDependencyFinding, PrivateTypeLeakFinding,
        UnusedExportFinding, UnusedFileFinding,
    };
    use fallow_types::results::{
        BoundaryViolation, CircularDependency, PrivateTypeLeak, UnusedExport, UnusedFile,
    };

    #[test]
    fn finding_id_filter_keeps_request_order_and_drops_duplicates() {
        let a = "dc1:unused-export:0123456789abcdef";
        let b = "dc1:unused-file:0123456789abcdef~1";

        let filter = FindingIdFilter::parse(&[b, a, b])
            .expect("valid ids")
            .expect("a filter");

        assert_eq!(filter.requested, vec![b.to_owned(), a.to_owned()]);
        assert_eq!(FindingIdFilter::parse::<&str>(&[]), Ok(None));
    }

    #[test]
    fn normalized_text_ignores_line_endings_and_trailing_newlines() {
        assert_eq!(normalized_text(b"dist\r\nbuild\r\n"), "dist\nbuild");
        assert_eq!(normalized_text(b"dist\nbuild\n\n"), "dist\nbuild");
        assert_eq!(normalized_text(b"dist\nbuild"), "dist\nbuild");
        assert_ne!(
            normalized_text(b"dist\nbuild"),
            normalized_text(b"dist\nbuilt")
        );
    }

    #[test]
    fn a_crlf_checkout_has_the_same_fingerprint_as_an_lf_checkout() {
        let write_project = |line_end: &str| {
            let dir = tempfile::tempdir().expect("project");
            let root = dir.path();
            std::fs::create_dir_all(root.join("src")).expect("src");
            std::fs::write(
                root.join(".gitignore"),
                format!("dist{line_end}coverage{line_end}"),
            )
            .expect("gitignore");
            std::fs::write(
                root.join("package.json"),
                format!("{{{line_end}  \"name\": \"crlf\"{line_end}}}{line_end}"),
            )
            .expect("package.json");
            dir
        };
        let config_at = |root: &std::path::Path| {
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Json,
                1,
                true,
                true,
                None,
            )
        };
        let lf = write_project("\n");
        let crlf = write_project("\r\n");

        assert_eq!(
            analysis_fingerprint_for_version(&config_at(lf.path()), "1.0.0"),
            analysis_fingerprint_for_version(&config_at(crlf.path()), "1.0.0")
        );
    }

    #[test]
    fn a_tsconfig_extends_target_outside_the_walk_is_an_input() {
        let dir = tempfile::tempdir().expect("project");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".config")).expect("hidden dir");
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{ "extends": "./.config/tsconfig.base.json" }"#,
        )
        .expect("tsconfig");
        let base = root.join(".config/tsconfig.base.json");
        std::fs::write(&base, r#"{ "compilerOptions": { "baseUrl": "." } }"#).expect("base");
        let config = fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        let before = analysis_fingerprint_for_version(&config, "1.0.0");

        std::fs::write(&base, r#"{ "compilerOptions": { "baseUrl": "src" } }"#).expect("edit");

        assert_ne!(analysis_fingerprint_for_version(&config, "1.0.0"), before);
    }

    #[test]
    fn analysis_fingerprint_depends_on_the_version_and_not_on_the_root() {
        let config_at = |root: &std::path::Path| {
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Json,
                1,
                true,
                true,
                None,
            )
        };
        let a = tempfile::tempdir().expect("project a");
        let b = tempfile::tempdir().expect("project b");
        let config_a = config_at(a.path());
        let config_b = config_at(b.path());

        let base = analysis_fingerprint_for_version(&config_a, "1.0.0");
        assert!(base.starts_with("af1:"), "{base}");
        assert_eq!(base, analysis_fingerprint_for_version(&config_a, "1.0.0"));
        assert_eq!(base, analysis_fingerprint_for_version(&config_b, "1.0.0"));
        assert_ne!(base, analysis_fingerprint_for_version(&config_a, "1.0.1"));
    }

    #[test]
    fn finding_id_filter_refuses_a_malformed_id() {
        let error = FindingIdFilter::parse(&["dc1:unused-export:helper"])
            .expect_err("malformed id refused");

        assert!(error.contains("dc1:unused-export:helper"), "{error}");
    }

    #[test]
    fn finding_id_rule_is_off_reads_rules_and_overrides() {
        let id = "dc1:unused-export:0123456789abcdef";
        let mut config = fallow_config::FallowConfig::default().resolve(
            PathBuf::from("/repo"),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        assert!(!finding_id_rule_is_off(id, &config));

        config.rules.unused_exports = Severity::Off;
        assert!(finding_id_rule_is_off(id, &config));
        assert!(!finding_id_rule_is_off(
            "dc1:unused-file:0123456789abcdef",
            &config
        ));

        let with_override = serde_json::from_str::<fallow_config::FallowConfig>(
            r#"{"overrides":[{"files":["src/a.ts"],"rules":{"unused-exports":"off"}}]}"#,
        )
        .expect("config parses")
        .resolve(
            PathBuf::from("/repo"),
            fallow_config::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        assert!(finding_id_rule_is_off(id, &with_override));
    }

    #[test]
    fn workspace_filter_keeps_findings_under_workspace_root() {
        let root = PathBuf::from("/repo/packages/app");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/unused.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/repo/packages/docs/src/unused.ts"),
            }));

        filter_to_workspaces(&mut results, std::slice::from_ref(&root));

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            root.join("src/unused.ts")
        );
    }

    #[test]
    fn configured_filter_removes_findings_added_after_engine_analysis() {
        let project = tempfile::tempdir().expect("project");
        let config = serde_json::from_str::<fallow_config::FallowConfig>(
            r#"{"ignoreFindings":["src/hidden.ts"]}"#,
        )
        .expect("config parses")
        .resolve(
            project.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: project.path().join("src/hidden.ts"),
                export_name: "publicApi".to_string(),
                type_name: "PrivateShape".to_string(),
                line: 1,
                col: 0,
                span_start: 0,
                semantic: None,
            }));
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: project.path().join("src/hidden.ts"),
                to_path: project.path().join("src/data.ts"),
                from_zone: "ui".to_string(),
                to_zone: "data".to_string(),
                import_specifier: "./data".to_string(),
                line: 1,
                col: 0,
                via_path: None,
            }));

        filter_configured_ignored_findings(&mut results, &config);

        assert!(results.private_type_leaks.is_empty());
        assert_eq!(results.boundary_violations.len(), 1);
    }

    fn unmatched_patterns(diagnostics: &[WorkspaceDiagnostic]) -> Vec<(&'static str, String)> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| match &diagnostic.kind {
                WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched { pattern } => {
                    Some(("ignoreFindings", pattern.clone()))
                }
                WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched { pattern } => {
                    Some(("ignoreDependencies", pattern.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn write_manifest(root: &Path, dependencies: &str) {
        std::fs::write(
            root.join("package.json"),
            format!(
                r#"{{"name":"app","private":true,"main":"src/index.ts","dependencies":{dependencies}}}"#
            ),
        )
        .expect("write package.json");
    }

    #[test]
    fn config_pattern_diagnostics_describe_the_latest_pass_only() {
        let project = tempfile::tempdir().expect("project");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::write(root.join("src/index.ts"), "export const main = 1;\n").expect("write entry");
        std::fs::write(root.join("src/orphan.ts"), "export const orphan = 1;\n")
            .expect("write orphan");
        write_manifest(root, r#"{"@acme/lib":"1.0.0"}"#);
        let config = serde_json::from_str::<fallow_config::FallowConfig>(
            r#"{"ignoreDependencies":["@acme/*","@typo/*"],"ignoreFindings":["src/legcy/**"]}"#,
        )
        .expect("config parses")
        .resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );

        crate::session::AnalysisSession::from_resolved_config(config.clone())
            .expect("session")
            .analyze_dead_code()
            .expect("first pass");
        assert_eq!(
            unmatched_patterns(&config_pattern_diagnostics(&config, true)),
            vec![
                ("ignoreFindings", "src/legcy/**".to_owned()),
                ("ignoreDependencies", "@typo/*".to_owned()),
            ]
        );
        assert_eq!(
            unmatched_patterns(&config_pattern_diagnostics(&config, false)),
            vec![("ignoreFindings", "src/legcy/**".to_owned())],
            "a run that reports no dependency findings omits the dependency globs"
        );

        // A long-lived process keeps the config. The second pass must not
        // inherit the `@acme/*` hit of the first pass.
        write_manifest(root, r#"{"react":"1.0.0"}"#);
        crate::session::AnalysisSession::from_resolved_config(config.clone())
            .expect("session")
            .analyze_dead_code()
            .expect("second pass");
        assert_eq!(
            unmatched_patterns(&config_pattern_diagnostics(&config, true)),
            vec![
                ("ignoreFindings", "src/legcy/**".to_owned()),
                ("ignoreDependencies", "@acme/*".to_owned()),
                ("ignoreDependencies", "@typo/*".to_owned()),
            ]
        );
    }

    fn config_with_override(
        pattern: &str,
        configure: impl FnOnce(&mut fallow_config::PartialRulesConfig),
    ) -> ResolvedConfig {
        let mut partial = fallow_config::PartialRulesConfig::default();
        configure(&mut partial);
        fallow_config::FallowConfig {
            rules: RulesConfig {
                private_type_leaks: Severity::Warn,
                ..RulesConfig::default()
            },
            overrides: vec![fallow_config::ConfigOverride {
                files: vec![pattern.to_string()],
                rules: partial,
            }],
            ..fallow_config::FallowConfig::default()
        }
        .resolve(
            PathBuf::from("/project"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    fn unused_export(path: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: "Unused".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    }

    fn private_type_leak(path: &str) -> PrivateTypeLeakFinding {
        PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: PathBuf::from(path),
            export_name: "Unused".to_string(),
            type_name: "Props".to_string(),
            line: 1,
            col: 0,
            span_start: 0,
            semantic: None,
        })
    }

    fn overridden_fixture() -> AnalysisResults {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(unused_export("/project/src/ui/kit.ts"));
        results
            .unused_exports
            .push(unused_export("/project/src/lib/util.ts"));
        results
            .private_type_leaks
            .push(private_type_leak("/project/src/ui/kit.ts"));
        results
            .private_type_leaks
            .push(private_type_leak("/project/src/lib/util.ts"));
        results
    }

    #[test]
    fn rule_severities_drop_findings_only_on_overridden_paths() {
        let config = config_with_override("src/ui/**", |rules| {
            rules.unused_exports = Some(Severity::Off);
            rules.private_type_leaks = Some(Severity::Off);
        });
        let mut results = overridden_fixture();

        apply_rule_severities(&mut results, &config);

        assert_eq!(
            results
                .unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("/project/src/lib/util.ts")]
        );
        assert_eq!(
            results
                .private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("/project/src/lib/util.ts")]
        );
    }

    #[test]
    fn rule_severities_are_idempotent() {
        // The editor path resolves severities once after analysis and again
        // after type-aware reconciliation, so a second pass must not change
        // the result set.
        let config = config_with_override("src/ui/**", |rules| {
            rules.unused_exports = Some(Severity::Off);
            rules.private_type_leaks = Some(Severity::Off);
        });

        let mut once = overridden_fixture();
        apply_rule_severities(&mut once, &config);
        let mut twice = overridden_fixture();
        apply_rule_severities(&mut twice, &config);
        apply_rule_severities(&mut twice, &config);

        assert_eq!(
            once.unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>(),
            twice
                .unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            once.private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>(),
            twice
                .private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rule_severities_keep_a_cycle_when_any_member_file_stays_enabled() {
        let config = config_with_override("src/ui/**", |rules| {
            rules.circular_dependencies = Some(Severity::Off);
        });
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/ui/a.ts"),
                        PathBuf::from("/project/src/lib/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/ui/c.ts"),
                        PathBuf::from("/project/src/ui/d.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        apply_rule_severities(&mut results, &config);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert_eq!(
            results.circular_dependencies[0].cycle.files[0],
            PathBuf::from("/project/src/ui/a.ts")
        );
    }

    #[test]
    fn rule_severities_clear_base_rules_without_overrides() {
        let config = fallow_config::FallowConfig {
            rules: RulesConfig {
                unused_exports: Severity::Off,
                private_type_leaks: Severity::Warn,
                ..RulesConfig::default()
            },
            ..fallow_config::FallowConfig::default()
        }
        .resolve(
            PathBuf::from("/project"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let mut results = overridden_fixture();

        apply_rule_severities(&mut results, &config);

        assert!(results.unused_exports.is_empty());
        assert_eq!(results.private_type_leaks.len(), 2);
    }
}
