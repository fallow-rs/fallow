//! Dependencies that a config names without a source import: the packages
//! that config imports credit, and the packages that a config needs installed.

use std::path::{Path, PathBuf};

use super::{PluginResult, config_parser};

/// Credit each package that a config file imports.
pub(super) fn add_import_referenced_dependencies(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
) {
    let imports = config_parser::extract_imports(source, config_path);
    for import in &imports {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(import));
    }
}

/// A package that a config needs installed, although no source file imports
/// it. A tool loads the package by name, for example a TypeScript language
/// service plugin, or the config sets a key that needs a companion package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredDependency {
    /// The package name, without a subpath.
    pub package_name: String,
    /// The absolute path of the config file.
    pub config_path: PathBuf,
    /// 1-based line of the value that names the need.
    pub line: u32,
    /// 0-based byte column of that value.
    pub col: u32,
    /// True when only a development tool loads the package. A `--production`
    /// run does not report a development requirement.
    pub dev_only: bool,
}

impl RequiredDependency {
    /// Build a requirement at byte `offset` of the config `source`.
    #[must_use]
    pub(crate) fn at_offset(
        package_name: String,
        config_path: &Path,
        source: &str,
        offset: usize,
        dev_only: bool,
    ) -> Self {
        let offset = offset.min(source.len());
        let before = source.get(..offset).unwrap_or_default();
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |index| index + 1);
        Self {
            package_name,
            config_path: config_path.to_path_buf(),
            line: u32::try_from(line).unwrap_or(u32::MAX),
            col: u32::try_from(offset - line_start).unwrap_or(u32::MAX),
            dev_only,
        }
    }
}
