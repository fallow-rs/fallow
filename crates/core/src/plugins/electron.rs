//! Electron plugin.
//!
//! Detects Electron projects and marks main/preload entry points and build
//! tool config files as always used. Parses `electron.vite.config.*` to seed
//! renderer / preload / main entry files declared in each section's
//! `build.rollupOptions.input` (commonly multi-window HTML renderer entries
//! declared via `resolve(__dirname, 'src/renderer/index.html')`).

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &[
    "electron",
    "electron-builder",
    "@electron-forge/cli",
    "electron-vite",
];

const ENTRY_PATTERNS: &[&str] = &[
    "src/main/**/*.{ts,tsx,js,jsx,mts,mjs}",
    "src/preload/**/*.{ts,tsx,js,jsx,mts,mjs}",
    "electron/main.{ts,js}",
];

const ALWAYS_USED: &[&str] = &[
    "electron-builder.{yml,yaml,json,json5,toml}",
    "forge.config.{ts,js,cjs}",
    "electron.vite.config.{ts,js,mjs}",
];

const CONFIG_PATTERNS: &[&str] = &["electron.vite.config.{ts,js,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "electron",
    "electron-builder",
    "electron-vite",
    "@electron/rebuild",
    "@electron-forge/cli",
];

/// electron-vite top-level sections. Each is a Vite config with its own
/// `build.rollupOptions.input`.
const VITE_SECTIONS: &[&str] = &["main", "preload", "renderer"];

define_plugin! {
    struct ElectronPlugin => "electron",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        // electron-vite declares per-window entries in
        // `<section>.build.rollupOptions.input`. Renderer entries are HTML files
        // whose `<script src>` trees are otherwise unreachable; main / preload
        // may add extra entries beyond the static globs. Values are commonly
        // `resolve(__dirname, 'src/renderer/index.html')`; the shared extractor
        // evaluates those path-helper calls (see issue #604) in string / array /
        // object positions. Each value is normalized relative to the config file
        // (correct for monorepo subpackage configs). See issue #600.
        for &section in VITE_SECTIONS {
            let inputs = config_parser::extract_config_string_or_array(
                source,
                config_path,
                &[section, "build", "rollupOptions", "input"],
            );
            for input in inputs {
                if let Some(normalized) =
                    config_parser::normalize_config_path(&input, config_path, root)
                {
                    result.push_entry_pattern(normalized);
                }
            }
        }

        result
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config_path() -> std::path::PathBuf {
        std::path::PathBuf::from("/project/electron.vite.config.ts")
    }

    fn entry_strings(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn resolve_config_extracts_renderer_multi_window_html_entries() {
        let source = r#"
            import { resolve } from "node:path";
            import { defineConfig } from "electron-vite";

            export default defineConfig({
                renderer: {
                    build: {
                        rollupOptions: {
                            input: {
                                index: resolve(__dirname, "src/renderer/index.html"),
                                settings: resolve(__dirname, "src/renderer/settings/index.html"),
                            },
                        },
                    },
                },
            });
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        let entries = entry_strings(&result);
        assert!(entries.contains(&"src/renderer/index.html".to_string()));
        assert!(entries.contains(&"src/renderer/settings/index.html".to_string()));
    }

    #[test]
    fn resolve_config_extracts_main_and_preload_inputs() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                main: {
                    build: { rollupOptions: { input: resolve(__dirname, "src/main/index.ts") } },
                },
                preload: {
                    build: {
                        rollupOptions: {
                            input: {
                                index: resolve(__dirname, "src/preload/index.ts"),
                                worker: resolve(__dirname, "src/preload/worker.ts"),
                            },
                        },
                    },
                },
            };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        let entries = entry_strings(&result);
        assert!(entries.contains(&"src/main/index.ts".to_string()));
        assert!(entries.contains(&"src/preload/index.ts".to_string()));
        assert!(entries.contains(&"src/preload/worker.ts".to_string()));
    }

    #[test]
    fn resolve_config_plain_string_input_form() {
        let source = r#"
            export default {
                renderer: {
                    build: { rollupOptions: { input: { index: "src/renderer/index.html" } } },
                },
            };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(entry_strings(&result).contains(&"src/renderer/index.html".to_string()));
    }

    #[test]
    fn resolve_config_normalizes_relative_to_config_dir_in_monorepo() {
        // Config in a subpackage: `resolve(__dirname, 'src/renderer/index.html')`
        // must seed `apps/desktop/src/renderer/index.html`, not a root-relative miss.
        let source = r#"
            import { resolve } from "node:path";
            export default {
                renderer: {
                    build: {
                        rollupOptions: {
                            input: { index: resolve(__dirname, "src/renderer/index.html") },
                        },
                    },
                },
            };
        "#;
        let result = ElectronPlugin.resolve_config(
            Path::new("/project/apps/desktop/electron.vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            entry_strings(&result),
            vec!["apps/desktop/src/renderer/index.html".to_string()]
        );
    }

    #[test]
    fn resolve_config_empty_or_malformed_config_yields_no_entries() {
        assert!(
            ElectronPlugin
                .resolve_config(&config_path(), "", Path::new("/project"))
                .entry_patterns
                .is_empty()
        );
        // No rollupOptions.input declared.
        let source = r"export default { renderer: { build: {} } };";
        assert!(
            ElectronPlugin
                .resolve_config(&config_path(), source, Path::new("/project"))
                .entry_patterns
                .is_empty()
        );
    }
}
