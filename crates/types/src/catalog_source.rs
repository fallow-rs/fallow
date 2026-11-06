//! Classification of the source file of a package manager catalog finding.

use std::ffi::OsStr;
use std::path::Path;

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// Whether a catalog finding path names a `pnpm-workspace.yaml`: the root
/// file or the file of an independent pnpm project below the root. A Bun
/// catalog finding names `package.json`.
#[must_use]
pub fn is_pnpm_catalog_source(path: &Path) -> bool {
    path.file_name() == Some(OsStr::new(PNPM_WORKSPACE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_nested_pnpm_files_are_pnpm_sources() {
        assert!(is_pnpm_catalog_source(Path::new("pnpm-workspace.yaml")));
        assert!(is_pnpm_catalog_source(Path::new(
            "site/pnpm-workspace.yaml"
        )));
        assert!(!is_pnpm_catalog_source(Path::new("package.json")));
        assert!(!is_pnpm_catalog_source(Path::new(
            "site/pnpm-workspace.yaml.bak"
        )));
    }
}
