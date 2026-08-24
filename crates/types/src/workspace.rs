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
    /// A source discovered with a stable [`FileId`](crate::discover::FileId)
    /// could not be read before parsing. Analysis continues with the remaining
    /// sparse module IDs and reports the underlying filesystem or UTF-8 error.
    SourceReadFailure {
        /// Filesystem or UTF-8 decoding error from `read_to_string`.
        error: String,
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
            Self::SourceReadFailure { .. } => "source-read-failure",
            Self::BunLockbOverrideResolutionSkipped => "bun-lockb-override-resolution-skipped",
            Self::BunLockOverrideResolutionSkipped => "bun-lock-override-resolution-skipped",
            Self::BunResolutionsShadowedByOverrides => "bun-resolutions-shadowed-by-overrides",
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
                | Self::SourceReadFailure { .. }
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
            Self::SkippedLargeFile { .. } | Self::SkippedMinifiedFile { .. }
        )
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
            | Self::BunResolutionsShadowedByOverrides => true,
            Self::UndeclaredWorkspace
            | Self::MalformedPackageJson { .. }
            | Self::GlobMatchedNoPackageJson { .. }
            | Self::MalformedTsconfig { .. }
            | Self::TsconfigReferenceDirMissing
            | Self::SkippedLargeFile { .. }
            | Self::SkippedMinifiedFile { .. }
            | Self::SourceReadFailure { .. } => false,
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
    #[must_use]
    pub fn into_root_relative(mut self, root: &Path) -> Self {
        if let Ok(relative) = self.path.strip_prefix(root) {
            self.path = relative.to_path_buf();
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
        WorkspaceDiagnosticKind::SourceReadFailure { error } => format!(
            "Could not read source '{display}' ({error}). Restore the file or its read permissions, \
             ensure it contains valid UTF-8 text, or add '{display}' to ignorePatterns."
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
        WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides => format!(
            "'{display}' declares both `overrides` and non-empty `resolutions`; bun applies \
             `overrides` and ignores `resolutions`. Move the intended pins into `overrides` or \
             remove the shadowed `resolutions` entries."
        ),
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

    #[test]
    fn source_walk_recorded_covers_only_the_kinds_a_walk_replaces() {
        for kind in [
            WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes: 1 },
            WorkspaceDiagnosticKind::SkippedMinifiedFile { size_bytes: 1 },
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
