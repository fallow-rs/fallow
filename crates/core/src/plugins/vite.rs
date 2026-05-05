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
    // Non-relative stylesheet specifiers with no local candidate are package
    // references, including bare packages like `bootstrap`.
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
    // Vite plugins create virtual modules with `virtual:` prefix
    // (e.g., `virtual:pwa-register`, `virtual:emoji-mart-lang-importer`)
    virtual_module_prefixes: &["virtual:"],
    // Under --include-entry-exports, the default export of vite.config.* is the
    // entry: Vite's CLI consumes it. Marking it framework-used prevents the
    // false-positive in #282 (mirrors the vitest fix in #271).
    used_exports: [("vite.config.{ts,js,mts,mjs}", CONFIG_EXPORTS)],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        for (find, replacement) in
            config_parser::extract_config_aliases(source, config_path, &["resolve", "alias"])
        {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }

        // build.rollupOptions.input → entry points (string, array, or object)
        let rollup_input = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "rollupOptions", "input"],
        );
        result.extend_entry_patterns(rollup_input);

        // build.lib.entry → entry points (string or array)
        let lib_entry = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["build", "lib", "entry"],
        );
        result.extend_entry_patterns(lib_entry);

        // optimizeDeps.include → referenced dependencies
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

        // optimizeDeps.exclude → referenced dependencies
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

        // ssr.external → referenced dependencies
        let ssr_external =
            config_parser::extract_config_string_array(source, config_path, &["ssr", "external"]);
        for dep in &ssr_external {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        // ssr.noExternal → referenced dependencies
        let ssr_no_external =
            config_parser::extract_config_string_array(source, config_path, &["ssr", "noExternal"]);
        for dep in &ssr_no_external {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(dep));
        }

        // css.preprocessorOptions.{scss,sass,less,stylus}.additionalData →
        // SCSS / Sass strings injected at the top of every preprocessed file.
        // The string body itself is not parsed, but `@use` / `@import` /
        // `@forward` / `@plugin` directives inside it reference real files that no source
        // file imports directly. Seed those files as entry points so they do
        // not get reported as `unused-files`. Function-form `additionalData`
        // is skipped (out of static-analysis scope) and stylesheet content is
        // the only string treated as preprocessor source. Specifiers are
        // stripped of their leading `./` because entry patterns are matched
        // against project-relative paths via globset (which does not normalize
        // `./` prefixes). See issue #195 (Case A).
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
