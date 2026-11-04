//! Editor-facing analysis contracts shared by LSP and future editor adapters.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_types::{discover::DiscoveredFile, extract::ModuleInfo};

use crate::{ChangeScope, ChangeScopeRequest, PackageBaselineError};

/// Editor-boundary alias for the clone-family payload.
pub type EditorCloneFamily = fallow_types::duplicates::CloneFamily;
/// Editor-boundary alias for the clone-group payload.
pub type EditorCloneGroup = fallow_types::duplicates::CloneGroup;
/// Editor-boundary alias for one clone occurrence within a group.
pub type EditorCloneInstance = fallow_types::duplicates::CloneInstance;
/// Editor-boundary alias for the full duplication report.
pub type EditorDuplicationReport = fallow_types::duplicates::DuplicationReport;
/// Editor-boundary alias for aggregate duplication statistics.
pub type EditorDuplicationStats = fallow_types::duplicates::DuplicationStats;
/// Editor-boundary alias for a mirrored-directory finding.
pub type EditorMirroredDirectory = fallow_types::duplicates::MirroredDirectory;
/// Editor-boundary alias for the refactoring-suggestion kind.
pub type EditorRefactoringKind = fallow_types::duplicates::RefactoringKind;
/// Editor-boundary alias for a refactoring suggestion.
pub type EditorRefactoringSuggestion = fallow_types::duplicates::RefactoringSuggestion;

/// Report-scoped clone fingerprint assignment for editor-facing duplication output.
#[derive(Debug, Clone)]
pub struct EditorCloneFingerprintSet {
    inner: fallow_engine::duplicates::CloneFingerprintSet,
}

impl EditorCloneFingerprintSet {
    /// Assign collision-free fingerprints for clone groups in one report.
    #[must_use]
    pub fn from_groups(groups: &[EditorCloneGroup]) -> Self {
        Self {
            inner: fallow_engine::duplicates::CloneFingerprintSet::from_groups(groups),
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

/// Duplication contracts re-exported under their unprefixed names so editor
/// adapters can import them as a module namespace.
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

impl From<fallow_engine::changed_files::ChangedFilesError> for ChangedFilesError {
    fn from(error: fallow_engine::changed_files::ChangedFilesError) -> Self {
        match error {
            fallow_engine::changed_files::ChangedFilesError::InvalidRef(err) => {
                Self::InvalidRef(err)
            }
            fallow_engine::changed_files::ChangedFilesError::GitMissing(err) => {
                Self::GitMissing(err)
            }
            fallow_engine::changed_files::ChangedFilesError::NotARepository => Self::NotARepository,
            fallow_engine::changed_files::ChangedFilesError::GitFailed(stderr) => {
                Self::GitFailed(stderr)
            }
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
    fallow_engine::changed_files::resolve_git_toplevel(cwd).map_err(ChangedFilesError::from)
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
    fallow_engine::changed_files::try_get_changed_files_with_toplevel(cwd, toplevel, git_ref)
        .map_err(ChangedFilesError::from)
}

/// Per-module extraction facts re-exported for editor adapters that inspect
/// retained parse artifacts.
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

/// Typed analysis-result and finding contracts re-exported for editor
/// adapters.
pub mod editor_results {
    pub use fallow_types::output_dead_code::DuplicateExportAliasFinding;
    pub use fallow_types::output_dead_code::{
        AbsentComponentPropFinding, BoundaryCallViolationFinding, BoundaryCoverageViolationFinding,
        BoundaryViolationFinding, CircularDependencyFinding, DeprecatedExportInUseFinding,
        DevDependencyInProductionFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
        DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
        MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
        MixedClientServerBarrelFinding, PackageCycleFinding, PolicyViolationFinding,
        PrivateTypeLeakFinding, PropDrillingChainFinding, ReExportCycleFinding,
        RouteCollisionFinding, TestOnlyDependencyFinding, ThinWrapperFinding,
        TypeOnlyDependencyFinding, UnlistedDependencyFinding, UnprovidedInjectFinding,
        UnrenderedComponentFinding, UnresolvedCatalogReferenceFinding, UnresolvedImportFinding,
        UnusedCatalogEntryFinding, UnusedClassMemberFinding, UnusedComponentEmitFinding,
        UnusedComponentInputFinding, UnusedComponentOutputFinding, UnusedComponentPropFinding,
        UnusedDependencyFinding, UnusedDependencyOverrideFinding, UnusedDevDependencyFinding,
        UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
        UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
        UnusedSvelteEventFinding, UnusedTypeFinding,
    };
    pub use fallow_types::output_dead_code::{
        UnproducedCatchTagFinding, UnusedEffectLayerFinding, UnusedTaggedErrorFinding,
    };
    pub use fallow_types::results::{
        AbsentComponentProp, ActiveSuppression, AnalysisResults, BoundaryCallViolation,
        BoundaryCoverageViolation, BoundaryViolation, CircularDependency, CircularDependencyEdge,
        ComponentPropCallSite, DependencyLocation, DependencyOverrideMisconfigReason,
        DependencyOverrideSource, DeprecatedConsumerKind, DeprecatedExportConsumer,
        DeprecatedExportInUse, DevDependencyInProduction, DuplicateExport, DuplicateLocation,
        DuplicatePropShape, DuplicatePropShapeMember, DynamicSegmentNameConflict,
        EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag, FlagConfidence, FlagKind,
        ImportSite, InvalidClientExport, MisconfiguredDependencyOverride, MisplacedDirective,
        MixedClientServerBarrel, PackageCycle, PackageCycleEdge, PolicyRuleKind, PolicyViolation,
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
    pub use fallow_types::results::{DuplicateExportAlias, DuplicateExportAliasLocation};
    pub use fallow_types::results::{
        EffectCatchSite, EffectLayerKind, UnproducedCatchTag, UnusedEffectLayer, UnusedTaggedError,
    };
}

/// Security catalogue lookups exposed at the editor boundary.
pub mod editor_security {
    /// Return the human-readable security catalogue title for a finding kind.
    #[must_use]
    pub fn security_catalogue_title(kind: &str) -> Option<&'static str> {
        fallow_engine::dead_code::security_catalogue_title(kind)
    }
}

/// Inline-suppression contracts re-exported for editor adapters.
pub mod editor_suppress {
    pub use fallow_types::suppress::{IssueKind, is_suppressed};
}

/// Editor-boundary alias for the typed dead-code analysis results.
pub type EditorAnalysisResults = fallow_types::results::AnalysisResults;

/// Dead-code output retained for editor integrations.
///
/// The engine produces the data, but the editor API owns this public contract
/// so LSP and future editor adapters do not depend on engine result structs.
#[derive(Debug)]
pub struct EditorDeadCodeAnalysisOutput {
    /// Typed dead-code findings from the analysis.
    pub results: EditorAnalysisResults,
    /// Retained per-module parse artifacts; `None` unless the run was asked
    /// to keep them for follow-up editor features.
    pub modules: Option<Vec<ModuleInfo>>,
    /// Retained discovered-file records matching `modules`.
    pub files: Option<Vec<DiscoveredFile>>,
}

impl EditorDeadCodeAnalysisOutput {
    fn from_engine(output: fallow_engine::dead_code::DeadCodeAnalysisOutput) -> Self {
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
    /// Absolute path of the file declaring the function.
    pub path: PathBuf,
    /// Function name as extracted from the source.
    pub name: String,
    /// One-based line of the function declaration.
    pub line: u32,
    /// Zero-based column of the function declaration.
    pub col: u32,
    /// Measured cyclomatic complexity.
    pub cyclomatic: u16,
    /// Measured cognitive complexity.
    pub cognitive: u16,
    /// Which configured threshold(s) the function exceeded.
    pub exceeded: EditorInlineComplexityExceeded,
}

/// Which health complexity threshold(s) a function exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorInlineComplexityExceeded {
    /// Only the cyclomatic threshold was exceeded.
    Cyclomatic,
    /// Only the cognitive threshold was exceeded.
    Cognitive,
    /// Both thresholds were exceeded.
    CyclomaticAndCognitive,
}

/// Collect inline complexity findings from retained editor analysis artifacts.
///
/// The rule is `fallow_engine::health::inline_complexity`, the one that the
/// health findings use, so the code lens and `fallow health` flag the same
/// functions, also under `health.thresholdOverrides`.
#[must_use]
pub fn collect_inline_complexity(
    config: &fallow_config::ResolvedConfig,
    output: &EditorDeadCodeAnalysisOutput,
) -> Vec<EditorInlineComplexityFinding> {
    let (Some(modules), Some(files)) = (output.modules.as_ref(), output.files.as_ref()) else {
        return Vec::new();
    };
    fallow_engine::health::inline_complexity(config, modules, files)
        .into_iter()
        .map(|finding| EditorInlineComplexityFinding {
            exceeded: match (finding.exceeds_cyclomatic, finding.exceeds_cognitive) {
                (true, true) => EditorInlineComplexityExceeded::CyclomaticAndCognitive,
                (true, false) => EditorInlineComplexityExceeded::Cyclomatic,
                (false, _) => EditorInlineComplexityExceeded::Cognitive,
            },
            path: finding.path,
            name: finding.name,
            line: finding.line,
            col: finding.col,
            cyclomatic: finding.cyclomatic,
            cognitive: finding.cognitive,
        })
        .collect()
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

/// Retain the inline complexity findings in a resolved change scope.
pub fn filter_inline_complexity_by_change_scope(
    findings: &mut Vec<EditorInlineComplexityFinding>,
    scope: &ChangeScope,
) {
    findings.retain(|finding| scope.contains(&finding.path));
}

/// The parse work of an editor session. See
/// [`fallow_engine::session::SessionParseCounts`].
pub type EditorSessionParseCounts = fallow_engine::session::SessionParseCounts;

/// Reusable editor analysis session owned by the API boundary.
#[derive(Debug)]
pub struct EditorAnalysisSession {
    inner: fallow_engine::session::AnalysisSession,
}

impl EditorAnalysisSession {
    /// Load config and discover files for an editor project root.
    ///
    /// # Errors
    ///
    /// Returns an engine error when project config loading fails.
    pub fn load(root: &Path, config_path: Option<&Path>) -> fallow_engine::EngineResult<Self> {
        fallow_engine::session::AnalysisSession::load(root, config_path).map(Self::from_engine)
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
        fallow_engine::session::AnalysisSession::load_with_config(root, config_path, configure)
            .map(Self::from_engine)
    }

    /// Load config with an explicit inheritance trust policy, apply one
    /// editor-specific adjustment, then discover files.
    ///
    /// # Errors
    ///
    /// Returns an engine error when project config loading fails.
    pub fn load_with_config_options(
        root: &Path,
        config_path: Option<&Path>,
        load_options: fallow_config::ConfigLoadOptions,
        configure: impl FnOnce(&mut fallow_config::ResolvedConfig),
    ) -> fallow_engine::EngineResult<Self> {
        fallow_engine::session::AnalysisSession::load_with_config_options(
            root,
            config_path,
            load_options,
            configure,
        )
        .map(Self::from_engine)
    }

    /// Build a session from built-in defaults, ignoring project config files.
    #[must_use]
    pub fn load_default(root: &Path) -> Self {
        Self::from_engine(fallow_engine::session::AnalysisSession::load_default(root))
    }

    /// Build a session from built-in defaults, apply one editor-specific
    /// adjustment, then discover files.
    #[must_use]
    pub fn load_default_with_config(
        root: &Path,
        configure: impl FnOnce(&mut fallow_config::ResolvedConfig),
    ) -> Self {
        Self::from_engine(
            fallow_engine::session::AnalysisSession::load_default_with_config(root, configure),
        )
    }

    /// Attach a caller-owned cancellation token to this session.
    ///
    /// The analyses of the session check the token at each pipeline stage
    /// boundary and in the per-file parse loop. Once the token is set, they
    /// return an engine error whose `is_cancelled()` is true, never a partial
    /// result. See
    /// [`fallow_engine::session::AnalysisSession::with_cancellation`] for the
    /// limits of the cooperative stop.
    #[must_use]
    pub fn with_cancellation(
        self,
        cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self::from_engine(self.inner.with_cancellation(cancellation))
    }

    /// Replace the cancellation token of a session that serves several runs.
    pub fn set_cancellation(
        &mut self,
        cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        self.inner.set_cancellation(cancellation);
    }

    /// Walk the project again before a run of a long-lived session. The
    /// parsed modules stay when the file set did not change. Returns whether
    /// the file set changed. See
    /// [`fallow_engine::session::AnalysisSession::refresh_discovery`].
    pub fn refresh_discovery(&mut self) -> bool {
        self.inner.refresh_discovery()
    }

    /// The plugin files, rule packs and `autoDiscover` directories that
    /// config resolution read for this session. A long-lived caller compares
    /// two snapshots of them to see that the config is out of date.
    #[must_use]
    pub fn config_inputs(&self) -> &fallow_config::ConfigInputs {
        self.inner.config_inputs()
    }

    /// The content of [`Self::config_inputs`] just before config resolution
    /// read them.
    #[must_use]
    pub fn config_inputs_before_resolve(&self) -> &fallow_config::ConfigInputsSnapshot {
        self.inner.config_inputs_before_resolve()
    }

    /// The estimated heap memory of the parsed modules that the session
    /// keeps between runs. Zero before the first parse.
    #[must_use]
    pub fn retained_bytes_estimate(&self) -> u64 {
        self.inner.retained_bytes_estimate()
    }

    /// Write the modules of incremental parses to the persisted parse cache.
    /// Call it before the session is dropped.
    pub fn flush_parse_cache(&self) {
        self.inner.flush_parse_cache();
    }

    /// Parse the project files into the session without analysis, so the
    /// first run starts from warm modules.
    ///
    /// # Errors
    ///
    /// Returns a cancelled engine error when the token of the session is set.
    pub fn prewarm(&self, retain_complexity_artifacts: bool) -> fallow_engine::EngineResult<()> {
        self.inner
            .prewarm_parsed_modules(retain_complexity_artifacts)
    }

    /// The parse work of this session since it was created.
    #[must_use]
    pub fn parse_counts(&self) -> EditorSessionParseCounts {
        self.inner.parse_counts()
    }

    /// Resolved project config.
    #[must_use]
    pub fn config(&self) -> &fallow_config::ResolvedConfig {
        self.inner.config()
    }

    /// Resolve the change scope of this project with the engine rule that
    /// the CLI and the programmatic API use.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the package map names an invalid or unknown
    /// workspace root, or a Git ref that does not resolve.
    pub fn change_scope(
        &self,
        request: ChangeScopeRequest<'_>,
    ) -> Result<ChangeScope, PackageBaselineError> {
        ChangeScope::resolve(request, self.inner.config(), self.inner.workspaces())
    }

    /// `workspace_diagnostics[]` entries for the config patterns
    /// (`ignoreFindings`, `ignoreDependencies`) that matched nothing in the
    /// latest analysis of this session. The CLI and the programmatic API
    /// build the same entries with the same engine function.
    #[must_use]
    pub fn unmatched_config_patterns(&self) -> Vec<fallow_config::WorkspaceDiagnostic> {
        fallow_engine::dead_code::config_pattern_diagnostics(self.inner.config(), true)
    }

    /// Config file path when one was loaded.
    #[must_use]
    pub fn config_path(&self) -> Option<&Path> {
        self.inner.config_path()
    }

    /// Refine this editor session's dead-code findings with exact TypeScript
    /// symbol evidence.
    ///
    /// # Errors
    ///
    /// Returns a programmatic error when the semantic companion cannot provide
    /// the requested analysis contract.
    pub fn refine_type_aware_dead_code(
        &self,
        options: &crate::TypeAwareOptions,
        filters: &crate::DeadCodeFilters,
        output: &mut EditorDeadCodeAnalysisOutput,
    ) -> Result<Option<fallow_types::envelope::TypeAwareMeta>, crate::ProgrammaticError> {
        let meta = crate::type_aware::refine_programmatic_dead_code(
            options,
            filters,
            &self.inner,
            &mut output.results,
        )?;
        // Reconciliation can add findings, so rule severities are resolved
        // again over the refined set. The pass removes findings and writes
        // each gate severity again, so repeating it is idempotent.
        fallow_engine::dead_code::apply_rule_severities(&mut output.results, self.inner.config());
        Ok(meta)
    }

    /// Refine editor findings through a root-bound persistent semantic session.
    pub fn refine_type_aware_dead_code_in_session(
        &self,
        semantic_session: &mut crate::TypeAwareSession,
        changes: Option<&crate::TypeAwareFileChanges>,
        options: &crate::TypeAwareOptions,
        filters: &crate::DeadCodeFilters,
        output: &mut EditorDeadCodeAnalysisOutput,
    ) -> Result<Option<fallow_types::envelope::TypeAwareMeta>, crate::ProgrammaticError> {
        let meta = crate::type_aware::refine_programmatic_dead_code_in_session(
            semantic_session,
            changes,
            options,
            filters,
            &self.inner,
            &mut output.results,
        )?;
        fallow_engine::dead_code::apply_rule_severities(&mut output.results, self.inner.config());
        Ok(meta)
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
            .map(|output| self.with_resolved_rule_severities(output))
    }

    /// Run dead-code and duplication analysis, optionally focusing duplication
    /// to files the editor already resolved as changed.
    ///
    /// Dead-code retains full graph context and semantic evidence scope.
    /// After type-aware refinement, call [`Self::apply_change_scope`] to narrow
    /// the findings and clone groups of this project.
    ///
    /// # Errors
    ///
    /// Returns an engine error when dead-code parsing or analysis fails.
    pub fn analyze_project_with_changed_files(
        &self,
        duplicates_config: &fallow_config::DuplicatesConfig,
        retain_complexity_artifacts: bool,
        changed_files: Option<&FxHashSet<PathBuf>>,
    ) -> fallow_engine::EngineResult<EditorProjectAnalysisOutput> {
        self.inner
            .analyze_project_with_artifacts(
                duplicates_config,
                fallow_engine::project_analysis::ProjectAnalysisArtifactOptions {
                    retain_complexity_artifacts,
                    changed_files: changed_files.cloned(),
                    ..fallow_engine::project_analysis::ProjectAnalysisArtifactOptions::default()
                },
            )
            .map(fallow_engine::project_analysis::ProjectAnalysisArtifacts::into_output)
            .map(EditorProjectAnalysisOutput::from_engine)
            .map(|output| self.with_resolved_rule_severities(output))
    }

    /// Narrow the dead-code findings and clone groups of this project to a
    /// resolved change scope, with the engine functions that the CLI, MCP and
    /// Node API use.
    ///
    /// Apply scope after type-aware refinement so added findings are scoped
    /// too. Each project in a multi-root editor session has its own
    /// `ignoreFindings`, so apply scope per project before merging.
    pub fn apply_change_scope(
        &self,
        output: &mut EditorProjectAnalysisOutput,
        scope: &ChangeScope,
    ) {
        fallow_engine::dead_code::apply_scope(
            &mut output.dead_code.results,
            &fallow_engine::dead_code::DeadCodeScope {
                workspace_roots: None,
                changes: Some(scope),
                diff: None,
                files: None,
            },
            self.inner.config(),
        );
        fallow_engine::duplicates::apply_scope(
            &mut output.duplication,
            &fallow_engine::duplicates::DuplicationScope {
                changes: Some(scope),
                diff: None,
                workspace_roots: None,
            },
            &self.inner.config().root,
        );
    }

    /// Resolve configured rule severities, including per-path
    /// `overrides[].rules`, against a freshly analyzed project slice.
    ///
    /// Each project root is filtered with its own config before a multi-root
    /// editor session merges the outputs, so an override only ever applies to
    /// the project that declares it.
    fn with_resolved_rule_severities(
        &self,
        mut output: EditorProjectAnalysisOutput,
    ) -> EditorProjectAnalysisOutput {
        fallow_engine::dead_code::apply_rule_severities(
            &mut output.dead_code.results,
            self.inner.config(),
        );
        output
    }

    const fn from_engine(inner: fallow_engine::session::AnalysisSession) -> Self {
        Self { inner }
    }
}

/// Dead-code and duplication project output owned by the editor API boundary.
#[derive(Debug)]
pub struct EditorProjectAnalysisOutput {
    /// Dead-code findings plus optionally retained parse artifacts.
    pub dead_code: EditorDeadCodeAnalysisOutput,
    /// Duplication report for the analyzed project slice.
    pub duplication: EditorDuplicationReport,
}

impl EditorProjectAnalysisOutput {
    fn from_engine(output: fallow_engine::project_analysis::ProjectAnalysisOutput) -> Self {
        Self {
            dead_code: EditorDeadCodeAnalysisOutput::from_engine(output.dead_code),
            duplication: output.duplication,
        }
    }
}

/// Dead-code and duplication output shaped for editor integrations.
#[derive(Debug, Default)]
pub struct EditorAnalysisOutput {
    /// Typed dead-code findings.
    pub results: EditorAnalysisResults,
    /// Duplication report.
    pub duplication: EditorDuplicationReport,
}

impl EditorAnalysisOutput {
    /// Pair dead-code results with a duplication report.
    #[must_use]
    pub const fn new(results: EditorAnalysisResults, duplication: EditorDuplicationReport) -> Self {
        Self {
            results,
            duplication,
        }
    }

    /// Merge another project analysis output into this accumulated output.
    pub fn merge_project_output(&mut self, output: EditorProjectAnalysisOutput) {
        self.merge_results(output.dead_code.results);
        self.merge_duplication(output.duplication);
    }

    /// Merge another dead-code results set into this one.
    pub fn merge_results(&mut self, source: EditorAnalysisResults) {
        self.results.merge_into(source);
    }

    /// Merge another duplication report into this one, summing the aggregate
    /// stats and recomputing the duplication percentage over the union.
    pub fn merge_duplication(&mut self, source: EditorDuplicationReport) {
        self.duplication.clone_groups.extend(source.clone_groups);
        self.duplication
            .clone_families
            .extend(source.clone_families);
        self.duplication
            .mirrored_directories
            .extend(source.mirrored_directories);
        self.duplication.stats.clone_groups += source.stats.clone_groups;
        self.duplication.stats.clone_families += source.stats.clone_families;
        self.duplication.stats.clone_instances += source.stats.clone_instances;
        self.duplication.stats.total_files += source.stats.total_files;
        self.duplication.stats.files_with_clones += source.stats.files_with_clones;
        self.duplication.stats.total_lines += source.stats.total_lines;
        self.duplication.stats.duplicated_lines += source.stats.duplicated_lines;
        self.duplication.stats.total_tokens += source.stats.total_tokens;
        self.duplication.stats.duplicated_tokens += source.stats.duplicated_tokens;
        self.duplication.stats.clone_groups_below_min_occurrences +=
            source.stats.clone_groups_below_min_occurrences;
        self.duplication.stats.clone_groups_ignored += source.stats.clone_groups_ignored;
        self.duplication.stats.near_candidates_skipped += source.stats.near_candidates_skipped;
        self.duplication.stats.duplication_percentage = if self.duplication.stats.total_lines > 0 {
            (self.duplication.stats.duplicated_lines as f64
                / self.duplication.stats.total_lines as f64)
                * 100.0
        } else {
            0.0
        };
    }

    /// Drop findings and clone groups that do not touch any changed file.
    ///
    /// Each project scopes dead-code findings with its own config in
    /// [`EditorAnalysisSession::apply_change_scope`]. This merged-output filter
    /// narrows clone groups and preserves the already scoped findings.
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
}

#[cfg(test)]
#[path = "editor_tests.rs"]
pub(crate) mod tests;
