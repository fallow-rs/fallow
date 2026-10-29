//! Shared Vitest/Vite alias extraction.
//!
//! Vitest merges `test.alias` and `resolve.alias` across object and workspace configs,
//! so both plugins route aliases through this helper to keep the fixes aligned.

use std::path::Path;

use super::PluginResult;
use super::alias_replacement::{
    alias_target_is_source_file, credit_unrouted_replacement, is_bare_package_specifier,
};
use super::config_parser;

/// Apply one alias entry to the plugin result.
///
/// Path aliases feed `path_aliases`, source-file targets seed setup files, and
/// bare-package aliases are credited as referenced dependencies. Bare-string
/// package-to-package aliases are special-cased so they credit both packages
/// without emitting a path alias.
pub(super) fn process_test_alias(
    result: &mut PluginResult,
    find: &str,
    replacement: &str,
    replacement_is_bare_string_literal: bool,
    config_path: &Path,
    root: &Path,
) {
    let find_is_pkg = is_bare_package_specifier(find);

    if find_is_pkg && replacement_is_bare_string_literal && is_bare_package_specifier(replacement) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(replacement));
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(find));
        return;
    }

    let Some(normalized) = config_parser::normalize_config_path(replacement, config_path, root)
    else {
        return;
    };

    result
        .path_aliases
        .push((find.to_owned(), normalized.clone()));
    credit_unrouted_replacement(result, find, &normalized, config_path);
    if alias_target_is_source_file(&normalized) {
        result.setup_files.push(root.join(&normalized));
    }
    if find_is_pkg {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(find));
    }

    tracing::debug!(find, target = %normalized, "test alias extracted");
}

/// Extract and apply the Vitest test-block aliases shared by the Vitest and Vite plugins.
pub(super) fn apply_test_block_aliases(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    for (find, replacement, is_bare) in
        config_parser::extract_config_aliases_kinded(source, config_path, &["test", "alias"])
    {
        process_test_alias(result, &find, &replacement, is_bare, config_path, root);
    }
    for (find, replacement, is_bare) in config_parser::extract_config_array_nested_aliases_kinded(
        source,
        config_path,
        &["test", "projects"],
        &["test", "alias"],
    ) {
        process_test_alias(result, &find, &replacement, is_bare, config_path, root);
    }
    for (find, replacement, is_bare) in config_parser::extract_config_array_nested_aliases_kinded(
        source,
        config_path,
        &["test", "projects"],
        &["resolve", "alias"],
    ) {
        process_test_alias(result, &find, &replacement, is_bare, config_path, root);
    }
}

/// Extract and apply aliases from a `vitest.workspace.{ts,js}` array file.
pub(super) fn apply_workspace_array_aliases(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    for alias_path in [["test", "alias"], ["resolve", "alias"]] {
        for (find, replacement, is_bare) in
            config_parser::extract_default_export_array_aliases_kinded(
                source,
                config_path,
                &alias_path,
            )
        {
            process_test_alias(result, &find, &replacement, is_bare, config_path, root);
        }
    }
}

/// Emit a debug line when a Vitest/Vite config is not statically reachable.
pub(super) fn debug_unreachable_config(source: &str, config_path: &Path) {
    if config_parser::config_default_export_unreachable(source, config_path) {
        tracing::debug!(
            config = %config_path.display(),
            "test/resolve aliases not extracted: config default export is not a statically \
             reachable object or array (e.g. mergeConfig / imported base config)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> std::path::PathBuf {
        std::path::PathBuf::from("/project/vitest.config.ts")
    }
    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from("/project")
    }

    #[test]
    fn path_specifier_key_credits_every_replacement_export() {
        for find in ["./clock.js", "../src/clock", "/abs/clock.js"] {
            let mut result = PluginResult::default();
            process_test_alias(
                &mut result,
                find,
                "./test/stubs/clock.ts",
                false,
                &cfg(),
                &root(),
            );
            assert_eq!(
                result.entry_export_credits,
                vec![crate::plugins::UsedExportRule::new(
                    "test/stubs/clock.ts",
                    ["*"]
                )],
                "key `{find}` must credit the replacement exports"
            );
        }
    }

    #[test]
    fn bare_or_directory_alias_adds_no_entry_export_credit() {
        let mut result = PluginResult::default();
        process_test_alias(
            &mut result,
            "vscode",
            "./mock/vscode.ts",
            false,
            &cfg(),
            &root(),
        );
        process_test_alias(&mut result, "@", "src", false, &cfg(), &root());
        assert!(
            result.entry_export_credits.is_empty(),
            "the resolver routes these imports through the alias: {:?}",
            result.entry_export_credits
        );
    }

    #[test]
    fn installed_bare_key_credits_every_replacement_export() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let pkg = root.join("node_modules/@scope/pkg");
        std::fs::create_dir_all(&pkg).expect("create package dir");
        std::fs::write(pkg.join("package.json"), r#"{ "name": "@scope/pkg" }"#)
            .expect("write manifest");
        let config_path = root.join("vitest.config.ts");

        let mut result = PluginResult::default();
        process_test_alias(
            &mut result,
            "@scope/pkg",
            "./mocks/pkg.ts",
            false,
            &config_path,
            root,
        );
        for find in ["@lib", "~/lib", "missing-pkg"] {
            process_test_alias(
                &mut result,
                find,
                "./src/lib/index.ts",
                false,
                &config_path,
                root,
            );
        }
        assert_eq!(
            result.entry_export_credits,
            vec![crate::plugins::UsedExportRule::new("mocks/pkg.ts", ["*"])],
            "only the installed package key credits the replacement exports"
        );
    }

    #[test]
    fn hoisted_bare_key_credits_replacement_of_nested_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let pkg = root.join("node_modules/pkg");
        std::fs::create_dir_all(&pkg).expect("create package dir");
        std::fs::write(pkg.join("package.json"), r#"{ "name": "pkg" }"#).expect("write manifest");
        let config_path = root.join("packages/app/vitest.config.ts");

        let mut result = PluginResult::default();
        process_test_alias(
            &mut result,
            "pkg",
            "./mocks/pkg.ts",
            false,
            &config_path,
            root,
        );
        assert_eq!(
            result.entry_export_credits,
            vec![crate::plugins::UsedExportRule::new(
                "packages/app/mocks/pkg.ts",
                ["*"]
            )],
            "a package hoisted above the config directory is installed"
        );
    }

    #[test]
    fn package_to_package_credits_both_no_path_alias() {
        let mut result = PluginResult::default();
        process_test_alias(&mut result, "lodash-es", "lodash", true, &cfg(), &root());
        assert!(
            result.path_aliases.is_empty(),
            "package-to-package must emit no path alias: {:?}",
            result.path_aliases
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lodash".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lodash-es".to_string())
        );
    }

    #[test]
    fn path_builder_directory_alias_is_path_not_package_even_without_src_on_disk() {
        let mut result = PluginResult::default();
        process_test_alias(&mut result, "@", "src", false, &cfg(), &root());
        assert_eq!(
            result.path_aliases,
            vec![("@".to_string(), "src".to_string())],
            "path-expression directory alias must emit a path alias"
        );
    }

    #[test]
    fn bare_string_directory_alias_residual_is_package_to_package() {
        let mut result = PluginResult::default();
        process_test_alias(&mut result, "@", "src", true, &cfg(), &root());
        assert!(
            result.path_aliases.is_empty(),
            "bare-string `@`->`src` is treated as package-to-package (documented residual)"
        );
    }

    #[test]
    fn kinded_extractor_flags_string_literal_vs_path_expression() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                resolve: {
                    alias: {
                        "@": resolve(__dirname, "src"),
                        "lodash-es": "lodash",
                        "@rel": "./src/x",
                        "@url": new URL("./mock.ts", import.meta.url)
                    }
                }
            };
        "#;
        let aliases = config_parser::extract_config_aliases_kinded(
            source,
            std::path::Path::new("/project/vitest.config.ts"),
            &["resolve", "alias"],
        );
        let is_bare = |key: &str| {
            aliases
                .iter()
                .find(|(f, _, _)| f == key)
                .map(|(_, _, b)| *b)
        };
        assert_eq!(
            is_bare("@"),
            Some(false),
            "path.resolve(...) is a path expr"
        );
        assert_eq!(
            is_bare("lodash-es"),
            Some(true),
            "bare string literal is package-to-package eligible"
        );
        assert_eq!(
            is_bare("@rel"),
            Some(false),
            "./-prefixed literal is a path"
        );
        assert_eq!(is_bare("@url"), Some(false), "new URL(...) is a path expr");
    }

    #[test]
    fn workspace_array_aliases_extracted_from_define_workspace() {
        let source = r#"
            import { defineWorkspace } from "vitest/config";
            export default defineWorkspace([
                { test: { alias: { vscode: "./test/mock/vscode.ts" } } },
                { resolve: { alias: { "@scope/pkg": "./__mocks__/pkg.ts" } } }
            ]);
        "#;
        let mut result = PluginResult::default();
        apply_workspace_array_aliases(
            &mut result,
            source,
            std::path::Path::new("/project/vitest.workspace.ts"),
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .path_aliases
                .contains(&("vscode".to_string(), "test/mock/vscode.ts".to_string())),
            "workspace element test.alias should be extracted: {:?}",
            result.path_aliases
        );
        assert!(
            result
                .path_aliases
                .contains(&("@scope/pkg".to_string(), "__mocks__/pkg.ts".to_string())),
            "workspace element resolve.alias should be extracted: {:?}",
            result.path_aliases
        );
    }
}
