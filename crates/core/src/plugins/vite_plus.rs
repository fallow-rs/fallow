//! Vite+ toolchain plugin.
//!
//! Vite+ (`vite-plus`) keeps the config of its tools in `vite.config.*`. The
//! Vitest plugin reads the `test` block. This plugin reads the `pack` block,
//! which is a tsdown config, and the `lint` block, which is an Oxlint config.
//! The `fmt` block holds formatter options only. The script layer reads the
//! commands of the `staged` and `run.tasks` blocks.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser, oxlint};

const ENABLERS: &[&str] = &["vite-plus"];

const CONFIG_PATTERNS: &[&str] = &["**/vite.config.{ts,js,mts,mjs,cts,cjs}"];

const ALWAYS_USED: &[&str] = &["vite.config.{ts,js,mts,mjs,cts,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["vite-plus"];

/// The package that a Vite+ config imports, also through a subpath such as
/// `vite-plus/test`.
const VITE_PLUS_PACKAGE: &str = "vite-plus";

/// The root config files that Vite+ loads, in the order that Vite tries them.
pub const VITE_CONFIG_FILES: &[&str] = &[
    "vite.config.js",
    "vite.config.mjs",
    "vite.config.ts",
    "vite.config.cjs",
    "vite.config.mts",
    "vite.config.cts",
];

pub struct VitePlusPlugin;

impl Plugin for VitePlusPlugin {
    fn name(&self) -> &'static str {
        "vite-plus"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    /// Activate on a `vite-plus` dependency, or on a root `vite.config.*` that
    /// imports `vite-plus`. A repository that develops Vite+ itself can link the
    /// package without a dependency entry in the root manifest.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter().any(|dep| dep == VITE_PLUS_PACKAGE)
            || VITE_CONFIG_FILES.iter().any(|name| {
                let path = root.join(name);
                std::fs::read_to_string(&path).is_ok_and(|source| imports_vite_plus(&source, &path))
            })
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        // `pack` is one tsdown config or an array of tsdown configs.
        let mut entries =
            config_parser::extract_config_string_or_array(source, config_path, &["pack", "entry"]);
        entries.extend(config_parser::extract_config_array_nested_string_or_array(
            source,
            config_path,
            &["pack"],
            &["entry"],
        ));
        result.extend_config_dir_entry_patterns(entries, config_path, root);

        oxlint::credit_oxlint_config(&mut result, source, config_path, root, &["lint"]);
        result
    }
}

/// Whether a config source imports `vite-plus` or one of its subpaths. The
/// text check skips the parse for a config that cannot match.
fn imports_vite_plus(source: &str, path: &Path) -> bool {
    source.contains(VITE_PLUS_PACKAGE)
        && config_parser::extract_imports(source, path)
            .iter()
            .any(|specifier| crate::resolve::extract_package_name(specifier) == VITE_PLUS_PACKAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(source: &str) -> PluginResult {
        let root = std::env::temp_dir().join("vite-plus-plugin");
        VitePlusPlugin.resolve_config(&root.join("vite.config.ts"), source, &root)
    }

    fn entries(result: &PluginResult) -> Vec<&str> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect()
    }

    #[test]
    fn pack_entry_object_map_and_array() {
        let result = resolve(
            r"
            import { defineConfig } from 'vite-plus';
            export default defineConfig({
                pack: { entry: { index: './src/index.ts', cli: './src/cli.ts' } },
            });
            ",
        );
        let mut found = entries(&result);
        found.sort_unstable();
        assert_eq!(found, ["src/cli.ts", "src/index.ts"]);

        let result = resolve("export default { pack: { entry: ['src/a.ts', 'src/b.ts'] } };");
        assert_eq!(entries(&result), ["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn pack_array_of_configs() {
        let result = resolve(
            r"
            export default defineConfig(() => ({
                pack: [
                    { entry: { index: './src/index.ts' } },
                    { entry: ['./src/azure/index.ts'] },
                    { entry: './src/gitlab/index.ts' },
                ],
            }));
            ",
        );
        assert_eq!(
            entries(&result),
            ["src/index.ts", "src/azure/index.ts", "src/gitlab/index.ts"]
        );
    }

    #[test]
    fn lint_block_credits_js_plugins_overrides_and_extends() {
        let result = resolve(
            r"
            import { defineConfig } from 'vite-plus';
            export default defineConfig({
                lint: {
                    jsPlugins: ['@e18e/eslint-plugin', { name: 'x', specifier: 'eslint-plugin-x' }],
                    overrides: [{ files: ['*.ts'], jsPlugins: ['eslint-plugin-override'] }],
                    extends: ['oxlint-config-shared', './lint/base.json'],
                    plugins: ['unicorn'],
                },
            });
            ",
        );
        let deps = &result.referenced_dependencies;
        for dep in [
            "vite-plus",
            "@e18e/eslint-plugin",
            "eslint-plugin-x",
            "eslint-plugin-override",
            "oxlint-config-shared",
        ] {
            assert!(deps.iter().any(|d| d == dep), "{dep} missing from {deps:?}");
        }
        assert!(!deps.iter().any(|d| d == "unicorn" || d == "x"), "{deps:?}");
        assert_eq!(result.setup_files.len(), 1);
        assert!(result.setup_files[0].ends_with("lint/base.json"));
    }

    #[test]
    fn local_js_plugin_is_a_setup_file() {
        let result = resolve("export default { lint: { jsPlugins: ['./lint/plugin.js'] } };");
        assert_eq!(result.setup_files.len(), 1);
        assert!(result.setup_files[0].ends_with("lint/plugin.js"));
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn top_level_js_plugins_outside_lint_are_not_credited() {
        let result = resolve("export default { jsPlugins: ['eslint-plugin-top'] };");
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn activation_by_dependency_or_root_config_import() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        assert!(VitePlusPlugin.is_enabled_with_deps(&["vite-plus".to_string()], root));
        assert!(!VitePlusPlugin.is_enabled_with_deps(&["vite".to_string()], root));

        std::fs::write(
            root.join("vite.config.ts"),
            "import { defineConfig } from 'vite';\nexport default defineConfig({});\n",
        )
        .expect("write config");
        assert!(!VitePlusPlugin.is_enabled_with_deps(&[], root));

        std::fs::write(
            root.join("vite.config.ts"),
            "import { defineConfig } from 'vite-plus';\nexport default defineConfig({});\n",
        )
        .expect("write config");
        assert!(VitePlusPlugin.is_enabled_with_deps(&[], root));
    }

    #[test]
    fn a_comment_that_names_the_package_does_not_activate() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("vite.config.mjs"),
            "// migrate to vite-plus later\nexport default {};\n",
        )
        .expect("write config");
        assert!(!VitePlusPlugin.is_enabled_with_deps(&[], temp.path()));
    }
}
