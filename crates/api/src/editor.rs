//! Editor-facing analysis contracts shared by LSP and future editor adapters.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_types::{discover::DiscoveredFile, extract::ModuleInfo};

pub type EditorCloneFamily = fallow_types::duplicates::CloneFamily;
pub type EditorCloneGroup = fallow_types::duplicates::CloneGroup;
pub type EditorCloneInstance = fallow_types::duplicates::CloneInstance;
pub type EditorDuplicationReport = fallow_types::duplicates::DuplicationReport;
pub type EditorDuplicationStats = fallow_types::duplicates::DuplicationStats;
pub type EditorMirroredDirectory = fallow_types::duplicates::MirroredDirectory;
pub type EditorRefactoringKind = fallow_types::duplicates::RefactoringKind;
pub type EditorRefactoringSuggestion = fallow_types::duplicates::RefactoringSuggestion;

/// Report-scoped clone fingerprint assignment for editor-facing duplication output.
#[derive(Debug, Clone)]
pub struct EditorCloneFingerprintSet {
    inner: fallow_engine::CloneFingerprintSet,
}

impl EditorCloneFingerprintSet {
    /// Assign collision-free fingerprints for clone groups in one report.
    #[must_use]
    pub fn from_groups(groups: &[EditorCloneGroup]) -> Self {
        Self {
            inner: fallow_engine::CloneFingerprintSet::from_groups(groups),
        }
    }

    /// Return the assigned fingerprint for a clone group.
    #[must_use]
    pub fn fingerprint_for_group(&self, group: &EditorCloneGroup) -> String {
        self.inner.fingerprint_for_group(group)
    }

    /// Return the assigned fingerprint for clone-group parts.
    #[must_use]
    pub fn fingerprint_for_parts(
        &self,
        instances: &[EditorCloneInstance],
        token_count: usize,
        line_count: usize,
    ) -> String {
        self.inner
            .fingerprint_for_parts(instances, token_count, line_count)
    }

    /// Find the group addressed by an assigned fingerprint.
    #[must_use]
    pub fn find_group<'a>(
        &self,
        groups: &'a [EditorCloneGroup],
        fingerprint: &str,
    ) -> Option<&'a EditorCloneGroup> {
        self.inner.find_group(groups, fingerprint)
    }
}

pub mod editor_duplicates {
    pub use crate::editor::{
        EditorCloneFamily as CloneFamily, EditorCloneFingerprintSet as CloneFingerprintSet,
        EditorCloneGroup as CloneGroup, EditorCloneInstance as CloneInstance,
        EditorDuplicationReport as DuplicationReport, EditorDuplicationStats as DuplicationStats,
        EditorMirroredDirectory as MirroredDirectory, EditorRefactoringKind as RefactoringKind,
        EditorRefactoringSuggestion as RefactoringSuggestion,
    };
}

/// Classification of a changed-file git failure for editor integrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedFilesError {
    /// Git ref failed validation before invoking `git`.
    InvalidRef(String),
    /// `git` binary not found or not executable.
    GitMissing(String),
    /// Command ran but the directory is not a git repository.
    NotARepository,
    /// Command ran but the ref is invalid or another git error occurred.
    GitFailed(String),
}

impl ChangedFilesError {
    /// Human-readable clause suitable for embedding in an error message.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::InvalidRef(err) => format!("invalid git ref: {err}"),
            Self::GitMissing(err) => format!("failed to run git: {err}"),
            Self::NotARepository => "not a git repository".to_owned(),
            Self::GitFailed(stderr) => {
                let lower = stderr.to_ascii_lowercase();
                if lower.contains("not a valid object name")
                    || lower.contains("unknown revision")
                    || lower.contains("ambiguous argument")
                {
                    format!(
                        "{stderr} (shallow clone? try `git fetch --unshallow`, or set `fetch-depth: 0` on actions/checkout / `GIT_DEPTH: 0` in GitLab CI)"
                    )
                } else {
                    stderr.clone()
                }
            }
        }
    }
}

impl From<fallow_engine::ChangedFilesError> for ChangedFilesError {
    fn from(error: fallow_engine::ChangedFilesError) -> Self {
        match error {
            fallow_engine::ChangedFilesError::InvalidRef(err) => Self::InvalidRef(err),
            fallow_engine::ChangedFilesError::GitMissing(err) => Self::GitMissing(err),
            fallow_engine::ChangedFilesError::NotARepository => Self::NotARepository,
            fallow_engine::ChangedFilesError::GitFailed(stderr) => Self::GitFailed(stderr),
        }
    }
}

/// Resolve the canonical git toplevel for `cwd`.
///
/// # Errors
///
/// Returns an API-owned changed-file error when git cannot inspect the
/// repository.
pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    fallow_engine::resolve_git_toplevel(cwd).map_err(ChangedFilesError::from)
}

/// Get changed files and the git toplevel used to resolve them.
///
/// # Errors
///
/// Returns an API-owned changed-file error when git cannot resolve the ref or
/// repository state.
pub fn try_get_changed_files_with_toplevel(
    cwd: &Path,
    toplevel: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    fallow_engine::try_get_changed_files_with_toplevel(cwd, toplevel, git_ref)
        .map_err(ChangedFilesError::from)
}

pub mod editor_extract {
    pub use fallow_types::extract::{
        AngularComponentSelector, AngularInputMember, AngularOutputMember,
        AngularTemplateMemberAccessFact, AngularThisSpreadFact, CalleeUse, ClassHeritageInfo,
        ComplexityContribution, ComplexityContributionKind, ComplexityMetric, ComponentEmit,
        ComponentFunction, ComponentFunctionKind, ComponentProp, CssAnalytics, CssDeclarationBlock,
        CssRuleMetric, DiFramework, DiKeySite, DiRole, DispatchedEvent,
        DynamicCustomElementRenderFact, DynamicImportInfo, DynamicImportPattern, ExportInfo,
        ExportName, FactoryCallMemberAccessFact, FactoryFnMemberAccessFact, FactoryReturnExport,
        FlagUse, FlagUseKind, FluentChainMemberAccessFact, FluentChainNewMemberAccessFact,
        ForwardAttr, FunctionComplexity, HookUse, HookUseKind, ImportInfo, ImportedName,
        InstanceExportBindingFact, LoadReturnKey, LocalTypeDeclaration, MemberAccess, MemberInfo,
        MemberKind, MisplacedDirectiveSite, ModuleInfo, NamespaceObjectAlias, PUBLIC_ENV_EXACT,
        PUBLIC_ENV_METADATA_TOKENS, PUBLIC_ENV_PREFIXES, ParseResult, PlaywrightFixtureAliasFact,
        PlaywrightFixtureDefinitionFact, PlaywrightFixtureTypeFact, PlaywrightFixtureUseFact,
        PublicSignatureTypeReference, ReExportInfo, RegisteredCustomElement, RenderEdge,
        RequireCallInfo, SECRET_ENV_TOKENS, SanitizedSinkArg, SanitizerScope, SecurityControlKind,
        SecurityControlSite, SecurityUrlShape, SemanticFact, SemanticFactView, SinkArgKind,
        SinkLiteralValue, SinkObjectProperty, SinkShape, SinkSite,
        SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason,
        SkippedSecurityCalleeSite, TaintedBinding, VisibilityTag,
    };
}

pub mod editor_results {
    pub use fallow_types::output_dead_code::{
        BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
        CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
        DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
        MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
        MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
        PropDrillingChainFinding, ReExportCycleFinding, RouteCollisionFinding,
        TestOnlyDependencyFinding, ThinWrapperFinding, TypeOnlyDependencyFinding,
        UnlistedDependencyFinding, UnprovidedInjectFinding, UnrenderedComponentFinding,
        UnresolvedCatalogReferenceFinding, UnresolvedImportFinding, UnusedCatalogEntryFinding,
        UnusedClassMemberFinding, UnusedComponentEmitFinding, UnusedComponentInputFinding,
        UnusedComponentOutputFinding, UnusedComponentPropFinding, UnusedDependencyFinding,
        UnusedDependencyOverrideFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding,
        UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
        UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
        UnusedSvelteEventFinding, UnusedTypeFinding,
    };
    pub use fallow_types::results::{
        ActiveSuppression, AnalysisResults, BoundaryCallViolation, BoundaryCoverageViolation,
        BoundaryViolation, CircularDependency, CircularDependencyEdge, DependencyLocation,
        DependencyOverrideMisconfigReason, DependencyOverrideSource, DuplicateExport,
        DuplicateLocation, DuplicatePropShape, DuplicatePropShapeMember,
        DynamicSegmentNameConflict, EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag,
        FlagConfidence, FlagKind, ImportSite, InvalidClientExport, MisconfiguredDependencyOverride,
        MisplacedDirective, MixedClientServerBarrel, PolicyRuleKind, PolicyViolation,
        PolicyViolationSeverity, PrivateTypeLeak, PropDrillHop, PropDrillingChain, ReExportCycle,
        ReExportCycleKind, ReactComponentIntel, ReactHookSummary, ReactPropDrill, ReactPropIntel,
        ReferenceLocation, RenderFanInComponent, RenderFanInMetric, RouteCollision,
        SecurityAttackSurfaceEntry, SecurityCandidate, SecurityCandidateBoundary,
        SecurityCandidateSink, SecurityDeadCodeContext, SecurityDeadCodeKind,
        SecurityDefensiveBoundary, SecurityDefensiveControl, SecurityFinding, SecurityFindingKind,
        SecurityNetworkContext, SecurityReachability, SecurityRuntimeContext, SecurityRuntimeState,
        SecuritySeverity, SecurityTaintFlow, SecurityUnresolvedCalleeDiagnostic,
        SecurityZoneCrossing, StaleSuppression, SuppressionOrigin, TaintConfidence, TaintEndpoint,
        TaintPath, TestOnlyDependency, ThinWrapper, TraceHop, TraceHopRole, TypeOnlyDependency,
        UnlistedDependency, UnprovidedInject, UnrenderedComponent, UnresolvedCatalogReference,
        UnresolvedImport, UnusedCatalogEntry, UnusedComponentEmit, UnusedComponentInput,
        UnusedComponentOutput, UnusedComponentProp, UnusedDependency, UnusedDependencyOverride,
        UnusedExport, UnusedFile, UnusedLoadDataKey, UnusedMember, UnusedServerAction,
        UnusedSvelteEvent,
    };
}

pub mod editor_security {
    /// Return the human-readable security catalogue title for a finding kind.
    #[must_use]
    pub fn security_catalogue_title(kind: &str) -> Option<&'static str> {
        fallow_engine::security_catalogue_title(kind)
    }
}

pub mod editor_suppress {
    pub use fallow_types::suppress::{IssueKind, is_suppressed};
}

pub type EditorAnalysisResults = fallow_types::results::AnalysisResults;

/// Dead-code output retained for editor integrations.
///
/// The engine produces the data, but the editor API owns this public contract
/// so LSP and future editor adapters do not depend on engine result structs.
#[derive(Debug)]
pub struct EditorDeadCodeAnalysisOutput {
    pub results: EditorAnalysisResults,
    pub modules: Option<Vec<ModuleInfo>>,
    pub files: Option<Vec<DiscoveredFile>>,
}

impl EditorDeadCodeAnalysisOutput {
    fn from_engine(output: fallow_engine::DeadCodeAnalysisOutput) -> Self {
        Self {
            results: output.results,
            modules: output.modules,
            files: output.files,
        }
    }
}

/// Editor-facing inline complexity signal for code lens and similar surfaces.
///
/// The finding is derived from retained typed engine parse artifacts, but the
/// editor API owns the stable shape so LSP and future editor adapters do not
/// need to inspect raw modules directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorInlineComplexityFinding {
    pub path: PathBuf,
    pub name: String,
    pub line: u32,
    pub col: u32,
    pub cyclomatic: u16,
    pub cognitive: u16,
    pub exceeded: EditorInlineComplexityExceeded,
}

/// Which health complexity threshold(s) a function exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorInlineComplexityExceeded {
    Cyclomatic,
    Cognitive,
    CyclomaticAndCognitive,
}

/// Collect inline complexity findings from retained editor analysis artifacts.
#[must_use]
pub fn collect_inline_complexity(
    config: &fallow_config::ResolvedConfig,
    output: &EditorDeadCodeAnalysisOutput,
) -> Vec<EditorInlineComplexityFinding> {
    let Some(modules) = output.modules.as_ref() else {
        return Vec::new();
    };
    let Some(files) = output.files.as_ref() else {
        return Vec::new();
    };

    let file_paths: rustc_hash::FxHashMap<_, _> =
        files.iter().map(|file| (file.id, &file.path)).collect();
    let ignore_set = build_health_ignore_set(&config.health.ignore);
    let mut findings = Vec::new();

    for module in modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set
            .as_ref()
            .is_some_and(|set| set.is_match(relative))
        {
            continue;
        }

        for function in &module.complexity {
            if fallow_types::suppress::is_suppressed(
                &module.suppressions,
                function.line,
                fallow_types::suppress::IssueKind::Complexity,
            ) {
                continue;
            }

            let exceeds_cyclomatic = function.cyclomatic > config.health.max_cyclomatic;
            let exceeds_cognitive = function.cognitive > config.health.max_cognitive;
            let exceeded = match (exceeds_cyclomatic, exceeds_cognitive) {
                (true, true) => EditorInlineComplexityExceeded::CyclomaticAndCognitive,
                (true, false) => EditorInlineComplexityExceeded::Cyclomatic,
                (false, true) => EditorInlineComplexityExceeded::Cognitive,
                (false, false) => continue,
            };

            findings.push(EditorInlineComplexityFinding {
                path: (*path).clone(),
                name: function.name.clone(),
                line: function.line,
                col: function.col,
                cyclomatic: function.cyclomatic,
                cognitive: function.cognitive,
                exceeded,
            });
        }
    }

    findings
}

/// Filter inline complexity findings to the changed-file set.
#[allow(
    clippy::implicit_hasher,
    reason = "editor analysis changed-file sets use the workspace FxHashSet convention"
)]
pub fn filter_inline_complexity_by_changed_files(
    findings: &mut Vec<EditorInlineComplexityFinding>,
    changed_files: &FxHashSet<PathBuf>,
) {
    findings.retain(|finding| changed_files.contains(&finding.path));
}

fn build_health_ignore_set(patterns: &[String]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }

    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let Ok(glob) = globset::Glob::new(pattern) else {
            continue;
        };
        builder.add(glob);
    }
    builder.build().ok()
}

/// Reusable editor analysis session owned by the API boundary.
#[derive(Debug)]
pub struct EditorAnalysisSession {
    inner: fallow_engine::AnalysisSession,
}

impl EditorAnalysisSession {
    /// Load config and discover files for an editor project root.
    ///
    /// # Errors
    ///
    /// Returns an engine error when project config loading fails.
    pub fn load(root: &Path, config_path: Option<&Path>) -> fallow_engine::EngineResult<Self> {
        fallow_engine::AnalysisSession::load(root, config_path).map(Self::from_engine)
    }

    /// Load config, apply one editor-specific adjustment, then discover files.
    ///
    /// # Errors
    ///
    /// Returns an engine error when project config loading fails.
    pub fn load_with_config(
        root: &Path,
        config_path: Option<&Path>,
        configure: impl FnOnce(&mut fallow_config::ResolvedConfig),
    ) -> fallow_engine::EngineResult<Self> {
        fallow_engine::AnalysisSession::load_with_config(root, config_path, configure)
            .map(Self::from_engine)
    }

    /// Build a session from built-in defaults, ignoring project config files.
    #[must_use]
    pub fn load_default(root: &Path) -> Self {
        Self::from_engine(fallow_engine::AnalysisSession::load_default(root))
    }

    /// Resolved project config.
    #[must_use]
    pub fn config(&self) -> &fallow_config::ResolvedConfig {
        self.inner.config()
    }

    /// Config file path when one was loaded.
    #[must_use]
    pub fn config_path(&self) -> Option<&Path> {
        self.inner.config_path()
    }

    /// Run dead-code and duplication analysis for this editor session.
    ///
    /// # Errors
    ///
    /// Returns an engine error when dead-code parsing or analysis fails.
    pub fn analyze_project_with(
        &self,
        duplicates_config: &fallow_config::DuplicatesConfig,
        retain_complexity_artifacts: bool,
    ) -> fallow_engine::EngineResult<EditorProjectAnalysisOutput> {
        self.inner
            .analyze_project_with(duplicates_config, retain_complexity_artifacts)
            .map(EditorProjectAnalysisOutput::from_engine)
    }

    const fn from_engine(inner: fallow_engine::AnalysisSession) -> Self {
        Self { inner }
    }
}

/// Dead-code and duplication project output owned by the editor API boundary.
#[derive(Debug)]
pub struct EditorProjectAnalysisOutput {
    pub dead_code: EditorDeadCodeAnalysisOutput,
    pub duplication: EditorDuplicationReport,
}

impl EditorProjectAnalysisOutput {
    fn from_engine(output: fallow_engine::ProjectAnalysisOutput) -> Self {
        Self {
            dead_code: EditorDeadCodeAnalysisOutput::from_engine(output.dead_code),
            duplication: output.duplication,
        }
    }
}

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
        fallow_engine::filter_results_by_changed_files(&mut self.results, changed_files);
        fallow_engine::filter_duplication_by_changed_files(
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

    use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationStats};

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

    #[test]
    fn editor_session_returns_api_owned_project_output() {
        let temp = tempfile::tempdir().expect("temp project");
        let root = temp.path();
        std::fs::create_dir_all(root.join("src")).expect("src dir");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"editor-api-session","main":"src/index.ts"}"#,
        )
        .expect("package.json");
        std::fs::write(
            root.join("src/index.ts"),
            "export const used = 1;\nconsole.log(used);\n",
        )
        .expect("source");

        let session = EditorAnalysisSession::load(root, None).expect("session loads");
        let output = session
            .analyze_project_with(&fallow_config::DuplicatesConfig::default(), true)
            .expect("analysis runs");

        assert!(output.dead_code.modules.is_some());
        assert!(
            output
                .dead_code
                .files
                .as_ref()
                .is_some_and(|files| !files.is_empty())
        );
    }

    #[test]
    fn build_health_ignore_set_returns_none_for_empty_patterns() {
        assert!(
            build_health_ignore_set(&[]).is_none(),
            "empty ignore pattern list should avoid building a matcher"
        );
    }

    #[test]
    fn build_health_ignore_set_matches_glob_patterns() {
        let set =
            build_health_ignore_set(&["**/*.test.ts".to_string(), "src/generated/**".to_string()])
                .expect("valid patterns build a glob set");

        assert!(set.is_match(Path::new("src/foo.test.ts")));
        assert!(set.is_match(Path::new("src/generated/client.ts")));
        assert!(!set.is_match(Path::new("src/app.ts")));
    }

    #[test]
    fn build_health_ignore_set_skips_invalid_patterns() {
        let result = build_health_ignore_set(&["[invalid-glob".to_string()]);

        match result {
            None => {}
            Some(set) => assert!(
                !set.is_match(Path::new("any/path.ts")),
                "set built from only invalid patterns must not match anything"
            ),
        }
    }

    fn make_inline_finding(path: PathBuf) -> EditorInlineComplexityFinding {
        EditorInlineComplexityFinding {
            path,
            name: "myFn".to_string(),
            line: 1,
            col: 0,
            cyclomatic: 5,
            cognitive: 4,
            exceeded: EditorInlineComplexityExceeded::Cyclomatic,
        }
    }

    #[test]
    fn filter_inline_complexity_keeps_findings_in_changed_set() {
        let changed: FxHashSet<PathBuf> = [PathBuf::from("/src/a.ts"), PathBuf::from("/src/b.ts")]
            .into_iter()
            .collect();
        let mut findings = vec![
            make_inline_finding(PathBuf::from("/src/a.ts")),
            make_inline_finding(PathBuf::from("/src/c.ts")),
        ];

        filter_inline_complexity_by_changed_files(&mut findings, &changed);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].path.to_string_lossy().replace('\\', "/"),
            "/src/a.ts"
        );
    }

    #[test]
    fn filter_inline_complexity_removes_all_when_changed_set_empty() {
        let changed: FxHashSet<PathBuf> = FxHashSet::default();
        let mut findings = vec![make_inline_finding(PathBuf::from("/src/a.ts"))];

        filter_inline_complexity_by_changed_files(&mut findings, &changed);

        assert!(
            findings.is_empty(),
            "empty changed-files set must drop all inline complexity findings"
        );
    }

    #[test]
    fn filter_inline_complexity_keeps_all_when_all_in_changed_set() {
        let path_a = PathBuf::from("/src/a.ts");
        let path_b = PathBuf::from("/src/b.ts");
        let changed: FxHashSet<PathBuf> = [path_a.clone(), path_b.clone()].into_iter().collect();
        let mut findings = vec![make_inline_finding(path_a), make_inline_finding(path_b)];

        filter_inline_complexity_by_changed_files(&mut findings, &changed);

        assert_eq!(
            findings.len(),
            2,
            "all findings in the changed set must be retained"
        );
    }
}
