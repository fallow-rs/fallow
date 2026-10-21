use super::*;

#[test]
fn segment_regexes_split_on_the_native_separator() {
    let regexes = vec![Regex::new("^_(components|hooks)$").expect("valid regex")];
    assert!(matches_segment_regex(
        "src/pages/_components/a.tsx",
        &regexes
    ));
    assert!(!matches_segment_regex(
        "src/pages/components/a.tsx",
        &regexes
    ));
    assert_eq!(
        matches_segment_regex("src\\pages\\_components\\a.tsx", &regexes),
        cfg!(windows),
        "a backslash separates segments only where it is the native separator"
    );
}
use std::path::Path;

#[test]
fn is_enabled_with_deps_exact_match() {
    let plugin = nextjs::NextJsPlugin;
    let deps = vec!["next".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
}

#[test]
fn is_enabled_with_deps_no_match() {
    let plugin = nextjs::NextJsPlugin;
    let deps = vec!["react".to_string()];
    assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
}

#[test]
fn is_enabled_with_deps_empty_deps() {
    let plugin = nextjs::NextJsPlugin;
    let deps: Vec<String> = vec![];
    assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
}

#[test]
fn environment_optional_peers_come_from_the_credit_catalogue() {
    for environment in [
        "jsdom",
        "jest-environment-jsdom",
        "vitest-environment-jsdom",
    ] {
        let mut result = PluginResult::default();
        credit_environment_optional_peers(environment, &mut result);
        assert_eq!(
            result.referenced_dependencies,
            vec!["canvas".to_string()],
            "expected the catalogue credit for {environment}"
        );
    }
}

#[test]
fn environment_without_a_catalogue_row_credits_nothing() {
    let mut result = PluginResult::default();
    credit_environment_optional_peers("happy-dom", &mut result);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn entry_point_role_defaults_are_centralized() {
    assert_eq!(vite::VitePlugin.entry_point_role(), EntryPointRole::Runtime);
    assert_eq!(
        vitest::VitestPlugin.entry_point_role(),
        EntryPointRole::Test
    );
    assert_eq!(
        storybook::StorybookPlugin.entry_point_role(),
        EntryPointRole::Support
    );
    assert_eq!(
        obsidian::ObsidianPlugin.entry_point_role(),
        EntryPointRole::Runtime
    );
    assert_eq!(knex::KnexPlugin.entry_point_role(), EntryPointRole::Support);
}

#[test]
fn plugins_with_entry_patterns_have_explicit_role_intent() {
    let runtime_or_test_or_support: rustc_hash::FxHashSet<&'static str> = TEST_ENTRY_POINT_PLUGINS
        .iter()
        .chain(RUNTIME_ENTRY_POINT_PLUGINS.iter())
        .chain(SUPPORT_ENTRY_POINT_PLUGINS.iter())
        .copied()
        .collect();

    for plugin in crate::plugins::registry::builtin::create_builtin_plugins() {
        if plugin.entry_patterns().is_empty() {
            continue;
        }
        assert!(
            runtime_or_test_or_support.contains(plugin.name()),
            "plugin '{}' exposes entry patterns but is missing from the entry-point role map",
            plugin.name()
        );
    }
}

/// The registry skips a config whose result `is_empty`, so every field that
/// carries a contribution must make the result non-empty on its own.
#[test]
fn plugin_result_is_empty_only_when_every_field_is_empty() {
    type Fill = fn(&mut PluginResult);

    assert!(PluginResult::default().is_empty());

    let rows: [(&str, Fill); 18] = [
        ("entry_patterns", |r| {
            r.entry_patterns.push(PathRule::new("src/*.ts"));
        }),
        ("used_exports", |r| {
            r.used_exports
                .push(UsedExportRule::new("src/*.ts", ["default"]));
        }),
        ("used_class_members", |r| {
            r.used_class_members
                .push(UsedClassMemberRule::from("render"));
        }),
        ("referenced_dependencies", |r| {
            r.referenced_dependencies.push("lodash".to_string());
        }),
        ("package_referenced_dependencies", |r| {
            r.package_referenced_dependencies
                .push((PathBuf::from("/project/pkg"), "lodash".to_string()));
        }),
        ("always_used_files", |r| {
            r.always_used_files.push("**/*.stories.tsx".to_string());
        }),
        ("path_aliases", |r| {
            r.path_aliases.push(("@".to_string(), "src".to_string()));
        }),
        ("setup_files", |r| {
            r.setup_files.push(PathBuf::from("/setup.ts"));
        }),
        ("fixture_patterns", |r| {
            r.fixture_patterns.push("**/__fixtures__/**/*".to_string());
        }),
        ("scss_include_paths", |r| {
            r.scss_include_paths.push(PathBuf::from("/project/styles"));
        }),
        ("static_dir_mappings", |r| {
            r.static_dir_mappings
                .push((PathBuf::from("/project/public"), "/".to_string()));
        }),
        ("framework_static_dir_mappings", |r| {
            r.framework_static_dir_mappings
                .push((PathBuf::from("/project/static"), "/".to_string()));
        }),
        ("provided_dependencies", |r| {
            r.provided_dependencies.push(ProvidedDependencyRule::new(
                "**/*.stories.tsx",
                ["react"],
                Vec::<String>::new(),
            ));
        }),
        ("config_diagnostics", |r| {
            r.config_diagnostics
                .push(PluginConfigDiagnostic::unreadable(
                    Path::new("/project/webpack.config.js"),
                    "webpack",
                    "exposes",
                    "dynamic-value",
                ));
        }),
        ("federation_sources", |r| {
            r.federation_sources.push(FederationSource {
                target: FederationSourceTarget::Remote("app".to_string()),
                config_path: PathBuf::from("/project/webpack.config.js"),
                plugin: "webpack".to_string(),
                key: "remotes",
            });
        }),
        ("jsx_import_sources", |r| {
            r.jsx_import_sources.push(JsxImportSourceRule {
                source: "./src/jsx".to_string(),
                config_dir: PathBuf::from("/project"),
                include: vec!["src/**/*.test.tsx".to_string()],
                exclude: vec![],
            });
        }),
        ("jsx_package_credits", |r| {
            r.jsx_package_credits.push(JsxImportSourceRule {
                source: "react".to_string(),
                config_dir: PathBuf::from("/project"),
                include: vec!["src/**/*.test.tsx".to_string()],
                exclude: vec![],
            });
        }),
        ("tsconfig_jsx_dirs", |r| {
            r.tsconfig_jsx_dirs.push(PathBuf::from("/project"));
        }),
    ];

    for (field, fill) in rows {
        let mut result = PluginResult::default();
        fill(&mut result);
        assert!(
            !result.is_empty(),
            "a result with only {field} set must not be empty"
        );
    }
}

#[test]
fn is_enabled_with_deps_prefix_match() {
    let plugin = storybook::StorybookPlugin;
    let deps = vec!["@storybook/react".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
}

#[test]
fn is_enabled_with_deps_prefix_no_match_without_slash() {
    let plugin = storybook::StorybookPlugin;
    let deps = vec!["@storybookish".to_string()];
    assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
}

#[test]
fn is_enabled_with_deps_multiple_enablers() {
    let plugin = vitest::VitestPlugin;
    let deps_vitest = vec!["vitest".to_string()];
    let deps_none = vec!["mocha".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_vitest, Path::new("/project")));
    assert!(!plugin.is_enabled_with_deps(&deps_none, Path::new("/project")));
}

#[test]
fn plugin_resolve_config_default_returns_empty() {
    let plugin = commitizen::CommitizenPlugin;
    let result = plugin.resolve_config(
        Path::new("/project/config.js"),
        "const x = 1;",
        Path::new("/project"),
    );
    assert!(result.is_empty());
}

#[test]
fn is_enabled_with_deps_exact_and_prefix_both_work() {
    let plugin = storybook::StorybookPlugin;
    let deps_exact = vec!["storybook".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_exact, Path::new("/project")));
    let deps_prefix = vec!["@storybook/vue3".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_prefix, Path::new("/project")));
}

#[test]
fn is_enabled_with_deps_multiple_enablers_remix() {
    let plugin = remix::RemixPlugin;
    let deps_node = vec!["@remix-run/node".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_node, Path::new("/project")));
    let deps_react = vec!["@remix-run/react".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_react, Path::new("/project")));
    let deps_cf = vec!["@remix-run/cloudflare".to_string()];
    assert!(plugin.is_enabled_with_deps(&deps_cf, Path::new("/project")));
}

struct MinimalPlugin;
impl Plugin for MinimalPlugin {
    fn name(&self) -> &'static str {
        "minimal"
    }
}

#[test]
fn default_resolve_config_returns_empty() {
    let r =
        MinimalPlugin.resolve_config(Path::new("config.js"), "export default {}", Path::new("/"));
    assert!(r.is_empty());
}

#[test]
fn default_package_json_metadata_hooks_are_empty() {
    let pkg = PackageJson::default();
    assert!(!MinimalPlugin.is_enabled_with_package_json(&pkg, Path::new("/")));
    assert!(
        MinimalPlugin
            .resolve_package_json(&pkg, Path::new("/"))
            .is_empty()
    );
}

#[test]
fn default_is_enabled_returns_false_when_no_enablers() {
    let deps = vec!["anything".to_string()];
    assert!(!MinimalPlugin.is_enabled_with_deps(&deps, Path::new("/")));
}

#[test]
fn all_builtin_plugin_names_are_non_empty_and_unique() {
    let plugins = registry::builtin::create_builtin_plugins();
    let mut seen = std::collections::BTreeSet::new();
    for p in &plugins {
        let name = p.name();
        assert!(
            !name.is_empty(),
            "builtin plugins must have a non-empty name"
        );
        assert!(seen.insert(name), "duplicate plugin name: {name}");
    }
}

#[test]
fn all_builtin_plugins_have_activation_signals() {
    // Plugins activated from package metadata or filesystem sentinels rather
    // than dependency enablers (napi binary name; deno.json presence;
    // kibana.jsonc presence; action.yml with a node runtime).
    const NON_DEPENDENCY_ACTIVATED_PLUGINS: &[&str] = &[
        "napi-rs",
        "deno",
        "kibana",
        "typescript-content-mapper",
        "github-action",
    ];
    let plugins = registry::builtin::create_builtin_plugins();
    for p in &plugins {
        assert!(
            !p.enablers().is_empty()
                || !p.script_enablers().is_empty()
                || NON_DEPENDENCY_ACTIVATED_PLUGINS.contains(&p.name()),
            "plugin '{}' has no activation signal",
            p.name()
        );
    }
}

#[test]
fn plugins_with_config_patterns_have_always_used() {
    let plugins = registry::builtin::create_builtin_plugins();
    for p in &plugins {
        if !p.config_patterns().is_empty() {
            assert!(
                !p.always_used().is_empty(),
                "plugin '{}' has config_patterns but no always_used",
                p.name()
            );
        }
    }
}

#[test]
fn framework_plugins_enablers() {
    let cases: Vec<(&dyn Plugin, &[&str])> = vec![
        (&nextjs::NextJsPlugin, &["next"]),
        (&nuxt::NuxtPlugin, &["nuxt"]),
        (&angular::AngularPlugin, &["@angular/core"]),
        (&ionic::IonicPlugin, &["@ionic/angular"]),
        (&sveltekit::SvelteKitPlugin, &["@sveltejs/kit"]),
        (&gatsby::GatsbyPlugin, &["gatsby"]),
    ];
    for (plugin, expected_enablers) in cases {
        let enablers = plugin.enablers();
        for expected in expected_enablers {
            assert!(
                enablers.contains(expected),
                "plugin '{}' should have '{}'",
                plugin.name(),
                expected
            );
        }
    }
}

#[test]
fn testing_plugins_enablers() {
    let cases: Vec<(&dyn Plugin, &str)> = vec![
        (&jest::JestPlugin, "jest"),
        (&vitest::VitestPlugin, "vitest"),
        (&playwright::PlaywrightPlugin, "@playwright/test"),
        (&cypress::CypressPlugin, "cypress"),
        (&mocha::MochaPlugin, "mocha"),
        (&stryker::StrykerPlugin, "@stryker-mutator/core"),
    ];
    for (plugin, enabler) in cases {
        assert!(
            plugin.enablers().contains(&enabler),
            "plugin '{}' should have '{}'",
            plugin.name(),
            enabler
        );
    }
}

#[test]
fn bundler_plugins_enablers() {
    let cases: Vec<(&dyn Plugin, &str)> = vec![
        (&vite::VitePlugin, "vite"),
        (&webpack::WebpackPlugin, "webpack"),
        (&rollup::RollupPlugin, "rollup"),
    ];
    for (plugin, enabler) in cases {
        assert!(
            plugin.enablers().contains(&enabler),
            "plugin '{}' should have '{}'",
            plugin.name(),
            enabler
        );
    }
}

#[test]
fn test_plugins_have_test_entry_patterns() {
    let test_plugins: Vec<&dyn Plugin> = vec![
        &bun::BunPlugin,
        &deno::DenoPlugin,
        &jest::JestPlugin,
        &vitest::VitestPlugin,
        &mocha::MochaPlugin,
        &tap::TapPlugin,
        &tsd::TsdPlugin,
    ];
    for plugin in test_plugins {
        let patterns = plugin.entry_patterns();
        assert!(
            !patterns.is_empty(),
            "test plugin '{}' should have entry patterns",
            plugin.name()
        );
        assert!(
            patterns
                .iter()
                .any(|p| p.contains("test") || p.contains("spec") || p.contains("__tests__")),
            "test plugin '{}' should have test/spec patterns",
            plugin.name()
        );
    }
}

#[test]
fn framework_plugins_have_entry_patterns() {
    let plugins: Vec<&dyn Plugin> = vec![
        &nextjs::NextJsPlugin,
        &nuxt::NuxtPlugin,
        &angular::AngularPlugin,
        &sveltekit::SvelteKitPlugin,
    ];
    for plugin in plugins {
        assert!(
            !plugin.entry_patterns().is_empty(),
            "framework plugin '{}' should have entry patterns",
            plugin.name()
        );
    }
}

#[test]
fn plugins_with_resolve_config_have_config_patterns() {
    let plugins: Vec<&dyn Plugin> = vec![
        &jest::JestPlugin,
        &vitest::VitestPlugin,
        &babel::BabelPlugin,
        &eslint::EslintPlugin,
        &webpack::WebpackPlugin,
        &storybook::StorybookPlugin,
        &typescript::TypeScriptPlugin,
        &postcss::PostCssPlugin,
        &nextjs::NextJsPlugin,
        &nuxt::NuxtPlugin,
        &angular::AngularPlugin,
        &nx::NxPlugin,
        &stryker::StrykerPlugin,
        &wuchale::WuchalePlugin,
        &rollup::RollupPlugin,
        &sveltekit::SvelteKitPlugin,
        &prettier::PrettierPlugin,
        &contentlayer::ContentlayerPlugin,
    ];
    for plugin in plugins {
        assert!(
            !plugin.config_patterns().is_empty(),
            "plugin '{}' with resolve_config should have config_patterns",
            plugin.name()
        );
    }
}

#[test]
fn plugin_tooling_deps_include_enabler_package() {
    let plugins: Vec<&dyn Plugin> = vec![
        &jest::JestPlugin,
        &vitest::VitestPlugin,
        &webpack::WebpackPlugin,
        &typescript::TypeScriptPlugin,
        &eslint::EslintPlugin,
        &prettier::PrettierPlugin,
        &danger::DangerPlugin,
        &stryker::StrykerPlugin,
        &wuchale::WuchalePlugin,
        &contentlayer::ContentlayerPlugin,
    ];
    for plugin in plugins {
        let tooling = plugin.tooling_dependencies();
        let enablers = plugin.enablers();
        assert!(
            enablers
                .iter()
                .any(|e| !e.ends_with('/') && tooling.contains(e)),
            "plugin '{}': at least one non-prefix enabler should be in tooling_dependencies",
            plugin.name()
        );
    }
}

#[test]
fn nextjs_has_used_exports_for_pages() {
    let plugin = nextjs::NextJsPlugin;
    let exports = plugin.used_exports();
    assert!(!exports.is_empty());
    assert!(exports.iter().any(|(_, names)| names.contains(&"default")));
}

#[test]
fn remix_has_used_exports_for_routes() {
    let plugin = remix::RemixPlugin;
    let exports = plugin.used_exports();
    assert!(!exports.is_empty());
    let route_entry = exports.iter().find(|(pat, _)| pat.contains("routes"));
    assert!(route_entry.is_some());
    let (_, names) = route_entry.unwrap();
    assert!(names.contains(&"loader"));
    assert!(names.contains(&"action"));
    assert!(names.contains(&"default"));
}

#[test]
fn sveltekit_has_used_exports_for_routes() {
    let plugin = sveltekit::SvelteKitPlugin;
    let exports = plugin.used_exports();
    assert!(!exports.is_empty());
    assert!(exports.iter().any(|(_, names)| names.contains(&"GET")));
}

#[test]
fn nuxt_has_hash_virtual_prefix() {
    assert!(nuxt::NuxtPlugin.virtual_module_prefixes().contains(&"#"));
}

#[test]
fn sveltekit_has_dollar_virtual_prefixes() {
    let prefixes = sveltekit::SvelteKitPlugin.virtual_module_prefixes();
    assert!(prefixes.contains(&"$app/"));
    assert!(prefixes.contains(&"$env/"));
    assert!(prefixes.contains(&"$lib/"));
}

#[test]
fn sveltekit_has_lib_path_alias() {
    let aliases = sveltekit::SvelteKitPlugin.path_aliases(Path::new("/project"));
    assert!(aliases.iter().any(|(prefix, _)| *prefix == "$lib/"));
}

#[test]
fn nuxt_has_tilde_path_alias() {
    let aliases = nuxt::NuxtPlugin.path_aliases(Path::new("/nonexistent"));
    assert!(aliases.iter().any(|(prefix, _)| *prefix == "~/"));
    assert!(aliases.iter().any(|(prefix, _)| *prefix == "~~/"));
}

#[test]
fn jest_has_package_json_config_key() {
    assert_eq!(jest::JestPlugin.package_json_config_key(), Some("jest"));
}

#[test]
fn tsd_has_package_json_config_key() {
    assert_eq!(tsd::TsdPlugin.package_json_config_key(), Some("tsd"));
}

#[test]
fn babel_has_package_json_config_key() {
    assert_eq!(babel::BabelPlugin.package_json_config_key(), Some("babel"));
}

#[test]
fn eslint_has_package_json_config_key() {
    assert_eq!(
        eslint::EslintPlugin.package_json_config_key(),
        Some("eslintConfig")
    );
}

#[test]
fn prettier_has_package_json_config_key() {
    assert_eq!(
        prettier::PrettierPlugin.package_json_config_key(),
        Some("prettier")
    );
}

#[test]
fn macro_generated_plugin_basic_properties() {
    let plugin = msw::MswPlugin;
    assert_eq!(plugin.name(), "msw");
    assert!(plugin.enablers().contains(&"msw"));
    assert!(!plugin.entry_patterns().is_empty());
    assert!(plugin.config_patterns().is_empty());
    assert!(!plugin.always_used().is_empty());
    assert!(!plugin.tooling_dependencies().is_empty());
}

#[test]
fn macro_generated_plugin_with_used_exports() {
    let plugin = remix::RemixPlugin;
    assert_eq!(plugin.name(), "remix");
    assert!(!plugin.used_exports().is_empty());
}

#[test]
fn macro_passes_through_virtual_package_suffixes() {
    define_plugin! {
        struct MacroSuffixSmokePlugin => "macro-suffix-smoke",
        enablers: &["macro-suffix-smoke"],
        virtual_package_suffixes: &["/__macro_smoke__"],
    }

    let plugin = MacroSuffixSmokePlugin;
    assert_eq!(
        plugin.virtual_package_suffixes(),
        &["/__macro_smoke__"],
        "macro-declared virtual_package_suffixes must propagate to the trait method"
    );
}

#[test]
fn macro_generated_plugin_imports_only_resolve_config() {
    let plugin = cypress::CypressPlugin;
    let source = r"
        import { defineConfig } from 'cypress';
        import coveragePlugin from '@cypress/code-coverage';
        export default defineConfig({});
    ";
    let result = plugin.resolve_config(
        Path::new("cypress.config.ts"),
        source,
        Path::new("/project"),
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"cypress".to_string())
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"@cypress/code-coverage".to_string())
    );
}

#[test]
fn builtin_plugin_count_is_expected() {
    let plugins = registry::builtin::create_builtin_plugins();
    assert!(
        plugins.len() >= 110,
        "expected at least 110 built-in plugins, got {}",
        plugins.len()
    );
}

/// A pattern that climbs out of its workspace with `../` resolves against
/// the workspace prefix, so it names a file in a sibling workspace. A climb
/// past the project root stays unresolved and matches no project file.
#[test]
fn a_parent_relative_pattern_resolves_against_the_workspace_prefix() {
    let parent_relative = |pattern: &str| {
        let mut rule = PathRule::new(pattern);
        rule.parent_relative = true;
        rule
    };
    assert_eq!(
        parent_relative("../shared/src/Thing.tsx")
            .prefixed("packages/app")
            .pattern,
        "packages/shared/src/Thing.tsx"
    );
    assert_eq!(
        parent_relative("../../lib/index.{ts,js}")
            .prefixed("apps/web/client")
            .pattern,
        "apps/lib/index.{ts,js}"
    );
    assert!(
        parent_relative("../../../outside/Thing.tsx")
            .prefixed("packages/app")
            .pattern
            .starts_with("../"),
        "a climb past the project root matches no project file"
    );
    assert_eq!(
        parent_relative("src/index.ts")
            .prefixed("packages/app")
            .pattern,
        "packages/app/src/index.ts"
    );
}

/// On Windows the workspace prefix uses backslashes, so a sibling-workspace
/// pattern must climb the same segments as with forward slashes.
#[test]
fn a_parent_relative_pattern_resolves_against_a_backslash_prefix() {
    let mut rule = PathRule::new("../../packages/ui/src/**/*.mdx");
    rule.parent_relative = true;
    assert_eq!(
        rule.prefixed("apps\\docs").pattern,
        "packages/ui/src/**/*.mdx"
    );
}

/// Any other pattern keeps the plain prefix, so a config-directory-relative
/// `../src/**` does not climb out of its workspace.
#[test]
fn a_plain_parent_pattern_is_not_resolved() {
    assert_eq!(
        PathRule::new("../src/**/*.stories.tsx")
            .prefixed("packages/ui")
            .pattern,
        "packages/ui/../src/**/*.stories.tsx"
    );
}
