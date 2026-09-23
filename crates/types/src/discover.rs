//! File discovery types: discovered files, file IDs, and entry points.

use std::path::{Path, PathBuf};

/// A discovered source file on disk.
///
/// # Examples
///
/// ```
/// use fallow_types::discover::{DiscoveredFile, FileId};
/// use std::path::PathBuf;
///
/// let file = DiscoveredFile {
///     id: FileId(0),
///     path: PathBuf::from("/project/src/index.ts"),
///     size_bytes: 2048,
/// };
/// assert_eq!(file.id, FileId(0));
/// assert_eq!(file.size_bytes, 2048);
/// ```
#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    /// Unique file index.
    pub id: FileId,
    /// Absolute path.
    pub path: PathBuf,
    /// File size in bytes (for sorting largest-first).
    pub size_bytes: u64,
}

/// Compact file identifier.
///
/// A newtype wrapper around `u32` used as a stable index into file arrays.
/// `FileId`s are path-sorted (not insertion order) for stable cross-run identity.
///
/// # Examples
///
/// ```
/// use fallow_types::discover::FileId;
///
/// let id = FileId(42);
/// assert_eq!(id.0, 42);
///
/// // Implements Copy
/// let copy = id;
/// assert_eq!(id, copy);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileId(pub u32);

const _: () = assert!(std::mem::size_of::<FileId>() == 4);
#[cfg(all(target_pointer_width = "64", unix))]
const _: () = assert!(std::mem::size_of::<DiscoveredFile>() == 40);

/// Persistable file identity for cache entries that need to survive `FileId`
/// churn across runs.
///
/// `FileId` remains a dense in-memory index. This key is path-derived, root
/// relative where possible, and uses `/` separators so graph-cache metadata can
/// compare file identity without relying on platform path display quirks.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct StableFileKey(String);

impl StableFileKey {
    /// Build a stable key from an absolute path and the analysis root.
    #[must_use]
    pub fn from_root_relative(root: &Path, path: &Path) -> Self {
        let relative = path.strip_prefix(root).unwrap_or(path);
        Self(normalize_path(relative))
    }

    /// Build a stable key from an already-root-relative path.
    #[must_use]
    pub fn from_relative(path: &Path) -> Self {
        Self(normalize_path(path))
    }

    /// Stable string used in persisted cache manifests.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// An entry point into the module graph.
#[derive(Debug, Clone)]
pub struct EntryPoint {
    /// Absolute path to the entry point file.
    pub path: PathBuf,
    /// How this entry point was discovered.
    pub source: EntryPointSource,
}

impl std::fmt::Display for EntryPointSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PackageJsonMain => f.write_str("package.json main"),
            Self::PackageJsonModule => f.write_str("package.json module"),
            Self::PackageJsonExports => f.write_str("package.json exports"),
            Self::PackageJsonBin => f.write_str("package.json bin"),
            Self::PackageJsonScript => f.write_str("package.json script"),
            Self::Plugin { name } => write!(f, "{name}"),
            Self::TestFile => f.write_str("test file"),
            Self::DefaultIndex => f.write_str("default index"),
            Self::ManualEntry => f.write_str("manual entry"),
            Self::InfrastructureConfig => f.write_str("infrastructure config"),
            Self::DynamicallyLoaded => f.write_str("dynamically loaded"),
        }
    }
}

/// Where an entry point was discovered from.
#[derive(Debug, Clone)]
pub enum EntryPointSource {
    /// The `main` field in package.json.
    PackageJsonMain,
    /// The `module` field in package.json.
    PackageJsonModule,
    /// The `exports` field in package.json.
    PackageJsonExports,
    /// The `bin` field in package.json.
    PackageJsonBin,
    /// A script command in package.json.
    PackageJsonScript,
    /// Detected by a framework plugin.
    Plugin {
        /// Name of the plugin that detected this entry point.
        name: String,
    },
    /// A test file (e.g., `*.test.ts`, `*.spec.ts`).
    TestFile,
    /// A default index file (e.g., `src/index.ts`).
    DefaultIndex,
    /// Manually configured in fallow config.
    ManualEntry,
    /// Discovered from infrastructure config files (Dockerfile, Procfile, fly.toml).
    InfrastructureConfig,
    /// Declared in `dynamicallyLoaded` config as a runtime-loaded file.
    DynamicallyLoaded,
}

#[cfg(test)]
mod stable_file_key_tests {
    use super::*;

    #[test]
    fn stable_file_key_strips_root_prefix() {
        let key = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/index.ts"),
        );

        assert_eq!(key.as_str(), "src/index.ts");
    }

    #[test]
    fn stable_file_key_keeps_path_when_outside_root() {
        let key =
            StableFileKey::from_root_relative(Path::new("/project"), Path::new("/other/file.ts"));

        assert_eq!(key.as_str(), "/other/file.ts");
    }

    #[test]
    fn stable_file_key_normalizes_windows_separators() {
        let key = StableFileKey::from_relative(Path::new(r"src\feature\file.ts"));

        assert_eq!(key.as_str(), "src/feature/file.ts");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_point_source_display_all_variants() {
        assert_eq!(
            EntryPointSource::PackageJsonMain.to_string(),
            "package.json main"
        );
        assert_eq!(
            EntryPointSource::PackageJsonModule.to_string(),
            "package.json module"
        );
        assert_eq!(
            EntryPointSource::PackageJsonExports.to_string(),
            "package.json exports"
        );
        assert_eq!(
            EntryPointSource::PackageJsonBin.to_string(),
            "package.json bin"
        );
        assert_eq!(
            EntryPointSource::PackageJsonScript.to_string(),
            "package.json script"
        );
        assert_eq!(
            EntryPointSource::Plugin {
                name: "vitest".to_string()
            }
            .to_string(),
            "vitest"
        );
        assert_eq!(EntryPointSource::TestFile.to_string(), "test file");
        assert_eq!(EntryPointSource::DefaultIndex.to_string(), "default index");
        assert_eq!(EntryPointSource::ManualEntry.to_string(), "manual entry");
        assert_eq!(
            EntryPointSource::InfrastructureConfig.to_string(),
            "infrastructure config"
        );
        assert_eq!(
            EntryPointSource::DynamicallyLoaded.to_string(),
            "dynamically loaded"
        );
    }
}
