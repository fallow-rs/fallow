//! The classification methods of [`WorkspaceDiagnosticKind`]: the wire id,
//! the stderr warning, and the stage that records each kind.

use super::WorkspaceDiagnosticKind;

impl WorkspaceDiagnosticKind {
    /// Stable kebab-case identifier used in dedupe keys and tracing payloads.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::UndeclaredWorkspace => "undeclared-workspace",
            Self::MalformedPackageJson { .. } => "malformed-package-json",
            Self::GlobMatchedNoPackageJson { .. } => "glob-matched-no-package-json",
            Self::WorkspaceGlobOutsideRoot { .. } => "workspace-glob-outside-root",
            Self::WorkspacePatternDirMissing { .. } => "workspace-pattern-dir-missing",
            Self::NestedProjectRoot => "nested-project-root",
            Self::MalformedTsconfig { .. } => "malformed-tsconfig",
            Self::TsconfigReferenceDirMissing => "tsconfig-reference-dir-missing",
            Self::MalformedPnpmWorkspaceYaml { .. } => "malformed-pnpm-workspace-yaml",
            Self::SkippedLargeFile { .. } => "skipped-large-file",
            Self::SkippedMinifiedFile { .. } => "skipped-minified-file",
            Self::SkippedSourceDotdir => "skipped-source-dotdir",
            Self::SourceReadFailure { .. } => "source-read-failure",
            Self::SourceParseDegraded { .. } => "source-parse-degraded",
            Self::SourceNestingTooDeep { .. } => "source-nesting-too-deep",
            Self::BunLockbOverrideResolutionSkipped => "bun-lockb-override-resolution-skipped",
            Self::BunLockOverrideResolutionSkipped => "bun-lock-override-resolution-skipped",
            Self::PnpmLockOverrideResolutionSkipped => "pnpm-lock-override-resolution-skipped",
            Self::NpmLockOverrideResolutionSkipped => "npm-lock-override-resolution-skipped",
            Self::BunResolutionsShadowedByOverrides => "bun-resolutions-shadowed-by-overrides",
            Self::PnpmWorkspaceOverridesIgnored { .. } => "pnpm-workspace-overrides-ignored",
            Self::NodeModulesMissing => "node-modules-missing",
            Self::BoundariesNotConfigured => "boundaries-not-configured",
            Self::RulePacksNotConfigured => "rule-packs-not-configured",
            Self::ClientServerLeakNoClientRoots => "client-server-leak-no-client-roots",
            Self::ProductionExcludedFileImported { .. } => "production-excluded-file-imported",
            Self::ExcludedByDefaultIgnore { .. } => "excluded-by-default-ignore",
            Self::NoSourceFilesAnalyzed { .. } => "no-source-files-analyzed",
            Self::FileScoresUnavailable { .. } => "file-scores-unavailable",
            Self::HotspotsSkipped { .. } => "hotspots-skipped",
            Self::ChangeCouplingSkipped { .. } => "change-coupling-skipped",
            Self::ShallowClone { .. } => "shallow-clone",
            Self::UnpinnedClock => "unpinned-clock",
            Self::OwnershipUnavailable { .. } => "ownership-unavailable",
            Self::TrendSnapshotUnreadable { .. } => "trend-snapshot-unreadable",
            Self::TrendGroupBaselineUnavailable { .. } => "trend-group-baseline-unavailable",
            Self::PluginConfigUnreadable { .. } => "plugin-config-unreadable",
            Self::PluginEffectNotModeled { .. } => "plugin-effect-not-modeled",
            Self::CoverageAutoDetected => "coverage-auto-detected",
            Self::FlagAgeShallowClone => "flag-age-shallow-clone",
            Self::FlagAgeUnavailable { .. } => "flag-age-unavailable",
            Self::IgnoreDependenciesGlobUnmatched { .. } => "ignore-dependencies-glob-unmatched",
            Self::IgnoreFindingsPatternUnmatched { .. } => "ignore-findings-pattern-unmatched",
            Self::EntryPatternRedundant { .. } => "entry-pattern-redundant",
            Self::IgnoreDependenciesEntryRedundant { .. } => "ignore-dependencies-entry-redundant",
            Self::IgnorePatternsEntryRedundant { .. } => "ignore-patterns-entry-redundant",
            Self::CircularDependenciesAllowUnmatched { .. } => {
                "circular-dependencies-allow-unmatched"
            }
            Self::OverrideRuleIneffective { .. } => "override-rule-ineffective",
            Self::RequestedRuleOff { .. } => "requested-rule-off",
            Self::BaselineInstallStateMismatch { .. } => "baseline-install-state-mismatch",
            Self::InertFlag { .. } => "inert-flag",
            Self::ConfigLoadFellBack { .. } => "config-load-fell-back",
            Self::ConfigFileShadowed { .. } => "config-file-shadowed",
            Self::UnknownSecurityCategory { .. } => "unknown-security-category",
            Self::EnvValueIgnored { .. } => "env-value-ignored",
        }
    }

    /// Whether this diagnostic is worth a `tracing::warn!` line on stderr, on
    /// top of its permanent entry in `workspace_diagnostics[]`.
    ///
    /// A warning is for a run whose RESULTS are degraded: something the user
    /// installed, wrote, or expected did not reach the analysis. The two
    /// unconfigured-check kinds are not that. They fire in the product's
    /// default state, on every project that never opted into boundaries or
    /// rule packs, and they will keep firing forever, because the remedy they
    /// offer is to write configuration in order to silence a warning about not
    /// having written configuration. They stay in the structured array, where a
    /// consumer that wants to distinguish "measured zero" from "measured
    /// nothing" can read them, and off the stderr surface that every other
    /// command shares.
    ///
    /// `coverage-auto-detected` answers false for a third reason: it reports
    /// the provenance of an input that DID load, so a consumer sentence about a
    /// degraded run would state something untrue about it. Its own note is
    /// printed by the health pipeline.
    ///
    /// `plugin-effect-not-modeled` answers false for the first reason: the
    /// config was readable and nothing the run could have measured was lost,
    /// so it would warn forever on a project whose `nuxt.config` fallow does
    /// not model. Its sibling `plugin-config-unreadable` answers true, because
    /// there a declaration the user wrote did not reach the analysis and
    /// findings can be wrong in either direction.
    ///
    /// `unknown-security-category` answers false for the first reason too:
    /// the run ignores the unknown id and each analysis runs on its full
    /// input. The run carries the kind in the diagnostics of every analysis,
    /// so a true answer would mark a dead-code, duplication or health result
    /// as degraded by a security setting.
    #[must_use]
    pub const fn warns_on_stderr(&self) -> bool {
        match self {
            Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ClientServerLeakNoClientRoots
            | Self::ExcludedByDefaultIgnore { .. }
            | Self::PluginEffectNotModeled { .. }
            | Self::CoverageAutoDetected
            | Self::TrendGroupBaselineUnavailable { .. }
            | Self::ChangeCouplingSkipped { .. }
            | Self::IgnoreDependenciesGlobUnmatched { .. }
            | Self::IgnoreFindingsPatternUnmatched { .. }
            | Self::EntryPatternRedundant { .. }
            | Self::IgnoreDependenciesEntryRedundant { .. }
            | Self::IgnorePatternsEntryRedundant { .. }
            | Self::CircularDependenciesAllowUnmatched { .. }
            | Self::OverrideRuleIneffective { .. }
            | Self::RequestedRuleOff { .. }
            | Self::InertFlag { .. }
            | Self::UnknownSecurityCategory { .. }
            | Self::NestedProjectRoot => false,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::WorkspaceGlobOutsideRoot { .. }
            | Self::WorkspacePatternDirMissing { .. }
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::SourceNestingTooDeep { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::PnpmLockOverrideResolutionSkipped
            | Self::NpmLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::PnpmWorkspaceOverridesIgnored { .. }
            | Self::NodeModulesMissing
            | Self::NoSourceFilesAnalyzed { .. }
            | Self::FileScoresUnavailable { .. }
            | Self::HotspotsSkipped { .. }
            | Self::ShallowClone { .. }
            | Self::UnpinnedClock
            | Self::OwnershipUnavailable { .. }
            | Self::TrendSnapshotUnreadable { .. }
            | Self::PluginConfigUnreadable { .. }
            | Self::FlagAgeShallowClone
            | Self::FlagAgeUnavailable { .. }
            | Self::ProductionExcludedFileImported { .. }
            | Self::BaselineInstallStateMismatch { .. }
            | Self::ConfigLoadFellBack { .. }
            | Self::ConfigFileShadowed { .. }
            | Self::EnvValueIgnored { .. } => true,
        }
    }

    /// Whether this diagnostic is produced by SOURCE discovery (the file walk in
    /// `discover_files`) rather than WORKSPACE discovery (config load). Source-
    /// discovery diagnostics are APPENDED to the registry after config load, so
    /// `stash_workspace_diagnostics` must preserve them when it replaces the
    /// workspace-discovery set, otherwise the per-analysis config re-loads in
    /// combined-mode (`fallow` with no subcommand re-loads config for check,
    /// dupes, and health) wipe them before the JSON envelope is built (issue
    /// #1086).
    #[must_use]
    pub const fn is_source_discovery(&self) -> bool {
        matches!(
            self,
            Self::SkippedLargeFile { .. }
                | Self::SkippedMinifiedFile { .. }
                | Self::SkippedSourceDotdir
                | Self::SourceReadFailure { .. }
                | Self::SourceParseDegraded { .. }
                | Self::SourceNestingTooDeep { .. }
                | Self::NodeModulesMissing
                | Self::ExcludedByDefaultIgnore { .. }
                | Self::NoSourceFilesAnalyzed { .. }
        )
    }

    /// Whether this diagnostic is written by the source file WALK
    /// (`discover_files`), the subset of [`Self::is_source_discovery`] that a
    /// walk replaces wholesale for its root. `source-read-failure` is the
    /// other source-discovery kind and is NOT one of these: the parse stage
    /// records it after the walk, so it has to keep reaching consumers through
    /// the registry.
    ///
    /// A walk-recorded entry must reach an analysis from its OWN walk's return
    /// value. Combined mode runs the dead-code and duplication walks under
    /// `rayon::join` whenever a per-analysis `production` split stops them from
    /// sharing a file list, so a registry read answers "whichever walk wrote
    /// last" and varies between runs of the same command (issue #2366).
    #[must_use]
    pub const fn is_source_walk_recorded(&self) -> bool {
        matches!(
            self,
            Self::SkippedLargeFile { .. }
                | Self::SkippedMinifiedFile { .. }
                | Self::SkippedSourceDotdir
                | Self::NodeModulesMissing
                | Self::ExcludedByDefaultIgnore { .. }
                | Self::NoSourceFilesAnalyzed { .. }
        )
    }

    /// Whether this diagnostic reports a source file whose contents this run
    /// never analyzed, so every import and export the file holds is invisible
    /// to the module graph.
    ///
    /// This is the class `reachability_caveats[]` exists for. A file the run
    /// never read credits nothing, so the modules it imports surface as
    /// confident `unused-file` and `unused-export` findings carrying
    /// `delete-file` and `remove-export` actions, and `fallow fix` would
    /// otherwise apply the removal against source that still imports the
    /// target.
    ///
    /// All four discovery-side kinds qualify, for the same reason and with the
    /// same consequence:
    ///
    /// - `skipped-large-file` and `skipped-minified-file`: the file is in the
    ///   project tree and was never opened, so its import list is unknown.
    /// - `skipped-source-dotdir`: the directory holds at least one source file
    ///   the project did not exclude, and none of them were traversed. The
    ///   diagnostic is capped, so it under-reports rather than over-reports;
    ///   its presence still proves unseen source exists.
    /// - `source-read-failure`: the file was discovered and then could not be
    ///   read, so nothing was extracted from it at all.
    ///
    /// `source-parse-degraded` is deliberately NOT one of these, though it
    /// belongs to the same family. Neither is `excluded-by-default-ignore`,
    /// for a different reason: that one reports designed behavior on generated
    /// output rather than a degraded run, and its own doc comment carries the
    /// argument.
    ///
    /// `source-parse-degraded`: that file WAS read, so it has a module and
    /// a graph node and its reachability is observable, which lets the caveat
    /// pass narrow it: a degraded module that is itself unreachable cannot
    /// change a reachability verdict. Every kind above has no node to ask (a
    /// read failure has one with nothing extracted into it), so no narrowing
    /// is available and the caveat they raise is run-level.
    ///
    /// The match is exhaustive on purpose: a new "the run did not see this
    /// file" kind has to be classified here, and answering `true` is the only
    /// wiring its findings need in order to inherit both the caveat and the
    /// `fallow fix` withholding that follows it.
    #[must_use]
    pub const fn source_never_analyzed(&self) -> bool {
        match self {
            Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. } => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::WorkspaceGlobOutsideRoot { .. }
            | Self::WorkspacePatternDirMissing { .. }
            | Self::NestedProjectRoot
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SourceParseDegraded { .. }
            | Self::SourceNestingTooDeep { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::PnpmLockOverrideResolutionSkipped
            | Self::NpmLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::PnpmWorkspaceOverridesIgnored { .. }
            | Self::NodeModulesMissing
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ClientServerLeakNoClientRoots
            | Self::ProductionExcludedFileImported { .. }
            | Self::ExcludedByDefaultIgnore { .. }
            | Self::NoSourceFilesAnalyzed { .. }
            | Self::FileScoresUnavailable { .. }
            | Self::HotspotsSkipped { .. }
            | Self::ChangeCouplingSkipped { .. }
            | Self::ShallowClone { .. }
            | Self::UnpinnedClock
            | Self::OwnershipUnavailable { .. }
            | Self::TrendSnapshotUnreadable { .. }
            | Self::TrendGroupBaselineUnavailable { .. }
            | Self::PluginConfigUnreadable { .. }
            | Self::PluginEffectNotModeled { .. }
            | Self::CoverageAutoDetected
            | Self::FlagAgeShallowClone
            | Self::FlagAgeUnavailable { .. }
            | Self::IgnoreDependenciesGlobUnmatched { .. }
            | Self::IgnoreFindingsPatternUnmatched { .. }
            | Self::EntryPatternRedundant { .. }
            | Self::IgnoreDependenciesEntryRedundant { .. }
            | Self::IgnorePatternsEntryRedundant { .. }
            | Self::CircularDependenciesAllowUnmatched { .. }
            | Self::OverrideRuleIneffective { .. }
            | Self::RequestedRuleOff { .. }
            | Self::BaselineInstallStateMismatch { .. }
            | Self::InertFlag { .. }
            | Self::ConfigLoadFellBack { .. }
            | Self::ConfigFileShadowed { .. }
            | Self::UnknownSecurityCategory { .. }
            | Self::EnvValueIgnored { .. } => false,
        }
    }

    /// Whether this diagnostic is recorded by the ANALYZE stage (the
    /// dependency-catalog and override detectors) rather than by workspace or
    /// source discovery. Analysis-stage diagnostics reach the registry through
    /// `record_workspace_diagnostics` after config load, so
    /// `stash_workspace_diagnostics` must preserve them across combined-mode's
    /// per-analysis config re-loads, and every analyze pass clears its previous
    /// entries before re-recording so a fixed cause drops out on the next run
    /// (issue #2366). The match is exhaustive on purpose: a new kind must be
    /// classified here before it compiles.
    ///
    /// Classify a kind `true` ONLY when a detector reachable from the dead-code
    /// analyze pass (`find_dead_code_full`) re-records it, because that pass is
    /// the single clear site. A kind recorded exclusively by another stage would
    /// be cleared by the next dead-code pass and never come back.
    #[must_use]
    pub const fn is_analysis_stage(&self) -> bool {
        match self {
            Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::PnpmLockOverrideResolutionSkipped
            | Self::NpmLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::PnpmWorkspaceOverridesIgnored { .. }
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ClientServerLeakNoClientRoots
            | Self::ProductionExcludedFileImported { .. } => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::WorkspaceGlobOutsideRoot { .. }
            | Self::WorkspacePatternDirMissing { .. }
            | Self::NestedProjectRoot
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::SourceNestingTooDeep { .. }
            | Self::NodeModulesMissing
            | Self::ExcludedByDefaultIgnore { .. }
            | Self::NoSourceFilesAnalyzed { .. }
            | Self::FileScoresUnavailable { .. }
            | Self::HotspotsSkipped { .. }
            | Self::ChangeCouplingSkipped { .. }
            | Self::ShallowClone { .. }
            | Self::UnpinnedClock
            | Self::OwnershipUnavailable { .. }
            | Self::TrendSnapshotUnreadable { .. }
            | Self::TrendGroupBaselineUnavailable { .. }
            | Self::PluginConfigUnreadable { .. }
            | Self::PluginEffectNotModeled { .. }
            | Self::CoverageAutoDetected
            | Self::FlagAgeShallowClone
            | Self::FlagAgeUnavailable { .. }
            | Self::IgnoreDependenciesGlobUnmatched { .. }
            | Self::IgnoreFindingsPatternUnmatched { .. }
            | Self::EntryPatternRedundant { .. }
            | Self::IgnoreDependenciesEntryRedundant { .. }
            | Self::IgnorePatternsEntryRedundant { .. }
            | Self::CircularDependenciesAllowUnmatched { .. }
            | Self::OverrideRuleIneffective { .. }
            | Self::RequestedRuleOff { .. }
            | Self::BaselineInstallStateMismatch { .. }
            | Self::InertFlag { .. }
            | Self::ConfigLoadFellBack { .. }
            | Self::ConfigFileShadowed { .. }
            | Self::UnknownSecurityCategory { .. }
            | Self::EnvValueIgnored { .. } => false,
        }
    }

    /// Whether this diagnostic is recorded by the HEALTH pipeline (scoring,
    /// churn, ownership, trend, coverage input resolution) rather than by
    /// workspace discovery, source discovery or the analyze stage.
    ///
    /// Health-stage diagnostics are appended to the registry after config
    /// load, so `stash_workspace_diagnostics` must preserve them across
    /// combined mode's per-analysis config re-loads, and the health run clears
    /// its previous entries before re-recording so a fixed CODEOWNERS drops out
    /// on the next run (issue #2689).
    ///
    /// They are deliberately NOT [`Self::is_analysis_stage`], although they
    /// share both of those properties. That predicate additionally means "the
    /// dead-code analyze pass re-records this", and the pass clears every kind
    /// answering it on entry. Health computes file scores by running that same
    /// pass, so a health-stage kind classified there would be wiped mid-run by
    /// the analysis it is reporting on.
    #[must_use]
    pub const fn is_health_stage(&self) -> bool {
        match self {
            Self::FileScoresUnavailable { .. }
            | Self::HotspotsSkipped { .. }
            | Self::ChangeCouplingSkipped { .. }
            | Self::ShallowClone { .. }
            | Self::UnpinnedClock
            | Self::OwnershipUnavailable { .. }
            | Self::TrendSnapshotUnreadable { .. }
            | Self::TrendGroupBaselineUnavailable { .. }
            | Self::CoverageAutoDetected => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::WorkspaceGlobOutsideRoot { .. }
            | Self::WorkspacePatternDirMissing { .. }
            | Self::NestedProjectRoot
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::SourceNestingTooDeep { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::PnpmLockOverrideResolutionSkipped
            | Self::NpmLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::PnpmWorkspaceOverridesIgnored { .. }
            | Self::NodeModulesMissing
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ClientServerLeakNoClientRoots
            | Self::ProductionExcludedFileImported { .. }
            | Self::ExcludedByDefaultIgnore { .. }
            | Self::PluginConfigUnreadable { .. }
            | Self::PluginEffectNotModeled { .. }
            | Self::NoSourceFilesAnalyzed { .. }
            | Self::FlagAgeShallowClone
            | Self::FlagAgeUnavailable { .. }
            | Self::IgnoreDependenciesGlobUnmatched { .. }
            | Self::IgnoreFindingsPatternUnmatched { .. }
            | Self::EntryPatternRedundant { .. }
            | Self::IgnoreDependenciesEntryRedundant { .. }
            | Self::IgnorePatternsEntryRedundant { .. }
            | Self::CircularDependenciesAllowUnmatched { .. }
            | Self::OverrideRuleIneffective { .. }
            | Self::RequestedRuleOff { .. }
            | Self::BaselineInstallStateMismatch { .. }
            | Self::InertFlag { .. }
            | Self::ConfigLoadFellBack { .. }
            | Self::ConfigFileShadowed { .. }
            | Self::UnknownSecurityCategory { .. }
            | Self::EnvValueIgnored { .. } => false,
        }
    }

    /// Whether this diagnostic is recorded by the PLUGIN stage (framework
    /// plugins reading their own build configs) rather than by workspace
    /// discovery, source discovery, the analyze stage or the health pipeline.
    ///
    /// Plugin-stage diagnostics are recorded after config load, so
    /// `stash_workspace_diagnostics` must preserve them across combined mode's
    /// per-analysis config re-loads, and each plugin run replaces the previous
    /// run's set so a fixed config drops out on the next run (issue #2736).
    ///
    /// They are deliberately NOT [`Self::is_analysis_stage`], although they
    /// share both of those properties. That predicate additionally means "the
    /// dead-code analyze pass re-records this", and the pass clears every kind
    /// answering it on entry. Plugins run in the prelude of that same pass, so
    /// a plugin-stage kind classified there would be wiped inside the run that
    /// produced it.
    ///
    /// The match is exhaustive on purpose: a new kind must be classified here
    /// before it compiles.
    #[must_use]
    pub const fn is_plugin_stage(&self) -> bool {
        match self {
            Self::PluginConfigUnreadable { .. } | Self::PluginEffectNotModeled { .. } => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::WorkspaceGlobOutsideRoot { .. }
            | Self::WorkspacePatternDirMissing { .. }
            | Self::NestedProjectRoot
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::SourceNestingTooDeep { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::PnpmLockOverrideResolutionSkipped
            | Self::NpmLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::PnpmWorkspaceOverridesIgnored { .. }
            | Self::NodeModulesMissing
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ClientServerLeakNoClientRoots
            | Self::ProductionExcludedFileImported { .. }
            | Self::ExcludedByDefaultIgnore { .. }
            | Self::NoSourceFilesAnalyzed { .. }
            | Self::FileScoresUnavailable { .. }
            | Self::HotspotsSkipped { .. }
            | Self::ChangeCouplingSkipped { .. }
            | Self::ShallowClone { .. }
            | Self::UnpinnedClock
            | Self::OwnershipUnavailable { .. }
            | Self::TrendSnapshotUnreadable { .. }
            | Self::TrendGroupBaselineUnavailable { .. }
            | Self::CoverageAutoDetected
            | Self::FlagAgeShallowClone
            | Self::FlagAgeUnavailable { .. }
            | Self::IgnoreDependenciesGlobUnmatched { .. }
            | Self::IgnoreFindingsPatternUnmatched { .. }
            | Self::EntryPatternRedundant { .. }
            | Self::IgnoreDependenciesEntryRedundant { .. }
            | Self::IgnorePatternsEntryRedundant { .. }
            | Self::CircularDependenciesAllowUnmatched { .. }
            | Self::OverrideRuleIneffective { .. }
            | Self::RequestedRuleOff { .. }
            | Self::BaselineInstallStateMismatch { .. }
            | Self::InertFlag { .. }
            | Self::ConfigLoadFellBack { .. }
            | Self::ConfigFileShadowed { .. }
            | Self::UnknownSecurityCategory { .. }
            | Self::EnvValueIgnored { .. } => false,
        }
    }
}

impl WorkspaceDiagnosticKind {
    /// Whether this kind is a notice of the run: a decision of the resolver
    /// or a flag of the request, not a finding of a discovery or an analysis.
    /// A surface shows these as one note each.
    #[must_use]
    pub const fn is_run_notice(&self) -> bool {
        matches!(
            self,
            Self::InertFlag { .. }
                | Self::ConfigLoadFellBack { .. }
                | Self::ConfigFileShadowed { .. }
                | Self::UnknownSecurityCategory { .. }
                | Self::EnvValueIgnored { .. }
        )
    }
}
