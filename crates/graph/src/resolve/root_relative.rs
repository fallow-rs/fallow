//! Resolve a specifier that starts with `/` against a project directory.
//!
//! Vite, Docusaurus and similar tools read such a specifier from a root
//! directory, not from the filesystem root.

use std::path::Path;

use super::project_root::is_inside_project_root;
use super::specifier::{resolve_with_extension_policy, try_html_root_relative_asset};
use super::types::{ResolveContext, ResolveResult};
use crate::read_trace::TracedPath;

pub(super) fn try_root_relative_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    if !specifier.starts_with('/') || !is_root_relative_importer(from_file) {
        return None;
    }

    let relative = format!(".{specifier}");
    let source_dir = from_file.parent().unwrap_or(ctx.root);
    let package_dir = nearest_package_dir_below_root(ctx.root, source_dir);
    let base_dirs = [
        Some(source_dir),
        package_dir,
        (source_dir != ctx.root).then_some(ctx.root),
    ];
    if let Some(result) = base_dirs
        .iter()
        .flatten()
        .find_map(|dir| resolve_root_relative_from_dir(ctx, dir, &relative, from_style))
    {
        return Some(result);
    }
    if let Some(result) = try_html_root_relative_asset(ctx, from_file, specifier) {
        return Some(result);
    }
    if is_mdx_importer(from_file)
        && let Some(result) = base_dirs
            .iter()
            .flatten()
            .find_map(|dir| resolve_root_relative_external(ctx, dir, &relative, from_style))
    {
        return Some(result);
    }
    Some(ResolveResult::Unresolvable(specifier.to_string()))
}

fn is_mdx_importer(from_file: &Path) -> bool {
    from_file
        .extension()
        .is_some_and(|extension| extension == "mdx")
}

/// Resolve a root-relative specifier to an existing project file that is not
/// in the file index. An MDX file imports content partials such as
/// `/docs/partials/_note.md`, and Fallow does not index `.md` files. A
/// relative import of the same file gives an external file too.
fn resolve_root_relative_external(
    ctx: &ResolveContext<'_>,
    base_dir: &Path,
    relative: &str,
    style_context: bool,
) -> Option<ResolveResult> {
    let resolved = resolve_with_extension_policy(ctx, base_dir, relative, style_context).ok()?;
    let path = resolved.path();
    is_inside_project_root(ctx, path).then(|| ResolveResult::ExternalFile(path.to_path_buf()))
}

/// Return the nearest directory above `source_dir` that holds a
/// `package.json`, when that directory is below the project root.
///
/// A Vite root is usually the package directory, so a root-relative
/// specifier in a nested package resolves against that package. The source
/// directory and the project root are tried separately.
fn nearest_package_dir_below_root<'p>(root: &Path, source_dir: &'p Path) -> Option<&'p Path> {
    source_dir
        .ancestors()
        .skip(1)
        .take_while(|dir| *dir != root && dir.starts_with(root))
        .find(|dir| dir.join("package.json").traced_is_file())
}

fn is_root_relative_importer(from_file: &Path) -> bool {
    from_file.extension().is_some_and(|extension| {
        matches!(
            extension.to_str(),
            Some(
                "html"
                    | "jsx"
                    | "tsx"
                    | "js"
                    | "ts"
                    | "mjs"
                    | "cjs"
                    | "mts"
                    | "cts"
                    | "gts"
                    | "gjs"
                    | "mdx"
            )
        )
    })
}

fn resolve_root_relative_from_dir(
    ctx: &ResolveContext<'_>,
    source_dir: &Path,
    relative: &str,
    style_context: bool,
) -> Option<ResolveResult> {
    let resolved = resolve_with_extension_policy(ctx, source_dir, relative, style_context).ok()?;
    let resolved_path = resolved.path();
    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    let canonical = ctx.canonicalize_cache.get(resolved_path)?;
    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Some(fallback) = ctx.canonical_fallback
        && let Some(file_id) = fallback.get(&canonical)
    {
        return Some(ResolveResult::InternalModule(file_id));
    }
    None
}
