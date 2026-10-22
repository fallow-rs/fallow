//! Dependency credit for imports that resolve, or point, into `node_modules`.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::fallbacks::{extract_package_name_from_node_modules_path, is_declaration_file_name};
use super::path_info::{
    extract_package_name, is_bare_specifier, is_path_alias, is_valid_package_name,
};
use super::specifier::{
    local_tsconfig_chain, path_alias_capture, path_alias_specificity, read_tsconfig_json_cached,
    tsconfig_paths_base_dir,
};
use super::types::ResolveContext;

/// Package-usage key for an import from `from_file` that resolved into
/// `node_modules`.
///
/// A specifier that a `paths` entry of the nearest tsconfig maps into the
/// resolved package is an alias, as in TypeScript, where the mapping wins over
/// a real package of the same name. See
/// [`package_usage_name_for_resolved_package`].
pub(super) fn package_usage_name_for_import(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    resolved_path: &Path,
) -> Option<String> {
    package_usage_name_for_resolved_package(specifier, resolved_path, |resolved_package| {
        nearest_tsconfig_alias_targets(ctx, from_file, specifier)
            .iter()
            .filter_map(|target| node_modules_package_root(target))
            .any(|(name, _)| name == resolved_package)
    })
}

/// Package-usage key for an import that resolved into `node_modules`.
///
/// pnpm can install a package under a declared name (e.g. `unstorage`) symlinked
/// to a `.pnpm` store whose inner package has a different "source" name (e.g.
/// `unstorage-nightly`). For a plain bare specifier we credit the declared import
/// name so `unused-dependency` / `unlisted-dependency` accounting matches what the
/// user wrote in `package.json`, not the on-disk source package. Path aliases that
/// happen to be bare (Node.js `#imports`, `~/`, `@/`, PascalCase scopes) are
/// excluded: they can map to an external package whose real name is only
/// recoverable from the resolved path, so they keep the resolved-package name.
/// The same holds when `alias_targets_package` reports that a tsconfig `paths`
/// target lies in the resolved package. A `paths` entry onto a declaration file
/// maps types only, so the runtime import keeps the declared name. Returns `None`
/// when the path is not inside a `node_modules` directory.
pub(super) fn package_usage_name_for_resolved_package(
    specifier: &str,
    resolved_path: &Path,
    alias_targets_package: impl FnOnce(&str) -> bool,
) -> Option<String> {
    let resolved_package = extract_package_name_from_node_modules_path(resolved_path)?;
    if !is_bare_specifier(specifier) || is_path_alias(specifier) {
        return Some(resolved_package);
    }
    let is_declaration = resolved_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_declaration_file_name);
    if !is_declaration && alias_targets_package(&resolved_package) {
        return Some(resolved_package);
    }
    Some(extract_package_name(specifier))
}

/// Package-usage key for a valid bare package specifier whose resolved file
/// canonicalized outside the analyzed root before a `node_modules` segment could
/// be observed. This preserves dependency accounting for pnpm workspace symlinks
/// when users analyze a workspace package directly.
pub(super) fn package_usage_name_for_external_bare_specifier(specifier: &str) -> Option<String> {
    if !is_bare_specifier(specifier) || is_path_alias(specifier) {
        return None;
    }
    if specifier.starts_with('@') && specifier.split('/').nth(1).is_none_or(str::is_empty) {
        return None;
    }

    let package_name = extract_package_name(specifier);
    is_valid_package_name(&package_name).then_some(package_name)
}

/// Package behind an unresolved tsconfig `paths` alias whose target lies in a
/// `node_modules` package that is not installed.
///
/// The first target under `node_modules`, in the order that the resolver tries
/// them, decides. When that package directory exists, the import is a real
/// broken import and this returns `None`. An `@types` target maps types only,
/// so it gives no runtime credit.
pub(super) fn uninstalled_tsconfig_alias_target_package(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<String> {
    let (name, package_root) = nearest_tsconfig_alias_targets(ctx, from_file, specifier)
        .iter()
        .find_map(|target| node_modules_package_root(target))?;
    (!name.starts_with("@types/") && !package_root.is_dir()).then_some(name)
}

/// Expanded `paths` targets for `specifier` from the nearest tsconfig with
/// `paths`, most specific pattern first, as `try_tsconfig_paths_from_config`
/// tries them. The catch-all pattern `*` never counts as an alias.
fn nearest_tsconfig_alias_targets(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Vec<PathBuf> {
    let chain = local_tsconfig_chain(ctx, from_file);
    let Some((tsconfig_path, json)) = chain.iter().find_map(|tsconfig_path| {
        let json = read_tsconfig_json_cached(ctx, tsconfig_path)?;
        json.get("compilerOptions")?.get("paths")?;
        Some((tsconfig_path, json))
    }) else {
        return Vec::new();
    };
    let Some(compiler_options) = json.get("compilerOptions") else {
        return Vec::new();
    };
    let Some(paths) = compiler_options.get("paths").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut matches: Vec<_> = paths
        .iter()
        .filter(|(pattern, _)| pattern.as_str() != "*")
        .filter_map(|(pattern, targets)| {
            path_alias_capture(pattern, specifier).map(|capture| (pattern, capture, targets))
        })
        .collect();
    if matches.is_empty() {
        return Vec::new();
    }
    matches.sort_by_key(|(pattern, _, _)| std::cmp::Reverse(path_alias_specificity(pattern)));
    let base_dir = tsconfig_paths_base_dir(ctx, tsconfig_path, compiler_options);
    matches
        .into_iter()
        .filter_map(|(_, capture, targets)| Some((capture, targets.as_array()?)))
        .flat_map(|(capture, targets)| {
            targets
                .iter()
                .filter_map(Value::as_str)
                .map(move |target| target.replacen('*', capture, 1))
        })
        .map(|target| base_dir.join(target))
        .collect()
}

/// Package name and package directory for a path that goes through
/// `node_modules/<name>` or `node_modules/@scope/<name>`.
fn node_modules_package_root(path: &Path) -> Option<(String, PathBuf)> {
    let name = extract_package_name_from_node_modules_path(path)?;
    let components: Vec<Component<'_>> = path.components().collect();
    let node_modules = components
        .iter()
        .rposition(|component| *component == Component::Normal("node_modules".as_ref()))?;
    let end = node_modules + 1 + name.split('/').count();
    let root = components.get(..end)?.iter().collect();
    Some((name, root))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        node_modules_package_root, package_usage_name_for_external_bare_specifier,
        package_usage_name_for_resolved_package,
    };

    #[test]
    fn tsconfig_alias_credits_the_resolved_package() {
        let resolved = Path::new("/p/node_modules/@acme/theme/src/components/shared/button.ts");
        assert_eq!(
            package_usage_name_for_resolved_package("@components/shared/button", resolved, |_| {
                true
            }),
            Some("@acme/theme".to_string()),
        );
        assert_eq!(
            package_usage_name_for_resolved_package("@components/shared/button", resolved, |_| {
                false
            }),
            Some("@components/shared".to_string()),
        );
    }

    #[test]
    fn tsconfig_alias_onto_declaration_file_keeps_the_declared_name() {
        let resolved = Path::new("/p/node_modules/@types/react/index.d.ts");
        assert_eq!(
            package_usage_name_for_resolved_package("react", resolved, |_| true),
            Some("react".to_string()),
        );
    }

    #[test]
    fn node_modules_package_root_stops_at_the_package_directory() {
        assert_eq!(
            node_modules_package_root(Path::new(
                "/p/docs/.vitepress/../node_modules/@acme/theme/src/assets/icons"
            )),
            Some((
                "@acme/theme".to_string(),
                PathBuf::from("/p/docs/.vitepress/../node_modules/@acme/theme"),
            )),
        );
        assert_eq!(
            node_modules_package_root(Path::new("/p/node_modules/lodash/fp")),
            Some((
                "lodash".to_string(),
                PathBuf::from("/p/node_modules/lodash")
            )),
        );
        assert_eq!(node_modules_package_root(Path::new("/p/src/x.ts")), None);
    }

    #[test]
    fn package_usage_name_prefers_declared_name_for_bare_specifiers() {
        // pnpm source alias: declared `unstorage` resolved into a `.pnpm` store dir
        // whose inner package is `unstorage-nightly`. Credit the declared name.
        let resolved = Path::new(
            "/p/node_modules/.pnpm/unstorage-nightly@2.0.0/node_modules/unstorage-nightly",
        );
        assert_eq!(
            package_usage_name_for_resolved_package("unstorage", resolved, |_| false),
            Some("unstorage".to_string()),
        );

        // Scoped subpath bare specifier reduces to the package name.
        let resolved_scoped =
            Path::new("/p/node_modules/.pnpm/@jsr+std__csv@1.0.6/node_modules/@jsr/std__csv");
        assert_eq!(
            package_usage_name_for_resolved_package("@std/csv/stringify", resolved_scoped, |_| {
                false
            }),
            Some("@std/csv".to_string()),
        );

        // Common case (declared name == source name) is unchanged.
        let resolved_plain = Path::new("/p/node_modules/lodash");
        assert_eq!(
            package_usage_name_for_resolved_package("lodash", resolved_plain, |_| { false }),
            Some("lodash".to_string()),
        );

        // Path aliases that are bare (Node.js `#imports`) must NOT be credited as the
        // package name: they can map to an external package whose real name is only on
        // the resolved path. Keep the resolved-package name instead.
        let resolved_import_map = Path::new("/p/node_modules/polyfill-pkg/index.js");
        assert_eq!(
            package_usage_name_for_resolved_package("#polyfill", resolved_import_map, |_| false),
            Some("polyfill-pkg".to_string()),
        );

        // Not under node_modules: no package-usage credit.
        assert_eq!(
            package_usage_name_for_resolved_package("unstorage", Path::new("/p/src/x.ts"), |_| {
                false
            }),
            None,
        );
    }

    #[test]
    fn external_bare_specifier_package_usage_credits_declared_package() {
        assert_eq!(
            package_usage_name_for_external_bare_specifier("@mre/shared"),
            Some("@mre/shared".to_string()),
        );
        assert_eq!(
            package_usage_name_for_external_bare_specifier("@mre/shared/subpath"),
            Some("@mre/shared".to_string()),
        );
        assert_eq!(package_usage_name_for_external_bare_specifier("@mre"), None,);
        assert_eq!(
            package_usage_name_for_external_bare_specifier("#polyfill"),
            None,
        );
        assert_eq!(
            package_usage_name_for_external_bare_specifier("@/shared"),
            None,
        );
    }
}
