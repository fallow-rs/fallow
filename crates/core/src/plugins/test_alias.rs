//! Shared Vitest/Vite alias extraction.
//!
//! Vitest merges `test.alias` AND `resolve.alias` (top-level and per
//! `test.projects[*]`) when running tests, and the same config can live in
//! `vitest.config.*`, `vite.config.*`, or a `vitest.workspace.*` array file.
//! Both the Vitest and Vite plugins funnel their alias sources through
//! [`process_test_alias`] here so the three false-positive-fix mechanisms stay
//! consistent across every surface. See issue #601 and its follow-up.

use std::path::Path;

use super::PluginResult;
use super::config_parser;

/// Source-file extensions an alias replacement may name. A mock alias always
/// points at a JS/TS file; directory targets (`@` -> `src`) have no extension
/// and are not seeded as entry points.
const ALIAS_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// True when `spec` is a bare npm package specifier (not a relative path, URL,
/// `data:`, or `@/` / `~/` / `#` style path alias key).
pub(super) fn is_bare_package_specifier(spec: &str) -> bool {
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

/// Apply one alias entry (`test.alias` or `resolve.alias`) to the plugin result.
///
/// Three mechanisms cooperate so the Vitest/Vite alias false-positive classes
/// disappear without introducing new ones:
/// - (A) push the alias into `path_aliases` so a virtual-module / alias-only
///   import (`vscode` -> `./mock/vscode.js`) resolves instead of surfacing as
///   `unresolved-import` / `unlisted-dependency`.
/// - (B) when the replacement names a local source FILE, seed it as a support
///   entry point so an aliased `__mocks__` file keeps its exports credited even
///   when the original package resolves through `node_modules` (in which case
///   the production import never reaches the path-alias fallback).
/// - (C) when the alias KEY is a bare package, credit it as a referenced
///   dependency so redirecting its import through the alias (only happens when
///   `node_modules` is absent) does not regress it into a false
///   `unused-dependency`.
///
/// Package-to-package aliases (`'lodash-es' -> 'lodash'`) are special-cased:
/// both package names are credited as referenced and NO path alias is emitted
/// (pushing one would turn the source import `Unresolvable` in a
/// no-`node_modules` run). The discriminator is FILESYSTEM-FREE:
/// `replacement_is_bare_string_literal` is true only when the replacement was
/// written as a plain bare string literal (`'lodash'`), not a path expression
/// (`path.resolve(__dirname, 'src')`, `fileURLToPath(new URL('./src', ...))`) or
/// a `./`-prefixed string. So a directory alias `@` -> `path.resolve(...,'src')`
/// takes the normal path-alias branch deterministically across every
/// environment (no `is_dir()` probe, which would flip on sparse checkouts /
/// Docker layers / npm tarballs where `src/` is absent).
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

    // (A)
    result
        .path_aliases
        .push((find.to_owned(), normalized.clone()));
    // (B)
    if alias_target_is_source_file(&normalized) {
        result.setup_files.push(root.join(&normalized));
    }
    // (C)
    if find_is_pkg {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(find));
    }

    tracing::debug!(find, target = %normalized, "test alias extracted");
}

/// Extract and apply the Vitest test-block aliases that BOTH the Vitest and Vite
/// plugins share: top-level `test.alias`, `test.projects[*].test.alias`, and
/// `test.projects[*].resolve.alias`. Top-level `resolve.alias` is intentionally
/// NOT handled here; each plugin owns it (Vitest routes it through
/// `process_test_alias`; Vite keeps its existing path-alias-only extraction).
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

/// Extract and apply aliases from a `vitest.workspace.{ts,js}` array-file shape
/// (`defineWorkspace([{ test: { alias } }, { resolve: { alias } }, ...])`). Each
/// array element's top-level `test.alias` and `resolve.alias` are applied. Nested
/// `test.projects` INSIDE a workspace element is one level too deep and out of
/// scope (documented non-goal). No-op on object-default-export config files.
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

/// Emit a single `tracing::debug!` when a parsed Vitest/Vite config has neither a
/// reachable object nor array default export, so a statically-unreadable shape
/// (`mergeConfig(base, defineConfig({...}))`, an imported-and-spread base config)
/// is diagnosable under `RUST_LOG=debug` rather than a silent miss.
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
        // Bare-string-literal package replacement: `'lodash-es' -> 'lodash'`.
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
        // CI-safety contract: `@` -> path.resolve(__dirname, 'src') arrives with
        // replacement_is_bare_string_literal == false, so it takes the path-alias
        // branch DETERMINISTICALLY, even though `src/` does not exist under the
        // (nonexistent) root and `src` would pass is_bare_package_specifier. No
        // filesystem probe is consulted.
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
        // Documented residual edge: a bare-string `'@': 'src'` (no path builder)
        // is structurally indistinguishable from package-to-package, so it is
        // treated as such. Rare and non-idiomatic (Vite dir aliases use absolute
        // paths / path builders).
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
