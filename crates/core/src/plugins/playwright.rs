//! Playwright test runner plugin.
//!
//! Detects Playwright projects and marks test files and config as entry points.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::config_parser;
use super::{Plugin, PluginResult};
use crate::scripts;

define_plugin!(
    struct PlaywrightPlugin => "playwright",
    enablers: &["@playwright/test"],
    entry_patterns: &[
        "**/*.spec.{ts,tsx,js,jsx}",
        "**/*.test.{ts,tsx,js,jsx}",
        "tests/**/*.{ts,tsx,js,jsx}",
        "e2e/**/*.{ts,tsx,js,jsx}",
    ],
    config_patterns: &["playwright.config.{ts,js}"],
    always_used: &["playwright.config.{ts,js}"],
    tooling_dependencies: &["@playwright/test", "playwright"],
    fixture_glob_patterns: &[
        "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
        "e2e/fixtures/**/*.{ts,tsx,js,jsx,json}",
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let config_dir = config_path
            .parent()
            .filter(|parent| parent.is_absolute())
            .unwrap_or(root);

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        if let Some(setup) =
            config_parser::extract_config_string(source, config_path, &["globalSetup"])
        {
            result
                .setup_files
                .push(config_dir.join(setup.trim_start_matches("./")));
        }
        if let Some(teardown) =
            config_parser::extract_config_string(source, config_path, &["globalTeardown"])
        {
            result
                .setup_files
                .push(config_dir.join(teardown.trim_start_matches("./")));
        }

        let (web_deps, web_setup) = collect_web_server(source, config_path, root, config_dir);
        result.referenced_dependencies.extend(web_deps);
        result.setup_files.extend(web_setup);

        result
    },
);

/// Parse Playwright `webServer.command` entries (object and array forms) into
/// referenced dependencies and reachable setup files.
///
/// Each command is run through the shared script parser ([`scripts::analyze_command`]),
/// so invoked npm binaries are credited as dependencies and local file arguments are
/// seeded as support entry files exactly as they would be in a package.json script.
/// `config_dir` is the directory of the config file: file arguments resolve there by
/// default, matching Playwright's `webServer.cwd` default. A `webServer.cwd` (per
/// object, or per array element) overrides that base, resolved relative to `config_dir`
/// (an absolute cwd replaces it). `root` is the project root, used only for
/// binary-to-package resolution (it owns `node_modules`). Commands that delegate to a
/// package manager (`npm run start`, `yarn dev`) credit nothing, since the underlying
/// script's own dependencies are analyzed separately.
fn collect_web_server(
    source: &str,
    config_path: &Path,
    root: &Path,
    config_dir: &Path,
) -> (Vec<String>, Vec<PathBuf>) {
    let mut commands: Vec<(String, Option<String>)> = Vec::new();

    if let Some(command) =
        config_parser::extract_config_command(source, config_path, &["webServer", "command"])
    {
        let cwd = config_parser::extract_config_string(source, config_path, &["webServer", "cwd"]);
        commands.push((command, cwd));
    }

    commands.extend(config_parser::extract_config_array_object_command_pairs(
        source,
        config_path,
        &["webServer"],
        "command",
        "cwd",
    ));

    let mut referenced_dependencies = Vec::new();
    let mut setup_files = Vec::new();

    for (command, cwd) in commands {
        let analysis = scripts::analyze_command(&command, root, &FxHashMap::default());
        referenced_dependencies.extend(analysis.used_packages);

        let base = cwd.map_or_else(
            || config_dir.to_path_buf(),
            |dir| config_dir.join(dir.trim_start_matches("./")),
        );
        for file in analysis
            .config_files
            .into_iter()
            .chain(analysis.entry_files)
        {
            setup_files.push(base.join(file.trim_start_matches("./")));
        }
    }

    (referenced_dependencies, setup_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_global_setup() {
        let source = r#"
            export default {
                globalSetup: "./global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_global_teardown() {
        let source = r#"
            export default {
                globalTeardown: "./global-teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-teardown.ts")]
        );
    }

    #[test]
    fn resolve_config_both_setup_and_teardown() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts",
                globalTeardown: "./teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![
                Path::new("/project/setup.ts"),
                Path::new("/project/teardown.ts"),
            ]
        );
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig, devices } from '@playwright/test';
            export default defineConfig({
                globalSetup: "./setup.ts"
            });
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@playwright/test".to_string())
        );
        assert_eq!(result.setup_files, vec![Path::new("/project/setup.ts")]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.setup_files.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_setup_strips_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "./tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_setup_without_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn fixture_patterns_are_set() {
        let plugin = PlaywrightPlugin;
        assert!(!plugin.fixture_glob_patterns().is_empty());
    }

    fn resolve(source: &str) -> PluginResult {
        PlaywrightPlugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        )
    }

    #[test]
    fn web_server_object_command_credits_cli_dependency() {
        let source = r#"
            export default {
                webServer: { command: "srvx --port 3000", url: "http://localhost:3000" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited, got {:?}",
            result.referenced_dependencies
        );
        assert!(
            result.setup_files.is_empty(),
            "a flag-only command seeds no files, got {:?}",
            result.setup_files
        );
    }

    #[test]
    fn web_server_template_command_credits_pnpm_exec_cli_dependency() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: {
                    command: `pnpm build && pnpm exec srvx --prod --port ${PORT} --hostname 127.0.0.1`
                }
            };
        ";
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited from pnpm exec template command, got {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn web_server_array_template_command_credits_pnpm_exec_cli_dependency() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: [
                    {
                        command: `pnpm exec srvx --prod --port ${PORT}`,
                    },
                ],
            };
        ";
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited from array template command, got {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn web_server_array_node_runner_seeds_file_and_credits_runner() {
        let source = r#"
            export default {
                webServer: [{ command: "tsx scripts/e2e-server.ts" }]
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"tsx".to_string()),
            "tsx node runner should be credited, got {:?}",
            result.referenced_dependencies
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/scripts/e2e-server.ts")]
        );
    }

    #[test]
    fn web_server_object_command_honors_cwd() {
        let source = r#"
            export default {
                webServer: { command: "node server.js", cwd: "packages/api" }
            };
        "#;
        let result = resolve(source);
        assert!(
            !result.referenced_dependencies.contains(&"node".to_string()),
            "node must not be credited as a dependency, got {:?}",
            result.referenced_dependencies
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/packages/api/server.js")],
            "server.js must resolve under webServer.cwd"
        );
    }

    #[test]
    fn web_server_array_per_element_cwd() {
        let source = r#"
            export default {
                webServer: [
                    { command: "tsx scripts/api.ts", cwd: "packages/api" },
                    { command: "tsx scripts/web.ts" }
                ]
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/packages/api/scripts/api.ts"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/scripts/web.ts"))
        );
    }

    #[test]
    fn web_server_package_manager_delegation_is_noop() {
        let source = r#"
            export default {
                webServer: { command: "npm run start" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.is_empty(),
            "npm run delegation must not credit a phantom dependency, got {:?}",
            result.referenced_dependencies
        );
        assert!(result.setup_files.is_empty());
    }

    #[test]
    fn web_server_and_global_setup_coexist() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts",
                webServer: { command: "tsx scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/setup.ts"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/scripts/e2e-server.ts"))
        );
    }

    #[test]
    fn web_server_strips_leading_dot_slash_in_file_args() {
        let source = r#"
            export default {
                webServer: { command: "tsx ./scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve(source);
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/scripts/e2e-server.ts")]
        );
    }

    #[test]
    fn no_web_server_seeds_nothing() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts"
            };
        "#;
        let result = resolve(source);
        assert_eq!(result.setup_files, vec![Path::new("/project/setup.ts")]);
    }

    /// Build a platform-absolute path from a `/project/...`-style logical path.
    /// On Windows a leading-slash path lacks a drive and is NOT absolute, so the
    /// `config_path.parent().is_absolute()` gate in `resolve_config` would fall
    /// back to the root and drop the nested config directory. The registry
    /// always passes a genuinely-absolute config path at runtime (drive-rooted
    /// on Windows), so these tests must do the same. On Unix this is the identity.
    fn abs(logical: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{}", logical.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(logical)
        }
    }

    /// Resolve with an absolute, nested config path (as the registry passes at
    /// runtime), to exercise the config-file-directory base.
    fn resolve_at(config_path: &str, source: &str) -> PluginResult {
        PlaywrightPlugin.resolve_config(&abs(config_path), source, &abs("/project"))
    }

    #[test]
    fn web_server_file_args_resolve_from_nested_config_dir_not_root() {
        let source = r#"
            export default {
                webServer: { command: "tsx scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/scripts/e2e-server.ts")],
            "nested-config file args must resolve under the config directory, not the project root"
        );
    }

    #[test]
    fn web_server_nested_config_cwd_resolves_relative_to_config_dir() {
        let source = r#"
            export default {
                webServer: { command: "tsx scripts/server.ts", cwd: "api" }
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/api/scripts/server.ts")],
            "cwd must resolve relative to the config directory"
        );
    }

    #[test]
    fn global_setup_resolves_from_nested_config_dir() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts"
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/setup.ts")],
            "globalSetup must resolve under the config directory"
        );
    }
}
