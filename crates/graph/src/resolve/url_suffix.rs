//! Query and fragment suffixes of import specifiers (`./a.css?inline`,
//! `pkg/a.wasm?url`, `./page.html#top`).

use std::path::Path;

use super::path_info::is_valid_package_name;
use super::types::{ResolveContext, ResolveResult};
use super::workspace_alias::resolve_failed_bare_package_specifier;

/// Return the specifier without its query string or hash fragment.
pub(super) fn strip_url_suffix(specifier: &str) -> &str {
    specifier
        .find(['?', '#'])
        .map_or(specifier, |idx| &specifier[..idx])
}

/// Vite asset query parameters that the bundler strips before it resolves
/// the path (`?url`, `?raw`, `?worker&inline`, and similar).
const VITE_ASSET_QUERY_PARAMS: &[&str] = &[
    "url",
    "raw",
    "worker",
    "sharedworker",
    "inline",
    "no-inline",
    "init",
];

/// Return the path part of a specifier whose query holds only Vite asset
/// query parameters.
pub(super) fn strip_vite_asset_query(specifier: &str) -> Option<&str> {
    let (base, query) = specifier.split_once('?')?;
    (!base.is_empty()
        && query
            .split('&')
            .all(|param| VITE_ASSET_QUERY_PARAMS.contains(&param)))
    .then_some(base)
}

/// Classify a bare package subpath with a Vite asset query that the resolver
/// could not resolve.
///
/// The workspace and alias fallbacks run first, the same as for the plain
/// subpath, so a workspace package gets the same result with or without its
/// `node_modules` link. The resolver strips the query, so an installed
/// external package means the file is really missing. When the package is not
/// installed, the import cannot be checked, so it credits the package the
/// same as the plain subpath does. An unresolved result keeps the full
/// specifier, so the report and `ignoreUnresolvedImports` match the text in
/// the source file.
pub(super) fn resolve_failed_bare_asset_query_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    base: &str,
    style_context: bool,
    matches_tsconfig_path_alias: bool,
) -> ResolveResult {
    if !is_valid_package_name(base) {
        return ResolveResult::Unresolvable(specifier.to_string());
    }
    match resolve_failed_bare_package_specifier(
        ctx,
        from_file,
        base,
        style_context,
        matches_tsconfig_path_alias,
    ) {
        ResolveResult::Unresolvable(_) => ResolveResult::Unresolvable(specifier.to_string()),
        ResolveResult::NpmPackage(package_name)
            if is_package_installed(from_file, &package_name) =>
        {
            ResolveResult::Unresolvable(specifier.to_string())
        }
        resolved => resolved,
    }
}

/// True when a `node_modules` directory in an ancestor of `from_file` holds
/// the package, which is the lookup that Node and the resolver do.
fn is_package_installed(from_file: &Path, package_name: &str) -> bool {
    from_file
        .ancestors()
        .skip(1)
        .any(|dir| dir.join("node_modules").join(package_name).is_dir())
}

#[cfg(test)]
mod tests {
    use super::{is_package_installed, strip_url_suffix, strip_vite_asset_query};

    #[test]
    fn strip_url_suffix_removes_query_string() {
        assert_eq!(strip_url_suffix("./style.css?inline"), "./style.css");
    }

    #[test]
    fn strip_url_suffix_removes_hash_fragment() {
        assert_eq!(strip_url_suffix("./page.html#section"), "./page.html");
    }

    #[test]
    fn strip_url_suffix_passes_through_plain_specifier() {
        assert_eq!(strip_url_suffix("react"), "react");
    }

    #[test]
    fn strip_vite_asset_query_accepts_known_params_only() {
        assert_eq!(strip_vite_asset_query("pkg/a.wasm?url"), Some("pkg/a.wasm"));
        assert_eq!(
            strip_vite_asset_query("pkg/w.js?worker&inline"),
            Some("pkg/w.js")
        );
        assert_eq!(strip_vite_asset_query("pkg/a.js"), None);
        assert_eq!(strip_vite_asset_query("pkg/a.js?url&custom"), None);
        assert_eq!(strip_vite_asset_query("pkg/a.js?"), None);
        assert_eq!(strip_vite_asset_query("?url"), None);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn installed_package_is_found_in_an_ancestor_node_modules() {
        let dir = tempfile::tempdir().expect("project");
        let root = dir.path();
        std::fs::create_dir_all(root.join("node_modules/@scope/pkg")).expect("package");
        std::fs::create_dir_all(root.join("apps/web/src")).expect("source");
        let importer = root.join("apps/web/src/index.ts");
        assert!(is_package_installed(&importer, "@scope/pkg"));
        assert!(!is_package_installed(&importer, "other-pkg"));
    }
}
