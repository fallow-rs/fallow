//! General tooling dependency detection.
//!
//! Known dev dependencies that are tooling (used by CLI/config, not imported in
//! application code). These complement the per-plugin `tooling_dependencies()`
//! lists with dependencies that aren't tied to any single plugin.
//!
//! The catalogue is community-maintainable: the prefix and exact lists live in
//! `crates/core/data/tooling.toml`, embedded via `include_str!` and parsed once
//! at startup. There is no regeneration step. To add a tool, edit one entry in
//! the TOML and open a PR. See `CONTRIBUTING.md`.

use rustc_hash::{FxHashMap, FxHashSet};

/// Embedded catalogue source. Because it is `include_str!`-embedded at compile
/// time, a green `catalogue_parses` test guarantees the released binary parses.
const CATALOGUE_TOML: &str = include_str!("../../data/tooling.toml");

/// Framework-plugin name markers. A package whose bare name OR whose
/// `@scope/`-stripped tail starts with one of these is a framework plugin and
/// must NOT be listed in the catalogue (its config-parsing plugin credits it
/// only when it actually appears in the config, avoiding a false negative). The
/// tail check catches scoped community forms like
/// `@ianvs/prettier-plugin-sort-imports`. Enforced by
/// `catalogue_rejects_framework_plugin_exact_entries`.
#[cfg(test)]
const FRAMEWORK_PLUGIN_FAMILY_PREFIXES: &[&str] = &[
    "vite-plugin-",
    "prettier-plugin-",
    "eslint-plugin-",
    "rollup-plugin-",
];

/// Official scoped framework-plugin namespaces, checked against the FULL name.
/// `@rollup/plugin-*` is Rollup's official plugin scope. This is deliberately
/// NOT generalized to `@scope/plugin-*`, because `@vitejs/plugin-react` and
/// peers are legitimately-kept tooling exacts.
#[cfg(test)]
const FRAMEWORK_PLUGIN_SCOPED_PREFIXES: &[&str] = &["@rollup/plugin-"];

#[derive(serde::Deserialize)]
struct ToolingCatalogue {
    #[serde(default)]
    prefix: Vec<PrefixEntry>,
    #[serde(default)]
    exact: Vec<ExactEntry>,
    #[serde(default)]
    ambient_types: Vec<AmbientTypesEntry>,
}

#[derive(serde::Deserialize)]
struct AmbientTypesEntry {
    /// Exact name of a type package that declares globals.
    name: String,
}

#[derive(serde::Deserialize)]
struct PrefixEntry {
    /// Match when `name.starts_with(pattern)`. Required and must be non-empty
    /// (an empty pattern would match every package, disabling unused-dep
    /// detection entirely).
    pattern: String,
    /// Optional human context; does not affect matching.
    #[expect(
        dead_code,
        reason = "documentation field, surfaced via the catalogue source"
    )]
    #[serde(default)]
    notes: Option<String>,
    /// Whether every package of the family is a command-line tool.
    #[serde(default)]
    cli: bool,
    /// Config file patterns of a command-line tool that no plugin covers.
    #[serde(default)]
    config: Vec<String>,
}

#[derive(serde::Deserialize)]
struct ExactEntry {
    /// Exact package name to credit as tooling.
    name: String,
    /// Optional grouping label; does not affect matching.
    #[expect(
        dead_code,
        reason = "documentation field, surfaced via the catalogue source"
    )]
    #[serde(default)]
    ecosystem: Option<String>,
    /// Whether the package is a command-line tool.
    #[serde(default)]
    cli: bool,
    /// Config file patterns of a command-line tool that no plugin covers.
    #[serde(default)]
    config: Vec<String>,
}

/// Parsed catalogue: ordered prefix patterns, an exact-match set, the
/// command-line tools with their config patterns, and the ambient global type
/// packages.
struct Catalogue {
    prefixes: Vec<String>,
    exact: FxHashSet<String>,
    cli_exact: FxHashMap<String, Vec<String>>,
    cli_prefixes: Vec<(String, Vec<String>)>,
    ambient_types: FxHashSet<String>,
}

/// Parse and cache the embedded catalogue once. Panics with a clear message if
/// the embedded TOML is malformed; this is unreachable in a released binary
/// because the bytes are compile-time-embedded and gated by `catalogue_parses`.
#[expect(
    clippy::expect_used,
    reason = "embedded tooling catalogue is compile-time data pinned by catalogue_parses"
)]
fn catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| {
        let parsed: ToolingCatalogue = toml::from_str(CATALOGUE_TOML).expect(
            "embedded crates/core/data/tooling.toml must parse; run \
             `cargo test -p fallow-core catalogue_parses` to see the error",
        );
        let cli_exact = parsed
            .exact
            .iter()
            .filter(|e| e.cli)
            .map(|e| (e.name.clone(), e.config.clone()))
            .collect();
        let cli_prefixes = parsed
            .prefix
            .iter()
            .filter(|p| p.cli)
            .map(|p| (p.pattern.clone(), p.config.clone()))
            .collect();
        Catalogue {
            prefixes: parsed.prefix.into_iter().map(|p| p.pattern).collect(),
            exact: parsed.exact.into_iter().map(|e| e.name).collect(),
            cli_exact,
            cli_prefixes,
            ambient_types: parsed.ambient_types.into_iter().map(|e| e.name).collect(),
        }
    })
}

/// Check whether a package is a known tooling/dev dependency by name.
///
/// This is the single source of truth for general tooling detection.
/// Per-plugin tooling dependencies are declared via `Plugin::tooling_dependencies()`
/// and aggregated separately in `AggregatedPluginResult`.
#[must_use]
pub fn is_known_tooling_dependency(name: &str) -> bool {
    let catalogue = catalogue();
    catalogue
        .prefixes
        .iter()
        .any(|p| name.starts_with(p.as_str()))
        || catalogue.exact.contains(name)
}

/// The config file patterns of a command-line tool in the catalogue, or
/// `None` when the catalogue does not mark `name` as a command-line tool.
///
/// The unused devDependency check credits a command-line tool only when a
/// script, a CI workflow or a git hook runs it, when one of these config files
/// exists, or when a plugin credits it. An empty slice means the tool has no
/// config file of its own that no plugin covers.
#[must_use]
pub fn cli_tooling_config_patterns(name: &str) -> Option<&'static [String]> {
    let catalogue = catalogue();
    if let Some(config) = catalogue.cli_exact.get(name) {
        return Some(config);
    }
    if catalogue.exact.contains(name) {
        return None;
    }
    catalogue
        .cli_prefixes
        .iter()
        .find(|(pattern, _)| name.starts_with(pattern.as_str()))
        .map(|(_, config)| config.as_slice())
}

/// Whether a package declares ambient globals (`@types/node`, `@types/jest`,
/// `bun-types`), so a project uses it without importing anything.
#[must_use]
pub fn is_ambient_types_package(name: &str) -> bool {
    catalogue().ambient_types.contains(name)
}

/// The package that a `@types/` package types, by the DefinitelyTyped naming
/// convention: `@types/node` types `node`, `@types/scope__pkg` types
/// `@scope/pkg`. `None` for a name outside the `@types/` scope.
#[must_use]
pub fn types_package_target(name: &str) -> Option<String> {
    let tail = name.strip_prefix("@types/")?;
    if tail.is_empty() {
        return None;
    }
    Some(match tail.split_once("__") {
        Some((scope, package)) if !scope.is_empty() && !package.is_empty() => {
            format!("@{scope}/{package}")
        }
        _ => tail.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_entries_carry_their_config_patterns() {
        assert_eq!(cli_tooling_config_patterns("tsx"), Some(&[][..]));
        assert_eq!(
            cli_tooling_config_patterns("jscpd").map(<[String]>::to_vec),
            Some(vec![".jscpd.json".to_string()])
        );
        assert_eq!(
            cli_tooling_config_patterns("oxlint-tsgolint"),
            Some(&[][..])
        );
        assert_eq!(cli_tooling_config_patterns("sass"), None);
        assert_eq!(cli_tooling_config_patterns("@types/node"), None);
        assert_eq!(cli_tooling_config_patterns("left-pad"), None);
    }

    #[test]
    fn cli_config_patterns_are_root_anchored() {
        let parsed: ToolingCatalogue = toml::from_str(CATALOGUE_TOML).unwrap();
        let patterns = parsed
            .exact
            .iter()
            .flat_map(|e| e.config.iter().map(move |c| (e.cli, c)))
            .chain(
                parsed
                    .prefix
                    .iter()
                    .flat_map(|p| p.config.iter().map(move |c| (p.cli, c))),
            );
        for (cli, pattern) in patterns {
            assert!(
                cli,
                "config pattern {pattern} needs `cli = true` on its entry"
            );
            assert!(
                !pattern.starts_with("**") && !pattern.starts_with('/'),
                "config pattern {pattern} must be relative to the package root"
            );
        }
    }

    #[test]
    fn ambient_types_cover_global_packages_only() {
        assert!(is_ambient_types_package("@types/node"));
        assert!(is_ambient_types_package("@types/jest"));
        assert!(is_ambient_types_package("bun-types"));
        assert!(!is_ambient_types_package("@types/react"));
        assert!(!is_ambient_types_package("node"));
    }

    #[test]
    fn types_package_target_follows_the_naming_convention() {
        assert_eq!(
            types_package_target("@types/node"),
            Some("node".to_string())
        );
        assert_eq!(
            types_package_target("@types/scope__pkg"),
            Some("@scope/pkg".to_string())
        );
        assert_eq!(types_package_target("@types/"), None);
        assert_eq!(types_package_target("node"), None);
        assert_eq!(types_package_target("bun-types"), None);
    }

    #[test]
    fn types_prefix_matches_scoped() {
        assert!(is_known_tooling_dependency("@types/node"));
        assert!(is_known_tooling_dependency("@types/react"));
        assert!(is_known_tooling_dependency("@types/express"));
    }

    #[test]
    fn types_prefix_does_not_match_similar_names() {
        assert!(!is_known_tooling_dependency("type-fest"));
        assert!(!is_known_tooling_dependency("typesafe-actions"));
    }

    #[test]
    fn storybook_not_blanket_matched() {
        assert!(!is_known_tooling_dependency("@storybook/react"));
        assert!(!is_known_tooling_dependency("@storybook/addon-essentials"));
        assert!(!is_known_tooling_dependency("storybook"));
    }

    #[test]
    fn testing_library_prefix_matches() {
        assert!(is_known_tooling_dependency("@testing-library/react"));
        assert!(is_known_tooling_dependency("@testing-library/jest-dom"));
    }

    #[test]
    fn babel_not_blanket_matched() {
        assert!(!is_known_tooling_dependency("@babel/core"));
        assert!(!is_known_tooling_dependency("@babel/preset-env"));
        assert!(!is_known_tooling_dependency("babel-loader"));
        assert!(!is_known_tooling_dependency("babel-jest"));
    }

    #[test]
    fn vitest_prefix_matches() {
        assert!(is_known_tooling_dependency("@vitest/coverage-v8"));
        assert!(is_known_tooling_dependency("@vitest/ui"));
    }

    #[test]
    fn eslint_not_blanket_matched() {
        assert!(!is_known_tooling_dependency("eslint"));
        assert!(!is_known_tooling_dependency("eslint-plugin-react"));
        assert!(!is_known_tooling_dependency("eslint-config-next"));
        assert!(!is_known_tooling_dependency("@typescript-eslint/parser"));
    }

    #[test]
    fn biomejs_prefix_matches() {
        assert!(is_known_tooling_dependency("@biomejs/biome"));
    }

    #[test]
    fn exact_typescript_matches() {
        assert!(is_known_tooling_dependency("typescript"));
    }

    #[test]
    fn exact_prettier_matches() {
        assert!(is_known_tooling_dependency("prettier"));
    }

    #[test]
    fn exact_vitest_matches() {
        assert!(is_known_tooling_dependency("vitest"));
    }

    #[test]
    fn exact_jest_matches() {
        assert!(is_known_tooling_dependency("jest"));
    }

    #[test]
    fn exact_vite_matches() {
        assert!(is_known_tooling_dependency("vite"));
    }

    #[test]
    fn exact_esbuild_matches() {
        assert!(is_known_tooling_dependency("esbuild"));
    }

    #[test]
    fn exact_tsup_matches() {
        assert!(is_known_tooling_dependency("tsup"));
    }

    #[test]
    fn exact_turbo_matches() {
        assert!(is_known_tooling_dependency("turbo"));
    }

    #[test]
    fn common_runtime_deps_not_tooling() {
        assert!(!is_known_tooling_dependency("react"));
        assert!(!is_known_tooling_dependency("react-dom"));
        assert!(!is_known_tooling_dependency("express"));
        assert!(!is_known_tooling_dependency("lodash"));
        assert!(!is_known_tooling_dependency("next"));
        assert!(!is_known_tooling_dependency("vue"));
        assert!(!is_known_tooling_dependency("axios"));
    }

    #[test]
    fn empty_string_not_tooling() {
        assert!(!is_known_tooling_dependency(""));
    }

    #[test]
    fn near_miss_not_tooling() {
        assert!(!is_known_tooling_dependency("type-fest"));
        assert!(!is_known_tooling_dependency("typestyle"));
        assert!(!is_known_tooling_dependency("prettier-bytes")); // not the exact "prettier"
    }

    #[test]
    fn sass_variants_are_tooling() {
        assert!(is_known_tooling_dependency("sass"));
        assert!(is_known_tooling_dependency("sass-embedded"));
    }

    #[test]
    fn framework_plugin_packages_no_longer_exact_matched() {
        assert!(!is_known_tooling_dependency("vite-plugin-svgr"));
        assert!(!is_known_tooling_dependency("vite-plugin-eslint"));
        assert!(!is_known_tooling_dependency("prettier-plugin-tailwindcss"));
        assert!(!is_known_tooling_dependency(
            "prettier-plugin-organize-imports"
        ));
        assert!(!is_known_tooling_dependency(
            "@ianvs/prettier-plugin-sort-imports"
        ));
    }

    #[test]
    fn electron_forge_prefix_matches() {
        assert!(is_known_tooling_dependency("@electron-forge/cli"));
        assert!(is_known_tooling_dependency(
            "@electron-forge/maker-squirrel"
        ));
    }

    #[test]
    fn electron_prefix_matches() {
        assert!(is_known_tooling_dependency("@electron/rebuild"));
        assert!(is_known_tooling_dependency("@electron/notarize"));
    }

    #[test]
    fn formatjs_prefix_matches() {
        assert!(is_known_tooling_dependency("@formatjs/cli"));
        assert!(is_known_tooling_dependency("@formatjs/intl"));
    }

    #[test]
    fn rollup_not_blanket_matched() {
        assert!(!is_known_tooling_dependency("@rollup/plugin-commonjs"));
        assert!(!is_known_tooling_dependency("@rollup/plugin-node-resolve"));
        assert!(!is_known_tooling_dependency("@rollup/plugin-typescript"));
    }

    #[test]
    fn semantic_release_prefix_matches() {
        assert!(is_known_tooling_dependency("@semantic-release/github"));
        assert!(is_known_tooling_dependency("@semantic-release/npm"));
        assert!(is_known_tooling_dependency("semantic-release"));
    }

    #[test]
    fn release_it_prefix_matches() {
        assert!(is_known_tooling_dependency(
            "@release-it/conventional-changelog"
        ));
    }

    #[test]
    fn lerna_lite_prefix_matches() {
        assert!(is_known_tooling_dependency("@lerna-lite/cli"));
        assert!(is_known_tooling_dependency("@lerna-lite/publish"));
    }

    #[test]
    fn changesets_prefix_matches() {
        assert!(is_known_tooling_dependency("@changesets/cli"));
        assert!(is_known_tooling_dependency("@changesets/changelog-github"));
    }

    #[test]
    fn graphql_codegen_prefix_matches() {
        assert!(is_known_tooling_dependency("@graphql-codegen/cli"));
        assert!(is_known_tooling_dependency(
            "@graphql-codegen/typescript-operations"
        ));
    }

    #[test]
    fn secretlint_prefix_matches() {
        assert!(is_known_tooling_dependency("secretlint"));
        assert!(is_known_tooling_dependency(
            "@secretlint/secretlint-rule-preset-recommend"
        ));
    }

    #[test]
    fn oxlint_prefix_matches() {
        assert!(is_known_tooling_dependency("oxlint"));
    }

    #[test]
    fn react_native_community_prefix_matches() {
        assert!(is_known_tooling_dependency("@react-native-community/cli"));
        assert!(is_known_tooling_dependency(
            "@react-native-community/cli-platform-android"
        ));
    }

    #[test]
    fn react_native_prefix_matches() {
        assert!(is_known_tooling_dependency("@react-native/metro-config"));
        assert!(is_known_tooling_dependency(
            "@react-native/typescript-config"
        ));
    }

    #[test]
    fn jest_prefix_matches() {
        assert!(is_known_tooling_dependency("@jest/globals"));
        assert!(is_known_tooling_dependency("@jest/types"));
    }

    #[test]
    fn playwright_prefix_matches() {
        assert!(is_known_tooling_dependency("@playwright/test"));
        assert!(is_known_tooling_dependency("playwright"));
    }

    #[test]
    fn tapjs_prefix_matches() {
        assert!(is_known_tooling_dependency("@tapjs/test"));
        assert!(is_known_tooling_dependency("@tapjs/snapshot"));
    }

    #[test]
    fn exact_tap_matches() {
        assert!(is_known_tooling_dependency("tap"));
    }

    #[test]
    fn exact_rolldown_matches() {
        assert!(is_known_tooling_dependency("rolldown"));
        assert!(is_known_tooling_dependency("rolldown-vite"));
    }

    #[test]
    fn exact_electron_matches() {
        assert!(is_known_tooling_dependency("electron"));
        assert!(is_known_tooling_dependency("electron-builder"));
        assert!(is_known_tooling_dependency("electron-vite"));
    }

    #[test]
    fn exact_sharp_matches() {
        assert!(is_known_tooling_dependency("sharp"));
    }

    #[test]
    fn exact_puppeteer_matches() {
        assert!(is_known_tooling_dependency("puppeteer"));
    }

    #[test]
    fn exact_madge_matches() {
        assert!(is_known_tooling_dependency("madge"));
    }

    #[test]
    fn exact_patch_package_matches() {
        assert!(is_known_tooling_dependency("patch-package"));
    }

    #[test]
    fn exact_nx_matches() {
        assert!(is_known_tooling_dependency("nx"));
    }

    #[test]
    fn exact_vue_tsc_matches() {
        assert!(is_known_tooling_dependency("vue-tsc"));
    }

    #[test]
    fn exact_tsconfig_packages_match() {
        assert!(is_known_tooling_dependency("@tsconfig/node20"));
        assert!(is_known_tooling_dependency("@tsconfig/react-native"));
        assert!(is_known_tooling_dependency("@vue/tsconfig"));
    }

    #[test]
    fn exact_vitejs_plugins_match() {
        assert!(is_known_tooling_dependency("@vitejs/plugin-vue"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-react"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-react-swc"));
        assert!(is_known_tooling_dependency("@vitejs/plugin-legacy"));
    }

    #[test]
    fn exact_oxc_transform_matches() {
        assert!(is_known_tooling_dependency("oxc-transform"));
    }

    #[test]
    fn exact_typescript_native_preview_matches() {
        assert!(is_known_tooling_dependency("@typescript/native-preview"));
    }

    #[test]
    fn exact_tw_animate_css_matches() {
        assert!(is_known_tooling_dependency("tw-animate-css"));
    }

    #[test]
    fn exact_manypkg_cli_matches() {
        assert!(is_known_tooling_dependency("@manypkg/cli"));
    }

    #[test]
    fn exact_swc_variants_match() {
        assert!(is_known_tooling_dependency("@swc/core"));
        assert!(is_known_tooling_dependency("@swc/jest"));
    }

    #[test]
    fn runtime_deps_with_similar_names_not_tooling() {
        assert!(!is_known_tooling_dependency("react-scripts"));
        assert!(!is_known_tooling_dependency("express-validator"));
        assert!(!is_known_tooling_dependency("sass-loader")); // "sass" is exact, not prefix
    }

    #[test]
    fn postcss_not_blanket_matched() {
        assert!(!is_known_tooling_dependency("postcss-modules"));
        assert!(!is_known_tooling_dependency("postcss-import"));
        assert!(!is_known_tooling_dependency("autoprefixer"));
        assert!(!is_known_tooling_dependency("tailwindcss"));
        assert!(!is_known_tooling_dependency("@tailwindcss/typography"));
    }

    #[test]
    fn catalogue_is_deterministic() {
        assert_eq!(
            is_known_tooling_dependency("typescript"),
            is_known_tooling_dependency("typescript")
        );
        assert!(is_known_tooling_dependency("typescript"));
    }

    #[test]
    fn catalogue_parses() {
        let cat = catalogue();
        assert!(!cat.prefixes.is_empty(), "catalogue must have prefixes");
        assert!(!cat.exact.is_empty(), "catalogue must have exact entries");
        assert!(cat.exact.contains("typescript"));
        assert!(cat.prefixes.iter().any(|p| p == "@types/"));
    }

    #[test]
    fn catalogue_has_no_empty_or_whitespace_prefixes() {
        for prefix in &catalogue().prefixes {
            assert!(
                !prefix.trim().is_empty(),
                "catalogue prefix must be non-empty / non-whitespace; got {prefix:?}"
            );
        }
    }

    #[test]
    fn catalogue_has_no_duplicate_entries() {
        let parsed: ToolingCatalogue = toml::from_str(CATALOGUE_TOML).unwrap();

        let mut seen_exact = FxHashSet::default();
        for entry in &parsed.exact {
            assert!(
                seen_exact.insert(entry.name.as_str()),
                "duplicate exact catalogue entry: {:?}",
                entry.name
            );
        }

        let mut seen_prefix = FxHashSet::default();
        for entry in &parsed.prefix {
            assert!(
                seen_prefix.insert(entry.pattern.as_str()),
                "duplicate prefix catalogue entry: {:?}",
                entry.pattern
            );
        }
    }

    #[test]
    fn catalogue_rejects_framework_plugin_exact_entries() {
        let parsed: ToolingCatalogue = toml::from_str(CATALOGUE_TOML).unwrap();
        for entry in &parsed.exact {
            let tail = entry
                .name
                .strip_prefix('@')
                .and_then(|rest| rest.split_once('/'))
                .map(|(_scope, tail)| tail);
            for bad in FRAMEWORK_PLUGIN_FAMILY_PREFIXES {
                assert!(
                    !entry.name.starts_with(bad) && !tail.is_some_and(|t| t.starts_with(bad)),
                    "exact catalogue entry {:?} is a framework plugin ({bad}); \
                     credit it in the relevant plugin's config parser instead of the catalogue",
                    entry.name,
                );
            }
            for bad in FRAMEWORK_PLUGIN_SCOPED_PREFIXES {
                assert!(
                    !entry.name.starts_with(bad),
                    "exact catalogue entry {:?} is a framework plugin ({bad}); \
                     credit it in the relevant plugin's config parser instead of the catalogue",
                    entry.name,
                );
            }
        }
    }
}
