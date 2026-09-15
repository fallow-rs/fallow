//! Workspace and source-discovery diagnostic data types.
//!
//! The serializable `WorkspaceDiagnostic` / `WorkspaceDiagnosticKind` pair
//! lives here, upstream of both `fallow-config` (which owns the registry and
//! emission logic and re-exports these types for back-compat) and
//! `fallow-output` (which embeds `Vec<WorkspaceDiagnostic>` in its JSON
//! envelopes). Keeping the data types in `fallow-types` lets the output layer
//! reference the real, schema-bearing type instead of an opaque
//! `serde_json::Value` newtype, so `workspace_diagnostics[]` keeps its typed
//! `kind`/`path`/`message` shape (and the typed `kind` oneOf) in
//! `docs/output-schema.json` without coupling output contracts to config
//! loading.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::serde_path;

/// Why a workspace-discovery candidate was rejected, or why a sibling
/// directory looked workspace-like but was not declared.
///
/// Wire-format names are kebab-case so JSON consumers (CI integrations, MCP
/// agents, LSP clients) get a stable, language-neutral identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkspaceDiagnosticKind {
    /// A directory contains `package.json` but is not declared as a workspace
    /// in `package.json` `workspaces`, `pnpm-workspace.yaml`, or
    /// `tsconfig.json` `references`. Surfaced by
    /// `find_undeclared_workspaces`.
    UndeclaredWorkspace,
    /// A declared workspace's `package.json` failed to parse. The directory is
    /// dropped from discovery, but analysis still proceeds (degraded).
    MalformedPackageJson {
        /// `serde_json` parse error text.
        error: String,
    },
    /// A workspace glob pattern matched a directory that contains no
    /// `package.json`. Honors the extended skip list and `ignorePatterns`
    /// before emitting.
    GlobMatchedNoPackageJson {
        /// The glob pattern that matched the directory.
        pattern: String,
    },
    /// `tsconfig.json` exists at the root but failed to parse. Project
    /// references cannot be discovered.
    MalformedTsconfig {
        /// JSONC parse error text.
        error: String,
    },
    /// `tsconfig.json` lists a `references[].path` that does not point to an
    /// existing directory.
    TsconfigReferenceDirMissing,
    /// `pnpm-workspace.yaml` exists but failed to parse as YAML. Catalog and
    /// dependency-override analysis proceeds with no entries (degraded), so
    /// `catalog:`-referenced dependencies may be misclassified until the
    /// syntax is fixed.
    MalformedPnpmWorkspaceYaml {
        /// `serde_yaml_ng` parse error text.
        error: String,
    },
    /// A source file was skipped at discovery because it exceeds the configured
    /// per-file size limit (`--max-file-size` / `FALLOW_MAX_FILE_SIZE`, default
    /// 5 MB). The file is never read, parsed, or analyzed, guarding against the
    /// out-of-memory blowup a single multi-MB generated/vendored/bundled file
    /// causes (issue #1086). Surfaced by source discovery, not workspace
    /// discovery, but shares this channel so the skip is visible in
    /// `workspace_diagnostics[]` on `fallow dead-code / dupes / health` JSON.
    SkippedLargeFile {
        /// On-disk size of the skipped file in bytes.
        size_bytes: u64,
    },
    /// A large JavaScript bundle was skipped at discovery because it appears to
    /// be minified generated output. The file is never parsed or analyzed,
    /// guarding against sub-limit bundles that can still create very large ASTs
    /// and extraction payloads (issue #1086). Use `--max-file-size 0` when the
    /// bundled file really should be analyzed.
    SkippedMinifiedFile {
        /// On-disk size of the skipped file in bytes.
        size_bytes: u64,
    },
    /// A dot-prefixed directory was not traversed by source discovery even
    /// though it contains at least one source file the project has not
    /// excluded. Hidden directories are skipped by default apart from a small
    /// convention allowlist (`.storybook`, `.vitepress`, `.well-known`,
    /// `.changeset`, `.github`) and the directories an active framework plugin
    /// or a `package.json` script reference contributes, so files inside are
    /// never parsed and their imports and exports are invisible to every
    /// analysis. No config field adds a directory to traversal: run fallow
    /// with `--root` against the directory to analyze it on its own, or add it
    /// to `ignorePatterns` to silence this (issue #461).
    ///
    /// "Not excluded" is measured the way the run measures it: a directory
    /// whose contents are gitignored, or excluded by `ignorePatterns`, or (on
    /// a `--production` run) excluded as test or story files, never earns this
    /// diagnostic, because the advertised remedies would find nothing there
    /// either. Generated tool output and non-git VCS metadata are excluded by
    /// name.
    ///
    /// The advisory is best-effort and bounded: one run inspects a fixed
    /// number of skipped directories with a fixed I/O budget, in sorted path
    /// order, so a pathological tree yields a deterministic prefix rather than
    /// an unbounded array or an unbounded scan. The stderr note says "at
    /// least" when a ceiling bound the run.
    ///
    /// Surfaced by source discovery, not workspace discovery, but shares this
    /// channel so the skip is visible in `workspace_diagnostics[]` on
    /// `fallow dead-code / dupes / health` JSON.
    ///
    /// Unlike the two skipped-file kinds beside it, this one is CAPPED. To
    /// bound the directory reads the check costs, a run classifies at most 64
    /// candidate directories and spends at most 1024 directory entries across
    /// all of them, so on a project that exceeds either ceiling the array is a
    /// prefix of the skipped directories rather than all of them, and the
    /// stderr note says "at least N". No measured repository comes close to
    /// either ceiling. A consumer needing an exact total should run fallow
    /// with `--root` against the tree rather than infer one from this array.
    SkippedSourceDotdir,
    /// A source discovered with a stable [`FileId`](crate::discover::FileId)
    /// could not be read before parsing. Analysis continues with the remaining
    /// sparse module IDs and reports the underlying filesystem or UTF-8 error.
    SourceReadFailure {
        /// Filesystem or UTF-8 decoding error from `read_to_string`.
        error: String,
    },
    /// A source file was read but parsed with diagnostics, so the module
    /// extracted from it may be missing imports, exports, or references after
    /// the first error. Analysis proceeds with the partial module, which is why
    /// this is reported: an import the parser never saw credits nothing, and its
    /// target can surface as a confident `unused-file` or `unused-export`
    /// finding with a `delete-file` or `remove-export` action on it.
    ///
    /// Recorded by the parse stage, alongside `source-read-failure`, and never
    /// used to withhold a finding. oxc reports recoverable errors for valid
    /// syntax newer than the parser as well as for genuinely broken files, so
    /// gating findings on this would mute real results project-wide instead of
    /// just the affected file.
    SourceParseDegraded {
        /// Number of parser diagnostics reported for the file.
        error_count: u32,
        /// `true` when the parser abandoned the file instead of recovering, so
        /// the extracted module is a fragment at best.
        panicked: bool,
    },
    /// Dependency-override resolution was skipped because bun's legacy binary
    /// `bun.lockb` sits next to this `package.json`, fallow cannot read the
    /// binary format, and no parseable text lockfile was found to use
    /// instead: no `bun.lock` that parses, and no readable `pnpm-lock.yaml`,
    /// `package-lock.json`, or `npm-shrinkwrap.json`. A `yarn.lock` is never
    /// consulted (yarn ignores `overrides`), so it does not prevent the skip
    /// either. The manifest declares overrides, so the
    /// `unused-dependency-overrides` check would otherwise have run; without
    /// resolution ground truth it would flag every transitive-only pin, so no
    /// unused-override findings are reported at all (issue #2358). Surfaced
    /// by the override analysis, not workspace discovery, but shares this
    /// channel so the skip is visible in `workspace_diagnostics[]` JSON and
    /// as a stderr warning.
    BunLockbOverrideResolutionSkipped,
    /// Dependency-override resolution was skipped because bun's text
    /// `bun.lock` exists but could not be parsed and no readable pnpm or npm
    /// lockfile was available as independent resolution ground truth.
    BunLockOverrideResolutionSkipped,
    /// A bun manifest declares both `overrides` and a non-empty `resolutions`
    /// object. Bun applies `overrides` and ignores `resolutions`, so fallow
    /// reports the shadowed configuration without offering removal advice.
    BunResolutionsShadowedByOverrides,
    /// The project has no `node_modules` directory and is not a Deno project
    /// that legitimately runs without one. Analysis proceeds, but three things
    /// degrade silently: package `exports` and conditional exports cannot be
    /// read, so imports into a dependency's subpaths resolve less precisely;
    /// framework plugins that activate on an installed package stay inactive,
    /// so their entry points and path aliases are missing; and a dependency's
    /// installed shape cannot be inspected, so type-only dependency
    /// classification falls back to declaration-based heuristics.
    ///
    /// Recorded once per run by the source walk, anchored at the missing
    /// `node_modules` directory so the reported path is a real location rather
    /// than the empty string a root-anchored diagnostic would render. This used
    /// to be a bare `tracing::warn!` duplicated in two pipelines, so it never
    /// reached JSON output and never reached `fallow doctor`, which reported
    /// `pass` on a tree that had never been installed.
    NodeModulesMissing,
    /// `boundaries` is empty while `boundary-violation` is not `off`, so the
    /// boundary detector never ran. Its summary counters are therefore
    /// structurally zero and say nothing about the project.
    ///
    /// This is the UNCONFIGURED zero, not the user-chosen one: a project that
    /// sets `boundary-violation: off` asked for silence and can see that
    /// choice in `fallow config`. A project that left `boundaries` empty
    /// cannot distinguish "no violations" from "nothing was measured".
    BoundariesNotConfigured,
    /// `rulePacks` is empty while `policy-violation` is not `off`, so the
    /// policy detector never ran and its summary counters are structurally
    /// zero. The unconfigured counterpart of
    /// [`Self::BoundariesNotConfigured`].
    RulePacksNotConfigured,
    /// One of fallow's built-in discovery ignore patterns (`**/dist/**`,
    /// `**/build/**`, `**/coverage/**`, and the four minified-bundle globs)
    /// removed at least one candidate source file from this walk. The files
    /// are never read, so their imports and exports are invisible to every
    /// analysis, and until issue #2638 the drop was completely silent:
    /// pointing fallow at a directory a built-in pattern matches returned a
    /// clean report with exit 0 and nothing said why.
    ///
    /// `**/node_modules/**` is carved out and never appears in `pattern`:
    /// installed dependencies are not the first-party source this diagnostic
    /// is about, and a project that does not gitignore them would get a
    /// five-figure count with no useful remedy. `**/.git/**` cannot fire,
    /// because hidden directories are not traversed.
    ///
    /// One entry per pattern, never per file or per directory, so the array
    /// grows by at most the number of built-in patterns on a project of any
    /// size. `path` anchors at the matched directory holding the most excluded
    /// files for that pattern, ties broken by the lexicographically first
    /// path, so two runs on one tree report the same location. On a nested
    /// match it is the DEEPEST segment the pattern matched
    /// (`build/tools/build`, not `build`), because that is the directory the
    /// `--root` remedy names and re-rooting at a shallower one would leave a
    /// matching segment behind. That directory is the
    /// largest group and not a majority: a flat monorepo can spread ten
    /// excluded files over ten sibling `dist/` directories and every one of
    /// them is then "the largest". `file_count` spans all of them, and
    /// `directory_count` says how many there were, so a reader can tell a
    /// single tree from a scattered one without a directory list in the
    /// payload.
    ///
    /// Three properties of the population are load-bearing and easy to
    /// misread:
    ///
    /// - **Gitignored trees count zero.** Source discovery honors
    ///   `.gitignore`, `.git/info/exclude`, and the global gitignore, and
    ///   prunes those directories before this check runs. The honest reading
    ///   is "candidate source files git did not already hide and a built-in
    ///   pattern then dropped", which is why a repository that gitignores its
    ///   own `dist/` never sees this diagnostic.
    /// - **A user `ignorePatterns` entry is not a surprise.** The compiled
    ///   ignore set is the union of `ignorePatterns` and the built-ins, so a
    ///   file both matched was an explicit project choice and is attributed to
    ///   no pattern here. The union also only ever adds: `ignorePatterns`
    ///   cannot negate a built-in, so a config edit is never the remedy.
    /// - **The remedy depends on the pattern's shape.** A directory-shaped
    ///   built-in (`**/dist/**`) is matched against the path relative to the
    ///   run root, so re-rooting inside the matched directory removes the
    ///   matched segment and the files become visible: the message advertises
    ///   `fallow --root <dir>`. A file-shaped built-in (`**/*.min.js` and the
    ///   three other bundle globs) matches on the file name and keeps matching
    ///   at any root, so the message says so and points at renaming instead of
    ///   handing out a command that provably does nothing.
    ///
    /// Deliberately NOT one of the [`Self::source_never_analyzed`] kinds. These
    /// exclusions are the product's designed behavior on generated output, not
    /// a degraded run: answering `true` would attach `IncompleteFileAnalysis`
    /// and `IncompleteImportGraph` caveats to findings on nearly every project
    /// that keeps a non-gitignored `dist/` or `coverage/`, and make `fallow
    /// fix` withhold `delete-file` and `remove-export` actions project-wide.
    ExcludedByDefaultIgnore {
        /// The built-in glob that matched, verbatim (for example
        /// `**/build/**`).
        pattern: String,
        /// Candidate source files this pattern excluded in this walk, across
        /// every directory it matched, not just the one `path` anchors at.
        /// Exact: the walk counts each excluded candidate once.
        file_count: u32,
        /// Distinct directories this pattern matched at, `path` included, and
        /// not the number of directories that held the files. A
        /// directory-shaped pattern (`**/dist/**`) matches at the directory it
        /// names, so an excluded subtree counts once however many nested
        /// directories inside it held source: a `dist/` holding files in three
        /// sub-directories reports `1`. A file-shaped pattern (`**/*.min.js`)
        /// has no directory to collapse to and counts each matched file's own
        /// parent. Exact either way, and anything above `1` says `path` names
        /// one matched location out of several.
        directory_count: u32,
    },
}

impl WorkspaceDiagnosticKind {
    /// Stable kebab-case identifier used in dedupe keys and tracing payloads.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        match self {
            Self::UndeclaredWorkspace => "undeclared-workspace",
            Self::MalformedPackageJson { .. } => "malformed-package-json",
            Self::GlobMatchedNoPackageJson { .. } => "glob-matched-no-package-json",
            Self::MalformedTsconfig { .. } => "malformed-tsconfig",
            Self::TsconfigReferenceDirMissing => "tsconfig-reference-dir-missing",
            Self::MalformedPnpmWorkspaceYaml { .. } => "malformed-pnpm-workspace-yaml",
            Self::SkippedLargeFile { .. } => "skipped-large-file",
            Self::SkippedMinifiedFile { .. } => "skipped-minified-file",
            Self::SkippedSourceDotdir => "skipped-source-dotdir",
            Self::SourceReadFailure { .. } => "source-read-failure",
            Self::SourceParseDegraded { .. } => "source-parse-degraded",
            Self::BunLockbOverrideResolutionSkipped => "bun-lockb-override-resolution-skipped",
            Self::BunLockOverrideResolutionSkipped => "bun-lock-override-resolution-skipped",
            Self::BunResolutionsShadowedByOverrides => "bun-resolutions-shadowed-by-overrides",
            Self::NodeModulesMissing => "node-modules-missing",
            Self::BoundariesNotConfigured => "boundaries-not-configured",
            Self::RulePacksNotConfigured => "rule-packs-not-configured",
            Self::ExcludedByDefaultIgnore { .. } => "excluded-by-default-ignore",
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
    #[must_use]
    pub const fn warns_on_stderr(&self) -> bool {
        match self {
            Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ExcludedByDefaultIgnore { .. } => false,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::NodeModulesMissing => true,
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
                | Self::NodeModulesMissing
                | Self::ExcludedByDefaultIgnore { .. }
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
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::MalformedPnpmWorkspaceYaml { .. }
            | Self::SourceParseDegraded { .. }
            | Self::BunLockbOverrideResolutionSkipped
            | Self::BunLockOverrideResolutionSkipped
            | Self::BunResolutionsShadowedByOverrides
            | Self::NodeModulesMissing
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured
            | Self::ExcludedByDefaultIgnore { .. } => false,
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
            | Self::BunResolutionsShadowedByOverrides
            | Self::BoundariesNotConfigured
            | Self::RulePacksNotConfigured => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SkippedSourceDotdir
            | Self::SourceReadFailure { .. }
            | Self::SourceParseDegraded { .. }
            | Self::NodeModulesMissing
            | Self::ExcludedByDefaultIgnore { .. } => false,
        }
    }
}

/// Render a byte count as a megabyte figure with one decimal place for
/// human-readable diagnostic messages (e.g. `12.3 MB`).
#[must_use]
fn format_size_mb(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only size figure; precision loss past 2^53 bytes is irrelevant"
    )]
    let mb = bytes as f64 / (1024.0 * 1024.0);
    format!("{mb:.1} MB")
}

/// A diagnostic about a workspace-discovery candidate.
///
/// The `message` field is a human-readable rendering derived from `kind`. It
/// always ends with a concrete next step ("fix the JSON syntax", "remove from
/// `workspaces`", "add to `ignorePatterns`") so first-time users have a path
/// forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct WorkspaceDiagnostic {
    /// Path to the directory or file that triggered the diagnostic.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Kind discriminator with the typed payload.
    #[serde(flatten)]
    pub kind: WorkspaceDiagnosticKind,
    /// Human-readable rendering derived from `kind` + `path`. Always ends
    /// with a next-step hint.
    pub message: String,
}

impl WorkspaceDiagnostic {
    /// Construct a diagnostic with the message rendered from `kind` + `path`.
    ///
    /// `root` is used to produce project-relative paths in the message text
    /// AND inside the variant payload (e.g. the `error` field of
    /// `MalformedPackageJson` / `MalformedTsconfig` which embed the absolute
    /// file path from `PackageJson::load()`'s error text). Without the
    /// payload-side normalisation the embedded path would survive
    /// environment-specific differences (CI vs Docker vs local) because the
    /// post-serialisation `strip_root_prefix` only catches whole-string
    /// matches, not paths embedded mid-sentence.
    ///
    /// If `path` is not under `root` (e.g. canonicalisation crossed a
    /// symlink), the absolute path is emitted instead.
    ///
    /// `path` also loses any no-op `.` component, for the same reason the
    /// payload loses a glob's `./` prefix: one directory reached through two
    /// spellings of one glob must be one diagnostic.
    #[must_use]
    pub fn new(root: &Path, path: PathBuf, kind: WorkspaceDiagnosticKind) -> Self {
        let path = normalise_diagnostic_path(path);
        let kind = normalise_payload_paths(root, kind);
        let message = render_message(root, &path, &kind);
        Self {
            path,
            kind,
            message,
        }
    }

    /// Return this diagnostic with `path` rewritten relative to `root`.
    ///
    /// `path` is stored absolute so callers can act on it. Every JSON envelope
    /// emits it project-relative instead: the analysis envelopes get there
    /// through the post-serialisation `strip_root_prefix` pass, which the
    /// `fallow workspaces` / `fallow list --workspaces` envelope and the MCP
    /// `project_info` tool never run, so those emitted the absolute path while
    /// the sibling `workspaces[].path` next to it was relative. They normalise
    /// at the typed layer with this method instead.
    ///
    /// Paths outside `root` (canonicalisation crossed a symlink) are left
    /// absolute, matching how [`Self::new`] renders the message.
    ///
    /// A diagnostic anchored at the root itself becomes `.`, not the empty
    /// path: an empty string is not a location, and the analysis envelopes'
    /// post-serialisation strip only removes a `root + separator` prefix, so a
    /// root-anchored path that stays absolute here leaks a host path.
    #[must_use]
    pub fn into_root_relative(mut self, root: &Path) -> Self {
        if let Ok(relative) = self.path.strip_prefix(root) {
            self.path = if relative.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                relative.to_path_buf()
            };
        }
        self
    }
}

/// Rebuild `path` from its components so one directory has one spelling.
///
/// The dedupe key was never the problem: [`Path`] equality already ignores an
/// interior `.`, so `<root>/./pkgs/aaa` and `<root>/pkgs/aaa` are one key. The
/// stored bytes were. A workspace glob spelled `./pkgs/*` in `package.json`
/// expands to the first spelling and the same glob spelled `pkgs/*` in
/// `pnpm-workspace.yaml` expands to the second, and the two envelope families
/// make a project-relative path differently: the analysis envelopes strip the
/// root as a string (leaving `./pkgs/aaa`) while the workspace listing
/// envelope uses [`WorkspaceDiagnostic::into_root_relative`] (leaving
/// `pkgs/aaa`). Whichever
/// manifest happened to be read first then decided which shape every consumer
/// saw. Collapsing at construction gives them one answer (issue #2366).
///
/// A path that is already component-clean rebuilds to itself. Serialization
/// normalises separators, so the rebuild is wire-invisible on Windows.
fn normalise_diagnostic_path(path: PathBuf) -> PathBuf {
    let rebuilt: PathBuf = path.components().collect();
    if rebuilt.as_os_str() == path.as_os_str() {
        path
    } else {
        rebuilt
    }
}

/// Strip the project root from absolute paths embedded inside variant
/// payloads (the `error` field of malformed-config and source-read failures),
/// and drop a glob pattern's no-op `./` prefix.
///
/// Mirrors the per-platform `display()` byte sequence so the substring match
/// works on Windows too.
///
/// The pattern prefix matters because the payload is part of the dedupe key in
/// [`merge_workspace_diagnostics`]. A repository whose `package.json` declares
/// `"./apps/**"` and whose `pnpm-workspace.yaml` declares `apps/**` names one
/// glob twice, and without this both spellings would report every package-less
/// directory under `apps/` a second time (issue #2366).
fn normalise_payload_paths(root: &Path, kind: WorkspaceDiagnosticKind) -> WorkspaceDiagnosticKind {
    let root_str = root.display().to_string();
    let root_alt = root_str.replace('\\', "/");
    let normalise = |text: String| -> String {
        let stripped = text
            .replace(&format!("{root_str}/"), "")
            .replace(&format!("{root_alt}/"), "");
        stripped
            .replace(&format!("{root_str}\\"), "")
            .replace(&format!("{root_alt}\\"), "")
    };
    match kind {
        WorkspaceDiagnosticKind::MalformedPackageJson { error } => {
            WorkspaceDiagnosticKind::MalformedPackageJson {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::MalformedTsconfig { error } => {
            WorkspaceDiagnosticKind::MalformedTsconfig {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::SourceReadFailure { error } => {
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => {
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: canonical_glob_pattern(pattern),
            }
        }
        other => other,
    }
}

/// Drop the leading `./` (or `.\`) a workspace glob may carry, so the same
/// pattern declared in two manifests is one payload.
///
/// A pattern that is nothing BUT the prefix (`"./"`, the root itself) keeps
/// its spelling: stripping it would report an empty `pattern` field and an
/// empty quoted glob in the warning text, which names no glob at all.
fn canonical_glob_pattern(pattern: String) -> String {
    for prefix in ["./", ".\\"] {
        if let Some(rest) = pattern.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return rest.to_owned();
        }
    }
    pattern
}

/// Concatenate two diagnostic lists, keeping the first occurrence of each
/// `(kind, path)` pair and the order of `primary` followed by the entries only
/// `secondary` has.
///
/// The single place diagnostics from two observation points are folded
/// together: an engine session's own capture plus the process registry, and
/// the combined run's per-analysis lists (issue #2366). A combined run walks
/// the project once per analysis, and per-analysis `production` modes can make
/// those walks see different file sets, so no single observation point holds
/// everything the run recorded; the union does, and folding it the same way
/// everywhere is what keeps the CLI and the programmatic route answering
/// identically.
///
/// The key is the WHOLE kind, payload included, not its
/// [`id`](WorkspaceDiagnosticKind::id). Two entries can share a kind id and a
/// path and still be two distinct diagnostics: overlapping workspace globs
/// (`["packages/*", "packages/*/*"]`) each report the same package-less
/// directory with their own `pattern`, and the standalone envelopes report
/// both. An id-keyed fold silently dropped the second one.
#[must_use]
pub fn merge_workspace_diagnostics(
    primary: Vec<WorkspaceDiagnostic>,
    secondary: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    let mut merged = Vec::with_capacity(primary.len() + secondary.len());
    let mut seen: FxHashSet<(WorkspaceDiagnosticKind, PathBuf)> = FxHashSet::default();
    for diagnostic in primary.into_iter().chain(secondary) {
        let key = (diagnostic.kind.clone(), diagnostic.path.clone());
        if seen.insert(key) {
            merged.push(diagnostic);
        }
    }
    merged
}

/// Keep the first occurrence of each `(kind, path)` pair in one list.
///
/// The single-list form of [`merge_workspace_diagnostics`], applied where
/// diagnostics are produced rather than where two observation points are
/// folded: workspace discovery reads `package.json` `workspaces`,
/// `pnpm-workspace.yaml` `packages`, `deno.json` `workspace` and the root
/// `tsconfig.json` references additively, so a repository that declares one
/// glob in two of them reports every package-less directory under it twice.
/// Deduplicating at that source is what keeps the JSON envelopes, the
/// aggregated stderr warning and the process registry telling one story
/// (issue #2366).
#[must_use]
pub fn dedupe_workspace_diagnostics(
    diagnostics: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    merge_workspace_diagnostics(diagnostics, Vec::new())
}

/// Render `path` relative to `root` with forward slashes. The forward-slash
/// normalisation is load-bearing for cross-platform output stability.
fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// The first segment of a glob that contains no glob metacharacter, so it
/// names a real directory rather than a wildcard.
///
/// Source discovery uses it to decide which directory a built-in ignore
/// pattern excluded a file "at"; [`render_message`] uses it to decide which
/// remedy is true for that pattern. The two have to agree, so the function
/// lives here rather than once per crate: a pattern with such a segment
/// (`**/dist/**`) is lifted by re-rooting inside the matched directory,
/// because the glob is matched against the path relative to the run root. A
/// pattern without one (`**/*.min.js`) matches on the file name and keeps
/// matching at every root.
#[must_use]
pub fn glob_first_literal_segment(pattern: &str) -> Option<&str> {
    pattern.split('/').find(|segment| {
        !segment.is_empty()
            && !segment.contains(['*', '?', '[', ']', '{', '}'])
            && *segment != "."
            && *segment != ".."
    })
}

fn render_message(root: &Path, path: &Path, kind: &WorkspaceDiagnosticKind) -> String {
    let display = display_relative(root, path);
    match kind {
        WorkspaceDiagnosticKind::UndeclaredWorkspace => format!(
            "Directory '{display}' contains package.json but is not declared as a workspace. \
             Add it to package.json workspaces or pnpm-workspace.yaml, or add it to ignorePatterns."
        ),
        WorkspaceDiagnosticKind::MalformedPackageJson { error } => format!(
            "Dropped workspace '{display}': package.json is not valid JSON ({error}). \
             Fix the JSON syntax or remove '{display}' from the workspaces pattern."
        ),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => format!(
            "Glob '{pattern}' matched '{display}' but no package.json is present. \
             Add a package.json, narrow the pattern, or add '{display}' to ignorePatterns."
        ),
        WorkspaceDiagnosticKind::MalformedTsconfig { error } => format!(
            "tsconfig.json at '{display}' failed to parse ({error}); \
             project references will be ignored. Fix the JSON syntax."
        ),
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing => format!(
            "tsconfig.json references '{display}' but the directory does not exist. \
             Update or remove the reference, or restore the missing directory."
        ),
        WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml { error } => format!(
            "'{display}' failed to parse ({error}); catalog and override entries \
             will be ignored. Fix the YAML syntax."
        ),
        WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes } => format!(
            "Skipped '{display}' ({size}): exceeds the max file size limit. \
             Its imports and exports are not analyzed. Raise the limit with \
             --max-file-size <MB> (or FALLOW_MAX_FILE_SIZE), or add '{display}' \
             to ignorePatterns.",
            size = format_size_mb(*size_bytes)
        ),
        WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes } => format!(
            "Skipped '{display}' ({size}): appears to be minified generated JavaScript. \
             Its imports and exports are not analyzed. Add '{display}' to ignorePatterns, \
             rename it with a .min.js suffix, or use --max-file-size 0 if this file \
             should be analyzed.",
            size = format_size_mb(*size_bytes)
        ),
        WorkspaceDiagnosticKind::SkippedSourceDotdir => format!(
            "Skipped hidden directory '{display}': it contains source files but hidden \
             directories are not traversed. Its imports and exports are not analyzed. \
             There is no config field that adds a directory to traversal. If it holds \
             first-party source, analyze it on its own with fallow --root {display}; if it \
             is tool or agent scratch state, add '{display}/**' to ignorePatterns to \
             silence this."
        ),
        WorkspaceDiagnosticKind::SourceReadFailure { error } => format!(
            "Could not read source '{display}' ({error}). Restore the file or its read permissions, \
             ensure it contains valid UTF-8 text, or add '{display}' to ignorePatterns."
        ),
        WorkspaceDiagnosticKind::SourceParseDegraded {
            error_count,
            panicked,
        } => {
            let outcome = if *panicked {
                "the parser stopped there"
            } else {
                "the parser recovered and continued"
            };
            format!(
                "Parsed '{display}' with {error_count} error(s); {outcome}. Imports, exports, and \
                 references it did not reach are missing from this run, so files and symbols it \
                 uses can be reported as unused. Fix the syntax, or ignore this if the file uses \
                 syntax newer than fallow's parser."
            )
        }
        WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped => format!(
            "Skipped dependency-override resolution for '{display}': bun's legacy binary bun.lockb \
             sits next to it, fallow cannot read the binary format, and no parseable text lockfile \
             (bun.lock, pnpm-lock.yaml, package-lock.json, or npm-shrinkwrap.json) was found to \
             use instead, so unused-dependency-overrides findings are not reported. Run bun install \
             --save-text-lockfile (bun 1.2 or newer) to write a text bun.lock, or delete the stale \
             bun.lockb if this repository no longer uses bun."
        ),
        WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped => format!(
            "Skipped dependency-override resolution because '{display}' could not be parsed and \
             no readable pnpm or npm lockfile was available, so unused-dependency-overrides \
             findings are not reported. Run bun install to regenerate the text lockfile, then \
             rerun fallow."
        ),
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides => format!(
            "'{display}' declares both `overrides` and non-empty `resolutions`; bun applies \
             `overrides` and ignores `resolutions`. Move the intended pins into `overrides` or \
             remove the shadowed `resolutions` entries."
        ),
        WorkspaceDiagnosticKind::NodeModulesMissing => format!(
            "'{display}' does not exist. Package exports and conditional exports cannot be read, \
             framework plugins that activate on an installed package stay inactive, and \
             dependency classification degrades, so imports and dependencies can be \
             misreported. Run npm install / pnpm install / yarn / bun install first."
        ),
        WorkspaceDiagnosticKind::BoundariesNotConfigured => {
            "No architecture boundaries are configured, so the boundary detector did not run and \
             its violation counts are zero because nothing was measured. Add `boundaries` to the \
             config, or set `boundary-violation` to off to state that the check is not wanted."
                .to_string()
        }
        WorkspaceDiagnosticKind::RulePacksNotConfigured => {
            "No rule packs are configured, so the policy detector did not run and its violation \
             counts are zero because nothing was measured. Add `rulePacks` to the config, or set \
             `policy-violation` to off to state that the check is not wanted."
                .to_string()
        }
        WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern,
            file_count,
            directory_count,
        } => {
            // `path` is a location, and an empty string is not one: a built-in
            // that matched a file sitting directly at the analysis root
            // anchors at the root itself.
            let display = if display.is_empty() {
                ".".to_owned()
            } else {
                display
            };
            // The payload carries no directory list, so the message names the
            // one directory `path` anchors at. With several excluded
            // directories that is the largest group and NOT a majority, so the
            // sentence says which claim it is making and how many directories
            // it is leaving unnamed.
            let location = if *directory_count > 1 {
                format!(
                    "Skipped {file_count} source files across {directory_count} directories, \
                     the largest group under '{display}'"
                )
            } else if *file_count == 1 {
                format!("Skipped 1 source file under '{display}'")
            } else {
                format!("Skipped {file_count} source files under '{display}'")
            };
            let singular = *file_count == 1 && *directory_count <= 1;
            let (subject, effect) = if singular {
                ("it matches", "it imports, exports, or defines")
            } else {
                ("they match", "they import, export, or define")
            };
            // Only a directory-shaped built-in is lifted by re-rooting. Telling
            // a user with a `vendor/lib.min.js` to run `fallow --root vendor`
            // hands them a command that excludes the same file again.
            let remedy = if glob_first_literal_segment(pattern).is_some() {
                format!(
                    "Move first-party source out of the matched directory, or analyze that \
                     directory on its own with fallow --root {display}."
                )
            } else {
                "This pattern matches a file name rather than a directory, so re-running under \
                 a different --root excludes the same files again. Rename first-party source \
                 that only looks generated, dropping the '.min' or '.bundle' infix."
                    .to_owned()
            };
            format!(
                "{location}: {subject} fallow's built-in ignore pattern '{pattern}', so nothing \
                 {effect} is visible to this run. Built-in ignores cannot be switched off \
                 through ignorePatterns. {remedy}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_large_file_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("src/vendor/app.bundle.js"),
            WorkspaceDiagnosticKind::SkippedLargeFile {
                size_bytes: 6 * 1024 * 1024,
            },
        );
        assert_eq!(diag.kind.id(), "skipped-large-file");
        assert!(
            diag.message.contains("src/vendor/app.bundle.js"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message.contains("6.0 MB"),
            "message reports the size: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--max-file-size"),
            "message names the override flag: {}",
            diag.message
        );
    }

    #[test]
    fn skipped_minified_file_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("src/assets/index-abc123.js"),
            WorkspaceDiagnosticKind::SkippedMinifiedFile {
                size_bytes: 2 * 1024 * 1024,
            },
        );
        assert_eq!(diag.kind.id(), "skipped-minified-file");
        assert!(
            diag.message.contains("src/assets/index-abc123.js"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message.contains("2.0 MB"),
            "message reports the size: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--max-file-size 0"),
            "message names the opt-out: {}",
            diag.message
        );
    }

    #[test]
    fn skipped_source_dotdir_diagnostic_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join(".claude"),
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
        );
        assert_eq!(diag.kind.id(), "skipped-source-dotdir");
        assert!(
            diag.message.contains(".claude"),
            "message names the project-relative path: {}",
            diag.message
        );
        assert!(
            diag.message
                .contains("Its imports and exports are not analyzed."),
            "message states the consequence: {}",
            diag.message
        );
        assert!(
            diag.message.contains("--root"),
            "message names the real remedy: {}",
            diag.message
        );
        assert!(
            diag.message.contains("ignorePatterns"),
            "message names the silencing route: {}",
            diag.message
        );
        assert!(
            diag.message.contains("no config field"),
            "the message must say plainly that no config field traverses it: {}",
            diag.message
        );
        assert_eq!(
            serde_json::to_value(&diag).expect("serializes")["kind"],
            "skipped-source-dotdir",
            "id() must byte-match the serde kebab-case tag"
        );
    }

    #[cfg(feature = "schema")]
    #[test]
    fn workspace_diagnostic_schema_includes_skipped_source_dotdir() {
        let schema = schemars::schema_for!(WorkspaceDiagnostic);
        let json = serde_json::to_string(&schema).expect("schema serializes");
        assert!(json.contains("skipped-source-dotdir"));
    }

    #[test]
    fn source_read_failure_serializes_typed_error_payload() {
        let root = Path::new("/project");
        let diagnostic = WorkspaceDiagnostic::new(
            root,
            root.join("src/removed.ts"),
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: "No such file or directory".to_string(),
            },
        );

        let json = serde_json::to_value(&diagnostic).expect("diagnostic serializes");
        assert_eq!(json["kind"], "source-read-failure");
        assert_eq!(
            json["path"],
            root.join("src/removed.ts")
                .display()
                .to_string()
                .replace('\\', "/")
        );
        assert_eq!(json["error"], "No such file or directory");
        assert!(
            json["message"]
                .as_str()
                .is_some_and(|message| message.contains("src/removed.ts"))
        );
    }

    #[cfg(feature = "schema")]
    #[test]
    fn workspace_diagnostic_schema_includes_source_read_failure() {
        let schema = schemars::schema_for!(WorkspaceDiagnostic);
        let json = serde_json::to_string(&schema).expect("schema serializes");
        assert!(json.contains("source-read-failure"));
        assert!(json.contains("error"));
    }

    #[test]
    fn bun_lockb_override_resolution_skipped_id_and_message() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("package.json"),
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        );
        assert_eq!(diag.kind.id(), "bun-lockb-override-resolution-skipped");
        assert!(
            diag.message.contains("'package.json'"),
            "message names the project-relative manifest: {}",
            diag.message
        );
        assert!(
            diag.message.contains("no parseable text lockfile"),
            "message states the cause: {}",
            diag.message
        );
        assert!(
            !diag.message.contains("only bun.lockb"),
            "message must not claim bun.lockb is the only lockfile; yarn.lock or an unparseable \
             bun.lock may sit beside it: {}",
            diag.message
        );
        assert!(
            diag.message.contains("bun install --save-text-lockfile")
                && diag.message.contains("delete the stale bun.lockb"),
            "message ends with the text-lockfile next step and the stale-lockb alternative: {}",
            diag.message
        );
        let json = serde_json::to_value(&diag).expect("diagnostic serializes");
        assert_eq!(json["kind"], "bun-lockb-override-resolution-skipped");
    }

    #[test]
    fn bun_override_diagnostic_ids_and_messages_are_actionable() {
        let root = Path::new("/project");
        let malformed = WorkspaceDiagnostic::new(
            root,
            root.join("bun.lock"),
            WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
        );
        assert_eq!(malformed.kind.id(), "bun-lock-override-resolution-skipped");
        assert!(malformed.message.contains("regenerate"));

        let shadowed = WorkspaceDiagnostic::new(
            root,
            root.join("package.json"),
            WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        );
        assert_eq!(shadowed.kind.id(), "bun-resolutions-shadowed-by-overrides");
        assert!(shadowed.message.contains("ignores `resolutions`"));
    }

    #[test]
    fn into_root_relative_strips_the_root_and_keeps_outside_paths_absolute() {
        let root = Path::new("/project");
        let inside = WorkspaceDiagnostic::new(
            root,
            root.join("packages/inner"),
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
        )
        .into_root_relative(root);
        assert_eq!(inside.path, Path::new("packages/inner"));

        let outside = WorkspaceDiagnostic::new(
            root,
            PathBuf::from("/elsewhere/packages/inner"),
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
        )
        .into_root_relative(root);
        assert_eq!(outside.path, Path::new("/elsewhere/packages/inner"));
    }

    #[test]
    fn analysis_stage_classification_covers_only_analyze_stage_kinds() {
        let analysis_stage = [
            WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml {
                error: "bad yaml".to_owned(),
            },
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        ];
        for kind in &analysis_stage {
            assert!(
                kind.is_analysis_stage() && !kind.is_source_discovery(),
                "{} is recorded by the analyze stage only",
                kind.id()
            );
        }

        let other = [
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
            WorkspaceDiagnosticKind::MalformedPackageJson {
                error: "trailing comma".to_owned(),
            },
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "packages/*".to_owned(),
            },
            WorkspaceDiagnosticKind::MalformedTsconfig {
                error: "unexpected token".to_owned(),
            },
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: "permission denied".to_owned(),
            },
        ];
        for kind in &other {
            assert!(
                !kind.is_analysis_stage(),
                "{} is a discovery kind, not an analyze-stage kind",
                kind.id()
            );
        }
    }

    #[test]
    fn merge_keeps_two_diagnostics_that_share_a_kind_id_and_path() {
        let root = Path::new("/project");
        let first = WorkspaceDiagnostic::new(
            root,
            root.join("packages/aaa"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "packages/*".to_owned(),
            },
        );
        let second = WorkspaceDiagnostic::new(
            root,
            root.join("packages/aaa"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "packages/a*".to_owned(),
            },
        );

        let merged =
            merge_workspace_diagnostics(vec![first.clone(), second.clone()], vec![first, second]);

        let patterns: Vec<String> = merged
            .iter()
            .map(|diagnostic| match &diagnostic.kind {
                WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => pattern.clone(),
                other => panic!("unexpected kind {}", other.id()),
            })
            .collect();
        assert_eq!(
            patterns,
            ["packages/*", "packages/a*"],
            "two overlapping globs report the same directory twice, with their own pattern; \
             the same entry seen from two observation points still folds to one"
        );
    }

    /// Issue #2366: a repository that declares one glob in two manifests
    /// (`"./apps/**"` in `package.json`, `apps/**` in `pnpm-workspace.yaml`)
    /// must not report every package-less directory under it twice now that the
    /// payload is part of the dedupe key.
    #[test]
    fn merge_folds_two_spellings_of_one_glob_into_one_diagnostic() {
        let root = Path::new("/project");
        let dotted = WorkspaceDiagnostic::new(
            root,
            root.join("apps/site/.next/cache"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "./apps/**".to_owned(),
            },
        );
        let bare = WorkspaceDiagnostic::new(
            root,
            root.join("apps/site/.next/cache"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "apps/**".to_owned(),
            },
        );
        assert_eq!(
            dotted.kind, bare.kind,
            "the no-op ./ prefix is normalised out of the recorded pattern"
        );
        assert!(
            dotted.message.contains("Glob 'apps/**'"),
            "the message renders the normalised pattern: {}",
            dotted.message
        );

        let merged = merge_workspace_diagnostics(vec![dotted], vec![bare]);
        assert_eq!(
            merged.len(),
            1,
            "one glob declared twice is one diagnostic: {merged:?}"
        );
    }

    /// A glob spelled exactly `"./"` (the project root itself) is the one
    /// pattern the prefix strip must leave alone: an empty `pattern` field
    /// names no glob, and the warning would quote nothing.
    #[test]
    fn new_keeps_a_root_only_glob_spelling_and_still_strips_a_real_prefix() {
        let root = Path::new("/project");
        let recorded = |pattern: &str| {
            let diagnostic = WorkspaceDiagnostic::new(
                root,
                root.join("pkgs"),
                WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                    pattern: pattern.to_owned(),
                },
            );
            let WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } = diagnostic.kind
            else {
                panic!("constructed a glob-matched-no-package-json diagnostic");
            };
            (pattern, diagnostic.message)
        };

        let (root_pattern, root_message) = recorded("./");
        assert_eq!(root_pattern, "./", "a root-only glob keeps its spelling");
        assert!(
            root_message.contains("Glob './'"),
            "the warning names the glob the manifest declared: {root_message}"
        );
        assert_eq!(recorded(".\\").0, ".\\");
        assert_eq!(recorded("./pkgs/*").0, "pkgs/*");
        assert_eq!(recorded(".\\pkgs\\*").0, "pkgs\\*");
    }

    /// Issue #2366, the path half of the same repository shape: expanding
    /// `./pkgs/*` joins the no-op `.` into every match, so the two manifests
    /// hand one directory to the diagnostic under two spellings. Both must
    /// store, render and serialise as the bare one, otherwise whichever
    /// manifest was read first decides whether the analysis envelopes print
    /// `./pkgs/aaa` while the workspace listing envelope prints `pkgs/aaa`.
    #[test]
    fn new_stores_one_spelling_for_a_directory_reached_through_a_dotted_glob() {
        let root = Path::new("/project");
        let dotted = WorkspaceDiagnostic::new(
            root,
            root.join("./pkgs/aaa"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "./pkgs/*".to_owned(),
            },
        );
        let bare = WorkspaceDiagnostic::new(
            root,
            root.join("pkgs/aaa"),
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "pkgs/*".to_owned(),
            },
        );

        let spelling = |diagnostic: &WorkspaceDiagnostic| {
            diagnostic.path.display().to_string().replace('\\', "/")
        };
        assert_eq!(
            spelling(&dotted),
            "/project/pkgs/aaa",
            "the stored path drops the no-op . component, which Path equality \
             hides but serialization does not"
        );
        assert_eq!(spelling(&dotted), spelling(&bare));
        assert_eq!(
            spelling(&dotted.clone().into_root_relative(root)),
            "pkgs/aaa"
        );

        let merged = merge_workspace_diagnostics(vec![dotted], vec![bare]);
        assert_eq!(
            merged.len(),
            1,
            "one directory reached through two spellings of one glob: {merged:?}"
        );
    }

    /// The single-list fold applied at workspace discovery keeps one entry per
    /// `(kind, path)` and leaves distinct payloads alone.
    #[test]
    fn dedupe_keeps_first_of_each_pair_and_every_distinct_payload() {
        let root = Path::new("/project");
        let glob = |pattern: &str, relative: &str| {
            WorkspaceDiagnostic::new(
                root,
                root.join(relative),
                WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                    pattern: pattern.to_owned(),
                },
            )
        };

        let deduped = dedupe_workspace_diagnostics(vec![
            glob("pkgs/*", "pkgs/aaa"),
            glob("pkgs/*", "pkgs/bbb"),
            glob("./pkgs/*", "./pkgs/aaa"),
            glob("pkgs/a*", "pkgs/aaa"),
        ]);

        let reported: Vec<(String, String)> = deduped
            .iter()
            .map(|diagnostic| match &diagnostic.kind {
                WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => (
                    pattern.clone(),
                    diagnostic.path.display().to_string().replace('\\', "/"),
                ),
                other => panic!("unexpected kind {}", other.id()),
            })
            .collect();

        assert_eq!(
            reported,
            vec![
                ("pkgs/*".to_owned(), "/project/pkgs/aaa".to_owned()),
                ("pkgs/*".to_owned(), "/project/pkgs/bbb".to_owned()),
                ("pkgs/a*".to_owned(), "/project/pkgs/aaa".to_owned()),
            ],
            "the duplicate spelling folds away and the overlapping glob stays"
        );
    }

    /// The class `reachability_caveats[]` is computed from. Every kind here
    /// means the run never read a file that is part of the project, so its
    /// imports credit nothing and the modules it imports can be reported
    /// unused with a removal action on them. Classifying a kind `true` is the
    /// only wiring its findings need to inherit the caveat and the `fallow fix`
    /// withholding that follows it.
    #[test]
    fn source_never_analyzed_covers_every_file_the_run_did_not_read() {
        for kind in [
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
            WorkspaceDiagnosticKind::SourceReadFailure {
                error: "permission denied".to_owned(),
            },
        ] {
            assert!(
                kind.source_never_analyzed(),
                "{} names a source file this run never read",
                kind.id()
            );
        }

        let degraded = WorkspaceDiagnosticKind::SourceParseDegraded {
            error_count: 3,
            panicked: false,
        };
        assert!(
            !degraded.source_never_analyzed(),
            "a degraded parse read the file, so it has a graph node and its reachability is \
             observable; the caveat pass narrows it instead of treating it as unread"
        );

        for kind in [
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
            WorkspaceDiagnosticKind::MalformedPackageJson {
                error: "trailing comma".to_owned(),
            },
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: "packages/*".to_owned(),
            },
            WorkspaceDiagnosticKind::MalformedTsconfig {
                error: "unexpected token".to_owned(),
            },
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
            WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml {
                error: "bad indent".to_owned(),
            },
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
            WorkspaceDiagnosticKind::NodeModulesMissing,
            WorkspaceDiagnosticKind::BoundariesNotConfigured,
            WorkspaceDiagnosticKind::RulePacksNotConfigured,
        ] {
            assert!(
                !kind.source_never_analyzed(),
                "{} says nothing about a source file's imports going unseen",
                kind.id()
            );
        }
    }

    #[test]
    fn source_walk_recorded_covers_only_the_kinds_a_walk_replaces() {
        for kind in [
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedSourceDotdir,
        ] {
            assert!(
                kind.is_source_walk_recorded() && kind.is_source_discovery(),
                "{} is written by the source walk",
                kind.id()
            );
        }

        let read_failure = WorkspaceDiagnosticKind::SourceReadFailure {
            error: "permission denied".to_owned(),
        };
        assert!(
            read_failure.is_source_discovery() && !read_failure.is_source_walk_recorded(),
            "the parse stage records source-read-failure after the walk, so it must keep \
             reaching sessions through the registry"
        );

        for kind in [
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
            WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
        ] {
            assert!(
                !kind.is_source_walk_recorded(),
                "{} is not written by the source walk",
                kind.id()
            );
        }
    }

    /// Issue #2638, the single most load-bearing classification in the new
    /// kind. Answering `true` here would attach `IncompleteFileAnalysis` and
    /// `IncompleteImportGraph` caveats to findings on nearly every project
    /// that keeps a non-gitignored `dist/` or `coverage/`, and make
    /// `fallow fix` withhold `delete-file` and `remove-export` project-wide.
    /// A built-in exclusion is designed behavior on generated output, not a
    /// degraded run.
    #[test]
    fn a_built_in_ignore_exclusion_is_not_a_file_the_run_failed_to_analyze() {
        let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/build/**".to_owned(),
            file_count: 3,
            directory_count: 1,
        };
        assert!(!kind.source_never_analyzed());
    }

    /// Issue #2638: these exclusions fire in the product's default state on
    /// most monorepos, so a default stderr line would be permanent noise that
    /// names no defect. The CLI prints a note under `--explain-skipped`
    /// instead.
    #[test]
    fn a_built_in_ignore_exclusion_does_not_warn_on_stderr_by_default() {
        let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/build/**".to_owned(),
            file_count: 3,
            directory_count: 1,
        };
        assert!(!kind.warns_on_stderr());
    }

    /// Issue #2638 plus issue #2366: the walk writes it, so it has to be
    /// classified as source-discovery (or combined mode's per-analysis config
    /// reloads wipe it before serialization) AND as walk-recorded (or a
    /// concurrent walk's tally is folded into another analysis's list).
    #[test]
    fn a_built_in_ignore_exclusion_is_walk_recorded_source_discovery() {
        let kind = WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
            pattern: "**/build/**".to_owned(),
            file_count: 3,
            directory_count: 1,
        };
        assert!(kind.is_source_discovery());
        assert!(kind.is_source_walk_recorded());
        assert!(!kind.is_analysis_stage());
        assert_eq!(kind.id(), "excluded-by-default-ignore");
    }

    /// Issue #2638: the message has to name the pattern the reader cannot see,
    /// the directory, and the only remedy that actually analyzes the tree.
    /// `ignorePatterns` is not that remedy: the compiled set unions, so it
    /// cannot negate a built-in.
    #[test]
    fn a_built_in_ignore_exclusion_message_names_the_pattern_and_the_root_remedy() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("packages/web/build"),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/build/**".to_owned(),
                file_count: 4,
                directory_count: 1,
            },
        );
        assert!(diag.message.contains("**/build/**"), "{}", diag.message);
        assert!(
            diag.message.contains("packages/web/build"),
            "{}",
            diag.message
        );
        assert!(
            diag.message.contains("fallow --root packages/web/build"),
            "the remedy is copy-pasteable: {}",
            diag.message
        );
        assert!(
            diag.message
                .contains("cannot be switched off through ignorePatterns"),
            "the message must not advertise a negation that does not exist: {}",
            diag.message
        );
    }

    /// One excluded file reads as one file, not as "1 source files".
    #[test]
    fn a_single_excluded_file_message_is_singular() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("dist"),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/dist/**".to_owned(),
                file_count: 1,
                directory_count: 1,
            },
        );
        assert!(
            diag.message
                .starts_with("Skipped 1 source file under 'dist'"),
            "{}",
            diag.message
        );
        assert!(diag.message.contains("it matches"), "{}", diag.message);
        assert!(
            diag.message
                .contains("nothing it imports, exports, or defines"),
            "the whole sentence agrees in number, not just its first clause: {}",
            diag.message
        );
    }

    /// The anchor directory is the largest group, never a majority: ten
    /// packages each holding one excluded file make every one of them "the
    /// largest", and a message claiming otherwise is false on exactly the flat
    /// monorepo shape issue #2638 is about.
    #[test]
    fn a_scattered_exclusion_names_the_largest_group_and_counts_the_directories() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("packages/a/dist"),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/dist/**".to_owned(),
                file_count: 10,
                directory_count: 10,
            },
        );
        assert!(
            diag.message.starts_with(
                "Skipped 10 source files across 10 directories, the largest group under \
                 'packages/a/dist'"
            ),
            "{}",
            diag.message
        );
        assert!(
            !diag.message.contains("the most of them"),
            "a max-of-group is not a majority: {}",
            diag.message
        );
    }

    /// A file-shaped built-in matches on the file name, so the `--root` remedy
    /// the directory-shaped patterns get would re-exclude the same file. The
    /// message must not print a command that provably does nothing.
    #[test]
    fn a_file_shaped_pattern_does_not_advertise_the_root_remedy() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("vendor"),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/*.min.js".to_owned(),
                file_count: 2,
                directory_count: 1,
            },
        );
        assert!(
            !diag.message.contains("fallow --root"),
            "the message explains why re-rooting fails, it does not prescribe it: {}",
            diag.message
        );
        assert!(
            diag.message.contains("matches a file name"),
            "the message says why: {}",
            diag.message
        );
        assert!(
            diag.message.contains("Rename"),
            "and names the remedy that does work: {}",
            diag.message
        );
    }

    /// A built-in that matched a file sitting directly at the analysis root
    /// anchors at the root, and an empty string is not a location.
    #[test]
    fn a_root_anchored_exclusion_renders_its_location_as_dot() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.to_path_buf(),
            WorkspaceDiagnosticKind::ExcludedByDefaultIgnore {
                pattern: "**/*.min.js".to_owned(),
                file_count: 1,
                directory_count: 1,
            },
        );
        assert!(
            diag.message.starts_with("Skipped 1 source file under '.'"),
            "{}",
            diag.message
        );
    }

    #[test]
    fn glob_first_literal_segment_skips_wildcards_and_dot_components() {
        assert_eq!(glob_first_literal_segment("**/build/**"), Some("build"));
        assert_eq!(glob_first_literal_segment("./dist/**"), Some("dist"));
        assert_eq!(glob_first_literal_segment("**/*.min.js"), None);
        assert_eq!(glob_first_literal_segment("**/*.bundle.js"), None);
        assert_eq!(glob_first_literal_segment("**/{a,b}/**"), None);
    }

    #[test]
    fn format_size_mb_one_decimal() {
        assert_eq!(format_size_mb(0), "0.0 MB");
        assert_eq!(format_size_mb(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size_mb(1024 * 1024 + 512 * 1024), "1.5 MB");
    }

    #[test]
    fn undeclared_workspace_message_has_next_step() {
        let root = Path::new("/project");
        let diag = WorkspaceDiagnostic::new(
            root,
            root.join("packages/legacy"),
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
        );
        assert_eq!(diag.kind.id(), "undeclared-workspace");
        assert!(diag.message.contains("packages/legacy"), "{}", diag.message);
        assert!(
            diag.message.contains("ignorePatterns"),
            "next-step hint preserved: {}",
            diag.message
        );
    }
}
