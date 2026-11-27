//! Electron plugin.
//!
//! Detects Electron projects and marks main/preload entry points and tool config files as always used.
//!
//! Without an electron-vite config, every file under `src/main` and
//! `src/preload` is an entry. With an electron-vite config, the plugin reads the
//! entries that electron-vite builds: the declared `build.rollupOptions.input`
//! (`build.rolldownOptions.input` with Vite 8) or `build.lib.entry`, else the
//! electron-vite default `src/<section>/index`.
//! A renderer section without an input gets the default `src/renderer/index.html`.
//!
//! In a Forge config, the plugin reads the `entry` and `config` paths that the
//! Forge `VitePlugin` builds. See [`forge`].

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

use super::config_parser;
use super::{Plugin, PluginResult};

#[path = "electron_forge.rs"]
mod forge;

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
    // electron-builder reads these names without `--config`; it skips `.mjs`.
    "electron-builder.{yml,yaml,json,json5,toml,js,cjs,ts}",
    "forge.config.{ts,js,cjs}",
    "electron.vite.config.{ts,js,mjs}",
];

const CONFIG_PATTERNS: &[&str] = &[
    "electron.vite.config.{ts,js,mjs}",
    "forge.config.{ts,js,cjs}",
];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[
    ("forge.config.{ts,js,cjs}", &["default"]),
    ("electron-builder.{js,cjs,ts}", &["default"]),
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "electron",
    "electron-builder",
    "electron-vite",
    "@electron/rebuild",
    "@electron-forge/cli",
];

/// electron-vite top-level sections. Each is a Vite config with its own
/// bundler `input`.
const VITE_SECTIONS: &[&str] = &["main", "preload", "renderer"];

/// electron-vite sections that build Node entries. electron-vite gives each a
/// default entry when the section declares none.
const NODE_SECTIONS: &[&str] = &["main", "preload"];

/// The renderer input that electron-vite builds, relative to the config root,
/// when the renderer section declares none.
const RENDERER_DEFAULT_ENTRY: &str = "src/renderer/index.html";

/// Property paths inside a section that declare the section entries.
const SECTION_ENTRY_PATHS: &[&[&str]] = &[
    &["build", "rollupOptions", "input"],
    &["build", "rolldownOptions", "input"],
    &["build", "lib", "entry"],
];

/// How a Node section of an electron-vite config declares its entries.
#[derive(Debug, PartialEq, Eq)]
enum SectionEntries {
    /// The config has no such section, so electron-vite does not build it.
    Absent,
    /// The section declares no entry, so electron-vite uses its default entry.
    Default,
    /// The section declares at least one entry.
    Declared,
    /// The section holds a value that static analysis cannot read.
    Unknown,
}

/// Classify the entry declaration of each Node section, or `None` when the
/// config has no statically readable config object.
fn classify_node_sections(
    source: &str,
    config_path: &std::path::Path,
) -> Option<Vec<(&'static str, SectionEntries)>> {
    config_parser::extract_from_source(source, config_path, |program| {
        let config = config_parser::find_config_object(program)?;
        Some(
            NODE_SECTIONS
                .iter()
                .map(|&section| (section, classify_section_object(config, section)))
                .collect(),
        )
    })
}

fn classify_renderer_section(
    source: &str,
    config_path: &std::path::Path,
) -> Option<SectionEntries> {
    config_parser::extract_from_source(source, config_path, |program| {
        config_parser::find_config_object(program)
            .map(|config| classify_section_object(config, "renderer"))
    })
}

fn classify_section_object(config: &ObjectExpression<'_>, section: &str) -> SectionEntries {
    let Some(section_expr) = config_parser::property_expr(config, section) else {
        return if has_spread(config) {
            SectionEntries::Unknown
        } else {
            SectionEntries::Absent
        };
    };
    let Some(section_obj) = config_parser::object_expression(section_expr) else {
        return SectionEntries::Unknown;
    };
    let mut declared = false;
    for path in SECTION_ENTRY_PATHS {
        match classify_entry_path(section_obj, path) {
            SectionEntries::Unknown => return SectionEntries::Unknown,
            SectionEntries::Declared => declared = true,
            SectionEntries::Absent | SectionEntries::Default => {}
        }
    }
    if declared {
        SectionEntries::Declared
    } else {
        SectionEntries::Default
    }
}

/// Walk one entry property path. A missing property means no declaration. A
/// spread, or a value that is not an object literal where the path continues,
/// means the declaration cannot be read.
fn classify_entry_path(section: &ObjectExpression<'_>, path: &[&str]) -> SectionEntries {
    let Some((last, parents)) = path.split_last() else {
        return SectionEntries::Default;
    };
    let mut obj = section;
    for key in parents {
        let Some(expr) = config_parser::property_expr(obj, key) else {
            return missing_property(obj);
        };
        let Some(next) = config_parser::object_expression(expr) else {
            return SectionEntries::Unknown;
        };
        obj = next;
    }
    match config_parser::property_expr(obj, last) {
        None => missing_property(obj),
        Some(expr) if config_parser::is_disabled_expression(expr) || is_undefined(expr) => {
            SectionEntries::Default
        }
        Some(_) => SectionEntries::Declared,
    }
}

/// A property that the object literal does not name. A spread element can
/// still supply it.
fn missing_property(obj: &ObjectExpression<'_>) -> SectionEntries {
    if has_spread(obj) {
        SectionEntries::Unknown
    } else {
        SectionEntries::Default
    }
}

fn has_spread(obj: &ObjectExpression<'_>) -> bool {
    obj.properties
        .iter()
        .any(|prop| matches!(prop, ObjectPropertyKind::SpreadProperty(_)))
}

fn is_undefined(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Identifier(ident) if ident.name == "undefined")
}

/// Push each statically readable path as an entry pattern relative to the
/// config file directory. Returns true when at least one path was pushed.
fn push_config_entries(
    result: &mut PluginResult,
    values: Vec<String>,
    config_path: &std::path::Path,
    root: &std::path::Path,
) -> bool {
    let mut pushed = false;
    for value in values {
        if let Some(normalized) = config_parser::normalize_config_path(&value, config_path, root) {
            result.push_entry_pattern(normalized);
            pushed = true;
        }
    }
    pushed
}

/// Push the entries of one Node section. Falls back to the broad static glob
/// for the section when its declaration cannot be read.
fn push_node_section_entries(
    result: &mut PluginResult,
    source: &str,
    config_path: &std::path::Path,
    root: &std::path::Path,
    section: &str,
    kind: &SectionEntries,
) {
    let declared = SECTION_ENTRY_PATHS
        .iter()
        .flat_map(|path| {
            let mut full = vec![section];
            full.extend_from_slice(path);
            config_parser::extract_config_string_or_array(source, config_path, &full)
        })
        .collect::<Vec<_>>();
    if push_config_entries(result, declared, config_path, root) {
        return;
    }
    let fallback = match kind {
        SectionEntries::Absent => return,
        // electron-vite reads `src/<section>/{index,<section>}.{js,ts,mjs,cjs}`
        // relative to the config root when the section declares no entry.
        SectionEntries::Default => format!("src/{section}/{{index,{section}}}.{{js,ts,mjs,cjs}}"),
        SectionEntries::Declared | SectionEntries::Unknown => {
            format!("src/{section}/**/*.{{ts,tsx,js,jsx,mts,mjs}}")
        }
    };
    push_config_entries(result, vec![fallback], config_path, root);
}

fn is_forge_config(config_path: &std::path::Path) -> bool {
    config_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("forge.config."))
}

/// Mark the entries and Vite configs that the Forge `VitePlugin` builds as
/// used. The plugin loads the default export of each Vite config, and it
/// needs `vite` installed.
fn resolve_forge_config(
    config_path: &std::path::Path,
    source: &str,
    root: &std::path::Path,
) -> PluginResult {
    let mut result = PluginResult::default();
    let forge = forge::read_forge_vite(source, config_path);
    for entry in forge.entries {
        if let Some(normalized) = config_parser::normalize_config_path(&entry, config_path, root) {
            result.push_entry_pattern(globset::escape(&normalized));
        }
    }
    for vite_config in forge.vite_configs {
        if let Some(normalized) =
            config_parser::normalize_config_path(&vite_config, config_path, root)
        {
            let pattern = globset::escape(&normalized);
            result.push_entry_pattern(pattern.clone());
            result.push_entry_export_credit(pattern, ["default"]);
        }
    }
    if forge.names_plugin {
        result.referenced_dependencies.push("vite".to_string());
    }
    result
}

define_plugin! {
    struct ElectronPlugin => "electron",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        if is_forge_config(config_path) {
            return resolve_forge_config(config_path, source, root);
        }
        let mut result = PluginResult::default();

        // A config with a main or preload section names the Node entries that
        // electron-vite builds, so they replace the broad static globs. A config
        // that cannot be read, or that has neither section, keeps the globs.
        let node_sections = classify_node_sections(source, config_path).filter(|sections| {
            sections.iter().any(|(_, kind)| *kind != SectionEntries::Absent)
        });
        if let Some(node_sections) = &node_sections {
            result.replace_entry_patterns = true;
            for (section, kind) in node_sections {
                push_node_section_entries(&mut result, source, config_path, root, section, kind);
            }
        }

        let input_sections: &[&str] = if node_sections.is_some() { &["renderer"] } else { VITE_SECTIONS };
        for &section in input_sections {
            for options_key in super::vite::BUNDLER_OPTIONS_KEYS {
                let inputs = config_parser::extract_config_string_or_array(
                    source,
                    config_path,
                    &[section, "build", options_key, "input"],
                );
                push_config_entries(&mut result, inputs, config_path, root);
            }
        }
        if classify_renderer_section(source, config_path) == Some(SectionEntries::Default) {
            push_config_entries(
                &mut result,
                vec![RENDERER_DEFAULT_ENTRY.to_string()],
                config_path,
                root,
            );
        }

        result.referenced_dependencies.extend(super::react_compiler::extract_dependencies(
            source,
            config_path,
            &[&["main", "plugins"], &["preload", "plugins"], &["renderer", "plugins"]],
        ));

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
    fn resolve_config_empty_config_yields_no_entries() {
        assert!(
            ElectronPlugin
                .resolve_config(&config_path(), "", Path::new("/project"))
                .entry_patterns
                .is_empty()
        );
    }

    #[test]
    fn resolve_config_renderer_without_input_uses_electron_vite_default_entry() {
        let source = r"export default { renderer: { build: {} } };";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert_eq!(
            entry_strings(&result),
            vec![RENDERER_DEFAULT_ENTRY.to_string()]
        );
    }

    #[test]
    fn resolve_config_renderer_default_entry_sits_beside_node_defaults() {
        let source = r#"
            import { resolve } from "node:path";
            import { defineConfig, externalizeDepsPlugin } from "electron-vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                main: { plugins: [externalizeDepsPlugin()] },
                preload: { plugins: [externalizeDepsPlugin()] },
                renderer: {
                    resolve: { alias: { "@": resolve(__dirname, "src/renderer/src") } },
                    plugins: [react()],
                },
            });
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec![
                MAIN_DEFAULT.to_string(),
                PRELOAD_DEFAULT.to_string(),
                RENDERER_DEFAULT_ENTRY.to_string(),
            ]
        );
    }

    #[test]
    fn resolve_config_renderer_default_entry_keeps_monorepo_prefix() {
        let source = "export default { renderer: {} };";
        let result = ElectronPlugin.resolve_config(
            Path::new("/project/apps/desktop/electron.vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            entry_strings(&result),
            vec![format!("apps/desktop/{RENDERER_DEFAULT_ENTRY}")]
        );
    }

    #[test]
    fn resolve_config_unreadable_renderer_adds_no_default_entry() {
        for source in [
            "import shared from './shared';\nexport default { renderer: shared };",
            "import shared from './shared';\nexport default { renderer: { ...shared } };",
        ] {
            let result =
                ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
            assert!(result.entry_patterns.is_empty(), "source: {source}");
        }
    }

    #[test]
    fn resolve_config_credits_react_compiler_preset_in_renderer_plugins() {
        let source = r"
            import { defineConfig } from 'electron-vite'
            import react, { reactCompilerPreset } from '@vitejs/plugin-react'
            import babel from '@rolldown/plugin-babel'

            export default defineConfig({
                main: { build: { rollupOptions: { input: 'src/main/index.ts' } } },
                renderer: {
                    plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
                },
            })
        ";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string()),
            "react compiler preset in renderer.plugins should be credited: {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn resolve_config_local_react_compiler_preset_in_renderer_does_not_credit() {
        let source = r"
            import { defineConfig } from 'electron-vite'
            import babel from '@rolldown/plugin-babel'

            function reactCompilerPreset() {
                return {};
            }

            export default defineConfig({
                renderer: { plugins: [babel({ presets: [reactCompilerPreset()] })] },
            })
        ";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(
            !result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    const MAIN_DEFAULT: &str = "src/main/{index,main}.{js,ts,mjs,cjs}";
    const PRELOAD_DEFAULT: &str = "src/preload/{index,preload}.{js,ts,mjs,cjs}";
    const MAIN_BROAD: &str = "src/main/**/*.{ts,tsx,js,jsx,mts,mjs}";

    #[test]
    fn resolve_config_empty_node_sections_use_electron_vite_default_entries() {
        let source = "export default { main: {}, preload: {} };";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec![MAIN_DEFAULT.to_string(), PRELOAD_DEFAULT.to_string()]
        );
    }

    #[test]
    fn resolve_config_declared_inputs_replace_default_entries() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                main: { build: { lib: { entry: { app: resolve(__dirname, "src/main/app.ts") } } } },
                preload: { build: { rollupOptions: { input: ["src/preload/bridge.ts"] } } },
            };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec![
                "src/main/app.ts".to_string(),
                "src/preload/bridge.ts".to_string()
            ]
        );
    }

    #[test]
    fn resolve_config_default_entries_keep_monorepo_prefix() {
        let source = "export default { main: { build: {} } };";
        let result = ElectronPlugin.resolve_config(
            Path::new("/project/apps/desktop/electron.vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            entry_strings(&result),
            vec![format!("apps/desktop/{MAIN_DEFAULT}")]
        );
    }

    #[test]
    fn resolve_config_unreadable_section_keeps_broad_entries() {
        for source in [
            "import shared from './shared';\nexport default { main: shared };",
            "import shared from './shared';\nexport default { main: { ...shared } };",
            "export default { main: { build: { rollupOptions: { input: inputs } } } };",
        ] {
            let result =
                ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
            assert_eq!(
                entry_strings(&result),
                vec![MAIN_BROAD.to_string()],
                "source: {source}"
            );
        }
    }

    #[test]
    fn resolve_config_reads_sections_through_a_spread_of_a_local_constant() {
        let source = r#"
            const base = { main: { build: { lib: { entry: "src/main/app.ts" } } } };
            const shared = { build: {} };
            export default { ...base, preload: { ...shared } };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec!["src/main/app.ts".to_string(), PRELOAD_DEFAULT.to_string()]
        );
    }

    #[test]
    fn resolve_config_without_node_sections_keeps_static_entries() {
        let source = r#"export default { renderer: { build: { rollupOptions: { input: "src/renderer/index.html" } } } };"#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(!result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec!["src/renderer/index.html".to_string()]
        );
    }

    fn forge_config_path() -> std::path::PathBuf {
        std::path::PathBuf::from("/project/forge.config.ts")
    }

    #[test]
    fn forge_vite_plugin_names_entries_and_vite_configs() {
        let source = r#"
            import { VitePlugin } from "@electron-forge/plugin-vite";
            export default {
                plugins: [
                    new VitePlugin({
                        build: [{ entry: "src/main.ts", config: "vite.main.config.ts" }],
                        renderer: [{ name: "main_window", config: "vite.renderer.config.ts" }],
                    }),
                ],
            };
        "#;
        let result =
            ElectronPlugin.resolve_config(&forge_config_path(), source, Path::new("/project"));
        assert!(!result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec![
                "src/main.ts".to_string(),
                "vite.main.config.ts".to_string(),
                "vite.renderer.config.ts".to_string(),
            ]
        );
        let credits: Vec<(String, Vec<String>)> = result
            .entry_export_credits
            .iter()
            .map(|rule| (rule.path.pattern.clone(), rule.exports.clone()))
            .collect();
        assert_eq!(
            credits,
            vec![
                (
                    "vite.main.config.ts".to_string(),
                    vec!["default".to_string()]
                ),
                (
                    "vite.renderer.config.ts".to_string(),
                    vec!["default".to_string()]
                ),
            ]
        );
        assert_eq!(result.referenced_dependencies, vec!["vite".to_string()]);
    }

    #[test]
    fn forge_vite_plugin_reads_require_namespace_and_object_forms() {
        for source in [
            r#"
                const { VitePlugin: Vite } = require("@electron-forge/plugin-vite");
                const config = { plugins: [new Vite({ build: [{ entry: "src/main.ts", config: "vite.main.config.ts" }] })] };
                module.exports = config;
            "#,
            r#"
                import * as forgeVite from "@electron-forge/plugin-vite";
                export default { plugins: [new forgeVite.VitePlugin({ build: [{ entry: "src/main.ts", config: "vite.main.config.ts" }] })] };
            "#,
            r#"
                module.exports = {
                    plugins: [{
                        name: "@electron-forge/plugin-vite",
                        config: { build: [{ entry: "src/main.ts", config: "vite.main.config.ts" }] },
                    }],
                };
            "#,
        ] {
            let result =
                ElectronPlugin.resolve_config(&forge_config_path(), source, Path::new("/project"));
            assert_eq!(
                entry_strings(&result),
                vec!["src/main.ts".to_string(), "vite.main.config.ts".to_string()],
                "source: {source}"
            );
            assert_eq!(
                result.referenced_dependencies,
                vec!["vite".to_string()],
                "source: {source}"
            );
        }
    }

    #[test]
    fn forge_config_ignores_an_unrelated_vite_plugin_class() {
        let source = r#"
            class VitePlugin { constructor(options) { this.options = options; } }
            export default { plugins: [new VitePlugin({ build: [{ entry: "src/main.ts", config: "vite.main.config.ts" }] })] };
        "#;
        let result =
            ElectronPlugin.resolve_config(&forge_config_path(), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn forge_vite_paths_keep_monorepo_prefix() {
        let source = r#"
            import { VitePlugin } from "@electron-forge/plugin-vite";
            export default { plugins: [new VitePlugin({ renderer: [{ name: "w", config: "./vite.renderer.config.mts" }] })] };
        "#;
        let result = ElectronPlugin.resolve_config(
            Path::new("/project/apps/desktop/forge.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            entry_strings(&result),
            vec!["apps/desktop/vite.renderer.config.mts".to_string()]
        );
    }

    #[test]
    fn resolve_config_without_config_object_keeps_static_entries() {
        let source = "export default makeConfig();";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
    }

    /// Vite 8 names the bundler options `rolldownOptions` (issue #5385).
    #[test]
    fn resolve_config_reads_rolldown_options_input_of_each_section() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                main: { build: { rolldownOptions: { input: "electron/main.ts" } } },
                preload: { build: { rolldownOptions: { input: ["electron/bridge.ts"] } } },
                renderer: {
                    build: {
                        rolldownOptions: {
                            input: { index: resolve(__dirname, "ui/index.html") },
                        },
                    },
                },
            };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        let mut entries = entry_strings(&result);
        entries.sort();
        assert_eq!(
            entries,
            ["electron/bridge.ts", "electron/main.ts", "ui/index.html"],
            "each section should have its declared input and no default entry"
        );
        assert!(result.replace_entry_patterns);
    }

    #[test]
    fn resolve_config_rolldown_options_without_input_keeps_the_default_entry() {
        let source = r#"
            export default {
                main: { build: { rolldownOptions: { output: { format: "es" } } } },
            };
        "#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert_eq!(
            entry_strings(&result),
            ["src/main/{index,main}.{js,ts,mjs,cjs}"],
            "a section without an input should keep the electron-vite default entry"
        );
    }
}
