//! Workspace and source-discovery diagnostic data types.
//!
//! The serializable `WorkspaceDiagnostic` / `WorkspaceDiagnosticKind` pair
//! lives here, upstream of both `fallow-config` (which owns the registry and
//! emission logic and re-exports these types for back-compat) and
//! `fallow-output` (which embeds `Vec<WorkspaceDiagnostic>` in its JSON
//! envelopes). Keeping the data types in `fallow-types` lets the output layer
//! reference the real, schema-bearing type instead of an opaque
//! `serde_json::Value` newtype, so `workspace_diagnostics[]` keeps its typed
//! `kind`/`path`/`message` shape (and the 7-variant `kind` oneOf) in
//! `docs/output-schema.json` without coupling output contracts to config
//! loading.

use std::path::{Path, PathBuf};

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
            Self::SkippedLargeFile { .. } => "skipped-large-file",
            Self::SkippedMinifiedFile { .. } => "skipped-minified-file",
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
            Self::SkippedLargeFile { .. } | Self::SkippedMinifiedFile { .. }
        )
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
    #[must_use]
    pub fn new(root: &Path, path: PathBuf, kind: WorkspaceDiagnosticKind) -> Self {
        let kind = normalise_payload_paths(root, kind);
        let message = render_message(root, &path, &kind);
        Self {
            path,
            kind,
            message,
        }
    }
}

/// Strip the project root from absolute paths embedded inside variant
/// payloads (today: the `error` field of `MalformedPackageJson` and
/// `MalformedTsconfig`). Mirrors the per-platform `display()` byte sequence
/// so the substring match works on Windows too.
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
        other => other,
    }
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
