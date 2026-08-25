//! size-limit plugin.
//!
//! Detects size-limit projects, marks config files as always used, and credits
//! installed `@size-limit/*` and `size-limit-*` presets and plugins (loaded by
//! convention, not import).

use std::path::Path;

use fallow_config::PackageJson;

use super::Plugin;

const ENABLERS: &[&str] = &["size-limit"];

/// The two package-name prefixes size-limit's `load-plugins.js` imports from the
/// manifest: the official scope and the community `size-limit-*` form. The
/// `size-limit` tooling dependency itself matches neither prefix.
const PLUGIN_PREFIXES: &[&str] = &["@size-limit/", "size-limit-"];

/// Mirrors the lilconfig `searchPlaces` in size-limit's `get-config.js`: the
/// extensionless `.size-limit` is a JSON file there, not a stray entry.
///
/// These globs anchor at the package that activates the plugin. A hoisted
/// monorepo keeps the tool at the root and the config inside a workspace
/// package, which the unused-file config predicate covers by basename.
const ALWAYS_USED: &[&str] = &[".size-limit", ".size-limit.{json,js,cjs,mjs,ts,cts,mts}"];

const TOOLING_DEPENDENCIES: &[&str] = &["size-limit"];

pub struct SizeLimitPlugin;

impl Plugin for SizeLimitPlugin {
    fn name(&self) -> &'static str {
        "size-limit"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn package_json_referenced_dependencies(&self, pkg: &PackageJson, _root: &Path) -> Vec<String> {
        let mut deps: Vec<String> = pkg
            .all_dependency_names()
            .into_iter()
            .filter(|dep| PLUGIN_PREFIXES.iter().any(|prefix| dep.starts_with(prefix)))
            .collect();
        deps.sort();
        deps.dedup();
        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_installed_size_limit_packages() {
        let pkg: PackageJson = serde_json::from_str(
            r#"{
                "devDependencies": {
                    "size-limit": "13.0.3",
                    "@size-limit/preset-small-lib": "13.0.3",
                    "@size-limit/file": "13.0.3",
                    "size-limit-node-esbuild": "0.4.0",
                    "oxlint": "1.0.0"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            SizeLimitPlugin.package_json_referenced_dependencies(&pkg, Path::new("/")),
            vec![
                "@size-limit/file".to_string(),
                "@size-limit/preset-small-lib".to_string(),
                "size-limit-node-esbuild".to_string()
            ]
        );
    }
}
