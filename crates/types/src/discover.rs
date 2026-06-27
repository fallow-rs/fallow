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
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    #[test]
    fn file_id_equality() {
        assert_eq!(FileId(0), FileId(0));
        assert_eq!(FileId(42), FileId(42));
        assert_ne!(FileId(0), FileId(1));
    }

    #[test]
    fn file_id_copy_semantics() {
        let a = FileId(5);
        let b = a; // Copy, not move
        assert_eq!(a, b);
    }

    #[test]
    fn file_id_hash_consistent() {
        let id = FileId(99);
        let hash1 = {
            let mut h = DefaultHasher::new();
            id.hash(&mut h);
            h.finish()
        };
        let hash2 = {
            let mut h = DefaultHasher::new();
            id.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn file_id_equal_values_same_hash() {
        let a = FileId(7);
        let b = FileId(7);
        let hash_a = {
            let mut h = DefaultHasher::new();
            a.hash(&mut h);
            h.finish()
        };
        let hash_b = {
            let mut h = DefaultHasher::new();
            b.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn file_id_inner_value_accessible() {
        let id = FileId(123);
        assert_eq!(id.0, 123);
    }

    #[test]
    fn file_id_debug_format() {
        let id = FileId(42);
        let debug = format!("{id:?}");
        assert!(
            debug.contains("42"),
            "Debug should show inner value: {debug}"
        );
    }

    #[test]
    fn discovered_file_clone() {
        let original = DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            size_bytes: 1024,
        };
        let cloned = original.clone();
        assert_eq!(cloned.id, original.id);
        assert_eq!(cloned.path, original.path);
        assert_eq!(cloned.size_bytes, original.size_bytes);
    }

    #[test]
    fn discovered_file_zero_size() {
        let file = DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/empty.ts"),
            size_bytes: 0,
        };
        assert_eq!(file.size_bytes, 0);
    }

    #[test]
    fn discovered_file_large_size() {
        let file = DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/large.ts"),
            size_bytes: u64::MAX,
        };
        assert_eq!(file.size_bytes, u64::MAX);
    }

    #[test]
    fn entry_point_clone() {
        let ep = EntryPoint {
            path: PathBuf::from("/project/src/main.ts"),
            source: EntryPointSource::PackageJsonMain,
        };
        let cloned = ep.clone();
        assert_eq!(cloned.path, ep.path);
        assert!(matches!(cloned.source, EntryPointSource::PackageJsonMain));
    }

    #[test]
    fn entry_point_source_all_variants_constructible() {
        let _ = EntryPointSource::PackageJsonMain;
        let _ = EntryPointSource::PackageJsonModule;
        let _ = EntryPointSource::PackageJsonExports;
        let _ = EntryPointSource::PackageJsonBin;
        let _ = EntryPointSource::PackageJsonScript;
        let _ = EntryPointSource::Plugin {
            name: "next".to_string(),
        };
        let _ = EntryPointSource::TestFile;
        let _ = EntryPointSource::DefaultIndex;
        let _ = EntryPointSource::ManualEntry;
        let _ = EntryPointSource::InfrastructureConfig;
        let _ = EntryPointSource::DynamicallyLoaded;
    }

    #[test]
    fn entry_point_source_plugin_preserves_name() {
        let source = EntryPointSource::Plugin {
            name: "vitest".to_string(),
        };
        match source {
            EntryPointSource::Plugin { name } => assert_eq!(name, "vitest"),
            _ => panic!("expected Plugin variant"),
        }
    }

    #[test]
    fn entry_point_source_plugin_clone_preserves_name() {
        let source = EntryPointSource::Plugin {
            name: "storybook".to_string(),
        };
        let cloned = source.clone();
        assert!(matches!(&source, EntryPointSource::Plugin { name } if name == "storybook"));
        match cloned {
            EntryPointSource::Plugin { name } => assert_eq!(name, "storybook"),
            _ => panic!("expected Plugin variant after clone"),
        }
    }

    #[test]
    fn entry_point_source_debug_format() {
        let source = EntryPointSource::PackageJsonMain;
        let debug = format!("{source:?}");
        assert!(
            debug.contains("PackageJsonMain"),
            "Debug should name the variant: {debug}"
        );

        let plugin = EntryPointSource::Plugin {
            name: "remix".to_string(),
        };
        let debug = format!("{plugin:?}");
        assert!(
            debug.contains("remix"),
            "Debug should show plugin name: {debug}"
        );
    }

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
