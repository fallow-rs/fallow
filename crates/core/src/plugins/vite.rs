//! Vite bundler plugin.
//!
//! Detects Vite projects and marks conventional entry points and config files.
//! Parses vite config to extract entry points, dependency references, and SSR externals.

use super::config_parser;
use super::{Plugin, PluginResult};

const CONFIG_EXPORTS: &[&str] = &["default"];

/// Vite ships the CSS integrations but not the implementation packages: naming
/// one under either of these keys makes the build fail until the project
/// installs it itself. The value-to-package rules are catalogue rows under the
/// `vite-css-implementation` surface.
const CSS_IMPLEMENTATION_SELECTOR_PATHS: &[&[&str]] =
    &[&["css", "transformer"], &["build", "cssMinify"]];

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

/// Credit the CSS implementation package named under one of the CSS keys.
///
/// The first key that carries a catalogued value wins; Vite loads one
/// implementation, so a second credit would exempt a package it never requires.
fn add_css_implementation_dependency(
    result: &mut PluginResult,
    source: &str,
    config_path: &std::path::Path,
) {
    for selector in CSS_IMPLEMENTATION_SELECTOR_PATHS {
        let Some(value) = config_parser::extract_config_string(source, config_path, selector)
        else {
            continue;
        };
        if super::credit_config_value(
            super::config_value_credits::CreditSurface::ViteCssImplementation,
            &value,
            result,
        ) {
            break;
        }
    }
}

/// Vite entries that sit under its root directory.
const ROOT_ENTRY_PATTERNS: &[&str] = &[
    "src/main.{ts,tsx,js,jsx}",
    "src/index.{ts,tsx,js,jsx}",
    "index.html",
];

/// Re-base the default entries under the directory that the `root` option names.
///
/// Vite resolves `root` as a filesystem path and reads `index.html` from it.
/// The static entry patterns stay at the config directory, so this adds only
/// the entries under a root other than that directory.
fn add_root_entry_patterns(
    result: &mut PluginResult,
    source: &str,
    config_path: &std::path::Path,
    root: &std::path::Path,
) {
    let Some(vite_root) = config_parser::extract_config_path(source, config_path, &["root"])
        .and_then(|raw| config_parser::normalize_filesystem_config_path(&raw, config_path, root))
    else {
        return;
    };
    let vite_root = vite_root.trim_end_matches('/');
    let config_dir = config_path
        .parent()
        .and_then(|dir| dir.strip_prefix(root).ok())
        .map(config_parser::path_to_config_string)
        .unwrap_or_default();
    if vite_root.is_empty() || vite_root == config_dir {
        return;
    }
    result.extend_entry_patterns(
        ROOT_ENTRY_PATTERNS
            .iter()
            .map(|pattern| format!("{vite_root}/{pattern}")),
    );
}

define_plugin!(
    struct VitePlugin => "vite",
    enablers: &["vite", "rolldown-vite"],
    entry_patterns: ROOT_ENTRY_PATTERNS,
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

        super::module_federation::apply_bundler_plugin_options(
            &mut result,
            source,
            config_path,
            root,
            super::module_federation::FederationBase::default(),
            "vite",
        );

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

        add_root_entry_patterns(&mut result, source, config_path, root);

        let rollup_input = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "rollupOptions", "input"],
        );
        result.extend_entry_patterns_and_dependencies(rollup_input, root);

        let lib_entry = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "lib", "entry"],
        );
        // Vite resolves `lib.entry` against its root with `path.resolve`, so the
        // value is a path only and never names a package.
        result.extend_entry_paths(lib_entry);

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

        add_css_implementation_dependency(&mut result, source, config_path);

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
    use std::path::Path;

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
    fn resolve_config_reads_alias_replacement_through_local_path_helper() {
        let source = r"
            import { defineConfig } from 'vite';
            import { fileURLToPath, URL } from 'node:url';
            import path from 'node:path';

            const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));
            function fromRoot(p) { return path.resolve(__dirname, p); }

            export default defineConfig({
                resolve: {
                    alias: [
                        { find: 'local-a', replacement: here('src/a.ts') },
                        { find: 'local-b', replacement: fromRoot('lib/b.ts') },
                    ]
                }
            });
        ";
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert_eq!(
            result.path_aliases,
            vec![
                ("local-a".to_string(), "src/a.ts".to_string()),
                ("local-b".to_string(), "lib/b.ts".to_string()),
            ]
        );
    }

    #[test]
    fn resolve_config_skips_alias_helper_with_two_parameters_or_a_condition() {
        let source = r"
            const two = (base, p) => path.resolve(base, p);
            const pick = (p) => (process.env.X ? p : './other');
            export default {
                resolve: {
                    alias: {
                        'two-a': two(__dirname, 'src/a.ts'),
                        'pick-b': pick('./src/b.ts'),
                    }
                }
            };
        ";
        let plugin = VitePlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(result.path_aliases.is_empty(), "{:?}", result.path_aliases);
    }

    /// Issue #2806: Vite reads a leading `/` as relative to the project root.
    #[test]
    fn resolve_config_reads_a_leading_slash_alias_as_root_relative() {
        let source = r#"
            export default {
                resolve: { alias: { "@": "/src" } },
            };
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

    fn entry_pattern_strings(result: &PluginResult) -> Vec<&str> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect()
    }

    #[test]
    fn resolve_config_evaluates_a_local_path_helper_call() {
        let source = r"
            import { fileURLToPath } from 'node:url';
            import { resolve } from 'node:path';

            const fromHere = (rel: string) => fileURLToPath(new URL(rel, import.meta.url));
            function fromDir(rel) {
                return resolve(__dirname, rel);
            }
            const viaPathname = function (rel) { return new URL(rel, import.meta.url).pathname; };

            export default {
                resolve: { alias: { 'some-mod': fromHere('./src/stub.ts') } },
                build: {
                    rollupOptions: {
                        input: {
                            main: fromHere('./src/main.ts'),
                            second: fromDir('src/second.ts'),
                            third: viaPathname('./src/third.ts'),
                        },
                    },
                },
            };
        ";
        let result = VitePlugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        let patterns = entry_pattern_strings(&result);
        for expected in ["src/main.ts", "src/second.ts", "src/third.ts"] {
            assert!(
                patterns.contains(&expected),
                "local helper call {expected} should be an entry: {patterns:?}"
            );
        }
        assert_eq!(
            result.path_aliases,
            vec![("some-mod".to_string(), "src/stub.ts".to_string())]
        );
    }

    #[test]
    fn resolve_config_ignores_a_local_helper_with_an_unsupported_body() {
        let source = r"
            const base = 'src';
            const fromBase = (rel) => `${base}/${rel}`;
            const twoArgs = (dir, rel) => dir + rel;
            const nested = (rel) => fromBase(rel);

            export default {
                resolve: { alias: { 'some-mod': fromBase('stub.ts') } },
                build: {
                    rollupOptions: {
                        input: {
                            a: fromBase('a.ts'),
                            b: twoArgs('src', '/b.ts'),
                            c: nested('c.ts'),
                        },
                    },
                },
            };
        ";
        let result = VitePlugin.resolve_config(
            std::path::Path::new("/project/vite.config.ts"),
            source,
            std::path::Path::new("/project"),
        );

        let patterns = entry_pattern_strings(&result);
        for unexpected in ["a.ts", "src/a.ts", "src/b.ts", "c.ts", "src/c.ts"] {
            assert!(
                !patterns.contains(&unexpected),
                "an unsupported helper body must yield nothing, found {unexpected}: {patterns:?}"
            );
        }
        assert!(result.path_aliases.is_empty(), "{:?}", result.path_aliases);
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

    #[test]
    fn resolve_config_lightningcss_transformer_credits_dependency() {
        let source = r#"export default defineConfig({ css: { transformer: "lightningcss" } });"#;
        let result = VitePlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lightningcss".to_string()),
            "{:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn resolve_config_lightningcss_css_minify_credits_dependency() {
        let source = r#"export default defineConfig({ build: { cssMinify: "lightningcss" } });"#;
        let result = VitePlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lightningcss".to_string())
        );
    }

    #[test]
    fn resolve_config_default_css_pipeline_does_not_credit_lightningcss() {
        let source = r#"export default defineConfig({ css: { transformer: "postcss" }, build: { cssMinify: "esbuild" } });"#;
        let result = VitePlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"lightningcss".to_string()),
            "the default pipeline must not exempt an unused lightningcss"
        );
    }

    #[test]
    fn resolve_config_lightningcss_string_under_another_key_does_not_credit_dependency() {
        let source = r#"export default defineConfig({ define: { __CSS_ENGINE__: "lightningcss" }, build: { minify: "lightningcss" } });"#;
        let result = VitePlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"lightningcss".to_string()),
            "only the two selector keys make the package load-bearing"
        );
    }

    #[test]
    fn resolve_config_reads_inline_module_federation_options() {
        let source = r#"
            import { federation } from "@module-federation/vite";

            export default defineConfig({
                plugins: [
                    federation({
                        name: "host",
                        exposes: { "./Button": "./src/Button.tsx" },
                        remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
                    }),
                ],
            });
        "#;
        let result = VitePlugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "src/Button.tsx")
        );
        assert_eq!(result.provided_dependencies.len(), 1);
        assert!(result.provided_dependencies[0].covers_specifier("checkout/Button"));
    }

    /// `build.rollupOptions.input` reads a bare value through both channels,
    /// while `build.lib.entry` is a path that vite resolves against its root,
    /// so it never credits a package (issue #2753).
    #[test]
    fn a_bare_rollup_input_credits_the_package_and_lib_entry_stays_a_path() {
        let source = r#"
            export default {
                build: {
                    rollupOptions: { input: "my-lib/client" },
                    lib: { entry: "src/lib" },
                },
            };
        "#;
        let result =
            VitePlugin.resolve_config(Path::new("vite.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"my-lib".to_string()),
            "got {:?}",
            result.referenced_dependencies
        );
        assert!(
            !result.referenced_dependencies.contains(&"src".to_string()),
            "lib.entry is a path only, got {:?}",
            result.referenced_dependencies
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert!(patterns.contains(&"my-lib/client"), "got {patterns:?}");
        assert!(
            patterns
                .iter()
                .any(|pattern| pattern.starts_with("src/lib.{")),
            "an extensionless lib entry resolves to the file, got {patterns:?}"
        );
    }

    fn entry_patterns_for(source: &str) -> Vec<String> {
        VitePlugin
            .resolve_config(
                Path::new("/project/vite.config.ts"),
                source,
                Path::new("/project"),
            )
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    const REBASED_WEB_ENTRIES: &[&str] = &[
        "web/index.html",
        "web/src/main.{ts,tsx,js,jsx}",
        "web/src/index.{ts,tsx,js,jsx}",
    ];

    #[test]
    fn resolve_config_root_rebases_the_default_entries() {
        for source in [
            r#"export default { root: "./web" };"#,
            r#"export default { root: "web/" };"#,
            r#"
                import { resolve } from "node:path";
                export default { root: resolve(__dirname, "web") };
            "#,
            r#"
                import { fileURLToPath, URL } from "node:url";
                export default { root: fileURLToPath(new URL("./web", import.meta.url)) };
            "#,
        ] {
            let patterns = entry_patterns_for(source);
            for expected in REBASED_WEB_ENTRIES {
                assert!(
                    patterns.iter().any(|pattern| pattern == expected),
                    "root should re-base the entry {expected}: {patterns:?} for {source}"
                );
            }
        }
    }

    #[test]
    fn resolve_config_root_is_read_relative_to_the_config_directory() {
        let result = VitePlugin.resolve_config(
            Path::new("/project/config/vite.config.ts"),
            r#"export default { root: "../web" };"#,
            Path::new("/project"),
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "web/index.html"),
            "got {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_without_a_nested_root_adds_no_rebased_entries() {
        for source in [
            "export default { build: { outDir: \"dist\" } };",
            r#"export default { root: "." };"#,
            r#"export default { root: "./" };"#,
        ] {
            let patterns = entry_patterns_for(source);
            assert!(
                !patterns
                    .iter()
                    .any(|pattern| pattern.ends_with("index.html")),
                "no root means no extra entry: {patterns:?} for {source}"
            );
        }
    }
}
