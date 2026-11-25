/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`). `.js` / `.ts`
/// files re-parsed through the JSX retry path also carry React IR, but React
/// detectors scope to the canonical JSX extensions to keep the surface tight.
pub(in crate::analyze) fn is_react_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}

/// The component name: the file stem (e.g. `UserCard` for `UserCard.vue`).
pub(in crate::analyze) fn component_name_for(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Check if a path is an HTML file.
///
/// HTML files are excluded from unused-file detection because they are entry-point-like:
/// nothing imports an HTML file, so "unused" is meaningless for them. They serve as
/// entry points in Vite/Parcel-style apps and their referenced assets are tracked
/// via `<script src>` and `<link href>` edges.
pub(in crate::analyze) fn is_html_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "html")
}

/// Compiled glob set over the test / spec / story / fixture subset of
/// [`PRODUCTION_EXCLUDE_PATTERNS`](crate::discover::PRODUCTION_EXCLUDE_PATTERNS),
/// built once. `literal_separator(true)` so `*` cannot cross a path separator,
/// matching production-mode exclusion semantics. The tooling-config patterns
/// (`*.config.*`, dot-prefixed) are intentionally excluded here: this predicate
/// answers "is this a TEST / SPEC file", not "is this any low-value anchor"
/// (the security layer's `is_low_value_anchor` adds the config-file arm on top).
fn test_or_spec_globset() -> &'static globset::GlobSet {
    use std::sync::OnceLock;
    static SET: OnceLock<globset::GlobSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in crate::discover::PRODUCTION_EXCLUDE_PATTERNS {
            // Skip the tooling-config arms (`*.config.*` and the `**/.*.{js,ts,..}`
            // dotfile rows); they are not test/spec files.
            if pattern.starts_with("*.config.") || pattern.starts_with("**/.*") {
                continue;
            }
            if let Ok(glob) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                builder.add(glob);
            }
        }
        builder
            .build()
            .unwrap_or_else(|_| globset::GlobSet::empty())
    })
}

/// Check if a path is a test / spec / story / fixture file (a `*.test.*`,
/// `*.spec.*`, `*.stories.*`, `__tests__/`, `test/`, `tests/`, etc. location).
///
/// Reuses the canonical [`PRODUCTION_EXCLUDE_PATTERNS`](crate::discover::PRODUCTION_EXCLUDE_PATTERNS)
/// test/spec subset so the definition never drifts from production-mode
/// exclusion. The match runs on the path with separators forward-slash
/// normalized so the `**/` globs anchor consistently across platforms.
pub(in crate::analyze) fn is_test_or_spec_file(path: &std::path::Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    test_or_spec_globset().is_match(&normalized)
}

const CONFIG_FILE_PREFIXES: &[&str] = &[
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
    "jest.config.",
    "jest.setup.",
    "vitest.config.",
    "vitest.ci.config.",
    "vitest.setup.",
    "vitest.workspace.",
    "playwright.config.",
    "cypress.config.",
    "karma.conf.",
    "eslint.config.",
    "prettier.config.",
    "stylelint.config.",
    "lint-staged.config.",
    "commitlint.config.",
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
    "typedoc.",
    "knip.config.",
    "fallow.config.",
    "i18next-parser.config.",
    "codegen.config.",
    "graphql.config.",
    "npmpackagejsonlint.config.",
    "release-it.",
    "release.config.",
    "contentlayer.config.",
    ".size-limit.",
    "next-env.d.",
    "env.d.",
    "vite-env.d.",
];

/// File name prefixes of the config and setup files that a test runner loads.
const TEST_RUNNER_CONFIG_PREFIXES: &[&str] = &[
    "jest.config.",
    "jest.setup.",
    "vitest.config.",
    "vitest.ci.config.",
    "vitest.setup.",
    "vitest.workspace.",
    "playwright.config.",
    "cypress.config.",
    "karma.conf.",
    ".mocharc.",
    "ava.config.",
    "wdio.conf.",
    "nightwatch.conf.",
];

/// Test runner names that also start a variant config file name, such as
/// `vitest.unit.config.ts` or `playwright-ct.config.ts`.
const TEST_RUNNER_NAMES: &[&str] = &[
    "jest",
    "vitest",
    "playwright",
    "cypress",
    "karma",
    "ava",
    "wdio",
    "nightwatch",
];

/// Name segments that mark a file as a config file.
const CONFIG_NAME_SEGMENTS: &[&str] = &[".config.", ".conf."];

/// Check if a file is a config or setup file that only a test runner loads
/// (`vitest.config.ts`, `jest.setup.js`, `playwright.config.ts`).
///
/// Variant names count too: a test runner name, then `.` or `-`, and a
/// `.config.` or `.conf.` segment (`vitest.unit.config.ts`,
/// `playwright-ct.config.ts`).
fn is_test_runner_config_file(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if TEST_RUNNER_CONFIG_PREFIXES
        .iter()
        .any(|p| name.starts_with(p))
    {
        return true;
    }
    let starts_with_runner = TEST_RUNNER_NAMES.iter().any(|runner| {
        name.strip_prefix(runner)
            .is_some_and(|rest| rest.starts_with(['.', '-']))
    });
    starts_with_runner && CONFIG_NAME_SEGMENTS.iter().any(|s| name.contains(s))
}

/// Tells which modules are test code for `test-only-dependencies`.
pub(in crate::analyze) struct TestImporters<'a> {
    graph: &'a crate::graph::ModuleGraph,
    root: &'a std::path::Path,
    /// The modules that a support entry point, such as a file that a
    /// package.json script runs, reaches. Computed on first use.
    support_reach: std::cell::OnceCell<rustc_hash::FxHashSet<crate::discover::FileId>>,
}

impl<'a> TestImporters<'a> {
    pub(in crate::analyze) fn new(
        graph: &'a crate::graph::ModuleGraph,
        root: &'a std::path::Path,
    ) -> Self {
        Self {
            graph,
            root,
            support_reach: std::cell::OnceCell::new(),
        }
    }

    fn support_reaches(&self, id: crate::discover::FileId) -> bool {
        self.support_reach
            .get_or_init(|| {
                let reach = self.graph.support_reachable();
                self.graph
                    .modules()
                    .iter()
                    .map(|module| module.file_id)
                    .filter(|id| reach.contains(id.0 as usize))
                    .collect()
            })
            .contains(&id)
    }

    /// Whether a module is test code: a test file by name, a test runner
    /// config file, or a test entry point of a test runner that no runtime
    /// or support entry point reaches. The last case covers a test runner
    /// setting such as Vitest `include: ['src/**/*.vitest.ts']` (issue
    /// #4618). A broad setting such as Playwright `testDir: './src'` can
    /// also match a production file that a script runs, so a support entry
    /// point keeps its reach production code.
    pub(in crate::analyze) fn contains(&self, id: crate::discover::FileId) -> bool {
        let Some(module) = self.graph.module(id) else {
            return false;
        };
        let relative = module.path.strip_prefix(self.root).unwrap_or(&module.path);
        is_test_or_spec_file(relative)
            || is_test_runner_config_file(&module.path)
            || (self.graph.test_entry_points.contains(&id)
                && !module.is_runtime_reachable()
                && !self.support_reaches(id))
    }
}

/// Check if a file is a configuration file consumed by tooling, not via imports.
///
/// These files should never be reported as unused because they are loaded by
/// their respective tools (e.g., Babel reads `babel.config.js`, `ESLint` reads
/// `eslint.config.ts`, etc.) rather than being imported by application code.
pub(in crate::analyze) fn is_config_file(path: &std::path::Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if name.starts_with('.') && !name.starts_with("..") {
        let lower = name.to_ascii_lowercase();
        if lower.contains("rc.") {
            return true;
        }
    }

    CONFIG_FILE_PREFIXES.iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_spec_file_matches_test_and_spec() {
        assert!(is_test_or_spec_file(std::path::Path::new(
            "src/components/Button.test.tsx"
        )));
        assert!(is_test_or_spec_file(std::path::Path::new(
            "src/utils/format.spec.ts"
        )));
        assert!(is_test_or_spec_file(std::path::Path::new(
            "src/__tests__/Button.tsx"
        )));
        assert!(is_test_or_spec_file(std::path::Path::new("test/setup.ts")));
        assert!(is_test_or_spec_file(std::path::Path::new(
            "tests/e2e/flow.ts"
        )));
        assert!(is_test_or_spec_file(std::path::Path::new(
            "src/Page.stories.tsx"
        )));
        assert!(is_test_or_spec_file(std::path::Path::new(
            "src/__fixtures__/data.ts"
        )));
    }

    /// Tooling config files and ordinary source are NOT test/spec files: this
    /// predicate is narrower than the security `is_low_value_anchor` (which adds
    /// the config-file arm on top).
    #[test]
    fn test_or_spec_file_excludes_config_and_source() {
        assert!(!is_test_or_spec_file(std::path::Path::new(
            "src/components/Button.tsx"
        )));
        assert!(!is_test_or_spec_file(std::path::Path::new(
            "vite.config.ts"
        )));
        assert!(!is_test_or_spec_file(std::path::Path::new("index.ts")));
        // A `testimonials` directory is not a `test/` directory (segment-anchored).
        assert!(!is_test_or_spec_file(std::path::Path::new(
            "src/testimonials/Card.tsx"
        )));
    }

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
    fn config_file_size_limit_dotfile_without_rc() {
        // size-limit searches `.size-limit.<ext>` from the package it runs in,
        // so a hoisted monorepo keeps the config inside a workspace package where
        // the root plugin's always-used globs never reach.
        assert!(is_config_file(std::path::Path::new(".size-limit.js")));
        assert!(is_config_file(std::path::Path::new(
            "packages/lib/.size-limit.ts"
        )));
        assert!(!is_config_file(std::path::Path::new("size-limit.js")));
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
    /// in this context , they are environment variable files, not JS/TS tool configs.
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
