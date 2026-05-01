//! Go language plugin.
//!
//! Enables Go 1.25+ project support: entry point detection for `cmd/*/main.go`
//! and `main.go` patterns, and `go.mod` as a tooling config file.

use std::path::Path;

use fallow_config::PackageJson;

use super::Plugin;

const ENTRY_PATTERNS: &[&str] = &[
    // Standard Go binary layout: cmd/<name>/main.go
    "cmd/*/main.go",
    // Flat layout: root-level main.go
    "main.go",
    // Also credit test files as entry points so they are never reported as unused.
    "**/*_test.go",
];

const ALWAYS_USED: &[&str] = &["go.mod", "go.sum", "go.work", "go.work.sum"];

const TOOLING_DEPENDENCIES: &[&str] = &["go"];

pub struct GoPlugin;

impl Plugin for GoPlugin {
    fn name(&self) -> &'static str {
        "go"
    }

    fn enablers(&self) -> &'static [&'static str] {
        &["go"]
    }

    fn is_enabled(&self, pkg: &PackageJson, root: &Path) -> bool {
        root.join("go.mod").exists()
            || root.join("go.work").exists()
            || root.join("main.go").exists()
            || pkg.all_dependency_names().iter().any(|dep| dep == "go")
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn is_enabled_when_go_mod_exists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module example.com/test\n").unwrap();

        assert!(GoPlugin.is_enabled(&PackageJson::default(), dir.path()));
    }

    #[test]
    fn is_enabled_when_go_work_exists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.work"), "go 1.25\nuse .\n").unwrap();

        assert!(GoPlugin.is_enabled(&PackageJson::default(), dir.path()));
    }

    #[test]
    fn is_not_enabled_in_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!GoPlugin.is_enabled(&PackageJson::default(), dir.path()));
    }
}
