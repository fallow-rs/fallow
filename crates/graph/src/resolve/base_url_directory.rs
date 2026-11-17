//! Bare specifiers that name a directory under the tsconfig `baseUrl`.
//!
//! With `"baseUrl": "./"`, `src/generated/client` is a project path.
//! TypeScript looks it up under `baseUrl` before `node_modules`. When the file
//! is missing (for example, a code generator did not run yet), the import is
//! an unresolved import. A package name `src` would tell the user to add a
//! dependency that does not exist.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::fallbacks::nearest_package_manifest;
use super::path_info::extract_package_name;
use super::specifier::{local_tsconfig_chain, read_tsconfig_json_cached};
use super::types::ResolveContext;
use crate::read_trace::TracedPath;

/// True when the first segment of the bare `specifier` is a directory under
/// the `baseUrl` of the nearest tsconfig, and the nearest `package.json` does
/// not declare the package name of `specifier`.
///
/// A declared package keeps its credit, because TypeScript falls back to
/// `node_modules` when the `baseUrl` lookup fails.
pub(super) fn names_undeclared_base_url_directory(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> bool {
    let Some(first_segment) = specifier.split('/').next().filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(base_dir) = nearest_base_url_dir(ctx, from_file) else {
        return false;
    };
    if !base_dir.join(first_segment).traced_is_dir() {
        return false;
    }
    !declares_package(ctx, from_file, &extract_package_name(specifier))
}

/// The `baseUrl` directory of the first tsconfig in the local chain that sets
/// one, the same lookup order that the `baseUrl` fallback uses.
fn nearest_base_url_dir(ctx: &ResolveContext<'_>, from_file: &Path) -> Option<PathBuf> {
    local_tsconfig_chain(ctx, from_file)
        .iter()
        .find_map(|tsconfig_path| {
            let json = read_tsconfig_json_cached(ctx, tsconfig_path)?;
            let base_url = json
                .get("compilerOptions")
                .and_then(|compiler_options| compiler_options.get("baseUrl"))
                .and_then(Value::as_str)?;
            let tsconfig_dir = tsconfig_path.parent().unwrap_or(ctx.root);
            Some(tsconfig_dir.join(base_url))
        })
}

fn declares_package(ctx: &ResolveContext<'_>, from_file: &Path, package_name: &str) -> bool {
    let Some(manifest) = nearest_package_manifest(ctx.package_manifests, from_file) else {
        return false;
    };
    let package_json = &manifest.package_json;
    [
        &package_json.dependencies,
        &package_json.dev_dependencies,
        &package_json.peer_dependencies,
        &package_json.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    .any(|dependencies| dependencies.contains_key(package_name))
}
