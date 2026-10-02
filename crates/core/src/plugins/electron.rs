//! Electron plugin.
//!
//! Detects Electron projects and marks main/preload entry points and tool config files as always used.
//!
//! Without an electron-vite config, every file under `src/main` and
//! `src/preload` is an entry. With an electron-vite config, the plugin reads the
//! entries that electron-vite builds: the declared `build.rollupOptions.input`
//! or `build.lib.entry`, else the electron-vite default `src/<section>/index`.

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

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

/// electron-vite sections that build Node entries. electron-vite gives each a
/// default entry when the section declares none.
const NODE_SECTIONS: &[&str] = &["main", "preload"];

/// Property paths inside a section that declare the section entries.
const SECTION_ENTRY_PATHS: &[&[&str]] = &[
    &["build", "rollupOptions", "input"],
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

define_plugin! {
    struct ElectronPlugin => "electron",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
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
            let inputs = config_parser::extract_config_string_or_array(
                source,
                config_path,
                &[section, "build", "rollupOptions", "input"],
            );
            push_config_entries(&mut result, inputs, config_path, root);
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
    fn resolve_config_empty_or_malformed_config_yields_no_entries() {
        assert!(
            ElectronPlugin
                .resolve_config(&config_path(), "", Path::new("/project"))
                .entry_patterns
                .is_empty()
        );
        let source = r"export default { renderer: { build: {} } };";
        assert!(
            ElectronPlugin
                .resolve_config(&config_path(), source, Path::new("/project"))
                .entry_patterns
                .is_empty()
        );
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
    fn resolve_config_without_node_sections_keeps_static_entries() {
        let source = r#"export default { renderer: { build: { rollupOptions: { input: "src/renderer/index.html" } } } };"#;
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(!result.replace_entry_patterns);
        assert_eq!(
            entry_strings(&result),
            vec!["src/renderer/index.html".to_string()]
        );
    }

    #[test]
    fn resolve_config_without_config_object_keeps_static_entries() {
        let source = "export default makeConfig();";
        let result = ElectronPlugin.resolve_config(&config_path(), source, Path::new("/project"));
        assert!(result.entry_patterns.is_empty());
    }
}
