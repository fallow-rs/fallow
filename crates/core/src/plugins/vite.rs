//! Vite bundler plugin.
//!
//! Detects Vite projects and marks conventional entry points and config files.
//! Parses vite config to extract entry points, dependency references, and SSR externals.

use super::config_parser;
use super::{Plugin, PluginResult};

const CONFIG_EXPORTS: &[&str] = &["default"];

fn additional_data_entry_pattern(
    root: &std::path::Path,
    source: &fallow_extract::css::CssImportSource,
) -> Option<String> {
    let normalized = source.normalized.trim_start_matches("./");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || is_additional_data_package_import(root, source, normalized)
    {
        return None;
    }
    Some(normalized.to_string())
}

fn additional_data_package_name(
    root: &std::path::Path,
    source: &fallow_extract::css::CssImportSource,
) -> Option<String> {
    let normalized = source.normalized.trim_start_matches("./");
    is_additional_data_package_import(root, source, normalized)
        .then(|| crate::resolve::extract_package_name(&source.raw))
}

fn is_additional_data_package_import(
    root: &std::path::Path,
    source: &fallow_extract::css::CssImportSource,
    normalized: &str,
) -> bool {
    let raw = source.raw.as_str();
    if raw.starts_with('.') || raw.starts_with('/') || raw.contains(':') {
        return false;
    }
    if local_style_candidate_exists(root, normalized) {
        return false;
    }
    true
}

fn local_style_candidate_exists(root: &std::path::Path, normalized: &str) -> bool {
    let path = std::path::Path::new(normalized);
    let exact = root.join(path);
    if exact.is_file() {
        return true;
    }

    let has_style_ext = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "css" | "scss" | "sass" | "less" | "stylus"
        )
    });
    if has_style_ext {
        return false;
    }

    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let with_parent =
        |name: &str| parent.map_or_else(|| root.join(name), |parent| root.join(parent).join(name));

    ["scss", "sass", "css", "less", "stylus"].iter().any(|ext| {
        with_parent(&format!("{file_name}.{ext}")).is_file()
            || with_parent(&format!("_{file_name}.{ext}")).is_file()
            || root.join(path).join(format!("_index.{ext}")).is_file()
            || root.join(path).join(format!("index.{ext}")).is_file()
    })
}

define_plugin!(
    struct VitePlugin => "vite",
    enablers: &["vite", "rolldown-vite"],
    entry_patterns: &[
        "src/main.{ts,tsx,js,jsx}",
        "src/index.{ts,tsx,js,jsx}",
        "index.html",
    ],
    config_patterns: &["vite.config.{ts,js,mts,mjs}"],
    always_used: &["vite.config.{ts,js,mts,mjs}"],
    tooling_dependencies: &["vite", "@vitejs/plugin-react", "@vitejs/plugin-vue"],
    virtual_module_prefixes: &["virtual:"],
    used_exports: [("vite.config.{ts,js,mts,mjs}", CONFIG_EXPORTS)],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }
        result.referenced_dependencies.extend(
            config_parser::extract_vite_react_babel_dependencies(source, config_path),
        );

        result.referenced_dependencies.extend(super::react_compiler::extract_dependencies(
            source,
            config_path,
            &[&["plugins"]],
        ));

        for (find, replacement) in
            config_parser::extract_config_path_aliases(source, config_path, &["resolve", "alias"])
        {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }

        super::test_alias::apply_test_block_aliases(&mut result, source, config_path, root);

        let rollup_input = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "rollupOptions", "input"],
        );
        result.extend_entry_patterns(rollup_input);

        let lib_entry = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "lib", "entry"],
        );
        result.extend_entry_patterns(lib_entry);

        let optimize_include = config_parser::extract_config_string_array(
            source,
            config_path,
            &["optimizeDeps", "include"],
        );
        for dep in &optimize_include {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let optimize_exclude = config_parser::extract_config_string_array(
            source,
            config_path,
            &["optimizeDeps", "exclude"],
        );
        for dep in &optimize_exclude {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let ssr_external =
            config_parser::extract_config_string_array(source, config_path, &["ssr", "external"]);
        for dep in &ssr_external {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        let ssr_no_external =
            config_parser::extract_config_string_array(source, config_path, &["ssr", "noExternal"]);
        for dep in &ssr_no_external {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        for preprocessor in ["scss", "sass", "less", "stylus"] {
            let body = config_parser::extract_config_string_or_array(
                source,
                config_path,
                &["css", "preprocessorOptions", preprocessor, "additionalData"],
            );
            let is_scss_like = matches!(preprocessor, "scss" | "sass");
            for blob in body {
                for spec in fallow_extract::css::extract_css_import_sources(&blob, is_scss_like) {
                    if let Some(dep) = additional_data_package_name(root, &spec) {
                        result.referenced_dependencies.push(dep);
                    }
                    if let Some(pattern) = additional_data_entry_pattern(root, &spec) {
                        result.push_entry_pattern(pattern);
                    }
                }
            }
        }

        result
    },
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_ssr_external() {
        let source = r#"
            export default {
                ssr: {
                    external: ["lodash", "express"],
                    noExternal: ["my-ui-lib"]
                }
            };
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"lodash".to_string()));
        assert!(deps.contains(&"express".to_string()));
        assert!(deps.contains(&"my-ui-lib".to_string()));
    }

    #[test]
    fn resolve_config_optimize_deps_exclude() {
        let source = r#"
            export default {
                optimizeDeps: {
                    include: ["react"],
                    exclude: ["@my/heavy-dep"]
                }
            };
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"@my/heavy-dep".to_string()));
    }

    #[test]
    fn resolve_config_credits_react_babel_plugin_dependencies() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: [["module:@preact/signals-react-transform", {}]],
                            presets: ["@babel/preset-react"],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"@preact/signals-react-transform".to_string()),
            "React Babel plugin dependency should be credited: {deps:?}"
        );
        assert!(
            deps.contains(&"@babel/preset-react".to_string()),
            "React Babel preset dependency should be credited: {deps:?}"
        );
    }

    #[test]
    fn resolve_config_extracts_aliases() {
        let source = r#"
            import { defineConfig } from 'vite';
            import { fileURLToPath, URL } from 'node:url';

            export default defineConfig({
                resolve: {
                    alias: {
                        "@": fileURLToPath(new URL("./src", import.meta.url))
                    }
                }
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![("@".to_string(), "src".to_string())]
        );
    }

    #[test]
    fn resolve_config_extracts_embedded_test_alias_and_project_resolve_alias() {
        let source = r#"
            import { defineConfig } from 'vite';
            export default defineConfig({
                resolve: { alias: { "@": "./src" } },
                test: {
                    alias: { vscode: "./test/mock/vscode.ts" },
                    projects: [
                        { test: { name: "browser" }, resolve: { alias: { "test-alias-from-vite": "./mock/to.ts" } } }
                    ]
                }
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .path_aliases
                .contains(&("vscode".to_string(), "test/mock/vscode.ts".to_string())),
            "test.alias in vite.config must be extracted: {:?}",
            result.path_aliases
        );
        assert!(
            result
                .path_aliases
                .contains(&("test-alias-from-vite".to_string(), "mock/to.ts".to_string())),
            "test.projects[*].resolve.alias in vite.config must be extracted: {:?}",
            result.path_aliases
        );
        assert!(
            result
                .path_aliases
                .contains(&("@".to_string(), "src".to_string())),
            "top-level resolve.alias unchanged: {:?}",
            result.path_aliases
        );
    }

    #[test]
    fn resolve_config_additional_data_marks_package_imports_as_referenced_dependencies() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let source = r#"
            import { defineConfig } from 'vite';

            export default defineConfig({
                css: {
                    preprocessorOptions: {
                        scss: { additionalData: `@use "bootstrap/scss/functions"; @use "bulma";` },
                    },
                },
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(&tmp.path().join("vite.config.ts"), source, tmp.path());

        assert!(
            result
                .referenced_dependencies
                .contains(&"bootstrap".to_string()),
            "additionalData package imports should credit the package dependency"
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"bulma".to_string()),
            "bare additionalData package imports should credit the package dependency"
        );
        assert!(
            !result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "bootstrap/scss/functions"),
            "package imports should not be seeded as project entry globs"
        );
        assert!(
            !result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "bulma"),
            "bare package imports should not be seeded as project entry globs"
        );
    }

    #[test]
    fn resolve_config_rollup_input_evaluates_path_helpers() {
        let source = r#"
            import { resolve, join } from "node:path";
            import path from "node:path";
            import { defineConfig } from "vite";

            export default defineConfig({
                build: {
                    rollupOptions: {
                        input: {
                            app: resolve(__dirname, "src/app.ts"),
                            modal: path.resolve(__dirname, "src/modal.ts"),
                            tabs: join(__dirname, "src/tabs.ts"),
                            timetable: resolve(import.meta.dirname, "src/timetable.ts"),
                            styles: resolve(__dirname, "src/index.css"),
                        },
                    },
                },
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        for expected in [
            "src/app.ts",
            "src/modal.ts",
            "src/tabs.ts",
            "src/timetable.ts",
            "src/index.css",
        ] {
            assert!(
                patterns.contains(&expected),
                "rollupOptions.input path-helper entry {expected} should be extracted: {patterns:?}"
            );
        }
    }

    #[test]
    fn resolve_config_lib_entry_evaluates_path_helper() {
        let source = r#"
            import { resolve } from "node:path";
            import { defineConfig } from "vite";

            export default defineConfig({
                build: {
                    lib: {
                        entry: resolve(__dirname, "src/index.ts"),
                    },
                },
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "src/index.ts"),
            "build.lib.entry path-helper call should be extracted: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_react_babel_plugin_references_react_compiler_dependency() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: ["babel-plugin-react-compiler"],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_react_babel_plugin_tuple_references_react_compiler_dependency() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: [["react-compiler", { target: "19" }]],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_rolldown_babel_plugin_references_react_compiler_dependency() {
        let source = r#"
            import { defineConfig } from "vite";
            import { babel } from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [
                    babel({
                        babel: {
                            plugins: ["babel-plugin-react-compiler"],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_react_compiler_preset_call_references_dependency() {
        let source = r#"
            import { defineConfig } from "vite";
            import react, { reactCompilerPreset } from "@vitejs/plugin-react";
            import babel from "@rolldown/plugin-babel";

            export default defineConfig({
                plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_unrelated_string_does_not_reference_react_compiler_dependency() {
        let source = r#"
            import { defineConfig } from "vite";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        notes: "babel-plugin-react-compiler",
                        babel: {
                            plugins: [["other-plugin", { note: "babel-plugin-react-compiler" }]],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            !result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_requires_imported_vite_plugin_call_provenance() {
        let source = r#"
            import { defineConfig } from "vite";

            function react(options) {
                return options;
            }

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: ["babel-plugin-react-compiler"],
                        },
                    }),
                ],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            !result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_local_react_compiler_preset_call_does_not_reference_dependency() {
        let source = r#"
            import { defineConfig } from "vite";

            function reactCompilerPreset() {
                return {};
            }

            export default defineConfig({
                plugins: [reactCompilerPreset()],
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            !result
                .referenced_dependencies
                .contains(&"babel-plugin-react-compiler".to_string())
        );
    }

    #[test]
    fn resolve_config_additional_data_keeps_existing_local_style_entries() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(tmp.path().join("src/styles")).expect("create styles dir");
        std::fs::write(tmp.path().join("src/styles/_tokens.scss"), "$primary: red;")
            .expect("write local partial");

        let source = r#"
            import { defineConfig } from 'vite';

            export default defineConfig({
                css: {
                    preprocessorOptions: {
                        scss: { additionalData: `@use "src/styles/tokens";` },
                    },
                },
            });
        "#;
        let plugin = VitePlugin;
        let result = plugin.resolve_config(&tmp.path().join("vite.config.ts"), source, tmp.path());

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "src/styles/tokens"),
            "existing local style references should remain entry patterns"
        );
        assert!(
            !result.referenced_dependencies.contains(&"src".to_string()),
            "local style references should not be misclassified as packages"
        );
    }
}
