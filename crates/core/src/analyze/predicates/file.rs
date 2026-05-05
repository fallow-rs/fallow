/// Check if a path is a TypeScript declaration file (`.d.ts`, `.d.mts`, `.d.cts`).
pub(in crate::analyze) fn is_declaration_file(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
}

/// Check if a path is an HTML file.
///
/// HTML files are excluded from unused-file detection because they are entry-point-like:
/// nothing imports an HTML file, so "unused" is meaningless for them. They serve as
/// entry points in Vite/Parcel-style apps and their referenced assets are tracked
/// via `<script src>` and `<link href>` edges.
// Keep in sync with fallow_extract::html::is_html_file (crate boundary prevents sharing)
pub(in crate::analyze) fn is_html_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "html")
}

/// Check if a file is a configuration file consumed by tooling, not via imports.
///
/// These files should never be reported as unused because they are loaded by
/// their respective tools (e.g., Babel reads `babel.config.js`, `ESLint` reads
/// `eslint.config.ts`, etc.) rather than being imported by application code.
pub(in crate::analyze) fn is_config_file(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Dotfiles with "rc" suffix pattern (e.g., .secretlintrc.cjs, .commitlintrc.js, .prettierrc.js)
    // Only match files with "rc." before the extension — avoids false matches on arbitrary dotfiles.
    if name.starts_with('.') && !name.starts_with("..") {
        let lower = name.to_ascii_lowercase();
        // .foorc.{ext} pattern — standard for tool configs
        if lower.contains("rc.") {
            return true;
        }
    }

    // Files matching common config naming patterns.
    // Each pattern is a prefix — the file must start with it.
    let config_patterns = [
        // Build tools
        "babel.config.",
        "rollup.config.",
        "webpack.config.",
        "postcss.config.",
        "stencil.config.",
        "remotion.config.",
        "metro.config.",
        "tsup.config.",
        "unbuild.config.",
        "esbuild.config.",
        "swc.config.",
        "turbo.",
        // Testing
        "jest.config.",
        "jest.setup.",
        "vitest.config.",
        "vitest.ci.config.",
        "vitest.setup.",
        "vitest.workspace.",
        "playwright.config.",
        "cypress.config.",
        "karma.conf.",
        // Linting & formatting
        "eslint.config.",
        "prettier.config.",
        "stylelint.config.",
        "lint-staged.config.",
        "commitlint.config.",
        // Frameworks / CMS
        "next.config.",
        "next-sitemap.config.",
        "nuxt.config.",
        "astro.config.",
        "sanity.config.",
        "vite.config.",
        "tailwind.config.",
        "drizzle.config.",
        "knexfile.",
        "sentry.client.config.",
        "sentry.server.config.",
        "sentry.edge.config.",
        "react-router.config.",
        // Documentation
        "typedoc.",
        // Analysis & misc
        "knip.config.",
        "fallow.config.",
        "i18next-parser.config.",
        "codegen.config.",
        "graphql.config.",
        "npmpackagejsonlint.config.",
        "release-it.",
        "release.config.",
        "contentlayer.config.",
        // Environment declarations
        "next-env.d.",
        "env.d.",
        "vite-env.d.",
    ];

    config_patterns.iter().any(|p| name.starts_with(p))
}

/// Check if a module is a barrel file (only re-exports) whose sources are reachable.
///
/// A barrel file like `index.ts` that only contains `export { Foo } from './source'`
/// lines serves an organizational purpose. If the source modules are reachable,
/// the barrel file should not be reported as unused — consumers may have bypassed
/// it with direct imports, but the barrel still provides valid re-exports.
pub(in crate::analyze) fn is_barrel_with_reachable_sources(
    module: &crate::graph::ModuleNode,
    graph: &crate::graph::ModuleGraph,
) -> bool {
    // Must have re-exports
    if module.re_exports.is_empty() {
        return false;
    }

    // Must be a pure barrel: no local exports with real spans (only re-export-generated
    // exports have span 0..0) and no CJS exports
    let has_local_exports = module
        .exports
        .iter()
        .any(|e| e.span.start != 0 || e.span.end != 0);
    if has_local_exports || module.has_cjs_exports() {
        return false;
    }

    // At least one re-export source must be reachable
    module.re_exports.iter().any(|re| {
        let source_idx = re.source_file.0 as usize;
        graph
            .modules
            .get(source_idx)
            .is_some_and(|m| m.is_reachable())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Declaration file tests (Issue 4)
    #[test]
    fn declaration_file_dts() {
        assert!(is_declaration_file(std::path::Path::new("styled.d.ts")));
        assert!(is_declaration_file(std::path::Path::new(
            "src/types/styled.d.ts"
        )));
        assert!(is_declaration_file(std::path::Path::new("env.d.ts")));
    }

    #[test]
    fn declaration_file_dmts_dcts() {
        assert!(is_declaration_file(std::path::Path::new("module.d.mts")));
        assert!(is_declaration_file(std::path::Path::new("module.d.cts")));
    }

    #[test]
    fn not_declaration_file() {
        assert!(!is_declaration_file(std::path::Path::new("index.ts")));
        assert!(!is_declaration_file(std::path::Path::new("component.tsx")));
        assert!(!is_declaration_file(std::path::Path::new("utils.js")));
        assert!(!is_declaration_file(std::path::Path::new("styles.d.css")));
    }

    // Config file tests
    #[test]
    fn config_file_known_patterns() {
        assert!(is_config_file(std::path::Path::new("webpack.config.js")));
        assert!(is_config_file(std::path::Path::new("jest.config.ts")));
        assert!(is_config_file(std::path::Path::new("karma.conf.js")));
        assert!(is_config_file(std::path::Path::new("vite.config.mts")));
        assert!(is_config_file(std::path::Path::new("playwright.config.ts")));
        assert!(is_config_file(std::path::Path::new("eslint.config.mjs")));
    }

    #[test]
    fn config_file_dotrc_pattern() {
        assert!(is_config_file(std::path::Path::new(".eslintrc.js")));
        assert!(is_config_file(std::path::Path::new(".babelrc.json")));
    }

    #[test]
    fn not_config_file() {
        assert!(!is_config_file(std::path::Path::new("index.ts")));
        assert!(!is_config_file(std::path::Path::new("utils.js")));
        assert!(!is_config_file(std::path::Path::new("config.ts")));
        assert!(!is_config_file(std::path::Path::new(
            "src/webpack-plugin.js"
        )));
    }

    // ---------------------------------------------------------------
    // is_config_file edge cases
    // ---------------------------------------------------------------

    #[test]
    fn config_file_testing_tool_configs() {
        assert!(is_config_file(std::path::Path::new("jest.config.ts")));
        assert!(is_config_file(std::path::Path::new("jest.config.js")));
        assert!(is_config_file(std::path::Path::new("jest.config.cjs")));
        assert!(is_config_file(std::path::Path::new("jest.setup.ts")));
        assert!(is_config_file(std::path::Path::new("vitest.config.ts")));
        assert!(is_config_file(std::path::Path::new("vitest.config.mts")));
        assert!(is_config_file(std::path::Path::new("vitest.setup.ts")));
        assert!(is_config_file(std::path::Path::new("vitest.workspace.ts")));
        assert!(is_config_file(std::path::Path::new("cypress.config.ts")));
        assert!(is_config_file(std::path::Path::new("playwright.config.ts")));
    }

    #[test]
    fn config_file_bundler_configs() {
        assert!(is_config_file(std::path::Path::new("webpack.config.js")));
        assert!(is_config_file(std::path::Path::new("webpack.config.mjs")));
        assert!(is_config_file(std::path::Path::new("rollup.config.mjs")));
        assert!(is_config_file(std::path::Path::new("rollup.config.js")));
        assert!(is_config_file(std::path::Path::new("tsup.config.ts")));
        assert!(is_config_file(std::path::Path::new("esbuild.config.js")));
        assert!(is_config_file(std::path::Path::new("swc.config.json")));
        assert!(is_config_file(std::path::Path::new("unbuild.config.ts")));
    }

    /// Nested config patterns like `vitest.ci.config.ts` are explicitly listed
    /// in the patterns array and match correctly.
    #[test]
    fn config_file_nested_patterns_listed() {
        assert!(is_config_file(std::path::Path::new("vitest.ci.config.ts")));
    }

    /// Config files with extra qualifiers (e.g., `webpack.prod.config.js`) do NOT
    /// match because `webpack.prod.config.js` does not start with `webpack.config.`.
    /// Only explicitly listed nested patterns (like `vitest.ci.config.`) are recognized.
    #[test]
    fn config_file_unlisted_nested_patterns_do_not_match() {
        assert!(!is_config_file(std::path::Path::new(
            "webpack.prod.config.js"
        )));
        assert!(!is_config_file(std::path::Path::new(
            "webpack.dev.config.js"
        )));
        assert!(!is_config_file(std::path::Path::new("jest.e2e.config.ts")));
        assert!(!is_config_file(std::path::Path::new(
            "rollup.lib.config.mjs"
        )));
    }

    #[test]
    fn config_file_rc_files_with_extensions() {
        assert!(is_config_file(std::path::Path::new(".eslintrc.js")));
        assert!(is_config_file(std::path::Path::new(".eslintrc.cjs")));
        assert!(is_config_file(std::path::Path::new(".eslintrc.json")));
        assert!(is_config_file(std::path::Path::new(".eslintrc.yaml")));
        assert!(is_config_file(std::path::Path::new(".prettierrc.json")));
        assert!(is_config_file(std::path::Path::new(".prettierrc.js")));
        assert!(is_config_file(std::path::Path::new(".prettierrc.cjs")));
        assert!(is_config_file(std::path::Path::new(".babelrc.json")));
        assert!(is_config_file(std::path::Path::new(".secretlintrc.cjs")));
        assert!(is_config_file(std::path::Path::new(".commitlintrc.js")));
    }

    /// Bare RC files without an extension (e.g., `.babelrc`, `.prettierrc`) do NOT
    /// match because the dotrc pattern requires `rc.` (with a dot before the extension).
    #[test]
    fn config_file_bare_rc_files_do_not_match() {
        assert!(!is_config_file(std::path::Path::new(".babelrc")));
        assert!(!is_config_file(std::path::Path::new(".prettierrc")));
        assert!(!is_config_file(std::path::Path::new(".eslintrc")));
    }

    /// Files that look like configs but aren't in the patterns list.
    #[test]
    fn not_config_file_similar_names() {
        assert!(!is_config_file(std::path::Path::new("config.ts")));
        assert!(!is_config_file(std::path::Path::new("my-config.js")));
        assert!(!is_config_file(std::path::Path::new("app.config.ts")));
        assert!(!is_config_file(std::path::Path::new("database.config.js")));
        assert!(!is_config_file(std::path::Path::new("firebase.config.ts")));
    }

    #[test]
    fn config_file_next_js_specific() {
        assert!(is_config_file(std::path::Path::new("next-env.d.ts")));
        assert!(is_config_file(std::path::Path::new("next.config.mjs")));
        assert!(is_config_file(std::path::Path::new("next.config.js")));
        assert!(is_config_file(std::path::Path::new("next.config.ts")));
    }

    #[test]
    fn config_file_environment_declarations() {
        assert!(is_config_file(std::path::Path::new("next-env.d.ts")));
        assert!(is_config_file(std::path::Path::new("env.d.ts")));
        assert!(is_config_file(std::path::Path::new("vite-env.d.ts")));
    }

    /// Dotenv files (`.env`, `.env.local`, `.env.production`) are NOT config files
    /// in this context — they are environment variable files, not JS/TS tool configs.
    #[test]
    fn not_config_file_dotenv_files() {
        assert!(!is_config_file(std::path::Path::new(".env")));
        assert!(!is_config_file(std::path::Path::new(".env.local")));
        assert!(!is_config_file(std::path::Path::new(".env.production")));
        assert!(!is_config_file(std::path::Path::new(".env.development")));
        assert!(!is_config_file(std::path::Path::new(".env.staging")));
    }

    #[test]
    fn config_file_framework_configs() {
        assert!(is_config_file(std::path::Path::new("astro.config.mjs")));
        assert!(is_config_file(std::path::Path::new("nuxt.config.ts")));
        assert!(is_config_file(std::path::Path::new("vite.config.ts")));
        assert!(is_config_file(std::path::Path::new("tailwind.config.js")));
        assert!(is_config_file(std::path::Path::new("tailwind.config.ts")));
        assert!(is_config_file(std::path::Path::new("drizzle.config.ts")));
        assert!(is_config_file(std::path::Path::new("postcss.config.js")));
    }

    #[test]
    fn config_file_sentry_configs() {
        assert!(is_config_file(std::path::Path::new(
            "sentry.client.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "sentry.server.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "sentry.edge.config.ts"
        )));
    }

    #[test]
    fn config_file_linting_and_formatting() {
        assert!(is_config_file(std::path::Path::new("eslint.config.mjs")));
        assert!(is_config_file(std::path::Path::new("prettier.config.js")));
        assert!(is_config_file(std::path::Path::new("stylelint.config.js")));
        assert!(is_config_file(std::path::Path::new(
            "lint-staged.config.js"
        )));
        assert!(is_config_file(std::path::Path::new("commitlint.config.js")));
    }

    /// Config file detection only considers the filename, not the directory path.
    #[test]
    fn config_file_ignores_directory_path() {
        assert!(is_config_file(std::path::Path::new(
            "src/config/jest.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "packages/app/vite.config.ts"
        )));
        assert!(!is_config_file(std::path::Path::new(
            "jest.config/index.ts"
        )));
    }

    // ---------------------------------------------------------------
    // is_declaration_file edge cases
    // ---------------------------------------------------------------

    /// Declaration files in deeply nested paths.
    #[test]
    fn declaration_file_nested_paths() {
        assert!(is_declaration_file(std::path::Path::new(
            "packages/ui/src/types/global.d.ts"
        )));
        assert!(is_declaration_file(std::path::Path::new(
            "node_modules/@types/react/index.d.ts"
        )));
    }

    /// Files ending with `.d.` but not valid declaration extensions.
    #[test]
    fn not_declaration_file_invalid_d_extensions() {
        assert!(!is_declaration_file(std::path::Path::new("file.d.js")));
        assert!(!is_declaration_file(std::path::Path::new("file.d.jsx")));
        assert!(!is_declaration_file(std::path::Path::new("file.d.css")));
        assert!(!is_declaration_file(std::path::Path::new("file.d.json")));
    }

    /// Files with `.d.ts` in the middle of the name (not at the end).
    #[test]
    fn not_declaration_file_d_ts_in_middle() {
        assert!(!is_declaration_file(std::path::Path::new("my.d.ts.backup")));
    }

    // ---------------------------------------------------------------
    // is_barrel_with_reachable_sources tests
    // ---------------------------------------------------------------

    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use crate::extract::VisibilityTag;
    use crate::graph::{ExportSymbol, ModuleGraph, ReExportEdge};
    use crate::resolve::ResolvedModule;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_graph(file_specs: &[(&str, bool)]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = file_specs
            .iter()
            .enumerate()
            .map(|(i, (path, _))| DiscoveredFile {
                id: FileId(i as u32),
                path: std::path::PathBuf::from(path),
                size_bytes: 0,
            })
            .collect();

        let entry_points: Vec<EntryPoint> = file_specs
            .iter()
            .filter(|(_, is_entry)| *is_entry)
            .map(|(path, _)| EntryPoint {
                path: std::path::PathBuf::from(path),
                source: EntryPointSource::ManualEntry,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                unused_import_bindings: rustc_hash::FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
            })
            .collect();

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    /// Module with no re-exports is not a barrel.
    #[test]
    fn barrel_no_re_exports_returns_false() {
        let graph = build_graph(&[("/src/entry.ts", true), ("/src/utils.ts", false)]);
        let module = &graph.modules[1];
        assert!(!is_barrel_with_reachable_sources(module, &graph));
    }

    /// Module with re-exports but also local exports is not a pure barrel.
    #[test]
    fn barrel_with_local_exports_returns_false() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/index.ts", false),
            ("/src/utils.ts", false),
        ]);
        graph.modules[2].set_reachable(true);
        // Add a re-export
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        // Add a local export with a real span (non-zero)
        graph.modules[1].exports = vec![ExportSymbol {
            name: crate::extract::ExportName::Named("localFn".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(10, 50),
            references: vec![],
            members: vec![],
        }];
        assert!(!is_barrel_with_reachable_sources(&graph.modules[1], &graph));
    }

    /// Module with re-exports and CJS exports is not a pure barrel.
    #[test]
    fn barrel_with_cjs_exports_returns_false() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/index.ts", false),
            ("/src/utils.ts", false),
        ]);
        graph.modules[2].set_reachable(true);
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        graph.modules[1].set_cjs_exports(true);
        assert!(!is_barrel_with_reachable_sources(&graph.modules[1], &graph));
    }

    /// Pure barrel with reachable source returns true.
    #[test]
    fn barrel_pure_with_reachable_source_returns_true() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/index.ts", false),
            ("/src/utils.ts", false),
        ]);
        graph.modules[2].set_reachable(true);
        // Only re-exports, no local exports, no CJS
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        // Only synthetic exports (span 0..0), which are from re-exports
        graph.modules[1].exports = vec![ExportSymbol {
            name: crate::extract::ExportName::Named("helper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 0),
            references: vec![],
            members: vec![],
        }];
        assert!(is_barrel_with_reachable_sources(&graph.modules[1], &graph));
    }

    /// Pure barrel where all sources are unreachable returns false.
    #[test]
    fn barrel_all_sources_unreachable_returns_false() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/index.ts", false),
            ("/src/utils.ts", false),
        ]);
        // utils (source) is NOT reachable
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        assert!(!is_barrel_with_reachable_sources(&graph.modules[1], &graph));
    }

    /// Barrel with out-of-bounds source FileId doesn't panic, returns false.
    #[test]
    fn barrel_out_of_bounds_source_returns_false() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/index.ts", false)]);
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(999), // out of bounds
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        // Should not panic, should return false
        assert!(!is_barrel_with_reachable_sources(&graph.modules[1], &graph));
    }

    // ---------------------------------------------------------------
    // is_config_file additional coverage
    // ---------------------------------------------------------------

    #[test]
    fn config_file_dotfiles_with_rc() {
        assert!(is_config_file(std::path::Path::new(".eslintrc.js")));
        assert!(is_config_file(std::path::Path::new(".prettierrc.cjs")));
        assert!(is_config_file(std::path::Path::new(".commitlintrc.ts")));
        assert!(is_config_file(std::path::Path::new(".secretlintrc.json")));
    }

    #[test]
    fn config_file_dotfiles_without_rc_not_matched() {
        assert!(!is_config_file(std::path::Path::new(".env")));
        assert!(!is_config_file(std::path::Path::new(".gitignore")));
    }

    #[test]
    fn config_file_standard_patterns() {
        assert!(is_config_file(std::path::Path::new("jest.config.ts")));
        assert!(is_config_file(std::path::Path::new("vitest.config.ts")));
        assert!(is_config_file(std::path::Path::new("webpack.config.js")));
        assert!(is_config_file(std::path::Path::new("eslint.config.mjs")));
        assert!(is_config_file(std::path::Path::new("next.config.js")));
        assert!(is_config_file(std::path::Path::new("tailwind.config.ts")));
        assert!(is_config_file(std::path::Path::new("drizzle.config.ts")));
        assert!(is_config_file(std::path::Path::new(
            "sentry.client.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "sentry.server.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "sentry.edge.config.ts"
        )));
        assert!(is_config_file(std::path::Path::new(
            "react-router.config.ts"
        )));
    }

    #[test]
    fn config_file_env_declarations() {
        assert!(is_config_file(std::path::Path::new("next-env.d.ts")));
        assert!(is_config_file(std::path::Path::new("env.d.ts")));
        assert!(is_config_file(std::path::Path::new("vite-env.d.ts")));
    }

    #[test]
    fn not_config_file_regular_source() {
        assert!(!is_config_file(std::path::Path::new("index.ts")));
        assert!(!is_config_file(std::path::Path::new("App.tsx")));
        assert!(!is_config_file(std::path::Path::new("utils.js")));
        assert!(!is_config_file(std::path::Path::new("config.ts")));
    }

    #[test]
    fn config_file_double_dot_prefix_not_matched() {
        assert!(!is_config_file(std::path::Path::new("..eslintrc.js")));
    }
}
