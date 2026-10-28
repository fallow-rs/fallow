//! Alias replacement files that the resolver never reaches.
//!
//! Fallow applies a Vite, Vitest or webpack alias only as a resolver fallback,
//! so the plugins of these tools share this check.

use std::path::Path;

use super::PluginResult;

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
pub(super) fn alias_target_is_source_file(normalized: &str) -> bool {
    Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ALIAS_SOURCE_EXTENSIONS.contains(&ext))
}

/// True when an alias key is a relative or absolute file specifier, such as
/// `./clock.js`. An import with such a specifier resolves to a file on disk, so
/// the path-alias fallback of the resolver never routes it to the replacement.
fn is_path_specifier(find: &str) -> bool {
    find.starts_with("./") || find.starts_with("../") || find.starts_with('/')
}

/// Credit every export of an alias replacement file when the resolver never
/// routes an import to it. The import of the original target is then the only
/// consumer, so no import names the exports of the replacement file.
///
/// The resolver applies a plugin alias only as a fallback, which misses two
/// cases:
/// - A relative or absolute key, such as `./clock.js`, resolves to the
///   original file (issue #3963).
/// - A bare key for an installed package, such as `pkg` with `node_modules/pkg`
///   present, resolves to the real package.
///
/// A bare key for a missing package and a path-alias key (`~/lib`, `@lib`)
/// reach the replacement file through the resolver, so their exports keep
/// precise tracking. A scope-only key such as `@lib` counts as a bare key, but
/// no installed package has that name.
///
/// Returns whether the replacement got the credit, so a caller with no other
/// seed for the replacement file can also make it an entry point.
pub(super) fn credit_unrouted_replacement(
    result: &mut PluginResult,
    find: &str,
    normalized: &str,
    config_path: &Path,
) -> bool {
    if !alias_target_is_source_file(normalized) {
        return false;
    }
    let unrouted = is_path_specifier(find)
        || (is_bare_package_specifier(find) && is_installed_package(find, config_path));
    if unrouted {
        result.push_entry_export_credit(globset::escape(normalized), ["*"]);
    }
    unrouted
}

/// True when the package of `specifier` is installed in a `node_modules`
/// directory at or above the config directory. A package that only a nested
/// workspace package installs is not found, so its replacement keeps the
/// current behavior.
fn is_installed_package(specifier: &str, config_path: &Path) -> bool {
    let Some(config_dir) = config_path.parent() else {
        return false;
    };
    let pkg_name = crate::resolve::extract_package_name(specifier);
    super::eslint::find_package_dir(config_dir, &pkg_name).is_some()
}
