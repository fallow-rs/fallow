//! Shared Vitest/Vite alias extraction.
//!
//! Vitest merges `test.alias` and `resolve.alias` across object and workspace configs,
//! so both plugins route aliases through this helper to keep the fixes aligned.

use std::path::Path;

use super::PluginResult;
use super::config_parser;

/// Source-file extensions an alias replacement may name. A mock alias always
/// points at a JS/TS file; directory targets (`@` -> `src`) have no extension
/// and are not seeded as entry points.
const ALIAS_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// True when `spec` is a bare npm package specifier (not a relative path, URL,
/// `data:`, or `@/` / `~/` / `#` style path alias key).
fn is_bare_package_specifier(spec: &str) -> bool {
    crate::resolve::is_bare_specifier(spec)
        && crate::resolve::is_valid_package_name(spec)
        && !crate::resolve::is_path_alias(spec)
}

/// True when a normalized alias replacement names a local source file (by
/// extension), as opposed to a directory.
fn alias_target_is_source_file(normalized: &str) -> bool {
    Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ALIAS_SOURCE_EXTENSIONS.contains(&ext))
}

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
