//! Resolution fallback strategies for import specifiers.
//!
//! Handles path alias fallbacks, output-to-source directory mapping, pnpm virtual
//! store detection, node_modules package extraction, and dynamic import glob patterns.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde_json::Value;

use fallow_types::discover::FileId;

use super::types::{OUTPUT_DIRS, PackageManifestInfo, ResolveContext, ResolveResult, SOURCE_EXTS};

/// Try resolving a specifier using plugin-provided path aliases.
///
/// Substitutes a matching alias prefix (e.g., `~/`) with a directory relative to the
/// project root (e.g., `app/`) and resolves the resulting path. This handles framework
/// aliases like Nuxt's `~/`, `~~/`, `#shared/` that aren't defined in tsconfig.json
/// but map to real filesystem paths.
pub(super) fn try_path_alias_fallback(
    ctx: &ResolveContext<'_>,
    specifier: &str,
) -> Option<ResolveResult> {
    for (prefix, replacement) in ctx.path_aliases {
        if !specifier.starts_with(prefix.as_str()) {
            continue;
        }

        let remainder = &specifier[prefix.len()..];
        // Build the substituted path relative to root.
        // If replacement is empty, remainder is relative to root directly.
        let substituted = if replacement.is_empty() {
            format!("./{remainder}")
        } else {
            format!("./{replacement}/{remainder}")
        };

        // Resolve relative to the project root directly. These plugin-provided
        // aliases have already been normalized to root-relative paths, so
        // tsconfig discovery is not needed here and can actually hurt for
        // solution-style roots (`tsconfig.json` with only `references`).
        if let Ok(resolved) = ctx.resolver.resolve(ctx.root, &substituted) {
            let resolved_path = resolved.path();
            // Try raw path lookup first
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return Some(ResolveResult::InternalModule(file_id));
            }
            // Fall back to canonical path lookup
            if let Ok(canonical) = dunce::canonicalize(resolved_path) {
                if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                    return Some(ResolveResult::InternalModule(file_id));
                }
                if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
                    return Some(ResolveResult::InternalModule(file_id));
                }
                if let Some(file_id) =
                    try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
                {
                    return Some(ResolveResult::InternalModule(file_id));
                }
                if let Some(pkg_name) = extract_package_name_from_node_modules_path(&canonical) {
                    return Some(ResolveResult::NpmPackage(pkg_name));
                }
                return Some(ResolveResult::ExternalFile(canonical));
            }
        }
    }
    None
}

/// Try SCSS partial resolution: `_filename` and `_index` conventions.
///
/// SCSS resolves imports in this order:
/// 1. `@use 'variables'` → `_variables.scss` (partial convention)
/// 2. `@use 'components'` → `components/_index.scss` or `components/index.scss` (directory index)
///
/// Handles both relative (`../styles/variables`) and bare (`variables`) specifiers
/// that were normalized to `./variables` during extraction.
pub(super) fn try_scss_partial_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    // SCSS built-in modules (`sass:math`) should not be retried
    if specifier.contains(':') {
        return None;
    }

    let spec_path = Path::new(specifier);
    let filename = spec_path.file_name()?.to_str()?;

    // Already has underscore prefix
    if filename.starts_with('_') {
        return None;
    }

    // 1. Try partial convention: prepend _ to the filename
    let partial_filename = format!("_{filename}");
    let partial_specifier = if let Some(parent) = spec_path.parent()
        && !parent.as_os_str().is_empty()
    {
        format!("{}/{partial_filename}", parent.display())
    } else {
        partial_filename
    };

    if let Some(result) = try_resolve_scss(ctx, from_file, &partial_specifier) {
        return Some(result);
    }

    // 2. Try directory index convention: specifier/_index and specifier/index
    let index_partial = format!("{specifier}/_index");
    if let Some(result) = try_resolve_scss(ctx, from_file, &index_partial) {
        return Some(result);
    }

    let index_plain = format!("{specifier}/index");
    try_resolve_scss(ctx, from_file, &index_plain)
}

/// Try non-partial CSS-extension resolution: `<spec>.scss`, `<spec>.sass`,
/// `<spec>.css` from the importing file's parent.
///
/// This is needed when the standard resolver's extension list contains both
/// `.vue` / `.svelte` / `.astro` AND CSS extensions. For an SFC `<style>` block
/// importing `./Foo`, the standard resolver picks `Foo.vue` (the SFC itself!)
/// before `Foo.scss` because `.vue` comes earlier in the extension list. SCSS
/// imports must restrict resolution to CSS-family extensions to avoid this
/// self-import collision. Only invoked when `from_style = true`. See issue #195.
pub(super) fn try_css_extension_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if specifier.contains(':') {
        return None;
    }
    // If the specifier already has a CSS extension, the standard resolver path
    // would have found it by name; a fallback re-entry with the same suffix is
    // a no-op.
    let spec_path = Path::new(specifier);
    let already_css_ext = spec_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            e.eq_ignore_ascii_case("css")
                || e.eq_ignore_ascii_case("scss")
                || e.eq_ignore_ascii_case("sass")
        });
    if already_css_ext {
        return try_resolve_scss(ctx, from_file, specifier);
    }
    for ext in ["scss", "sass", "css"] {
        let candidate = format!("{specifier}.{ext}");
        if let Some(result) = try_resolve_scss(ctx, from_file, &candidate) {
            return Some(result);
        }
    }
    None
}

/// Attempt to resolve a single SCSS specifier and map to an internal module.
fn try_resolve_scss(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    let resolved = ctx.resolver.resolve_file(from_file, specifier).ok()?;
    let resolved_path = resolved.path();

    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalModule(file_id));
    }
    if let Ok(canonical) = dunce::canonicalize(resolved_path)
        && let Some(&file_id) = ctx.path_to_id.get(canonical.as_path())
    {
        return Some(ResolveResult::InternalModule(file_id));
    }
    None
}

/// Try SCSS `includePaths` fallback: resolve the specifier against each
/// framework-contributed include directory.
///
/// Angular's `stylePreprocessorOptions.includePaths` (and Nx's equivalent via
/// project.json) adds extra search paths that SCSS resolves against before
/// falling back to node_modules. Bare `@use 'variables'` statements that were
/// normalized to `./variables` at extraction time fail the usual file-local
/// resolution, so when the importing file is `.scss`/`.sass` and the spec
/// originated from such a bare specifier, we retry against each include path,
/// applying the SCSS partial (`_variables`) and directory-index conventions.
/// SFC `<style lang="scss">` imports pass `from_style = true` because their
/// filesystem importer is `.vue` / `.svelte`, not `.scss` / `.sass`.
///
/// The specifier arrives with a `./` prefix because `normalize_css_import_path`
/// rewrites bare extensionless SCSS specifiers to relative ones. We strip that
/// prefix here to re-enter the include-path search from the root of each
/// directory. Relative specifiers that already escape the importing file
/// (e.g. `../shared/variables`) are left untouched — include paths only
/// disambiguate bare specifiers, not explicit relative paths.
pub(super) fn try_scss_include_path_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    if ctx.scss_include_paths.is_empty() {
        return None;
    }
    let is_scss_importer = from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass");
    if !is_scss_importer && !from_style {
        return None;
    }
    // SCSS built-in modules (`sass:math`) should not be retried
    if specifier.contains(':') {
        return None;
    }
    // Only bare (normalized) specifiers benefit from include-path search.
    // Parent-relative specifiers like `../shared/vars` explicitly escape the
    // importing file's directory and should not be silently redirected.
    let bare = specifier.strip_prefix("./")?;
    if bare.starts_with("..") || bare.starts_with('/') {
        return None;
    }

    for include_dir in ctx.scss_include_paths {
        if let Some(file_id) = find_scss_in_dir(include_dir, bare, ctx) {
            return Some(ResolveResult::InternalModule(file_id));
        }
    }
    None
}

/// Probe an SCSS include directory for a bare specifier, applying the standard
/// SCSS resolution order: exact file, `_`-prefixed partial, `_index` / `index`
/// directory conventions. Supports `.scss` and `.sass` extensions.
fn find_scss_in_dir(include_dir: &Path, bare: &str, ctx: &ResolveContext<'_>) -> Option<FileId> {
    let bare_path = Path::new(bare);
    let has_scss_ext = matches!(
        bare_path.extension().and_then(|e| e.to_str()),
        Some(ext) if ext.eq_ignore_ascii_case("scss") || ext.eq_ignore_ascii_case("sass")
    );

    // Split bare spec so we can build the `_`-prefixed partial for the final
    // component while preserving any leading directory segments.
    let parent = bare_path.parent();
    let stem_with_ext = bare_path.file_name()?.to_str()?;
    let stem_without_ext = bare_path.file_stem().and_then(|s| s.to_str())?;

    let build = |rel: &Path| -> std::path::PathBuf { include_dir.join(rel) };
    let join_with_parent = |name: &str| -> std::path::PathBuf {
        parent.map_or_else(|| build(Path::new(name)), |p| build(&p.join(name)))
    };

    let exts: &[&str] = if has_scss_ext {
        &[""]
    } else {
        &["scss", "sass"]
    };

    for ext in exts {
        let suffix = if ext.is_empty() {
            String::new()
        } else {
            format!(".{ext}")
        };
        // 1. Direct file: include_dir/<bare><ext>
        let direct = if ext.is_empty() {
            build(bare_path)
        } else {
            join_with_parent(&format!("{stem_with_ext}{suffix}"))
        };
        if let Some(fid) = lookup_scss_path(&direct, ctx) {
            return Some(fid);
        }
        // 2. Partial: include_dir/<parent>/_<stem><ext>
        let partial_name = if ext.is_empty() {
            format!("_{stem_with_ext}")
        } else {
            format!("_{stem_without_ext}{suffix}")
        };
        let partial = join_with_parent(&partial_name);
        if let Some(fid) = lookup_scss_path(&partial, ctx) {
            return Some(fid);
        }
        if ext.is_empty() {
            // Already has extension; directory index candidates below don't apply.
            continue;
        }
        // 3. Directory index: include_dir/<bare>/_index.<ext>
        let idx_partial = build(bare_path).join(format!("_index{suffix}"));
        if let Some(fid) = lookup_scss_path(&idx_partial, ctx) {
            return Some(fid);
        }
        let idx_plain = build(bare_path).join(format!("index{suffix}"));
        if let Some(fid) = lookup_scss_path(&idx_plain, ctx) {
            return Some(fid);
        }
    }
    None
}

/// Look up an absolute candidate path in the file index, falling back to
/// canonical path lookup for intra-project symlinks.
fn lookup_scss_path(candidate: &Path, ctx: &ResolveContext<'_>) -> Option<FileId> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(candidate) {
        return Some(file_id);
    }
    if let Ok(canonical) = dunce::canonicalize(candidate) {
        if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
            return Some(file_id);
        }
        if let Some(fallback) = ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            return Some(file_id);
        }
    }
    None
}

/// Try SCSS `node_modules` fallback: resolve a bare specifier by walking up
/// from the importing file and probing each ancestor's `node_modules/` dir.
///
/// Sass's `@import` / `@use` resolution algorithm searches `node_modules/` for
/// bare specifiers after the file-local and `includePaths` searches fail.
/// `@import 'bootstrap/scss/functions'` resolves to
/// `node_modules/bootstrap/scss/_functions.scss` via the standard partial
/// convention; `@import 'animate.css/animate.min'` resolves to
/// `node_modules/animate.css/animate.min.css` via the CSS-extension fallback.
///
/// Files inside `node_modules/` are not in fallow's file index (the default
/// ignore patterns exclude them), so this function returns
/// `ResolveResult::NpmPackage` when a candidate exists on disk. That ensures
/// (1) the `@import` is not reported as unresolved and (2) the npm package is
/// marked as a used dependency so `unused-dependencies` / `unlisted-dependencies`
/// stay accurate.
///
/// The specifier arrives with a `./` prefix because `normalize_css_import_path`
/// rewrites bare extensionless SCSS specifiers to relative ones. Parent-relative
/// specifiers are skipped — they explicitly escape the importing file and must
/// not be silently redirected to `node_modules`. See issue #125.
pub(super) fn try_scss_node_modules_fallback(
    _ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> Option<ResolveResult> {
    // SCSS built-in modules (`sass:math`) should not be retried
    if specifier.contains(':') {
        return None;
    }
    let is_scss_importer = from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass");
    if !is_scss_importer && !from_style {
        return None;
    }
    // Only bare (normalized) specifiers should search node_modules. Explicit
    // parent-relative paths (`../shared/vars`) are intentional and must not be
    // redirected.
    let bare = specifier.strip_prefix("./")?;
    if bare.starts_with("..") || bare.starts_with('/') {
        return None;
    }
    // The first segment of a bare specifier is the package name (or the start
    // of a scoped package name). Require it before probing node_modules to
    // avoid spurious syscalls on malformed specifiers.
    if bare.is_empty() {
        return None;
    }

    // Walk up from the importing file's parent directory to the filesystem
    // root, matching Node.js / Sass `node_modules` resolution. Covers all
    // common layouts: flat single project, non-hoisted monorepo, and hoisted
    // monorepo where `node_modules` lives above the fallow project root
    // (e.g., fallow run on `/monorepo/packages/my-lib` needs to reach
    // `/monorepo/node_modules`). The walk is bounded by `Path::parent()`
    // returning `None` at the filesystem root.
    let mut dir = from_file.parent()?;
    loop {
        let nm_dir = dir.join("node_modules");
        if nm_dir.is_dir()
            && let Some(path) = find_scss_in_node_modules(&nm_dir, bare)
            && let Some(pkg_name) = extract_package_name_from_node_modules_path(&path)
        {
            return Some(ResolveResult::NpmPackage(pkg_name));
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent;
    }
    None
}

/// Probe candidate filesystem paths for a bare SCSS specifier inside a single
/// `node_modules/` directory, applying Sass resolution conventions.
///
/// Candidate order:
/// 1. `<bare>.scss` / `<bare>.sass` / `<bare>.css` (extension append)
/// 2. `<parent>/_<stem>.scss` / `<parent>/_<stem>.sass` (partial convention)
/// 3. `<bare>/_index.scss` / `<bare>/index.scss` (and `.sass` variants)
/// 4. `<bare>` (exact, for specifiers that already carry an extension)
fn find_scss_in_node_modules(nm_dir: &Path, bare: &str) -> Option<PathBuf> {
    let bare_path = Path::new(bare);
    let file_name = bare_path.file_name()?.to_str()?;
    let parent = bare_path.parent();
    let join_with_parent = |name: &str| -> PathBuf {
        parent.map_or_else(|| nm_dir.join(name), |p| nm_dir.join(p).join(name))
    };

    // 1. Append extension. Covers both SCSS partials (with ext .scss/.sass
    // added via the separate partial probe below) and CSS files where Sass
    // appends `.css` to an extensionless specifier like `animate.css/animate.min`.
    for ext in &["scss", "sass", "css"] {
        let candidate = join_with_parent(&format!("{file_name}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // 2. SCSS partial: prepend underscore to the file name component only.
    // Skip `.css` here — CSS has no partial convention.
    for ext in &["scss", "sass"] {
        let candidate = join_with_parent(&format!("_{file_name}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    // 3. Directory index: `<bare>/_index.<ext>` or `<bare>/index.<ext>`.
    for ext in &["scss", "sass"] {
        let idx_partial = nm_dir.join(bare).join(format!("_index.{ext}"));
        if idx_partial.is_file() {
            return Some(idx_partial);
        }
        let idx_plain = nm_dir.join(bare).join(format!("index.{ext}"));
        if idx_plain.is_file() {
            return Some(idx_plain);
        }
    }
    // 4. Exact file — covers specifiers that already carry an extension
    // (e.g., `bootstrap/dist/css/bootstrap.min.css`).
    let exact = nm_dir.join(bare);
    if exact.is_file() {
        return Some(exact);
    }
    None
}

/// Try to map a resolved output path (e.g., `packages/ui/dist/utils.js`) back to
/// the corresponding source file (e.g., `packages/ui/src/utils.ts`).
///
/// This handles cross-workspace imports that go through `exports` maps pointing to
/// built output directories. Since fallow ignores `dist/`, `build/`, etc. by default,
/// the resolved path won't be in the file set, but the source file will be.
///
/// Nested output subdirectories (e.g., `dist/esm/utils.mjs`, `build/cjs/index.cjs`)
/// are handled by finding the last output directory component (closest to the file,
/// avoiding false matches on parent directories) and then walking backwards to collect
/// all consecutive output directory components before it.
pub(super) fn try_source_fallback(
    resolved: &Path,
    path_to_id: &FxHashMap<&Path, FileId>,
) -> Option<FileId> {
    let components: Vec<_> = resolved.components().collect();

    let is_output_dir = |c: &std::path::Component| -> bool {
        if let std::path::Component::Normal(s) = c
            && let Some(name) = s.to_str()
        {
            return OUTPUT_DIRS.contains(&name);
        }
        false
    };

    // Find the LAST output directory component (closest to the file).
    // Using rposition avoids false matches on parent directories that happen to
    // be named "build", "dist", etc.
    let last_output_pos = components.iter().rposition(&is_output_dir)?;

    // Walk backwards to find the start of consecutive output directory components.
    // e.g., for `dist/esm/utils.mjs`, rposition finds `esm`, then we walk back to `dist`.
    let mut first_output_pos = last_output_pos;
    while first_output_pos > 0 && is_output_dir(&components[first_output_pos - 1]) {
        first_output_pos -= 1;
    }

    // Build the path prefix (everything before the first consecutive output dir)
    let prefix: PathBuf = components[..first_output_pos].iter().collect();

    // Build the relative path after the last consecutive output dir
    let suffix: PathBuf = components[last_output_pos + 1..].iter().collect();
    suffix.file_stem()?; // Ensure the suffix has a filename

    // Try replacing the output dirs with "src" and each source extension
    for ext in SOURCE_EXTS {
        let source_candidate = prefix.join("src").join(suffix.with_extension(ext));
        if let Some(&file_id) = path_to_id.get(source_candidate.as_path()) {
            return Some(file_id);
        }
    }

    None
}

/// Try to resolve a package `imports` entry from the nearest owning package.
///
/// `#...` specifiers are package-local by definition, so this fallback is only
/// allowed when the importing file's nearest package manifest has a matching
/// `imports` key. That keeps unrelated hash-prefixed path aliases unresolved.
pub(super) fn try_package_imports_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with('#') {
        return None;
    }
    let manifest = nearest_package_manifest(ctx.package_manifests, from_file)?;
    let imports = manifest.package_json.imports.as_ref()?;
    let PackageMapTarget::Target(target) =
        package_map_target(imports, specifier, ctx.condition_names)
    else {
        return None;
    };
    let source_subpath = package_import_source_subpath(manifest, specifier);
    resolve_package_map_target(ctx, manifest, &target, source_subpath.as_deref()).map(|file_id| {
        match &manifest.name {
            Some(package_name) => ResolveResult::InternalPackageModule {
                file_id,
                package_name: package_name.clone(),
            },
            None => ResolveResult::InternalModule(file_id),
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageMapTarget {
    NoMatch,
    Blocked,
    Target(String),
}

fn package_map_match_value(
    value: &Value,
    condition_names: &[String],
    capture: Option<&str>,
) -> PackageMapTarget {
    resolve_package_map_value(value, condition_names, capture)
        .map_or(PackageMapTarget::Blocked, PackageMapTarget::Target)
}

fn package_map_target(
    map: &Value,
    specifier_key: &str,
    condition_names: &[String],
) -> PackageMapTarget {
    let Some(obj) = map.as_object() else {
        if specifier_key == "." {
            return package_map_match_value(map, condition_names, None);
        }
        return PackageMapTarget::NoMatch;
    };

    let has_subpath_keys = obj
        .keys()
        .any(|key| key == "." || key.starts_with("./") || key.starts_with('#'));
    if !has_subpath_keys {
        if specifier_key == "." {
            return package_map_match_value(map, condition_names, None);
        }
        return PackageMapTarget::NoMatch;
    }

    if let Some(value) = obj.get(specifier_key) {
        return package_map_match_value(value, condition_names, None);
    }

    let mut patterns: Vec<(&str, &Value, String)> = obj
        .iter()
        .filter_map(|(pattern, value)| {
            package_map_pattern_capture(pattern, specifier_key)
                .map(|capture| (pattern.as_str(), value, capture))
        })
        .collect();
    patterns.sort_by(|(left, _, _), (right, _, _)| {
        package_map_pattern_specificity(right).cmp(&package_map_pattern_specificity(left))
    });

    patterns
        .first()
        .map_or(PackageMapTarget::NoMatch, |(_, value, capture)| {
            package_map_match_value(value, condition_names, Some(capture))
        })
}

fn resolve_package_map_value(
    value: &Value,
    condition_names: &[String],
    capture: Option<&str>,
) -> Option<String> {
    match value {
        Value::String(target) => Some(match capture {
            Some(capture) => target.replace('*', capture),
            None => target.clone(),
        }),
        Value::Object(map) => {
            for (condition, value) in map {
                if (condition == "default"
                    || condition_names
                        .iter()
                        .any(|active_condition| active_condition == condition))
                    && let Some(target) = resolve_package_map_value(value, condition_names, capture)
                {
                    return Some(target);
                }
            }
            None
        }
        Value::Array(_) | Value::Bool(_) | Value::Null | Value::Number(_) => None,
    }
}

fn package_map_pattern_capture(pattern: &str, specifier: &str) -> Option<String> {
    let star = pattern.find('*')?;
    if pattern[star + 1..].contains('*') {
        return None;
    }
    let (prefix, suffix_with_star) = pattern.split_at(star);
    let suffix = &suffix_with_star[1..];
    let captured = specifier.strip_prefix(prefix)?.strip_suffix(suffix)?;
    Some(captured.to_string())
}

fn package_map_pattern_specificity(pattern: &str) -> (usize, usize) {
    let star = pattern.find('*').unwrap_or(pattern.len());
    (star, pattern.len())
}

fn package_import_source_subpath(
    manifest: &PackageManifestInfo,
    specifier: &str,
) -> Option<PathBuf> {
    let stripped = specifier.strip_prefix('#')?;
    let without_package_name = manifest
        .name
        .as_deref()
        .and_then(|name| stripped.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(stripped);
    if without_package_name.is_empty() {
        None
    } else {
        Some(PathBuf::from(without_package_name))
    }
}

fn nearest_package_manifest<'a>(
    manifests: &'a [PackageManifestInfo],
    from_file: &Path,
) -> Option<&'a PackageManifestInfo> {
    manifests
        .iter()
        .filter(|manifest| {
            from_file.starts_with(&manifest.root) || from_file.starts_with(&manifest.canonical_root)
        })
        .max_by_key(|manifest| manifest.root.components().count())
}

fn find_package_manifest<'a>(
    manifests: &'a [PackageManifestInfo],
    package_name: &str,
) -> Option<&'a PackageManifestInfo> {
    manifests
        .iter()
        .find(|manifest| manifest.name.as_deref() == Some(package_name))
}

fn resolve_package_map_target(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    target: &str,
    source_subpath: Option<&Path>,
) -> Option<FileId> {
    let target = target.strip_prefix("./")?;
    if target.starts_with("../") || target.starts_with('/') {
        return None;
    }
    let target_path = manifest.root.join(target);

    lookup_internal_file_id(ctx, &target_path)
        .or_else(|| try_source_fallback(&target_path, ctx.raw_path_to_id))
        .or_else(|| try_source_fallback(&target_path, ctx.path_to_id))
        .or_else(|| source_subpath.and_then(|subpath| try_source_subpath(ctx, manifest, subpath)))
}

fn try_source_subpath(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    subpath: &Path,
) -> Option<FileId> {
    if subpath.as_os_str().is_empty()
        && let Some(source) = manifest.package_json.source.as_deref()
        && let Some(source) = source.strip_prefix("./")
        && let Some(file_id) = lookup_internal_file_id(ctx, &manifest.root.join(source))
    {
        return Some(file_id);
    }

    for ext in SOURCE_EXTS {
        let direct = if subpath.as_os_str().is_empty() {
            manifest.root.join("src").join(format!("index.{ext}"))
        } else {
            manifest.root.join("src").join(subpath).with_extension(ext)
        };
        if let Some(file_id) = lookup_internal_file_id(ctx, &direct) {
            return Some(file_id);
        }

        if !subpath.as_os_str().is_empty() {
            let index = manifest
                .root
                .join("src")
                .join(subpath)
                .join(format!("index.{ext}"));
            if let Some(file_id) = lookup_internal_file_id(ctx, &index) {
                return Some(file_id);
            }
        }

        if subpath.as_os_str().is_empty() {
            let root_index = manifest.root.join(format!("index.{ext}"));
            if let Some(file_id) = lookup_internal_file_id(ctx, &root_index) {
                return Some(file_id);
            }
        }
    }

    None
}

fn lookup_internal_file_id(ctx: &ResolveContext<'_>, candidate: &Path) -> Option<FileId> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(candidate) {
        return Some(file_id);
    }
    if let Some(&file_id) = ctx.path_to_id.get(candidate) {
        return Some(file_id);
    }
    if let Ok(canonical) = dunce::canonicalize(candidate) {
        if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
            return Some(file_id);
        }
        if let Some(fallback) = ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            return Some(file_id);
        }
    }
    None
}

/// Extract npm package name from a resolved path inside `node_modules`.
///
/// Given a path like `/project/node_modules/react/index.js`, returns `Some("react")`.
/// Given a path like `/project/node_modules/@scope/pkg/dist/index.js`, returns `Some("@scope/pkg")`.
/// Returns `None` if the path doesn't contain a `node_modules` segment.
pub fn extract_package_name_from_node_modules_path(path: &Path) -> Option<String> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    // Find the last "node_modules" component (handles nested node_modules)
    let nm_idx = components.iter().rposition(|&c| c == "node_modules")?;

    let after = &components[nm_idx + 1..];
    if after.is_empty() {
        return None;
    }

    if after[0].starts_with('@') {
        // Scoped package: @scope/pkg
        if after.len() >= 2 {
            Some(format!("{}/{}", after[0], after[1]))
        } else {
            Some(after[0].to_string())
        }
    } else {
        Some(after[0].to_string())
    }
}

/// Try to map a pnpm virtual store path back to a workspace source file.
///
/// When pnpm uses injected dependencies or certain linking strategies, canonical
/// paths go through `.pnpm`:
///   `/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/index.js`
///
/// This function detects such paths, extracts the package name, checks if it
/// matches a workspace package, and tries to find the source file in that workspace.
pub(super) fn try_pnpm_workspace_fallback(
    path: &Path,
    path_to_id: &FxHashMap<&Path, FileId>,
    workspace_roots: &FxHashMap<&str, &Path>,
) -> Option<FileId> {
    // Only relevant for paths containing .pnpm
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    // Find .pnpm component
    let pnpm_idx = components.iter().position(|&c| c == ".pnpm")?;

    // After .pnpm, find the inner node_modules (the actual package location)
    // Structure: .pnpm/<name>@<version>/node_modules/<package>/...
    let after_pnpm = &components[pnpm_idx + 1..];

    // Find "node_modules" inside the .pnpm directory
    let inner_nm_idx = after_pnpm.iter().position(|&c| c == "node_modules")?;
    let after_inner_nm = &after_pnpm[inner_nm_idx + 1..];

    if after_inner_nm.is_empty() {
        return None;
    }

    // Extract package name (handle scoped packages)
    let (pkg_name, pkg_name_components) = if after_inner_nm[0].starts_with('@') {
        if after_inner_nm.len() >= 2 {
            (format!("{}/{}", after_inner_nm[0], after_inner_nm[1]), 2)
        } else {
            return None;
        }
    } else {
        (after_inner_nm[0].to_string(), 1)
    };

    // Check if this package is a workspace package
    let ws_root = workspace_roots.get(pkg_name.as_str())?;

    // Get the relative path within the package (after the package name components)
    let relative_parts = &after_inner_nm[pkg_name_components..];
    if relative_parts.is_empty() {
        return None;
    }

    let relative_path: PathBuf = relative_parts.iter().collect();

    // Try direct file lookup in workspace root
    let direct = ws_root.join(&relative_path);
    if let Some(&file_id) = path_to_id.get(direct.as_path()) {
        return Some(file_id);
    }

    // Try source fallback (dist/ → src/ etc.) within the workspace
    try_source_fallback(&direct, path_to_id)
}

/// Try to resolve a bare specifier as a workspace package reference.
///
/// When the specifier's package name matches a workspace package, resolve the
/// subpath against that package's root directory directly instead of going
/// through `node_modules`. Covers two cases:
///
/// 1. **Self-referencing package imports**: Node.js v12+ lets a package import
///    itself via its own name (`import { X } from '@org/pkg/subentry'` from
///    inside `@org/pkg`). Angular libraries built with `ng-packagr` rely on
///    this to declare secondary entry points.
/// 2. **Cross-workspace imports without `node_modules` symlinks**: monorepos
///    that have not been installed yet, or bundlers that bypass `node_modules`
///    entirely, still need to resolve `@org/other-pkg/sub` to the sibling
///    workspace's source file.
///
/// Strategy: prefer a matching package `exports` target when the manifest has
/// one, then strip the package name prefix and resolve the remainder as a
/// relative path from inside the package root. The manifest branch covers
/// source-only workspaces whose `exports` point at missing `dist` output.
///
/// See issues #106 and #641.
pub(super) fn try_workspace_package_fallback(
    ctx: &ResolveContext<'_>,
    specifier: &str,
) -> Option<ResolveResult> {
    // Must look like a bare package specifier to avoid matching `./button`, etc.
    if !super::path_info::is_bare_specifier(specifier) {
        return None;
    }
    let pkg_name = super::path_info::extract_package_name(specifier);

    // Remainder after the package name. Empty for `@org/pkg`, `"button"` for
    // `@org/pkg/button`, `"internal/base"` for `@org/pkg/internal/base`.
    let subpath = specifier
        .strip_prefix(pkg_name.as_str())
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or("");
    let source_subpath = PathBuf::from(subpath);

    if let Some(manifest) = find_package_manifest(ctx.package_manifests, pkg_name.as_str()) {
        let export_key = if subpath.is_empty() {
            ".".to_string()
        } else {
            format!("./{subpath}")
        };
        if let Some(exports) = manifest.package_json.exports.as_ref() {
            match package_map_target(exports, &export_key, ctx.condition_names) {
                PackageMapTarget::Target(target) => {
                    if let Some(file_id) = resolve_package_map_target(
                        ctx,
                        manifest,
                        &target,
                        Some(source_subpath.as_path()),
                    ) {
                        return Some(ResolveResult::InternalPackageModule {
                            file_id,
                            package_name: pkg_name,
                        });
                    }
                }
                PackageMapTarget::NoMatch | PackageMapTarget::Blocked => return None,
            }
        }
    }

    let (ws_root, package_name) =
        if let Some(manifest) = find_package_manifest(ctx.package_manifests, pkg_name.as_str()) {
            (manifest.root.as_path(), pkg_name)
        } else {
            (*ctx.workspace_roots.get(pkg_name.as_str())?, pkg_name)
        };

    // Synthetic importer inside the workspace root so tsconfig discovery walks
    // up from the correct directory and relative specifiers anchor there.
    let root_file = ws_root.join("__fallow_ws_self_resolve__");
    let rel_spec = if subpath.is_empty() {
        "./".to_string()
    } else {
        format!("./{subpath}")
    };

    let resolved = ctx.resolver.resolve_file(&root_file, &rel_spec).ok()?;
    let resolved_path = resolved.path();

    if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
        return Some(ResolveResult::InternalPackageModule {
            file_id,
            package_name,
        });
    }
    if let Ok(canonical) = dunce::canonicalize(resolved_path) {
        if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
            return Some(ResolveResult::InternalPackageModule {
                file_id,
                package_name,
            });
        }
        if let Some(fallback) = ctx.canonical_fallback
            && let Some(file_id) = fallback.get(&canonical)
        {
            return Some(ResolveResult::InternalPackageModule {
                file_id,
                package_name,
            });
        }
        if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
            return Some(ResolveResult::InternalPackageModule {
                file_id,
                package_name,
            });
        }
    }
    None
}

/// Convert a `DynamicImportPattern` to a glob string for file matching.
pub(super) fn make_glob_from_pattern(
    pattern: &fallow_types::extract::DynamicImportPattern,
) -> String {
    // If the prefix already contains glob characters (from import.meta.glob), use as-is
    if pattern.prefix.contains('*') || pattern.prefix.contains('{') {
        return pattern.prefix.clone();
    }
    pattern.suffix.as_ref().map_or_else(
        || format!("{}*", pattern.prefix),
        |suffix| format!("{}*{}", pattern.prefix, suffix),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    #[test]
    fn test_extract_package_name_from_node_modules_path_regular() {
        let path = PathBuf::from("/project/node_modules/react/index.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("react".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_scoped() {
        let path = PathBuf::from("/project/node_modules/@babel/core/lib/index.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("@babel/core".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_nested() {
        // Nested node_modules: should use the last (innermost) one
        let path = PathBuf::from("/project/node_modules/pkg-a/node_modules/pkg-b/dist/index.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("pkg-b".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_deep_subpath() {
        let path = PathBuf::from("/project/node_modules/react-dom/cjs/react-dom.production.min.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("react-dom".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_no_node_modules() {
        let path = PathBuf::from("/project/src/components/Button.tsx");
        assert_eq!(extract_package_name_from_node_modules_path(&path), None);
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_just_node_modules() {
        let path = PathBuf::from("/project/node_modules");
        assert_eq!(extract_package_name_from_node_modules_path(&path), None);
    }

    #[test]
    fn test_extract_package_name_from_node_modules_path_scoped_only_scope() {
        // Edge case: path ends at scope without package name
        let path = PathBuf::from("/project/node_modules/@scope");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("@scope".to_string())
        );
    }

    #[test]
    fn test_resolve_specifier_node_modules_returns_npm_package() {
        // When oxc_resolver resolves to a node_modules path that is NOT in path_to_id,
        // it should return NpmPackage instead of ExternalFile.
        // We can't easily test resolve_specifier directly without a real resolver,
        // but the extract_package_name_from_node_modules_path function covers the
        // core logic that was missing.
        let path =
            PathBuf::from("/project/node_modules/styled-components/dist/styled-components.esm.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("styled-components".to_string())
        );

        let path = PathBuf::from("/project/node_modules/next/dist/server/next.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("next".to_string())
        );
    }

    #[test]
    fn test_try_source_fallback_dist_to_src() {
        let src_path = PathBuf::from("/project/packages/ui/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        let dist_path = PathBuf::from("/project/packages/ui/dist/utils.js");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(0)),
            "dist/utils.js should fall back to src/utils.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_build_to_src() {
        let src_path = PathBuf::from("/project/packages/core/src/index.tsx");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(1));

        let build_path = PathBuf::from("/project/packages/core/build/index.js");
        assert_eq!(
            try_source_fallback(&build_path, &path_to_id),
            Some(FileId(1)),
            "build/index.js should fall back to src/index.tsx"
        );
    }

    #[test]
    fn test_try_source_fallback_no_match() {
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();

        let dist_path = PathBuf::from("/project/packages/ui/dist/utils.js");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            None,
            "should return None when no source file exists"
        );
    }

    #[test]
    fn test_try_source_fallback_non_output_dir() {
        let src_path = PathBuf::from("/project/packages/ui/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        // A path that's not in an output directory should not trigger fallback
        let normal_path = PathBuf::from("/project/packages/ui/scripts/utils.js");
        assert_eq!(
            try_source_fallback(&normal_path, &path_to_id),
            None,
            "non-output directory path should not trigger fallback"
        );
    }

    #[test]
    fn test_try_source_fallback_nested_path() {
        let src_path = PathBuf::from("/project/packages/ui/src/components/Button.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(2));

        let dist_path = PathBuf::from("/project/packages/ui/dist/components/Button.js");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(2)),
            "nested dist path should fall back to nested src path"
        );
    }

    #[test]
    fn test_try_source_fallback_nested_dist_esm() {
        let src_path = PathBuf::from("/project/packages/ui/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        let dist_path = PathBuf::from("/project/packages/ui/dist/esm/utils.mjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(0)),
            "dist/esm/utils.mjs should fall back to src/utils.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_nested_build_cjs() {
        let src_path = PathBuf::from("/project/packages/core/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(1));

        let build_path = PathBuf::from("/project/packages/core/build/cjs/index.cjs");
        assert_eq!(
            try_source_fallback(&build_path, &path_to_id),
            Some(FileId(1)),
            "build/cjs/index.cjs should fall back to src/index.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_nested_dist_esm_deep_path() {
        let src_path = PathBuf::from("/project/packages/ui/src/components/Button.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(2));

        let dist_path = PathBuf::from("/project/packages/ui/dist/esm/components/Button.mjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(2)),
            "dist/esm/components/Button.mjs should fall back to src/components/Button.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_triple_nested_output_dirs() {
        let src_path = PathBuf::from("/project/packages/ui/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        let dist_path = PathBuf::from("/project/packages/ui/out/dist/esm/utils.mjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(0)),
            "out/dist/esm/utils.mjs should fall back to src/utils.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_parent_dir_named_build() {
        let src_path = PathBuf::from("/home/user/build/my-project/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        let dist_path = PathBuf::from("/home/user/build/my-project/dist/utils.js");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(0)),
            "should resolve dist/ within project, not match parent 'build' dir"
        );
    }

    #[test]
    fn package_map_exact_entry_beats_pattern_entry() {
        let map = serde_json::json!({
            "#nitro/runtime/task": "./dist/special/task.mjs",
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Target("./dist/special/task.mjs".to_string())
        );
    }

    #[test]
    fn package_map_wildcard_substitutes_capture() {
        let map = serde_json::json!({
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Target("./dist/runtime/internal/task.mjs".to_string())
        );
    }

    #[test]
    fn package_map_exact_entry_with_no_target_blocks_pattern_entry() {
        let map = serde_json::json!({
            "#nitro/runtime/task": null,
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    fn package_map_best_pattern_with_no_target_blocks_broader_pattern() {
        let map = serde_json::json!({
            "#nitro/runtime/internal/*": null,
            "#nitro/runtime/*": "./dist/runtime/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/internal/task", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    fn package_map_unmatched_subpath_is_not_a_target() {
        let map = serde_json::json!({
            "./query": "./dist/query/index.js"
        });
        assert_eq!(
            package_map_target(&map, "./private", &conditions()),
            PackageMapTarget::NoMatch
        );
    }

    #[test]
    fn package_map_nested_conditions_follow_manifest_order() {
        let map = serde_json::json!({
            "./query/react": {
                "types": "./dist/query/react/index.d.ts",
                "import": {
                    "development": "./src/query/react/index.ts",
                    "default": "./dist/query/react/index.js"
                },
                "default": "./dist/query/react/index.cjs"
            }
        });
        assert_eq!(
            package_map_target(&map, "./query/react", &conditions()),
            PackageMapTarget::Target("./dist/query/react/index.d.ts".to_string())
        );
    }

    #[test]
    fn package_map_import_before_types_selects_runtime_branch() {
        let map = serde_json::json!({
            ".": {
                "import": "./dist/index.js",
                "types": "./dist/index.d.ts"
            }
        });
        assert_eq!(
            package_map_target(&map, ".", &conditions()),
            PackageMapTarget::Target("./dist/index.js".to_string())
        );
    }

    #[test]
    fn package_map_condition_order_follows_manifest_order() {
        let map = serde_json::json!({
            ".": {
                "node": "./dist/node.js",
                "import": "./dist/index.js"
            }
        });
        assert_eq!(
            package_map_target(&map, ".", &conditions()),
            PackageMapTarget::Target("./dist/node.js".to_string())
        );
    }

    #[test]
    fn package_map_unsupported_shapes_are_skipped() {
        let map = serde_json::json!({
            "#array": ["./dist/array.js"],
            "#null": null,
            "#false": false
        });
        assert_eq!(
            package_map_target(&map, "#array", &conditions()),
            PackageMapTarget::Blocked
        );
        assert_eq!(
            package_map_target(&map, "#null", &conditions()),
            PackageMapTarget::Blocked
        );
        assert_eq!(
            package_map_target(&map, "#false", &conditions()),
            PackageMapTarget::Blocked
        );
    }

    #[test]
    fn package_map_non_relative_target_does_not_trigger_source_fallback() {
        let root = PathBuf::from("/project");
        let manifest = PackageManifestInfo {
            root: root.clone(),
            canonical_root: root,
            name: Some("pkg".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let raw_path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();
        let condition_names = conditions();
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions::default());
        let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
        let ctx = ResolveContext {
            resolver: &resolver,
            style_resolver: &resolver,
            extensions: &[],
            path_to_id: &path_to_id,
            raw_path_to_id: &raw_path_to_id,
            workspace_roots: &workspace_roots,
            package_manifests: std::slice::from_ref(&manifest),
            condition_names: &condition_names,
            path_aliases: &[],
            scss_include_paths: &[],
            root: &manifest.root,
            canonical_fallback: None,
            tsconfig_warned: &tsconfig_warned,
        };

        assert!(resolve_package_map_target(&ctx, &manifest, "lodash", None).is_none());
        assert!(resolve_package_map_target(&ctx, &manifest, "../dist/index.js", None).is_none());
    }

    #[test]
    fn package_imports_fallback_supports_unnamed_packages() {
        let root = PathBuf::from("/project");
        let src_path = root.join("src/runtime/task.ts");
        let manifest = PackageManifestInfo {
            root: root.clone(),
            canonical_root: root.clone(),
            name: None,
            package_json: fallow_config::PackageJson {
                imports: Some(serde_json::json!({
                    "#runtime/*": "./dist/runtime/*.mjs"
                })),
                ..Default::default()
            },
        };
        let mut raw_path_to_id = FxHashMap::default();
        raw_path_to_id.insert(src_path.as_path(), FileId(7));
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();
        let condition_names = conditions();
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions::default());
        let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
        let ctx = ResolveContext {
            resolver: &resolver,
            style_resolver: &resolver,
            extensions: &[],
            path_to_id: &path_to_id,
            raw_path_to_id: &raw_path_to_id,
            workspace_roots: &workspace_roots,
            package_manifests: std::slice::from_ref(&manifest),
            condition_names: &condition_names,
            path_aliases: &[],
            scss_include_paths: &[],
            root: &manifest.root,
            canonical_fallback: None,
            tsconfig_warned: &tsconfig_warned,
        };

        let result =
            try_package_imports_fallback(&ctx, &root.join("src/index.ts"), "#runtime/task");
        assert!(matches!(
            result,
            Some(ResolveResult::InternalModule(FileId(7)))
        ));
    }

    #[test]
    fn test_pnpm_store_path_extract_package_name() {
        // pnpm virtual store paths should correctly extract package name
        let path =
            PathBuf::from("/project/node_modules/.pnpm/react@18.2.0/node_modules/react/index.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("react".to_string())
        );
    }

    #[test]
    fn test_pnpm_store_path_scoped_package() {
        let path = PathBuf::from(
            "/project/node_modules/.pnpm/@babel+core@7.24.0/node_modules/@babel/core/lib/index.js",
        );
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("@babel/core".to_string())
        );
    }

    fn conditions() -> Vec<String> {
        vec![
            "development".to_string(),
            "import".to_string(),
            "require".to_string(),
            "default".to_string(),
            "types".to_string(),
            "node".to_string(),
        ]
    }

    #[test]
    fn test_pnpm_store_path_with_peer_deps() {
        let path = PathBuf::from(
            "/project/node_modules/.pnpm/webpack@5.0.0_esbuild@0.19.0/node_modules/webpack/lib/index.js",
        );
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("webpack".to_string())
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_dist_to_src() {
        let src_path = PathBuf::from("/project/packages/ui/src/utils.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(0));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        // pnpm virtual store path with dist/ output
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/utils.js",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(0)),
            ".pnpm workspace path should fall back to src/utils.ts"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_direct_source() {
        let src_path = PathBuf::from("/project/packages/core/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(1));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/core");
        workspace_roots.insert("@myorg/core", ws_root.as_path());

        // pnpm path pointing directly to src/
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+core@workspace/node_modules/@myorg/core/src/index.ts",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(1)),
            ".pnpm workspace path with src/ should resolve directly"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_non_workspace_package() {
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        // External package (not a workspace) — should return None
        let pnpm_path =
            PathBuf::from("/project/node_modules/.pnpm/react@18.2.0/node_modules/react/index.js");
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            None,
            "non-workspace package in .pnpm should return None"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_unscoped_package() {
        let src_path = PathBuf::from("/project/packages/utils/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(2));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/utils");
        workspace_roots.insert("my-utils", ws_root.as_path());

        // Unscoped workspace package in pnpm store
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/my-utils@1.0.0/node_modules/my-utils/dist/index.js",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(2)),
            "unscoped workspace package in .pnpm should resolve"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_nested_path() {
        let src_path = PathBuf::from("/project/packages/ui/src/components/Button.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(3));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        // Nested path within the package
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/components/Button.js",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(3)),
            "nested .pnpm workspace path should resolve through source fallback"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_no_pnpm() {
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();

        // Regular path without .pnpm — should return None immediately
        let regular_path = PathBuf::from("/project/node_modules/react/index.js");
        assert_eq!(
            try_pnpm_workspace_fallback(&regular_path, &path_to_id, &workspace_roots),
            None,
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_with_peer_deps() {
        let src_path = PathBuf::from("/project/packages/ui/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(4));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        // pnpm path with peer dependency suffix
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0_react@18.2.0/node_modules/@myorg/ui/dist/index.js",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(4)),
            ".pnpm path with peer dep suffix should still resolve"
        );
    }

    // ── make_glob_from_pattern ───────────────────────────────────────

    #[test]
    fn make_glob_prefix_only_no_suffix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./locales/*");
    }

    #[test]
    fn make_glob_prefix_with_suffix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: Some(".json".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./locales/*.json");
    }

    #[test]
    fn make_glob_passthrough_star() {
        // Prefix already contains glob characters — use as-is
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./pages/**/*.tsx".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./pages/**/*.tsx");
    }

    #[test]
    fn make_glob_passthrough_brace() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./i18n/{en,de,fr}.json".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./i18n/{en,de,fr}.json");
    }

    #[test]
    fn make_glob_empty_prefix_no_suffix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: String::new(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "*");
    }

    #[test]
    fn make_glob_empty_prefix_with_suffix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: String::new(),
            suffix: Some(".ts".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "*.ts");
    }

    // ── make_glob_from_pattern: template literal patterns ──────────

    #[test]
    fn make_glob_template_literal_prefix_only() {
        // `./pages/${page}` extracts prefix="./pages/", suffix=None
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./pages/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./pages/*");
    }

    #[test]
    fn make_glob_template_literal_with_extension_suffix() {
        // `./locales/${lang}.json` extracts prefix="./locales/", suffix=".json"
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: Some(".json".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./locales/*.json");
    }

    #[test]
    fn make_glob_template_literal_deep_prefix() {
        // `./modules/${area}/components/${name}.tsx`
        // Extractor captures prefix="./modules/", suffix=None (only first dynamic part)
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./modules/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./modules/*");
    }

    #[test]
    fn make_glob_string_concat_prefix() {
        // `'./pages/' + name` extracts prefix="./pages/", suffix=None
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./pages/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./pages/*");
    }

    #[test]
    fn make_glob_string_concat_with_extension() {
        // `'./views/' + name + '.vue'` extracts prefix="./views/", suffix=".vue"
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./views/".to_string(),
            suffix: Some(".vue".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./views/*.vue");
    }

    // ── make_glob_from_pattern: import.meta.glob ──────────────────

    #[test]
    fn make_glob_import_meta_glob_recursive() {
        // import.meta.glob('./components/**/*.vue')
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./components/**/*.vue".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(
            make_glob_from_pattern(&pattern),
            "./components/**/*.vue",
            "import.meta.glob patterns with * should pass through as-is"
        );
    }

    #[test]
    fn make_glob_import_meta_glob_brace_expansion() {
        // import.meta.glob('./plugins/{auth,analytics}.ts')
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./plugins/{auth,analytics}.ts".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(
            make_glob_from_pattern(&pattern),
            "./plugins/{auth,analytics}.ts",
            "import.meta.glob patterns with braces should pass through as-is"
        );
    }

    #[test]
    fn make_glob_import_meta_glob_star_with_brace() {
        // import.meta.glob('./routes/**/*.{ts,tsx}')
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./routes/**/*.{ts,tsx}".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(
            make_glob_from_pattern(&pattern),
            "./routes/**/*.{ts,tsx}",
            "combined * and brace patterns should pass through"
        );
    }

    #[test]
    fn make_glob_import_meta_glob_ignores_suffix_when_star_present() {
        // Edge case: prefix contains *, suffix is provided (unlikely but defensive)
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./*.ts".to_string(),
            suffix: Some(".extra".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(
            make_glob_from_pattern(&pattern),
            "./*.ts",
            "when prefix has glob chars, suffix is ignored (prefix used as-is)"
        );
    }

    // ── make_glob_from_pattern: edge cases ────────────────────────

    #[test]
    fn make_glob_single_dot_prefix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./*");
    }

    #[test]
    fn make_glob_prefix_without_trailing_slash() {
        // `'./config' + ext` -> prefix="./config", suffix might be extension
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./config".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./config*");
    }

    #[test]
    fn make_glob_prefix_with_dotdot() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "../shared/".to_string(),
            suffix: Some(".ts".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "../shared/*.ts");
    }

    // ── extract_package_name: additional edge cases ───────────────

    #[test]
    fn test_extract_package_name_with_pnpm_plus_encoded_scope() {
        // pnpm encodes @scope/pkg as @scope+pkg in store path
        // but the inner node_modules still uses the real scope
        let path = PathBuf::from(
            "/project/node_modules/.pnpm/@mui+material@5.15.0/node_modules/@mui/material/index.js",
        );
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("@mui/material".to_string())
        );
    }

    #[test]
    fn test_extract_package_name_windows_style_path() {
        // Windows-style paths should still work since we filter for Normal components
        let path = PathBuf::from("/project/node_modules/typescript/lib/tsc.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("typescript".to_string())
        );
    }

    // ── try_source_fallback: additional output dir patterns ───────

    #[test]
    fn test_try_source_fallback_out_dir() {
        let src_path = PathBuf::from("/project/packages/api/src/handler.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(5));

        let out_path = PathBuf::from("/project/packages/api/out/handler.js");
        assert_eq!(
            try_source_fallback(&out_path, &path_to_id),
            Some(FileId(5)),
            "out/handler.js should fall back to src/handler.ts"
        );
    }

    #[test]
    fn test_try_source_fallback_mts_extension() {
        let src_path = PathBuf::from("/project/packages/lib/src/utils.mts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(6));

        let dist_path = PathBuf::from("/project/packages/lib/dist/utils.mjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(6)),
            "dist/utils.mjs should fall back to src/utils.mts"
        );
    }

    #[test]
    fn test_try_source_fallback_cts_extension() {
        let src_path = PathBuf::from("/project/packages/lib/src/config.cts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(7));

        let dist_path = PathBuf::from("/project/packages/lib/dist/config.cjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(7)),
            "dist/config.cjs should fall back to src/config.cts"
        );
    }

    #[test]
    fn test_try_source_fallback_jsx_extension() {
        let src_path = PathBuf::from("/project/packages/ui/src/App.jsx");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(8));

        let build_path = PathBuf::from("/project/packages/ui/build/App.js");
        assert_eq!(
            try_source_fallback(&build_path, &path_to_id),
            Some(FileId(8)),
            "build/App.js should fall back to src/App.jsx"
        );
    }

    #[test]
    fn test_try_source_fallback_no_file_stem() {
        // Path with no filename at all should return None gracefully
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let dist_path = PathBuf::from("/project/packages/ui/dist/");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            None,
            "directory path with no file should return None"
        );
    }

    #[test]
    fn test_try_source_fallback_esm_subdir() {
        // esm is an output directory, so dist/esm -> src
        let src_path = PathBuf::from("/project/lib/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(10));

        let dist_path = PathBuf::from("/project/lib/esm/index.mjs");
        assert_eq!(
            try_source_fallback(&dist_path, &path_to_id),
            Some(FileId(10)),
            "standalone esm/ directory should fall back to src/"
        );
    }

    #[test]
    fn test_try_source_fallback_cjs_subdir() {
        let src_path = PathBuf::from("/project/lib/src/index.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(11));

        let cjs_path = PathBuf::from("/project/lib/cjs/index.cjs");
        assert_eq!(
            try_source_fallback(&cjs_path, &path_to_id),
            Some(FileId(11)),
            "standalone cjs/ directory should fall back to src/"
        );
    }

    // ── try_pnpm_workspace_fallback: edge cases ──────────────────

    #[test]
    fn test_try_pnpm_workspace_fallback_empty_after_pnpm() {
        // Path that has .pnpm but nothing after the inner node_modules
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();

        let pnpm_path = PathBuf::from("/project/node_modules/.pnpm/pkg@1.0.0/node_modules");
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            None,
            "path ending at node_modules with nothing after should return None"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_scoped_package_only_scope() {
        // Path has .pnpm/inner-node_modules/@scope but no package name after scope
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();

        let pnpm_path =
            PathBuf::from("/project/node_modules/.pnpm/@scope+pkg@1.0.0/node_modules/@scope");
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            None,
            "scoped package without full name and no matching workspace should return None"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_no_inner_node_modules() {
        // Path has .pnpm but no inner node_modules
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();

        let pnpm_path = PathBuf::from("/project/node_modules/.pnpm/pkg@1.0.0/dist/index.js");
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            None,
            "path without inner node_modules after .pnpm should return None"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_package_without_relative_path() {
        // Path ends right at the package name, no file path after it
        let path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        let pnpm_path =
            PathBuf::from("/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui");
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            None,
            "path ending at package name with no relative file should return None"
        );
    }

    #[test]
    fn test_try_pnpm_workspace_fallback_nested_dist_esm() {
        let src_path = PathBuf::from("/project/packages/ui/src/Button.ts");
        let mut path_to_id = FxHashMap::default();
        path_to_id.insert(src_path.as_path(), FileId(10));

        let mut workspace_roots = FxHashMap::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        workspace_roots.insert("@myorg/ui", ws_root.as_path());

        // Nested output dirs within pnpm workspace path
        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/esm/Button.mjs",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(10)),
            "pnpm path with nested dist/esm should resolve through source fallback"
        );
    }
}
