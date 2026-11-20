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

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::path_util::display_relative;
use crate::serde_path;

#[path = "workspace_config_entry.rs"]
mod config_entry;
#[path = "workspace_health_text.rs"]
mod health_text;
#[path = "workspace_kind_class.rs"]
mod kind_class;
#[path = "workspace_merge.rs"]
mod merge;
#[path = "workspace_plugin_text.rs"]
mod plugin_text;
#[path = "workspace_production_text.rs"]
mod production_text;
#[path = "workspace_run_notices.rs"]
mod run_notices;

pub use config_entry::{OVERRIDE_REASON_BASE_RULE_OFF, OVERRIDE_REASON_PROJECT_LEVEL};
pub use merge::{dedupe_workspace_diagnostics, merge_workspace_diagnostics};

/// Why the declared pnpm version ignores the `overrides` section of
/// `pnpm-workspace.yaml`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum PnpmWorkspaceOverridesIgnoredCause {
    /// pnpm 10 or earlier, and the root `package.json` declares non-empty
    /// `pnpm.overrides` or `resolutions`. pnpm uses these entries in place
    /// of the `pnpm-workspace.yaml` overrides.
    PackageJsonOverrides,
    /// pnpm before 10.5.1, which does not read the `overrides` section of
    /// `pnpm-workspace.yaml`.
    PnpmVersion,
}

/// Whether the project had a `node_modules` directory at a point in time.
///
/// A dead-code baseline records this when it is saved, so a later run in a
/// different install state can say that its results can differ.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum NodeModulesState {
    /// `node_modules` existed, or the project legitimately runs without it.
    Present,
    /// `node_modules` did not exist.
    Missing,
}

impl NodeModulesState {
    /// The wire token, as it appears in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Missing => "missing",
        }
    }
}

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
    /// A workspace entry is an absolute path or has `..` segments that lead
    /// above the project root, so it can name directories outside the root. Fallow skips the entry
    /// before it reads the disk and analyzes no package from it. `path` is
    /// the file that declares the entry (`package.json`,
    /// `pnpm-workspace.yaml`, `deno.json` or `deno.jsonc`), or the project
    /// root for an entry of the fallow config `workspaces.patterns`. The
    /// diagnostic never names a directory outside the root.
    ///
    /// npm, pnpm and yarn accept such an entry, so fallow does not exit with
    /// an error on it. The project root is the security boundary of a run.
    WorkspaceGlobOutsideRoot {
        /// The workspace entry, as written in the file.
        pattern: String,
    },
    /// A positive workspace pattern names a directory that does not exist.
    /// The check reads the literal part of the pattern before the first
    /// segment with a glob character (`vite` for `vite/packages/*`). A pattern
    /// without a literal part (`**/pkg`) never gets this diagnostic, and a
    /// literal directory that exists but holds no package does not get it
    /// either. `path` is the file that declares the pattern, or the project
    /// root for an entry of the fallow config `workspaces.patterns`.
    ///
    /// A repository that clones other repositories into the workspace in CI
    /// declares such patterns. The packages there can consume catalog
    /// entries, so the run does not report `unused-catalog-entries` while a
    /// pattern of this kind exists. For a `packages` entry of the
    /// `pnpm-workspace.yaml` of a nested pnpm project, the pattern is
    /// relative to that file, and the run does not report the unused entries
    /// of that catalog only.
    WorkspacePatternDirMissing {
        /// The workspace pattern, as written in the file.
        pattern: String,
    },
    /// A directory has its own `package.json` and `pnpm-workspace.yaml`, and
    /// no workspace pattern of the root manifests declares it. pnpm installs
    /// it as an independent project. Fallow analyzes it as a workspace, so its
    /// `package.json` owns its files and dependencies. The members that the
    /// `packages` list of its `pnpm-workspace.yaml` declares are workspaces
    /// too. The `catalog:` references of the project and its members resolve
    /// against its own catalog, not against the root catalog.
    ///
    /// Information only: nothing is lost from the analysis, so
    /// `degrades_analysis` is false.
    NestedProjectRoot,
    /// The root `tsconfig.json`, or a config in its `extends` chain, exists
    /// but failed to parse. `path` names the file that failed. Fallow ignores
    /// the project references, path aliases and compiler options of that file.
    MalformedTsconfig {
        /// JSONC parse error text.
        error: String,
    },
    /// `tsconfig.json` lists a `references[].path` that does not point to an
    /// existing directory.
    TsconfigReferenceDirMissing,
    /// `pnpm-workspace.yaml` exists but failed to parse as YAML. The catalog
    /// checks are skipped and dependency-override analysis proceeds without
    /// the file's entries (degraded) until the syntax is fixed.
    MalformedPnpmWorkspaceYaml {
        /// YAML parse error text.
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
    /// analysis. A file, export or dependency that only the directory uses can
    /// be reported as unused. A `!<dir>/**` entry in `ignorePatterns` adds the
    /// directory to traversal (issue #2452). Or add the file to `entry`, the
    /// export to `ignoreExports` or the dependency to `ignoreDependencies` to
    /// stop that false positive, or add the directory to `ignorePatterns` to
    /// silence this (issue #461). Running
    /// fallow with `--root` against the directory analyzes it on its own and
    /// does not fix the main run (issue #2797).
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
    /// Unlike the two skipped-file kinds beside it, this one is capped. To
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
    ///
    /// A file that `ignoreFindings` covers is not reported. The user marked it
    /// as broken on purpose, so its partial parse also adds no caveat to other
    /// findings and does not count for the parse-error gate.
    SourceParseDegraded {
        /// Number of parser diagnostics reported for the file.
        error_count: u32,
        /// `true` when the parser abandoned the file instead of recovering, so
        /// the extracted module is a fragment at best.
        panicked: bool,
    },
    /// A source file nests brackets deeper than the parser cap, or needs more
    /// stack than fallow reserves, so it was not parsed at all. The file can be
    /// valid code, so this is not reported as a syntax error, but it degrades
    /// the run in the same way: its imports credit nothing. It counts for the
    /// parse-error gate and adds the same caveats as `source-parse-degraded`.
    SourceNestingTooDeep {
        /// The bracket nesting cap of the pre-parse scan.
        limit: u32,
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
    /// Dependency-override resolution was skipped because `pnpm-lock.yaml`
    /// exists but could not be parsed, for example because it still holds
    /// unresolved merge-conflict markers, and no other parseable lockfile was
    /// available as independent resolution ground truth.
    PnpmLockOverrideResolutionSkipped,
    /// Dependency-override resolution was skipped because `package-lock.json`
    /// or `npm-shrinkwrap.json` exists but could not be parsed, for example
    /// because it still holds unresolved merge-conflict markers, and no other
    /// parseable lockfile was available as independent resolution ground
    /// truth.
    NpmLockOverrideResolutionSkipped,
    /// A bun manifest declares both `overrides` and a non-empty `resolutions`
    /// object. Bun applies `overrides` and ignores `resolutions`, so fallow
    /// reports the shadowed configuration without offering removal advice.
    BunResolutionsShadowedByOverrides,
    /// The declared pnpm version ignores the `overrides` section of
    /// `pnpm-workspace.yaml`. pnpm does not print a warning. Fallow reports
    /// no findings for the ignored entries. `cause` tells why pnpm ignores
    /// the section.
    PnpmWorkspaceOverridesIgnored {
        /// Why pnpm ignores the section. The cause decides the remedy.
        cause: PnpmWorkspaceOverridesIgnoredCause,
    },
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
    /// This is the unconfigured zero, not the user-chosen one: a project that
    /// sets `boundary-violation: off` asked for silence and can see that
    /// choice in `fallow config`. A project that left `boundaries` empty
    /// cannot distinguish "no violations" from "nothing was measured".
    BoundariesNotConfigured,
    /// `rulePacks` is empty while `policy-violation` is not `off`, so the
    /// policy detector never ran and its summary counters are structurally
    /// zero. The unconfigured counterpart of
    /// [`Self::BoundariesNotConfigured`].
    RulePacksNotConfigured,
    /// `client-server-leak` is not `off`, but no analyzed file has a
    /// top-level `"use client"` directive and no framework plugin supplies a
    /// client root. The detector starts only from those files, so it had no
    /// starting point and its zero count measures nothing. A plain Vite SPA
    /// is a typical project of this shape. The React Router plugin supplies
    /// its route modules and `.client` modules as client roots.
    ///
    /// Like [`Self::BoundariesNotConfigured`], an analyze pass with the rule
    /// `off` does not report it. `fallow security` raises a top-level `off`
    /// to `warn` before the pass, so that command always reports it.
    ClientServerLeakNoClientRoots,
    /// In production mode, a built-in production pattern (for example
    /// `**/test/**`) excluded at least one source file that a production file
    /// imports (issue #4498). Fallow does not analyze an excluded file, so an
    /// export or a parameter that only this file uses can show up in a
    /// finding.
    ///
    /// One entry per pattern. `path` is the first excluded file in path
    /// order. The dead-code pass records the entry from the resolved imports
    /// of the analyzed files. It reads no excluded file. The production
    /// patterns do not change.
    ProductionExcludedFileImported {
        /// The built-in production pattern that matched, verbatim.
        pattern: String,
        /// Excluded files that this pattern matched and that a production
        /// file imports. Exact.
        file_count: u32,
        /// The first excluded files in path order, project-root-relative
        /// with forward slashes. Capped at five entries.
        files: Vec<String>,
    },
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
    /// match it is the deepest segment the pattern matched
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
    ///   no pattern here. A `!` entry in `ignorePatterns` lifts a built-in for
    ///   the paths it matches (issue #2940), and a lifted file is discovered,
    ///   so it is never counted here.
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
    /// The walk finished with no source file to analyze at all, so every
    /// finding count this run reports is zero because nothing was measured
    /// rather than because the project is clean (issue #2686).
    ///
    /// Distinct from [`Self::ExcludedByDefaultIgnore`], which reports one
    /// pattern's exclusions and is designed behavior on generated output. The
    /// alarm is not the exclusion, it is having nothing left afterwards, and
    /// that condition also fires with no exclusion at all: a docs-only
    /// repository, a workspace member with no TypeScript, or a path filter that
    /// matched nothing. `excluded_file_count` names the built-in-ignore
    /// contribution so the common cause is still attributable, and is `0` when
    /// no built-in pattern took part.
    ///
    /// This is the kind a CI consumer reads to tell "measured zero" from
    /// "measured nothing": the human report has said so since 3.26.0, but only
    /// in human format and only under the built-in-ignore cause, so `--quiet
    /// --format json` saw a clean green either way.
    NoSourceFilesAnalyzed {
        /// Candidate source files the built-in ignore patterns removed from
        /// this walk, summed across every pattern. `0` when the walk found no
        /// candidate to exclude in the first place.
        excluded_file_count: u32,
    },
    /// Per-file health scoring failed, so the score list is empty and the
    /// scored-file count is `0` because nothing was measured rather than
    /// because the project has no files worth scoring. Every score-derived
    /// number (the average maintainability index, the refactoring targets, the
    /// hotspot complexity half) is then structurally zero (issue #2689).
    FileScoresUnavailable {
        /// Scoring error text.
        error: String,
    },
    /// Churn-based hotspot analysis was skipped, so the hotspots, churn and
    /// ownership sections report nothing at all. The remaining health sections
    /// are unaffected.
    HotspotsSkipped {
        /// Which input stopped it, as a kebab-case token: `not-a-repository`,
        /// `no-commits`, `invalid-since` or `churn-file-unreadable`. The set is
        /// open.
        ///
        /// The cause decides the remedy, which is why it is on the wire: a run
        /// outside a repository is fixed by running fallow inside one, a
        /// branch without a commit by committing, a malformed `--since` by
        /// respelling the flag, and a churn file that changed under the run by
        /// rerunning it. A consumer reading only the kind would offer the first
        /// remedy for all four.
        cause: String,
    },
    /// Change coupling found no pairs because the history cannot show them.
    /// The `change_coupling` section is present with an empty `pairs` list,
    /// and this entry tells why the list is empty.
    ChangeCouplingSkipped {
        /// Kebab-case token: `shallow-clone` (the fetched history is
        /// incomplete) or `too-few-commits` (fewer analyzed commits than the
        /// minimum revisions of one file). The set is open.
        cause: String,
    },
    /// The repository is a shallow clone, so churn is measured over the fetched
    /// history only and every hotspot figure is incomplete.
    ShallowClone {
        /// `true` when the run also asked for ownership attribution, which a
        /// shallow clone skews further by inflating single-author dominance.
        ownership_requested: bool,
    },
    /// No commit timestamp was available, so churn recency and ownership
    /// staleness were measured against the wall clock and drift between two
    /// runs over the same commit.
    UnpinnedClock,
    /// Ownership attribution was requested but its inputs did not load, so
    /// hotspot entries carry degraded or absent owner signals.
    OwnershipUnavailable {
        /// Which input failed, as a kebab-case token: `invalid-bot-pattern` or
        /// `codeowners-parse-failed`. The set is open.
        cause: String,
        /// Underlying error text.
        error: String,
    },
    /// A saved health snapshot could not be read or parsed, so the trend is
    /// computed over fewer snapshots than the project has on disk and a
    /// direction can flip on the missing point alone.
    TrendSnapshotUnreadable {
        /// Filesystem or JSON error text.
        error: String,
    },
    /// A grouped health run asked for a trend, but the baseline snapshot holds
    /// no group data that matches this run, so the groups carry no trend. The
    /// project trend is not affected. `path` names the baseline snapshot.
    TrendGroupBaselineUnavailable {
        /// Why the groups have no baseline, as a kebab-case token:
        /// `snapshot-has-no-groups` or `grouped-by-mismatch`. The set is open.
        cause: String,
    },
    /// A framework plugin read a build config and could not read one of its
    /// keys in full, so part of what the key declares never reached the
    /// analysis. `path` names the config file.
    ///
    /// The reader is syntactic, so a key whose value is computed at build time
    /// is invisible to it: a Module Federation `exposes: makeExposes()` or a
    /// `remotes` map spread from an environment module declares entries this
    /// run does not know about. The consequence is a finding, not a missing
    /// number: an unread `exposes` target is not registered as an entry point
    /// and its file can surface as `unused-file`, and an unread `remotes` alias
    /// is not treated as provided by a remote container and its import can
    /// surface as an unlisted dependency.
    ///
    /// Recorded by the plugin stage, which runs before analysis and is not
    /// cached, so the entry is present on a warm cache too. A bare
    /// `tracing::warn!` from inside the plugin reaches no envelope and no CI
    /// consumer (issue #2736).
    ///
    /// A source file that calls the Module Federation runtime API gets the same
    /// entry: `path` names the source file, `key` names the runtime function
    /// (`registerRemotes`, `loadRemote`, `init`, `createInstance`) and
    /// `reason` is `dynamic-argument` when the call receives a value that is
    /// not a static literal. The analysis records it from the facts of the
    /// parse, which a warm cache restores (issue #2795). A `.vue` or `.svelte`
    /// file gets it for a call in its `<script>` blocks (issue #2876).
    PluginConfigUnreadable {
        /// The plugin that read the config, as it labels itself:
        /// `module-federation` for a standalone `module-federation.config.*`,
        /// or the bundler plugin (`webpack`, `rspack`, `rsbuild`, `vite`) that
        /// read the same options inline from its own config.
        plugin: String,
        /// The config key that was present and not fully readable (`exposes`,
        /// `remotes`), or the Module Federation runtime function whose
        /// argument was not readable (`registerRemotes`, `loadRemote`, `init`,
        /// `createInstance`). The set is open.
        key: String,
        /// Why it could not be read, as a kebab-case token:
        /// `not-object-literal`, `array-form`, `spread`,
        /// `unreadable-entries`, `unrecognized-call`,
        /// `import-target-unreadable` or `dynamic-argument`. The set is open.
        ///
        /// The reason decides the remedy, which is why it is on the wire: a
        /// value that is not an object literal is fixed by writing one, while
        /// unreadable entries are fixed by naming those entries in the config
        /// option the message points at.
        reason: String,
    },
    /// A framework plugin read a config key it understands, does not model
    /// that key's effect, and therefore stood a modeled default down. `path`
    /// names the config file.
    ///
    /// A file can also be the `path`: a Nuxt file that reads `#components` or
    /// `#imports` in a way fallow cannot narrow to names, such as a spread of
    /// a namespace import, has `key` set to that module and `reason` set to
    /// `key-effect-not-modeled`. Every name of the module then counts as used.
    ///
    /// The Nuxt auto-import gate is the case this exists for. With
    /// `autoImports` enabled fallow drops the Nuxt convention entry patterns
    /// so a genuinely unreferenced convention file is reported, and a
    /// `components:` or `imports:` block whose effect it cannot model keeps
    /// them, which silently costs the user the findings they opted in for.
    ///
    /// Deliberately NOT one of the [`Self::warns_on_stderr`] kinds. Nothing
    /// was lost that the run could have measured: the patterns stayed, so
    /// findings are suppressed rather than invented, and a project in this
    /// state would otherwise warn on every run forever with "write different
    /// config" as the only remedy, which is the reason
    /// `boundaries-not-configured` is off stderr as well.
    PluginEffectNotModeled {
        /// The plugin that read the config, as it labels itself (`nuxt`).
        plugin: String,
        /// The config key whose effect is not modeled (`components`,
        /// `imports`), or the virtual module a file reads (`#components`,
        /// `#imports`). The set is open.
        key: String,
        /// Why the effect is not modeled, as a kebab-case token:
        /// `key-effect-not-modeled` when the key's own value is the reason,
        /// `config-property-unreadable` when a top-level property of the same
        /// config file could not be read statically, so no surface in it can
        /// be classified at all. The set is open.
        reason: String,
    },
    /// Test coverage was auto-detected on disk rather than passed with
    /// `--coverage`, and `path` names the file that fed the CRAP scores.
    ///
    /// Deliberately NOT one of the [`Self::warns_on_stderr`] kinds: nothing
    /// degraded, the run measured exactly what it found. It is provenance, and
    /// it is on the wire because a score computed against a file the user did
    /// not name is not reproducible and nothing else says which file it was.
    CoverageAutoDetected,
    /// `fallow flags --retirement` asked for flag age, but the repository is
    /// a shallow clone. Blame and pickaxe see the fetched history only, so
    /// every age would be too young. The report gives no age.
    FlagAgeShallowClone,
    /// `fallow flags --retirement` asked for flag age, but git history is
    /// not available. The report gives no age.
    FlagAgeUnavailable {
        /// Why no history is available, as a kebab-case token:
        /// `not-a-repository` or `no-commits`. The set is open.
        cause: String,
    },
    /// A glob in `ignoreDependencies` matched no dependency that a
    /// `package.json` of the project declares, so the entry had no effect in
    /// this run. The usual cause is a typo in the scope or the name.
    ///
    /// The dead-code run records it only when the unused-dependency check ran
    /// and the run reports dependency findings. A run filtered to other issue
    /// types (`--unused-files`) or scoped to files (`--file`) does not record
    /// it. An exact-name entry never produces it. `path` is the project root.
    ///
    /// Not a degraded run: the analysis measured what it was asked to, so
    /// `degrades_analysis` is false and no finding carries a caveat.
    IgnoreDependenciesGlobUnmatched {
        /// The `ignoreDependencies` entry, as written in the config.
        pattern: String,
    },
    /// A pattern in `ignoreFindings` matched no finding of the dead-code run,
    /// so the entry had no effect in this run. The usual cause is a typo in
    /// the path.
    ///
    /// The pattern is compared with every finding the analysis produced,
    /// before issue-type filters and scope filters, so a filtered run does not
    /// make a pattern look unused. `path` is the project root.
    ///
    /// Not a degraded run, for the same reason as
    /// [`Self::IgnoreDependenciesGlobUnmatched`].
    IgnoreFindingsPatternUnmatched {
        /// The `ignoreFindings` entry, as written in the config.
        pattern: String,
    },
    /// An `entry` pattern matched only files that another source (a
    /// `package.json` field or script, a plugin, `dynamicallyLoaded`) makes
    /// runtime entry points (issue #3824). Not for a pattern that matches no
    /// file, nor a `--production` run. `path` is the root. Not degraded.
    EntryPatternRedundant {
        /// The `entry` pattern, as written in the config.
        pattern: String,
    },
    /// An `ignoreDependencies` entry hid no dependency finding (issue #3824).
    /// A glob that matches no declared dependency is
    /// [`Self::IgnoreDependenciesGlobUnmatched`] instead. Same conditions as
    /// that kind, and not a `--production` run. `path` is the root.
    IgnoreDependenciesEntryRedundant {
        /// The `ignoreDependencies` entry, as written in the config.
        pattern: String,
    },
    /// An `ignorePatterns` glob excluded no file that the run would otherwise
    /// analyze (issue #3824). Never a `!` exception, a glob that names a
    /// hidden directory, or a `--production` run. A glob for build output has
    /// an effect only after a build. `path` is the root. Not degraded.
    IgnorePatternsEntryRedundant {
        /// The `ignorePatterns` glob, as written in the config.
        pattern: String,
    },
    /// A `circularDependencies.allow` entry matched no cycle of the run
    /// (issue #4623): no cycle has exactly these files in this order, from
    /// any start file. The usual cause is a cycle that the code no longer
    /// has, or a path that is not relative to the project root. Not for a
    /// `--production` run, nor a run with the `circular-dependencies` rule
    /// off. `path` is the root. Not degraded.
    CircularDependenciesAllowUnmatched {
        /// The entry as one line: the files joined by `, ` in brackets.
        pattern: String,
        /// The files of the entry, as written in the config.
        files: Vec<String>,
    },
    /// An `overrides` rule cannot change the result (issue #4065). `reason` is
    /// `project-level` (fallow reads only the top-level severity of the rule)
    /// or `base-rule-off` (the top-level rule is off after CLI flags, so its
    /// detector does not run). The token set is open. `path` is the root. Not
    /// degraded, and no finding or exit code changes.
    OverrideRuleIneffective {
        /// The canonical rule name, also when the config uses an alias.
        rule: String,
        /// The `overrides[].files` globs, as written in the config.
        files: Vec<String>,
        /// Why the rule has no effect, as a kebab-case token.
        reason: String,
    },
    /// An explicit issue-type selection asked for a rule that the config sets
    /// to `off`, so that check did not run (issue #4067). One entry per rule.
    /// The opt-in flags that turn their rule on and a run with no selection
    /// never report it. `path` is the root. Not degraded: the run measured
    /// what the config asked for, and no finding or exit code changes.
    RequestedRuleOff {
        /// The canonical rule name in `rules`.
        rule: String,
        /// The canonical CLI selector of the request, such as `--unused-deps`.
        /// A typed API or MCP request reports the same selector.
        flag: String,
    },
    /// The dead-code baseline was saved in a different install state than
    /// this run: one of the two runs had `node_modules` and the other did not.
    /// Findings that need installed packages (dependency, unlisted,
    /// unresolved-import and script-binary findings) can differ, so the run
    /// can report baseline findings as new. `path` is the baseline file.
    ///
    /// Only a baseline that records its install state can report this. A
    /// legacy baseline has no record, so the state is unknown.
    BaselineInstallStateMismatch {
        /// The install state when the baseline was saved.
        baseline_node_modules: NodeModulesState,
        /// The install state of this run.
        run_node_modules: NodeModulesState,
    },
    /// A flag of the request had no effect on the run, for example
    /// `--dupes-baseline` when the config turns the duplication analysis off,
    /// or `--production` on a command that does not read the production mode.
    /// The run goes on. `path` is the root. Not degraded: the run measured
    /// what the config and the command ask for.
    InertFlag {
        /// The flag, as the user spelled it, such as `--dupes-baseline`.
        flag: String,
        /// The analysis that the flag is for: `dead-code`, `duplication` or
        /// `health`.
        analysis: String,
        /// Why the flag has no effect, as a kebab-case token: `off-by-config`
        /// (a config key turns the analysis off) or `not-read-by-command`.
        /// The token set is open.
        reason: String,
        /// The config key that turns the analysis off, for `off-by-config`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    /// The discovered config did not load, validate or give workspace
    /// metadata, so the run used the default config. Only a surface that
    /// keeps running on a broken config, such as the language server, falls
    /// back. `path` is the root.
    ConfigLoadFellBack {
        /// The error of the discovered config.
        error: String,
    },
    /// Two or more config files exist in one directory. The run loaded the
    /// file with the highest precedence and ignored the other one. One entry
    /// per ignored file. `path` is the ignored file.
    ConfigFileShadowed {
        /// The file name that the run loaded.
        used: String,
        /// The file name that the run ignored.
        ignored: String,
    },
    /// The `security.categories` config names a category that fallow does not
    /// know, so the run ignores that entry. `path` is the root.
    UnknownSecurityCategory {
        /// The category id, as written in the config.
        id: String,
    },
    /// An environment variable that the run reads has a value that fallow
    /// cannot read, so the run ignores the variable. The entry holds the name
    /// of the variable and never its value. `path` is the root.
    EnvValueIgnored {
        /// The name of the variable, such as `FALLOW_PRODUCTION`.
        name: String,
    },
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
    /// True when this diagnostic reports a run whose results are degraded:
    /// something the user installed, wrote, or expected did not reach the
    /// analysis. Projected from [`WorkspaceDiagnosticKind::warns_on_stderr`],
    /// which is the same classification that decides whether the CLI prints a
    /// stderr line, so a CI log built from this field and a local non-quiet run
    /// say the same thing.
    ///
    /// Omitted when false, which is what keeps every clean run byte-identical.
    /// The two unconfigured-check kinds answer false on purpose: they fire in
    /// the product's default state on every project that never opted into
    /// boundaries or rule packs, so warning on them would warn forever. So does
    /// `excluded-by-default-ignore`, which is designed behavior on generated
    /// output; the alarm for that case is `no-source-files-analyzed`.
    ///
    /// Read this instead of hardcoding a kind allowlist: a degrading kind added
    /// in a later release then reaches an unchanged consumer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub degrades_analysis: bool,
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
        let degrades_analysis = kind.warns_on_stderr();
        Self {
            path,
            kind,
            message,
            degrades_analysis,
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
        WorkspaceDiagnosticKind::FileScoresUnavailable { error } => {
            WorkspaceDiagnosticKind::FileScoresUnavailable {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::OwnershipUnavailable { cause, error } => {
            WorkspaceDiagnosticKind::OwnershipUnavailable {
                cause,
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::TrendSnapshotUnreadable { error } => {
            WorkspaceDiagnosticKind::TrendSnapshotUnreadable {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::ConfigLoadFellBack { error } => {
            WorkspaceDiagnosticKind::ConfigLoadFellBack {
                error: normalise(error),
            }
        }
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson { pattern } => {
            WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
                pattern: canonical_glob_pattern(pattern),
            }
        }
        WorkspaceDiagnosticKind::WorkspaceGlobOutsideRoot { pattern } => {
            WorkspaceDiagnosticKind::WorkspaceGlobOutsideRoot {
                pattern: canonical_glob_pattern(pattern),
            }
        }
        WorkspaceDiagnosticKind::WorkspacePatternDirMissing { pattern } => {
            WorkspaceDiagnosticKind::WorkspacePatternDirMissing {
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

/// The first segment of a glob that contains no glob metacharacter, so it
/// names a real directory rather than a wildcard.
///
/// Source discovery uses it to decide which directory a built-in ignore
/// pattern excluded a file "at"; `render_message` uses it to decide which
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

/// The catalog entries that a `workspace-pattern-dir-missing` diagnostic
/// keeps out of the run, as the subject of a sentence. `source_display` is
/// the declaring file, relative to the project root.
///
/// A missing directory of a root pattern hides consumers of every catalog. A
/// missing directory of a `packages` entry of a nested `pnpm-workspace.yaml`
/// hides consumers of the catalog of that file only.
#[must_use]
pub fn missing_pattern_dir_skipped_entries(source_display: &str) -> &'static str {
    if source_display.contains('/') && source_display.ends_with("pnpm-workspace.yaml") {
        "Unused entries of the catalog in that file"
    } else {
        "Unused catalog entries"
    }
}

/// The directory part of a workspace pattern: the segments before the first
/// segment with a glob character, joined with `/`.
///
/// `vite/packages/*` gives `vite/packages`, `vite` gives `vite`, and a
/// pattern that starts with a glob segment (`**/pkg`, `{a,b}/*`) gives
/// `None`. A leading `./` and empty or `.` segments are dropped. Workspace
/// discovery uses it to find a pattern whose directory does not exist, and
/// the diagnostic message uses it to name that directory, so both read the
/// same part of the pattern.
#[must_use]
pub fn workspace_pattern_literal_dir(pattern: &str) -> Option<String> {
    let literal: Vec<&str> = pattern
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .take_while(|segment| !segment.contains(['*', '?', '[', ']', '{', '}']))
        .collect();
    if literal.is_empty() {
        None
    } else {
        Some(literal.join("/"))
    }
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
        WorkspaceDiagnosticKind::WorkspaceGlobOutsideRoot { pattern } => {
            let source = if display.is_empty() {
                "the fallow config `workspaces.patterns`".to_owned()
            } else {
                format!("'{display}'")
            };
            format!(
                "Skipped workspace entry '{pattern}' in {source}: it is an absolute path \
                 or its '..' segments lead above the project root, so it can match directories \
                 outside the root. \
                 Fallow analyzes no package from it. Move the package inside the project root, \
                 or run fallow with --root set to a directory that holds both."
            )
        }
        WorkspaceDiagnosticKind::WorkspacePatternDirMissing { pattern } => {
            let source = if display.is_empty() {
                "the fallow config `workspaces.patterns`".to_owned()
            } else {
                format!("'{display}'")
            };
            let dir = workspace_pattern_literal_dir(pattern).unwrap_or_else(|| pattern.clone());
            let skipped = missing_pattern_dir_skipped_entries(&display);
            format!(
                "Workspace pattern '{pattern}' in {source} names directory '{dir}', which does \
                 not exist. Fallow analyzes no package from it. {skipped} are not \
                 reported in this run, because packages in that directory can use them. \
                 Create the directory before the run, or remove the pattern."
            )
        }
        WorkspaceDiagnosticKind::NestedProjectRoot => format!(
            "Directory '{display}' has its own pnpm-workspace.yaml, so it is an independent \
             project. Fallow analyzes it as a workspace: its package.json owns its files and \
             dependencies, the packages that its pnpm-workspace.yaml declares are workspaces, \
             and their catalog references resolve against its own catalog. Add '{display}' to \
             ignorePatterns to exclude it."
        ),
        WorkspaceDiagnosticKind::MalformedTsconfig { error } => format!(
            "tsconfig at '{display}' failed to parse ({error}); \
             its project references, path aliases and compiler options are ignored. \
             Fix the JSON syntax."
        ),
        WorkspaceDiagnosticKind::TsconfigReferenceDirMissing => format!(
            "tsconfig.json references '{display}' but the directory does not exist. \
             Update or remove the reference, or restore the missing directory."
        ),
        WorkspaceDiagnosticKind::MalformedPnpmWorkspaceYaml { error } => format!(
            "'{display}' is not valid YAML ({error}). Catalog checks are skipped \
             and its override entries are ignored until it parses. pnpm install \
             can accept this file, but `pnpm add` and YAML formatters reject it. \
             Indent each continuation line of a quoted value or a `[...]` / \
             `{{...}}` list deeper than its key."
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
             A file, export or dependency that only this directory uses can be reported as \
             unused. To analyze it, add '!{display}/**' to ignorePatterns. To stop \
             that false positive without that, add the file to entry, the export to \
             ignoreExports or the dependency to ignoreDependencies. To silence this message, \
             add '{display}/**' to ignorePatterns. fallow --root {display} analyzes only \
             that directory on its own and does not fix this run."
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
        WorkspaceDiagnosticKind::SourceNestingTooDeep { limit } => format!(
            "Did not parse '{display}': it nests brackets deeper than {limit} levels, or it \
             needs more stack than fallow reserves, so a parse could overflow the stack. Its \
             imports and exports are missing from this run, so files and symbols it uses can be \
             reported as unused, and while the file is reachable `fallow fix` holds back its \
             actions for the whole project. Add '{display}' to ignoreFindings if it is generated \
             code, or reduce its nesting."
        ),
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
        WorkspaceDiagnosticKind::PnpmLockOverrideResolutionSkipped => format!(
            "Skipped dependency-override resolution because '{display}' could not be parsed and \
             no other parseable lockfile was available, so unused-dependency-overrides findings \
             are not reported. Resolve any merge-conflict markers in the lockfile, or run pnpm \
             install to regenerate it, then rerun fallow."
        ),
        WorkspaceDiagnosticKind::NpmLockOverrideResolutionSkipped => format!(
            "Skipped dependency-override resolution because '{display}' could not be parsed and \
             no other parseable lockfile was available, so unused-dependency-overrides findings \
             are not reported. Resolve any merge-conflict markers in the lockfile, or run npm \
             install to regenerate it, then rerun fallow."
        ),
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides => format!(
            "'{display}' declares both `overrides` and non-empty `resolutions`; bun applies \
             `overrides` and ignores `resolutions`. Move the intended pins into `overrides` or \
             remove the shadowed `resolutions` entries."
        ),
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PackageJsonOverrides,
        } => format!(
            "pnpm 10 and earlier ignore the `overrides` in '{display}' because the root package.json \
             declares `pnpm.overrides` or `resolutions`, so fallow does not check them. Move the \
             entries into one source."
        ),
        WorkspaceDiagnosticKind::PnpmWorkspaceOverridesIgnored {
            cause: PnpmWorkspaceOverridesIgnoredCause::PnpmVersion,
        } => format!(
            "pnpm versions before 10.5.1 do not read the `overrides` in '{display}', so fallow \
             does not check them. Update `packageManager` to pnpm 10.5.1 or later, or move the \
             entries into `pnpm.overrides` in the root package.json."
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
        WorkspaceDiagnosticKind::ClientServerLeakNoClientRoots => {
            "No file has a top-level \"use client\" directive and no framework plugin supplies \
             a client root, so the client-server-leak detector had no starting point and its zero \
             count measures nothing. The rule does not apply to projects whose client code has \
             neither, such as a Vite SPA."
                .to_string()
        }
        WorkspaceDiagnosticKind::ProductionExcludedFileImported {
            pattern,
            file_count,
            files,
        } => production_text::excluded_import_message(pattern, *file_count, files),
        WorkspaceDiagnosticKind::NoSourceFilesAnalyzed {
            excluded_file_count,
        } => {
            if *excluded_file_count == 0 {
                "No source files were analyzed, so every finding count this run reports is zero \
                 because nothing was measured. Check the analysis root, ignorePatterns, and any \
                 path or workspace filter this run applied."
                    .to_owned()
            } else {
                format!(
                    "No source files were analyzed. Fallow's built-in ignore patterns excluded \
                     {excluded_file_count} candidate files, so every finding count this run \
                     reports is zero because nothing was measured; run with --explain-skipped \
                     for the breakdown."
                )
            }
        }
        WorkspaceDiagnosticKind::FileScoresUnavailable { .. }
        | WorkspaceDiagnosticKind::HotspotsSkipped { .. }
        | WorkspaceDiagnosticKind::ChangeCouplingSkipped { .. }
        | WorkspaceDiagnosticKind::ShallowClone { .. }
        | WorkspaceDiagnosticKind::UnpinnedClock
        | WorkspaceDiagnosticKind::OwnershipUnavailable { .. }
        | WorkspaceDiagnosticKind::TrendSnapshotUnreadable { .. }
        | WorkspaceDiagnosticKind::TrendGroupBaselineUnavailable { .. }
        | WorkspaceDiagnosticKind::CoverageAutoDetected => {
            health_text::health_stage_message(&display, kind)
        }
        WorkspaceDiagnosticKind::InertFlag { .. }
        | WorkspaceDiagnosticKind::ConfigLoadFellBack { .. }
        | WorkspaceDiagnosticKind::ConfigFileShadowed { .. }
        | WorkspaceDiagnosticKind::UnknownSecurityCategory { .. }
        | WorkspaceDiagnosticKind::EnvValueIgnored { .. } => {
            run_notices::run_notice_message(&display, kind)
        }
        WorkspaceDiagnosticKind::FlagAgeShallowClone => {
            "This is a shallow clone, so the flag retirement report gives no flag age. Run git \
             fetch --unshallow for the full history, or pass --flag-age off."
                .to_owned()
        }
        WorkspaceDiagnosticKind::BaselineInstallStateMismatch {
            baseline_node_modules,
            ..
        } => {
            let (saved, now, remedy) = match baseline_node_modules {
                NodeModulesState::Present => (
                    "present",
                    "has no node_modules",
                    "Install dependencies before you compare",
                ),
                NodeModulesState::Missing => (
                    "missing",
                    "has node_modules",
                    "Compare without installed dependencies",
                ),
            };
            format!(
                "The baseline '{display}' was saved with node_modules {saved}, and this run \
                 {now}. Findings that need installed packages (dependency, unlisted, \
                 unresolved-import and script-binary findings) can differ, so the run can report \
                 baseline findings as new. {remedy}, or save the baseline again in the same \
                 install state."
            )
        }
        WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched { .. }
        | WorkspaceDiagnosticKind::IgnoreFindingsPatternUnmatched { .. }
        | WorkspaceDiagnosticKind::EntryPatternRedundant { .. }
        | WorkspaceDiagnosticKind::IgnoreDependenciesEntryRedundant { .. }
        | WorkspaceDiagnosticKind::IgnorePatternsEntryRedundant { .. }
        | WorkspaceDiagnosticKind::CircularDependenciesAllowUnmatched { .. }
        | WorkspaceDiagnosticKind::OverrideRuleIneffective { .. }
        | WorkspaceDiagnosticKind::RequestedRuleOff { .. } => {
            config_entry::config_entry_message(kind)
        }
        WorkspaceDiagnosticKind::FlagAgeUnavailable { cause } => {
            if cause == "no-commits" {
                "The flag retirement report gives no flag age, because the current branch has no \
                 commit. Commit the code, or pass --flag-age off."
                    .to_owned()
            } else {
                "The flag retirement report gives no flag age, because no git repository was \
                 found at the project root. Run fallow inside the repository, or pass --flag-age \
                 off."
                    .to_owned()
            }
        }
        WorkspaceDiagnosticKind::PluginConfigUnreadable {
            plugin,
            key,
            reason,
        } => {
            let (consequence, advice) = plugin_text::unreadable_key_consequence(key, reason);
            format!(
                "Plugin '{plugin}': `{key}` in '{display}' {situation}, so {consequence}. {advice}",
                situation = plugin_text::unreadable_situation(reason)
            )
        }
        WorkspaceDiagnosticKind::PluginEffectNotModeled {
            plugin,
            key,
            reason,
        } => {
            // Two causes, one effect, one remedy. Each cause gets its own
            // sentence: the cause and the effect on the findings are separate
            // facts, and one sentence with two `so` clauses states neither fact
            // clearly.
            let effect = "`autoImports` kept the convention entry patterns for that surface, and \
                          fallow reports no unused file there. Write the setting as static \
                          literals, or remove the key to use the framework defaults.";
            if key.starts_with('#') {
                format!(
                    "Plugin '{plugin}': fallow cannot read which names '{display}' takes from \
                     `{key}`, so every name of `{key}` counts as used, and fallow reports no \
                     unused file for these names. Read each name with a member access such as \
                     `C.Card`, or import it by name."
                )
            } else if reason == "config-property-unreadable" {
                format!(
                    "Plugin '{plugin}': fallow cannot read a top-level property in '{display}', so \
                     it cannot classify the `{key}` surface. {effect}"
                )
            } else {
                format!(
                    "Plugin '{plugin}': fallow does not model the effect of `{key}` in \
                     '{display}'. {effect}"
                )
            }
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
                    "To analyze first-party source there, add '!{display}/**' to \
                     ignorePatterns, or analyze that directory on its own with \
                     fallow --root {display}."
                )
            } else {
                "This pattern matches a file name rather than a directory, so re-running under \
                 a different --root excludes the same files again. Rename first-party source \
                 that only looks generated, dropping the '.min' or '.bundle' infix, or add a \
                 '!' entry for the file to ignorePatterns."
                    .to_owned()
            };
            format!(
                "{location}: {subject} fallow's built-in ignore pattern '{pattern}', so nothing \
                 {effect} is visible to this run. {remedy}"
            )
        }
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
