//! k6 load testing plugin.
//!
//! k6 executes project-local JavaScript load-test files and provides runtime
//! modules such as `k6/http` outside npm resolution.

use std::path::{Path, PathBuf};

use super::Plugin;

const ENABLERS: &[&str] = &["k6"];
const ENTRY_PATTERNS: &[&str] = &[
    "**/*.k6.{js,ts,mjs,cjs,mts,cts}",
    "load/*.k6.{js,ts,mjs,cjs,mts,cts}",
];
const TOOLING_DEPENDENCIES: &[&str] = &["k6"];
const K6_SCRIPT_SUFFIXES: &[&str] = &[
    ".k6.js", ".k6.ts", ".k6.mjs", ".k6.cjs", ".k6.mts", ".k6.cts",
];

pub struct K6Plugin;

impl Plugin for K6Plugin {
    fn name(&self) -> &'static str {
        "k6"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> bool {
        self.is_enabled_with_deps(deps, root)
            || discovered_files.iter().any(|path| is_k6_script_path(path))
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }
}

fn is_k6_script_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            K6_SCRIPT_SUFFIXES
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    #[test]
    fn activates_from_k6_dependency() {
        let plugin = K6Plugin;
        let deps = vec!["k6".to_string()];

        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
        assert!(plugin.is_enabled_with_files(&deps, Path::new("/project"), &[]));
    }

    #[test]
    fn activates_from_discovered_k6_script_files() {
        let plugin = K6Plugin;
        let files = vec![PathBuf::from("/project/load/smoke.k6.js")];

        assert!(plugin.is_enabled_with_files(&[], Path::new("/project"), &files));
    }

    #[test]
    fn does_not_activate_from_similar_filenames() {
        let plugin = K6Plugin;
        let files = vec![
            PathBuf::from("/project/load/smoke.k6ish.js"),
            PathBuf::from("/project/load/k6-tools.js"),
            PathBuf::from("/project/load/k6.ts"),
        ];

        assert!(!plugin.is_enabled_with_files(&[], Path::new("/project"), &files));
    }

    #[test]
    fn exposes_k6_entry_patterns_tooling_and_role() {
        let plugin = K6Plugin;

        assert_eq!(plugin.entry_patterns(), ENTRY_PATTERNS);
        assert_eq!(plugin.tooling_dependencies(), TOOLING_DEPENDENCIES);
        assert_eq!(plugin.entry_point_role(), EntryPointRole::Test);
    }
}
