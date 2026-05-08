use super::common::{create_config, fixture_path};

// ── Framework entry points (Next.js) ───────────────────────────

#[test]
fn nextjs_page_default_export_not_flagged() {
    let root = fixture_path("nextjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // page.tsx is a Next.js App Router entry point, so it should NOT be unused
    assert!(
        !unused_file_names.contains(&"page.tsx".to_string()),
        "page.tsx should be treated as framework entry point, unused files: {unused_file_names:?}"
    );

    // utils.ts is not imported by anything, so it should be unused
    assert!(
        unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn nextjs_unused_util_export_flagged() {
    let root = fixture_path("nextjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // unusedUtil is exported but never imported — however, since utils.ts is an
    // unreachable file, it may be reported as unused file instead of unused export.
    // The key point is that it IS flagged as a problem in some way.
    let has_unused_export = results
        .unused_exports
        .iter()
        .any(|e| e.export_name == "unusedUtil");
    let has_unused_file = results
        .unused_files
        .iter()
        .any(|f| f.path.file_name().is_some_and(|n| n == "utils.ts"));

    assert!(
        has_unused_export || has_unused_file,
        "unusedUtil should be flagged as unused export or utils.ts as unused file"
    );
}

#[test]
fn nextjs_convention_exports_are_not_flagged() {
    let root = fixture_path("nextjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();

    for expected_used in [
        "revalidate",
        "dynamic",
        "generateMetadata",
        "viewport",
        "GET",
        "runtime",
        "preferredRegion",
        "proxy",
        "config",
        "register",
        "onRequestError",
        "onRouterTransitionStart",
        "reportWebVitals",
    ] {
        assert!(
            !unused_export_names.contains(&expected_used),
            "{expected_used} should be treated as a framework-used Next.js export, found: {unused_export_names:?}"
        );
    }
}

#[test]
fn nextjs_special_file_exports_are_not_flagged() {
    let root = fixture_path("nextjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.export_name.clone(),
            )
        })
        .collect();

    for (file, export) in [
        ("loading.tsx", "default"),
        ("error.tsx", "default"),
        ("not-found.tsx", "default"),
        ("template.tsx", "default"),
        ("default.tsx", "default"),
        ("global-error.tsx", "default"),
        ("global-not-found.tsx", "default"),
        ("global-not-found.tsx", "metadata"),
        ("mdx-components.tsx", "useMDXComponents"),
    ] {
        assert!(
            !unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should be treated as framework-used, found: {unused_exports:?}"
        );
    }

    for (file, export) in [
        ("loading.tsx", "unusedLoadingHelper"),
        ("proxy.ts", "unusedProxyHelper"),
        ("instrumentation.ts", "unusedInstrumentationHelper"),
        ("instrumentation-client.ts", "unusedClientHelper"),
        ("mdx-components.tsx", "unusedMdxHelper"),
        ("global-not-found.tsx", "unusedGlobalNotFoundHelper"),
    ] {
        assert!(
            unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn nextjs_config_referenced_dependencies_are_not_flagged_unused() {
    let root = fixture_path("nextjs-config-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"@acme/ui"),
        "@acme/ui should be treated as used via next.config transpilePackages: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "left-pad should remain unused as a control dependency: {unused_dep_names:?}"
    );
}

#[test]
fn turborepo_generator_config_is_used_without_globbing_generator_directory() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("turbo/generators")).unwrap();

    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "turborepo-generator-fixture",
            "devDependencies": {
                "@turbo/gen": "*",
                "turbo": "*"
            }
        }"#,
    )
    .unwrap();
    std::fs::write(root.join("turbo.json"), "{}").unwrap();
    std::fs::write(
        root.join("turbo/generators/config.ts"),
        r#"
            import type { PlopTypes } from "@turbo/gen";
            import { registerGenerator } from "./helper";

            export default function generator(plop: PlopTypes.NodePlopAPI): void {
                registerGenerator(plop);
            }
        "#,
    )
    .unwrap();
    std::fs::write(
        root.join("turbo/generators/helper.ts"),
        "export function registerGenerator(_plop: unknown): void {}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("turbo/generators/orphan.ts"),
        "export const orphan = true;\n",
    )
    .unwrap();

    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.path
                .strip_prefix(&root)
                .unwrap_or(&file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|path| path == "turbo/generators/config.ts"),
        "generator config should be treated as a Turborepo config file, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "turbo/generators/helper.ts"),
        "helper should be reachable through the generator config import, unused files: {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path == "turbo/generators/orphan.ts"),
        "unimported generator files should not be kept alive by directory globbing, unused files: {unused_files:?}"
    );
}

// ── Test runner entry points ──────────────────────────────────

#[test]
fn tap_test_files_are_not_flagged_unused() {
    let root = fixture_path("tap-project");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(&root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_files.iter().any(|path| path == "test/basic.test.js"),
        "tap test file should be treated as a test entry point, unused files: {unused_files:?}"
    );
}

#[test]
fn tsd_test_files_are_not_flagged_unused() {
    let root = fixture_path("tsd-project");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.path
                .strip_prefix(&root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|path| path == "test/types/index.test-d.ts"),
        "tsd test file should be treated as a configured test entry point, unused files: {unused_files:?}"
    );
}

// ── Path aliases ───────────────────────────────────────────────

#[test]
fn path_alias_not_flagged_as_unlisted() {
    let root = fixture_path("path-aliases");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();

    // @/utils is a path alias, not an npm package, so it should NOT be flagged
    assert!(
        !unlisted_names.contains(&"@/utils"),
        "@/utils should not be flagged as unlisted dependency, found: {unlisted_names:?}"
    );
}

#[test]
fn path_aliases_mixed_exports_no_false_positive_unused_files() {
    let root = fixture_path("path-aliases-mixed-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    // types.ts and helpers.ts have SOME used exports (imported via @/ path alias)
    // — they should NOT be in unused_files even though they also have unused exports
    assert!(
        !unused_file_names.contains(&"types.ts".to_string()),
        "types.ts has used exports and should not be an unused file: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"helpers.ts".to_string()),
        "helpers.ts has used exports and should not be an unused file: {unused_file_names:?}"
    );

    // orphan.ts is truly unused — no file imports it
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file: {unused_file_names:?}"
    );

    // Verify unused exports are correctly detected on reachable files
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedExport"),
        "unusedExport should be detected: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedHelper"),
        "unusedHelper should be detected: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"usedExport"),
        "usedExport should NOT be in unused exports: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"usedHelper"),
        "usedHelper should NOT be in unused exports: {unused_export_names:?}"
    );
}

// ── CSS/Tailwind ───────────────────────────────────────────────

#[test]
fn css_apply_marks_tailwind_as_used() {
    let root = fixture_path("css-apply-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // tailwindcss should NOT be in unused dependencies (it's used via @apply in styles.css)
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"tailwindcss"),
        "tailwindcss should not be unused, it's referenced via @apply in CSS: {unused_dep_names:?}"
    );

    // unused.css should be detected as an unused file
    let unused_files: Vec<&str> = results
        .unused_files
        .iter()
        .filter_map(|f| f.path.file_name())
        .filter_map(|f| f.to_str())
        .collect();
    assert!(
        unused_files.contains(&"unused.css"),
        "unused.css should be detected as unused: {unused_files:?}"
    );
}

#[test]
fn css_package_subpath_imports_resolve_from_node_modules() {
    let root = fixture_path("css-package-subpath-import");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("tailwindcss/theme.css")),
        "CSS package subpath import should resolve via node_modules: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("tailwindcss/utilities.css")),
        "CSS package subpath import should resolve via node_modules: {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("shadcn/tailwind.css")),
        "CSS package subpath import should resolve through package.json exports style condition: \
         {unresolved_specs:?}"
    );
    assert!(
        !unresolved_specs
            .iter()
            .any(|s| s.contains("components/button.css")),
        "CSS local subpath import should still resolve relative to the importing file: \
         {unresolved_specs:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"tailwindcss"),
        "tailwindcss imported via CSS package subpaths must not be unused: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"shadcn"),
        "shadcn imported via package.json exports style condition must not be unused: \
         {unused_dep_names:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.path.file_name())
        .filter_map(|f| f.to_str())
        .map(String::from)
        .collect();
    assert!(
        !unused_file_names.contains(&"button.css".to_string()),
        "local CSS subpath imports should keep nested CSS files reachable: {unused_file_names:?}"
    );
}

#[test]
fn tailwind_plugin_directive_marks_plugin_targets_used() {
    let root = fixture_path("tailwind-plugin-directive");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@tailwindcss/typography"),
        "@tailwindcss/typography should be credited via @plugin: {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"daisyui"),
        "daisyui should be credited via @plugin: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"unused-plugin"),
        "unreferenced dependencies should still be reported: {unused_dep_names:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.path.file_name())
        .filter_map(|f| f.to_str())
        .map(String::from)
        .collect();
    assert!(
        !unused_file_names.contains(&"tailwind-local-plugin.js".to_string()),
        "relative @plugin target should be reachable: {unused_file_names:?}"
    );
    assert!(
        unused_file_names.contains(&"unused.css".to_string()),
        "unreferenced CSS files should still be reported: {unused_file_names:?}"
    );

    let unused_exports: Vec<(&str, &str)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.path.file_name().and_then(|f| f.to_str()).unwrap_or(""),
                e.export_name.as_str(),
            )
        })
        .collect();
    assert!(
        !unused_exports.contains(&("tailwind-local-plugin.js", "default")),
        "relative @plugin target should consume the plugin default export: {unused_exports:?}"
    );
}

#[test]
fn pandacss_config_is_not_flagged_unused() {
    let root = fixture_path("pandacss-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| f.path.file_name())
        .filter_map(|f| f.to_str())
        .map(String::from)
        .collect();
    assert!(
        !unused_file_names.contains(&"panda.config.ts".to_string()),
        "panda.config.ts should not be flagged unused when @pandacss/dev is a dependency: {unused_file_names:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@pandacss/dev"),
        "@pandacss/dev should not be unused (it's tooling referenced by the config): {unused_dep_names:?}"
    );
    assert!(
        !unused_dep_names.contains(&"@pandacss/preset-panda"),
        "@pandacss/preset-panda should not be unused (it's imported by panda.config.ts): {unused_dep_names:?}"
    );
}

#[test]
fn vite_aliases_from_config_resolve_internal_modules() {
    let root = fixture_path("vite-alias-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.contains(&"@/utils/messages"),
        "vite alias import should resolve, found unresolved: {unresolved_specs:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"messages.ts".to_string()),
        "messages.ts should be reachable via vite alias import: {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedMessage"),
        "reachable aliased module should still report unused exports: {unused_export_names:?}"
    );
}

#[test]
fn webpack_aliases_from_config_resolve_internal_modules() {
    let root = fixture_path("webpack-alias-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|u| u.package_name.as_str())
        .collect();
    assert!(
        !unlisted_names.contains(&"@components/Button"),
        "webpack alias import should not be treated as an unlisted dependency: {unlisted_names:?}"
    );
    assert!(
        !unlisted_names.contains(&"@utils/messages"),
        "webpack alias import should not be treated as an unlisted dependency: {unlisted_names:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"app.ts".to_string()),
        "app.ts should be reachable via webpack context + entry descriptor: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"Button.ts".to_string()),
        "Button.ts should be reachable via webpack alias import: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"messages.ts".to_string()),
        "messages.ts should be reachable via webpack alias import: {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedMessage"),
        "reachable aliased module should still report unused exports: {unused_export_names:?}"
    );
}

#[test]
fn webpack_descriptor_without_context_resolves_relative_entry() {
    let root = fixture_path("webpack-no-context-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"app.ts".to_string()),
        "descriptor entry './src/app.ts' should resolve without an accompanying context: \
         {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"greet.ts".to_string()),
        "module imported transitively from a resolved descriptor entry should be reachable: \
         {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedGreet"),
        "reachable module should still report its unused exports: {unused_export_names:?}"
    );
}

#[test]
fn sveltekit_aliases_from_config_resolve_internal_modules() {
    let root = fixture_path("sveltekit-alias-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.contains(&"$utils/greeting"),
        "sveltekit alias import should resolve, found unresolved: {unresolved_specs:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"greeting.ts".to_string()),
        "greeting.ts should be reachable via sveltekit alias import: {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedGreeting"),
        "reachable aliased module should still report unused exports: {unused_export_names:?}"
    );
}

#[test]
fn nuxt_custom_dirs_and_aliases_reduce_false_positives() {
    let root = fixture_path("nuxt-custom-dirs");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.contains(&"@shared/utils"),
        "nuxt alias import should resolve, found unresolved: {unresolved_specs:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"utils.ts".to_string()),
        "utils.ts should be reachable via nuxt alias import: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"useGreeting.ts".to_string()),
        "custom nuxt auto-import dir should keep composable alive: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"FancyCard.vue".to_string()),
        "custom nuxt component dir should keep component alive: {unused_file_names:?}"
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedShared"),
        "reachable nuxt aliased module should still report unused exports: {unused_export_names:?}"
    );
}

#[test]
fn nuxt_src_dir_config_reduces_false_positives() {
    let root = fixture_path("nuxt-src-dir");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specs: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specs.contains(&"@shared/utils"),
        "nuxt srcDir alias import should resolve, found unresolved: {unresolved_specs:?}"
    );

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    for expected_used in [
        "utils.ts",
        "useGreeting.ts",
        "FancyCard.vue",
        "app.vue",
        "app.config.ts",
        "error.vue",
    ] {
        assert!(
            !unused_file_names.contains(&expected_used.to_string()),
            "{expected_used} should be kept alive by Nuxt srcDir support: {unused_file_names:?}"
        );
    }

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unusedShared"),
        "reachable nuxt srcDir aliased module should still report unused exports: {unused_export_names:?}"
    );
}

#[test]
fn nuxt_default_scan_keeps_nested_plugin_index_but_not_nested_helpers() {
    let root = fixture_path("nuxt-default-scan");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    for expected_unused in ["useHidden.ts", "format.ts", "helper.ts"] {
        assert!(
            unused_file_names.contains(&expected_unused.to_string()),
            "{expected_unused} should stay unused because Nuxt does not scan nested helpers by default: {unused_file_names:?}"
        );
    }

    for expected_used in [
        "index.ts",
        "format-shared-greeting.ts",
        "shared-greeting.ts",
    ] {
        assert!(
            !unused_file_names.contains(&expected_used.to_string()),
            "{expected_used} should stay reachable via Nuxt default scanning: {unused_file_names:?}"
        );
    }
}

#[test]
fn nuxt_runtime_conventions_report_dead_named_exports_without_unused_file_noise() {
    let root = fixture_path("nuxt-runtime-conventions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    for expected_used in ["RootBadge.vue", "bootstrap.ts", "auth.ts", "logger.ts"] {
        assert!(
            !unused_file_names.contains(&expected_used.to_string()),
            "{expected_used} should be kept alive by Nuxt runtime conventions: {unused_file_names:?}"
        );
    }

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.export_name.clone(),
            )
        })
        .collect();
    for (file, export) in [
        ("RootBadge.vue", "deadNamed"),
        ("bootstrap.ts", "deadPluginHelper"),
        ("auth.ts", "deadMiddlewareHelper"),
        ("logger.ts", "deadServerMiddlewareHelper"),
    ] {
        assert!(
            unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn nuxt_configured_runtime_paths_reduce_false_positives_and_keep_dead_exports_visible() {
    let root = fixture_path("nuxt-config-runtime-paths");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    for expected_used in [
        "FeatureCard.vue",
        "plain-plugin.ts",
        "object-plugin.ts",
        "auth.ts",
    ] {
        assert!(
            !unused_file_names.contains(&expected_used.to_string()),
            "{expected_used} should be kept alive by configured Nuxt runtime paths: {unused_file_names:?}"
        );
    }

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.export_name.clone(),
            )
        })
        .collect();
    for (file, export) in [
        ("FeatureCard.vue", "deadFeatureNamed"),
        ("plain-plugin.ts", "deadPlainPluginHelper"),
        ("object-plugin.ts", "deadObjectPluginHelper"),
        ("auth.ts", "deadAppMiddlewareHelper"),
    ] {
        assert!(
            unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should be reported as unused, found: {unused_exports:?}"
        );
    }
}

#[test]
fn nuxt_css_tilde_alias_keeps_app_assets_alive() {
    // nuxt.config.ts with css:['~/assets/main.css'] must not flag
    // app/assets/main.css as unused (default Nuxt 4 srcDir = app/).
    let root = fixture_path("nuxt-css-alias");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|p| p.ends_with("app/assets/main.css")),
        "app/assets/main.css should be kept alive by nuxt.config.ts css entry: {unused_files:?}"
    );
}

#[test]
fn nuxt_convention_exports_preserve_defaults_but_report_dead_helpers() {
    let root = fixture_path("nuxt-convention-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.path.file_name().unwrap().to_string_lossy().to_string(),
                e.export_name.clone(),
            )
        })
        .collect();

    for (file, export) in [
        ("app.vue", "default"),
        ("app.config.ts", "default"),
        ("index.vue", "default"),
        ("default.vue", "default"),
        ("FancyCard.vue", "default"),
        ("client.ts", "default"),
        ("hello.ts", "default"),
        ("custom.ts", "default"),
    ] {
        assert!(
            !unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should be framework-used in Nuxt, found: {unused_exports:?}"
        );
    }

    for (file, export) in [
        ("app.vue", "unusedAppHelper"),
        ("app.config.ts", "unusedConfigHelper"),
        ("index.vue", "unusedPageHelper"),
        ("default.vue", "unusedLayoutHelper"),
        ("FancyCard.vue", "unusedCardHelper"),
        ("client.ts", "unusedPluginHelper"),
        ("hello.ts", "unusedRouteHelper"),
        ("custom.ts", "unusedModuleHelper"),
    ] {
        assert!(
            unused_exports
                .iter()
                .any(|(unused_file, unused_export)| unused_file == file && unused_export == export),
            "{file}:{export} should still be reported as unused, found: {unused_exports:?}"
        );
    }
}
