//! Resolution fallback strategies for import specifiers.
//!
//! Handles path alias fallbacks, output-to-source directory mapping, pnpm virtual
//! store detection, node_modules package extraction, and dynamic import glob patterns.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde_json::Value;

use fallow_types::discover::FileId;

use super::path_info::{extract_package_name, is_bare_specifier, is_valid_package_name};
use super::types::{OUTPUT_DIRS, PackageManifestInfo, ResolveContext, ResolveResult, SOURCE_EXTS};

/// Return the post-prefix remainder when `specifier` matches the alias `prefix`
/// at a path boundary, else `None`.
///
/// The match is segment-aware: a bare exact-key alias (e.g. `@scope/sdk` or
/// `vscode`) matches only on an exact hit or a `/`-delimited continuation, so it
/// never captures a longer package that merely shares the prefix
/// (`@scope/sdk-extra`). Prefixes that already end in `/` (`~/`, `@/`, `$lib/`)
/// match any continuation by construction, preserving their existing behavior.
fn alias_match_remainder<'a>(specifier: &'a str, prefix: &str) -> Option<&'a str> {
    let remainder = specifier.strip_prefix(prefix)?;
    (remainder.is_empty() || prefix.ends_with('/') || remainder.starts_with('/'))
        .then_some(remainder)
}

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
        let Some(remainder) = alias_match_remainder(specifier, prefix) else {
            continue;
        };

        let substituted = match (replacement.is_empty(), remainder.is_empty()) {
            (true, _) => format!("./{remainder}"),
            (false, true) => format!("./{replacement}"),
            (false, false) => format!("./{replacement}/{remainder}"),
        };

        if let Ok(resolved) = ctx.resolver.resolve(ctx.root, &substituted) {
            let resolved_path = resolved.path();
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return Some(ResolveResult::InternalModule(file_id));
            }
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
    if specifier.contains(':') {
        return None;
    }

    let spec_path = Path::new(specifier);
    let filename = spec_path.file_name()?.to_str()?;

    if filename.starts_with('_') {
        return None;
    }

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
    if specifier.contains(':') {
        return None;
    }
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
        let direct = if ext.is_empty() {
            build(bare_path)
        } else {
            join_with_parent(&format!("{stem_with_ext}{suffix}"))
        };
        if let Some(fid) = lookup_scss_path(&direct, ctx) {
            return Some(fid);
        }
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
            continue;
        }
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
    if specifier.contains(':') {
        return None;
    }
    let is_scss_importer = from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass");
    if !is_scss_importer && !from_style {
        return None;
    }
    let bare = specifier.strip_prefix("./")?;
    if bare.starts_with("..") || bare.starts_with('/') {
        return None;
    }
    if bare.is_empty() {
        return None;
    }

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

    for ext in &["scss", "sass", "css"] {
        let candidate = join_with_parent(&format!("{file_name}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for ext in &["scss", "sass"] {
        let candidate = join_with_parent(&format!("_{file_name}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
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

    let last_output_pos = components.iter().rposition(&is_output_dir)?;

    let mut first_output_pos = last_output_pos;
    while first_output_pos > 0 && is_output_dir(&components[first_output_pos - 1]) {
        first_output_pos -= 1;
    }

    let prefix: PathBuf = components[..first_output_pos].iter().collect();

    let suffix: PathBuf = components[last_output_pos + 1..].iter().collect();
    suffix.file_stem()?; // Ensure the suffix has a filename

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
    let PackageMapTarget::Targets(targets) =
        package_map_target(imports, specifier, ctx.condition_names)
    else {
        return None;
    };
    let source_subpath = package_import_source_subpath(manifest, specifier);
    resolve_package_import_targets(ctx, manifest, &targets, source_subpath.as_deref()).map(
        |target| match target {
            PackageImportTarget::Internal(file_id) => match &manifest.name {
                Some(package_name) => ResolveResult::InternalPackageModule {
                    file_id,
                    package_name: package_name.clone(),
                },
                None => ResolveResult::InternalModule(file_id),
            },
            PackageImportTarget::ExternalPackage(package_name) => {
                ResolveResult::NpmPackage(package_name)
            }
        },
    )
}

/// Resolve a relative import that lands on a known package root whose built
/// entry points are absent but whose package metadata points at source files.
pub(super) fn try_relative_package_root_source_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }

    let from_dir = from_file.parent()?;
    let candidate = from_dir.join(specifier);
    let normalized_candidate = normalize_path_lexically(&candidate);
    #[cfg(not(miri))]
    let canonical_candidate = dunce::canonicalize(&candidate).ok();
    #[cfg(miri)]
    let canonical_candidate: Option<PathBuf> = None;

    ctx.package_manifests.iter().find_map(|manifest| {
        let matches_manifest = candidate == manifest.root
            || normalized_candidate == manifest.root
            || canonical_candidate
                .as_deref()
                .is_some_and(|canonical| canonical == manifest.canonical_root);
        matches_manifest
            .then(|| try_source_subpath(ctx, manifest, Path::new("")))
            .flatten()
            .map(ResolveResult::InternalModule)
    })
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageMapTarget {
    NoMatch,
    Blocked,
    Targets(Vec<String>),
}

enum PackageImportTarget {
    Internal(FileId),
    ExternalPackage(String),
}

fn package_map_match_value(
    value: &Value,
    condition_names: &[String],
    capture: Option<&str>,
) -> PackageMapTarget {
    resolve_package_map_value(value, condition_names, capture)
        .filter(|targets| !targets.is_empty())
        .map_or(PackageMapTarget::Blocked, PackageMapTarget::Targets)
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
) -> Option<Vec<String>> {
    match value {
        Value::String(target) => Some(vec![match capture {
            Some(capture) => target.replace('*', capture),
            None => target.clone(),
        }]),
        Value::Object(map) => {
            for (condition, value) in map {
                if (condition == "default"
                    || condition_names
                        .iter()
                        .any(|active_condition| active_condition == condition))
                    && let Some(targets) =
                        resolve_package_map_value(value, condition_names, capture)
                {
                    return Some(targets);
                }
            }
            None
        }
        Value::Array(values) => {
            let targets: Vec<String> = values
                .iter()
                .filter_map(|value| resolve_package_map_value(value, condition_names, capture))
                .flatten()
                .collect();
            (!targets.is_empty()).then_some(targets)
        }
        Value::Bool(_) | Value::Null | Value::Number(_) => None,
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

fn resolve_package_map_targets(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    targets: &[String],
    source_subpath: Option<&Path>,
) -> Option<FileId> {
    targets
        .iter()
        .find_map(|target| resolve_package_map_target(ctx, manifest, target, source_subpath))
}

fn resolve_package_import_targets(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    targets: &[String],
    source_subpath: Option<&Path>,
) -> Option<PackageImportTarget> {
    targets.iter().find_map(|target| {
        resolve_package_map_target(ctx, manifest, target, source_subpath)
            .map(PackageImportTarget::Internal)
            .or_else(|| {
                package_import_external_target(target).map(PackageImportTarget::ExternalPackage)
            })
    })
}

fn package_import_external_target(target: &str) -> Option<String> {
    if is_bare_specifier(target) && is_valid_package_name(target) {
        Some(extract_package_name(target))
    } else {
        None
    }
}

fn try_source_subpath(
    ctx: &ResolveContext<'_>,
    manifest: &PackageManifestInfo,
    subpath: &Path,
) -> Option<FileId> {
    if subpath.as_os_str().is_empty()
        && let Some(source) = manifest.package_json.source.as_deref()
        && let Some(source_path) = safe_relative_package_source_path(source)
        && let Some(file_id) = lookup_internal_file_id(ctx, &manifest.root.join(source_path))
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

fn safe_relative_package_source_path(source: &str) -> Option<&Path> {
    let source = source.strip_prefix("./").unwrap_or(source);
    let path = Path::new(source);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        None
    } else {
        Some(path)
    }
}

fn lookup_internal_file_id(ctx: &ResolveContext<'_>, candidate: &Path) -> Option<FileId> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(candidate) {
        return Some(file_id);
    }
    if let Some(&file_id) = ctx.path_to_id.get(candidate) {
        return Some(file_id);
    }
    #[cfg(not(miri))]
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

    let nm_idx = components.iter().rposition(|&c| c == "node_modules")?;

    let after = &components[nm_idx + 1..];
    if after.is_empty() {
        return None;
    }

    if after[0].starts_with('@') {
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
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    let pnpm_idx = components.iter().position(|&c| c == ".pnpm")?;

    let after_pnpm = &components[pnpm_idx + 1..];

    let inner_nm_idx = after_pnpm.iter().position(|&c| c == "node_modules")?;
    let after_inner_nm = &after_pnpm[inner_nm_idx + 1..];

    if after_inner_nm.is_empty() {
        return None;
    }

    let (pkg_name, pkg_name_components) = if after_inner_nm[0].starts_with('@') {
        if after_inner_nm.len() >= 2 {
            (format!("{}/{}", after_inner_nm[0], after_inner_nm[1]), 2)
        } else {
            return None;
        }
    } else {
        (after_inner_nm[0].to_string(), 1)
    };

    let ws_root = workspace_roots.get(pkg_name.as_str())?;

    let relative_parts = &after_inner_nm[pkg_name_components..];
    if relative_parts.is_empty() {
        return None;
    }

    let relative_path: PathBuf = relative_parts.iter().collect();

    let direct = ws_root.join(&relative_path);
    if let Some(&file_id) = path_to_id.get(direct.as_path()) {
        return Some(file_id);
    }

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
/// one, then try the package source layout directly when no `exports` map exists,
/// and finally resolve the stripped subpath as a relative path from inside the
/// package root. The manifest branches cover source-only workspaces whose
/// package metadata points at missing `dist` output.
///
/// See issues #106, #641, and #725.
pub(super) fn try_workspace_package_fallback(
    ctx: &ResolveContext<'_>,
    specifier: &str,
) -> Option<ResolveResult> {
    if !super::path_info::is_bare_specifier(specifier) {
        return None;
    }
    let pkg_name = super::path_info::extract_package_name(specifier);

    let subpath = specifier
        .strip_prefix(pkg_name.as_str())
        .and_then(|s| s.strip_prefix('/'))
        .unwrap_or("");
    let source_subpath = PathBuf::from(subpath);

    match try_manifest_workspace_resolution(ctx, &pkg_name, subpath, &source_subpath) {
        ManifestWorkspaceResolution::Resolved(result) => return Some(result),
        ManifestWorkspaceResolution::Blocked => return None,
        ManifestWorkspaceResolution::Continue => {}
    }

    let ws_root =
        if let Some(manifest) = find_package_manifest(ctx.package_manifests, pkg_name.as_str()) {
            manifest.root.as_path()
        } else {
            *ctx.workspace_roots.get(pkg_name.as_str())?
        };

    resolve_workspace_self_reference(ctx, ws_root, subpath, pkg_name)
}

/// Outcome of attempting workspace resolution through a matching package
/// manifest's `exports` map or source layout.
enum ManifestWorkspaceResolution {
    /// A target was resolved.
    Resolved(ResolveResult),
    /// An `exports` map exists but the subpath is unmatched or null-blocked.
    Blocked,
    /// No manifest matched, or it had no usable resolution; keep trying.
    Continue,
}

/// Try resolving via the package manifest: `exports` map first, then the source
/// layout for manifests without an `exports` map.
fn try_manifest_workspace_resolution(
    ctx: &ResolveContext<'_>,
    pkg_name: &str,
    subpath: &str,
    source_subpath: &Path,
) -> ManifestWorkspaceResolution {
    let Some(manifest) = find_package_manifest(ctx.package_manifests, pkg_name) else {
        return ManifestWorkspaceResolution::Continue;
    };

    if let Some(exports) = manifest.package_json.exports.as_ref() {
        let export_key = if subpath.is_empty() {
            ".".to_string()
        } else {
            format!("./{subpath}")
        };
        return match package_map_target(exports, &export_key, ctx.condition_names) {
            PackageMapTarget::Targets(targets) => {
                match resolve_package_map_targets(ctx, manifest, &targets, Some(source_subpath)) {
                    Some(file_id) => ManifestWorkspaceResolution::Resolved(
                        ResolveResult::InternalPackageModule {
                            file_id,
                            package_name: pkg_name.to_string(),
                        },
                    ),
                    None => ManifestWorkspaceResolution::Continue,
                }
            }
            PackageMapTarget::NoMatch | PackageMapTarget::Blocked => {
                ManifestWorkspaceResolution::Blocked
            }
        };
    }

    if let Some(file_id) = try_source_subpath(ctx, manifest, source_subpath) {
        return ManifestWorkspaceResolution::Resolved(ResolveResult::InternalPackageModule {
            file_id,
            package_name: pkg_name.to_string(),
        });
    }

    ManifestWorkspaceResolution::Continue
}

/// Resolve the stripped subpath as a relative import from inside the package
/// root, mapping the resolved path back to an internal module via the id maps
/// and source fallback.
fn resolve_workspace_self_reference(
    ctx: &ResolveContext<'_>,
    ws_root: &Path,
    subpath: &str,
    package_name: String,
) -> Option<ResolveResult> {
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

    fn with_package_map_ctx(
        root: PathBuf,
        name: Option<&str>,
        package_json: fallow_config::PackageJson,
        raw_files: &[(PathBuf, FileId)],
        f: impl FnOnce(&ResolveContext<'_>, &PackageManifestInfo, &Path),
    ) {
        let manifest = PackageManifestInfo {
            root: root.clone(),
            canonical_root: root,
            name: name.map(str::to_string),
            package_json,
        };
        let manifests = [manifest];
        let mut raw_path_to_id = FxHashMap::default();
        for (path, file_id) in raw_files {
            raw_path_to_id.insert(path.as_path(), *file_id);
        }
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
            package_manifests: &manifests,
            condition_names: &condition_names,
            path_aliases: &[],
            scss_include_paths: &[],
            static_dir_mappings: &[],
            root: &manifests[0].root,
            canonical_fallback: None,
            tsconfig_warned: &tsconfig_warned,
        };

        f(&ctx, &manifests[0], &manifests[0].root);
    }

    #[test]
    fn alias_match_remainder_exact_key() {
        assert_eq!(alias_match_remainder("vscode", "vscode"), Some(""));
        assert_eq!(alias_match_remainder("@scope/sdk", "@scope/sdk"), Some(""));
    }

    #[test]
    fn alias_match_remainder_slash_continuation() {
        assert_eq!(
            alias_match_remainder("@scope/sdk/sub", "@scope/sdk"),
            Some("/sub")
        );
        assert_eq!(alias_match_remainder("@/foo", "@/"), Some("foo"));
        assert_eq!(
            alias_match_remainder("~/components/x", "~/"),
            Some("components/x")
        );
        assert_eq!(alias_match_remainder("$lib/util", "$lib/"), Some("util"));
    }

    #[test]
    fn alias_match_remainder_rejects_prefix_collision() {
        assert_eq!(
            alias_match_remainder("@scope/sdk-extra", "@scope/sdk"),
            None
        );
        assert_eq!(
            alias_match_remainder("vscode-languageserver", "vscode"),
            None
        );
        assert_eq!(alias_match_remainder("#shared-utils", "#shared"), None);
    }

    #[test]
    fn alias_match_remainder_non_match() {
        assert_eq!(alias_match_remainder("react", "vscode"), None);
    }

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
        let path = PathBuf::from("/project/node_modules/@scope");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("@scope".to_string())
        );
    }

    #[test]
    fn test_resolve_specifier_node_modules_returns_npm_package() {
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
            PackageMapTarget::Targets(vec!["./dist/special/task.mjs".to_string()])
        );
    }

    #[test]
    fn package_map_wildcard_substitutes_capture() {
        let map = serde_json::json!({
            "#nitro/runtime/*": "./dist/runtime/internal/*.mjs"
        });
        assert_eq!(
            package_map_target(&map, "#nitro/runtime/task", &conditions()),
            PackageMapTarget::Targets(vec!["./dist/runtime/internal/task.mjs".to_string()])
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
            PackageMapTarget::Targets(vec!["./dist/query/react/index.d.ts".to_string()])
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
            PackageMapTarget::Targets(vec!["./dist/index.js".to_string()])
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
            PackageMapTarget::Targets(vec!["./dist/node.js".to_string()])
        );
    }

    #[test]
    fn package_map_arrays_preserve_fallback_order() {
        let map = serde_json::json!({
            "#array": ["./dist/missing.js", "./src/array.ts"],
            "#null": null,
            "#false": false
        });
        assert_eq!(
            package_map_target(&map, "#array", &conditions()),
            PackageMapTarget::Targets(vec![
                "./dist/missing.js".to_string(),
                "./src/array.ts".to_string()
            ])
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
        with_package_map_ctx(
            PathBuf::from("/project"),
            Some("pkg"),
            fallow_config::PackageJson::default(),
            &[],
            |ctx, manifest, _| {
                assert!(resolve_package_map_target(ctx, manifest, "lodash", None).is_none());
                assert!(
                    resolve_package_map_target(ctx, manifest, "../dist/index.js", None).is_none()
                );
            },
        );
    }

    #[test]
    fn package_map_targets_use_first_reachable_target() {
        let root = PathBuf::from("/project");
        let src_path = root.join("src/feature.ts");
        let targets = vec![
            "./dist/missing.js".to_string(),
            "./src/feature.ts".to_string(),
        ];

        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson::default(),
            &[(src_path, FileId(9))],
            |ctx, manifest, _| {
                assert_eq!(
                    resolve_package_map_targets(ctx, manifest, &targets, None),
                    Some(FileId(9))
                );
            },
        );
    }

    #[test]
    fn package_imports_fallback_supports_external_package_targets() {
        let root = PathBuf::from("/project");
        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson {
                imports: Some(serde_json::json!({
                    "#pad": "left-pad",
                    "#scoped": "@scope/pkg/subpath"
                })),
                ..Default::default()
            },
            &[],
            |ctx, _, root| {
                let pad = try_package_imports_fallback(ctx, &root.join("src/index.ts"), "#pad");
                assert!(matches!(pad, Some(ResolveResult::NpmPackage(pkg)) if pkg == "left-pad"));

                let scoped =
                    try_package_imports_fallback(ctx, &root.join("src/index.ts"), "#scoped");
                assert!(
                    matches!(scoped, Some(ResolveResult::NpmPackage(pkg)) if pkg == "@scope/pkg")
                );
            },
        );
    }

    #[test]
    fn package_imports_fallback_supports_unnamed_packages() {
        let root = PathBuf::from("/project");
        let src_path = root.join("src/runtime/task.ts");
        with_package_map_ctx(
            root,
            None,
            fallow_config::PackageJson {
                imports: Some(serde_json::json!({
                    "#runtime/*": "./dist/runtime/*.mjs"
                })),
                ..Default::default()
            },
            &[(src_path, FileId(7))],
            |ctx, _, root| {
                let result =
                    try_package_imports_fallback(ctx, &root.join("src/index.ts"), "#runtime/task");
                assert!(matches!(
                    result,
                    Some(ResolveResult::InternalModule(FileId(7)))
                ));
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn relative_package_root_source_fallback_uses_package_source_entry() {
        let root = PathBuf::from("/project");
        let source_path = root.join("custom/entry.js");
        with_package_map_ctx(
            root,
            Some("pkg"),
            fallow_config::PackageJson {
                source: Some("custom/entry.js".to_string()),
                ..Default::default()
            },
            &[(source_path, FileId(11))],
            |ctx, _, root| {
                let result = try_relative_package_root_source_fallback(
                    ctx,
                    &root.join("test/shared/exports.test.js"),
                    "../../",
                );
                assert!(matches!(
                    result,
                    Some(ResolveResult::InternalModule(FileId(11)))
                ));
            },
        );
    }

    #[test]
    fn package_source_path_accepts_relative_source_entries() {
        assert_eq!(
            safe_relative_package_source_path("src/index.js"),
            Some(Path::new("src/index.js"))
        );
        assert_eq!(
            safe_relative_package_source_path("./custom/entry.ts"),
            Some(Path::new("custom/entry.ts"))
        );
    }

    #[test]
    fn package_source_path_rejects_unsafe_entries() {
        assert_eq!(safe_relative_package_source_path(""), None);
        assert_eq!(safe_relative_package_source_path("./"), None);
        assert_eq!(safe_relative_package_source_path("../src/index.js"), None);
        assert_eq!(safe_relative_package_source_path("src/../index.js"), None);
        assert_eq!(safe_relative_package_source_path("/src/index.js"), None);

        #[cfg(windows)]
        assert_eq!(safe_relative_package_source_path("C:\\src\\index.js"), None);
    }

    #[test]
    fn test_pnpm_store_path_extract_package_name() {
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

        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0_react@18.2.0/node_modules/@myorg/ui/dist/index.js",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(4)),
            ".pnpm path with peer dep suffix should still resolve"
        );
    }

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

    #[test]
    fn make_glob_template_literal_prefix_only() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./pages/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./pages/*");
    }

    #[test]
    fn make_glob_template_literal_with_extension_suffix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./locales/".to_string(),
            suffix: Some(".json".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./locales/*.json");
    }

    #[test]
    fn make_glob_template_literal_deep_prefix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./modules/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./modules/*");
    }

    #[test]
    fn make_glob_string_concat_prefix() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./pages/".to_string(),
            suffix: None,
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./pages/*");
    }

    #[test]
    fn make_glob_string_concat_with_extension() {
        let pattern = fallow_types::extract::DynamicImportPattern {
            prefix: "./views/".to_string(),
            suffix: Some(".vue".to_string()),
            span: oxc_span::Span::default(),
        };
        assert_eq!(make_glob_from_pattern(&pattern), "./views/*.vue");
    }

    #[test]
    fn make_glob_import_meta_glob_recursive() {
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

    #[test]
    fn test_extract_package_name_with_pnpm_plus_encoded_scope() {
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
        let path = PathBuf::from("/project/node_modules/typescript/lib/tsc.js");
        assert_eq!(
            extract_package_name_from_node_modules_path(&path),
            Some("typescript".to_string())
        );
    }

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

    #[test]
    fn test_try_pnpm_workspace_fallback_empty_after_pnpm() {
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

        let pnpm_path = PathBuf::from(
            "/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/esm/Button.mjs",
        );
        assert_eq!(
            try_pnpm_workspace_fallback(&pnpm_path, &path_to_id, &workspace_roots),
            Some(FileId(10)),
            "pnpm path with nested dist/esm should resolve through source fallback"
        );
    }

    // --- package_map_target: non-object map branches (lines 583-586) ---

    #[test]
    fn package_map_target_string_value_dot_key() {
        // A non-object top-level map with specifier_key "." delegates to
        // package_map_match_value immediately.
        let map = serde_json::Value::String("./src/index.ts".to_string());
        let conds = conditions();
        // A string value resolves to Targets.
        let result = package_map_target(&map, ".", &conds);
        assert!(
            matches!(result, PackageMapTarget::Targets(_)),
            "string map with '.' key should return Targets"
        );
    }

    #[test]
    fn package_map_target_string_value_non_dot_key_no_match() {
        // A non-object top-level map with a non-"." specifier returns NoMatch.
        let map = serde_json::Value::String("./src/index.ts".to_string());
        let conds = conditions();
        let result = package_map_target(&map, "./sub", &conds);
        assert!(
            matches!(result, PackageMapTarget::NoMatch),
            "string map with non-dot key should return NoMatch"
        );
    }

    #[test]
    fn package_map_target_null_value_dot_key() {
        // A null top-level map with "." returns Blocked (null means blocked).
        let map = serde_json::Value::Null;
        let conds = conditions();
        let result = package_map_target(&map, ".", &conds);
        assert!(
            matches!(result, PackageMapTarget::Blocked),
            "null map with '.' key should return Blocked"
        );
    }

    #[test]
    fn package_map_target_bool_value_non_dot_key() {
        // A bool top-level map with non-dot key is not an object, returns NoMatch.
        let map = serde_json::Value::Bool(true);
        let conds = conditions();
        let result = package_map_target(&map, "./sub", &conds);
        assert!(
            matches!(result, PackageMapTarget::NoMatch),
            "bool map with non-dot key should return NoMatch"
        );
    }

    // --- package_map_target: condition-only object map (lines 592-596) ---

    #[test]
    fn package_map_target_condition_only_object_dot_key() {
        // An object whose keys are all conditions (not "." or "./...") is treated
        // as a condition map when specifier_key is ".".
        let map = serde_json::json!({
            "import": "./src/index.mjs",
            "require": "./src/index.cjs"
        });
        let conds = conditions();
        let result = package_map_target(&map, ".", &conds);
        assert!(
            matches!(result, PackageMapTarget::Targets(_)),
            "condition-only object with '.' key should return Targets"
        );
    }

    #[test]
    fn package_map_target_condition_only_object_non_dot_key() {
        // Same object, but specifier_key != "." returns NoMatch because no
        // subpath key like "./foo" exists.
        let map = serde_json::json!({
            "import": "./src/index.mjs",
            "require": "./src/index.cjs"
        });
        let conds = conditions();
        let result = package_map_target(&map, "./nonexistent", &conds);
        assert!(
            matches!(result, PackageMapTarget::NoMatch),
            "condition-only object with non-dot key should return NoMatch"
        );
    }

    // --- resolve_package_map_value: unmatched conditions, bool, null (lines 641-654) ---

    #[test]
    fn resolve_package_map_value_unmatched_conditions_returns_none() {
        // Object whose only key is not in the active condition set returns None.
        let value = serde_json::json!({ "browser": "./src/browser.js" });
        let conds = conditions(); // does not include "browser"
        assert_eq!(
            resolve_package_map_value(&value, &conds, None),
            None,
            "unmatched condition should return None"
        );
    }

    #[test]
    fn resolve_package_map_value_bool_returns_none() {
        let value = serde_json::Value::Bool(false);
        let conds = conditions();
        assert_eq!(
            resolve_package_map_value(&value, &conds, None),
            None,
            "bool value should return None"
        );
    }

    #[test]
    fn resolve_package_map_value_number_returns_none() {
        let value = serde_json::Value::Number(42.into());
        let conds = conditions();
        assert_eq!(
            resolve_package_map_value(&value, &conds, None),
            None,
            "number value should return None"
        );
    }

    #[test]
    fn resolve_package_map_value_null_returns_none() {
        let value = serde_json::Value::Null;
        let conds = conditions();
        assert_eq!(
            resolve_package_map_value(&value, &conds, None),
            None,
            "null value should return None"
        );
    }

    #[test]
    fn resolve_package_map_value_array_all_null_returns_none() {
        // Array where every element resolves to None yields None.
        let value = serde_json::json!([null, false, 42]);
        let conds = conditions();
        assert_eq!(
            resolve_package_map_value(&value, &conds, None),
            None,
            "array of unresolvable values should return None"
        );
    }

    #[test]
    fn resolve_package_map_value_array_with_valid_entry() {
        // Array where one element is a valid string target.
        let value = serde_json::json!([null, "./src/index.ts"]);
        let conds = conditions();
        let result = resolve_package_map_value(&value, &conds, None);
        assert_eq!(
            result,
            Some(vec!["./src/index.ts".to_string()]),
            "array with a valid string entry should return that entry"
        );
    }

    // --- package_map_pattern_capture: two-star and no-star branches (lines 659-665) ---

    #[test]
    fn package_map_pattern_capture_no_star_returns_none() {
        // A pattern without '*' returns None (no star found).
        assert_eq!(
            package_map_pattern_capture("./exact", "./exact"),
            None,
            "pattern with no star should return None"
        );
    }

    #[test]
    fn package_map_pattern_capture_two_stars_returns_none() {
        // A pattern with more than one '*' returns None.
        assert_eq!(
            package_map_pattern_capture("./*/*.js", "./foo/bar.js"),
            None,
            "pattern with two stars should return None"
        );
    }

    #[test]
    fn package_map_pattern_capture_single_star_captures() {
        // Sanity: the happy path still works after the guard checks.
        assert_eq!(
            package_map_pattern_capture("./dist/*/index.js", "./dist/utils/index.js"),
            Some("utils".to_string()),
        );
    }

    #[test]
    fn package_map_pattern_capture_no_prefix_match_returns_none() {
        // Specifier does not start with the pattern prefix.
        assert_eq!(
            package_map_pattern_capture("./lib/*.js", "./src/foo.js"),
            None,
        );
    }

    // --- resolve_package_map_target: parent-dir and root-absolute guard (lines 719-721) ---

    #[test]
    fn resolve_package_map_target_no_dot_slash_prefix_returns_none() {
        // A target that does not start with "./" is rejected by strip_prefix.
        let root = PathBuf::from("/project/packages/ui");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, Some("@myorg/ui"), pj, &[], |ctx, manifest, _root| {
            let result = resolve_package_map_target(ctx, manifest, "src/index.ts", None);
            assert_eq!(result, None, "target without './' should return None");
        });
    }

    #[test]
    fn resolve_package_map_target_parent_dir_returns_none() {
        // A target starting with "../" is rejected as a path escape.
        let root = PathBuf::from("/project/packages/ui");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, Some("@myorg/ui"), pj, &[], |ctx, manifest, _root| {
            let result = resolve_package_map_target(ctx, manifest, "./../outside/file.ts", None);
            assert_eq!(result, None, "parent-dir target should return None");
        });
    }

    #[test]
    fn resolve_package_map_target_absolute_path_returns_none() {
        // A target starting with "/" after stripping "./" prefix is rejected.
        let root = PathBuf::from("/project/packages/ui");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, Some("@myorg/ui"), pj, &[], |ctx, manifest, _root| {
            // "./" + "/" -> "/" after strip_prefix("./") which is no-op, but
            // an absolute path disguised as ".//abs" yields "/" start after strip.
            let result = resolve_package_map_target(ctx, manifest, ".//abs/path.ts", None);
            assert_eq!(result, None, "absolute target should return None");
        });
    }

    #[test]
    fn resolve_package_map_target_valid_target_hits_raw_path_map() {
        // A valid "./" target resolves when the path is in raw_path_to_id.
        let root = PathBuf::from("/project/packages/ui");
        let src = root.join("src/index.ts");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(
            root,
            Some("@myorg/ui"),
            pj,
            &[(src, FileId(5))],
            |ctx, manifest, _root| {
                let result = resolve_package_map_target(ctx, manifest, "./src/index.ts", None);
                assert_eq!(
                    result,
                    Some(FileId(5)),
                    "valid target in raw_path_to_id should resolve"
                );
            },
        );
    }

    // --- package_import_source_subpath: variants (lines 673-689) ---

    #[test]
    fn package_import_source_subpath_strips_hash_and_package_name() {
        let manifest = PackageManifestInfo {
            root: PathBuf::from("/project"),
            canonical_root: PathBuf::from("/project"),
            name: Some("my-pkg".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let result = package_import_source_subpath(&manifest, "#my-pkg/utils");
        assert_eq!(
            result,
            Some(PathBuf::from("utils")),
            "should strip '#', package name, and '/' separator"
        );
    }

    #[test]
    fn package_import_source_subpath_no_package_name_match_keeps_full_subpath() {
        // When the specifier after '#' does not start with the package name,
        // the full stripped specifier is returned.
        let manifest = PackageManifestInfo {
            root: PathBuf::from("/project"),
            canonical_root: PathBuf::from("/project"),
            name: Some("my-pkg".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let result = package_import_source_subpath(&manifest, "#utils");
        assert_eq!(
            result,
            Some(PathBuf::from("utils")),
            "without package-name prefix the full subpath should be kept"
        );
    }

    #[test]
    fn package_import_source_subpath_empty_hash_returns_none() {
        // "#" with nothing after returns None because stripped is empty and
        // the empty string is rejected by the is_empty guard.
        let manifest = PackageManifestInfo {
            root: PathBuf::from("/project"),
            canonical_root: PathBuf::from("/project"),
            name: Some("my-pkg".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        // "#" strips to "", which is_empty is true, so returns None.
        let result = package_import_source_subpath(&manifest, "#");
        assert_eq!(
            result, None,
            "specifier '#' with empty body should return None"
        );
    }

    #[test]
    fn package_import_source_subpath_no_hash_returns_none() {
        // A specifier not starting with '#' returns None.
        let manifest = PackageManifestInfo {
            root: PathBuf::from("/project"),
            canonical_root: PathBuf::from("/project"),
            name: Some("my-pkg".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let result = package_import_source_subpath(&manifest, "no-hash");
        assert_eq!(result, None, "specifier without '#' should return None");
    }

    #[test]
    fn package_import_source_subpath_no_manifest_name() {
        // When the manifest has no name the full stripped specifier is returned.
        let manifest = PackageManifestInfo {
            root: PathBuf::from("/project"),
            canonical_root: PathBuf::from("/project"),
            name: None,
            package_json: fallow_config::PackageJson::default(),
        };
        let result = package_import_source_subpath(&manifest, "#internal/helper");
        assert_eq!(
            result,
            Some(PathBuf::from("internal/helper")),
            "manifest without name should return the full stripped specifier"
        );
    }

    // --- nearest_package_manifest: deepest selection (lines 691-701) ---

    #[test]
    fn nearest_package_manifest_returns_deepest_match() {
        let root1 = PathBuf::from("/project");
        let root2 = PathBuf::from("/project/packages/ui");
        let m1 = PackageManifestInfo {
            root: root1.clone(),
            canonical_root: root1,
            name: Some("root".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let m2 = PackageManifestInfo {
            root: root2.clone(),
            canonical_root: root2,
            name: Some("@myorg/ui".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let manifests = [m1, m2];
        let from_file = Path::new("/project/packages/ui/src/index.ts");
        let result = nearest_package_manifest(&manifests, from_file);
        assert_eq!(
            result.and_then(|m| m.name.as_deref()),
            Some("@myorg/ui"),
            "should pick the deepest (longest path) manifest that contains the file"
        );
    }

    #[test]
    fn nearest_package_manifest_no_match_returns_none() {
        let root = PathBuf::from("/project/packages/ui");
        let m = PackageManifestInfo {
            root: root.clone(),
            canonical_root: root,
            name: Some("@myorg/ui".to_string()),
            package_json: fallow_config::PackageJson::default(),
        };
        let manifests = [m];
        // File is outside the manifest root.
        let from_file = Path::new("/other/project/src/index.ts");
        let result = nearest_package_manifest(&manifests, from_file);
        assert!(
            result.is_none(),
            "file outside all manifest roots should return None"
        );
    }

    // --- lookup_internal_file_id: path_to_id fallback (line 832) ---

    #[test]
    fn lookup_internal_file_id_uses_path_to_id_when_raw_misses() {
        // When raw_path_to_id does not contain the path but path_to_id does,
        // lookup_internal_file_id should fall back to path_to_id.
        let target = PathBuf::from("/project/src/index.ts");
        let mut path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        path_to_id.insert(target.as_path(), FileId(99));
        let raw_path_to_id: FxHashMap<&Path, FileId> = FxHashMap::default();
        let workspace_roots: FxHashMap<&str, &Path> = FxHashMap::default();
        let condition_names = conditions();
        let resolver = oxc_resolver::Resolver::new(oxc_resolver::ResolveOptions::default());
        let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
        let root = PathBuf::from("/project");
        let ctx = ResolveContext {
            resolver: &resolver,
            style_resolver: &resolver,
            extensions: &[],
            path_to_id: &path_to_id,
            raw_path_to_id: &raw_path_to_id,
            workspace_roots: &workspace_roots,
            package_manifests: &[],
            condition_names: &condition_names,
            path_aliases: &[],
            scss_include_paths: &[],
            static_dir_mappings: &[],
            root: &root,
            canonical_fallback: None,
            tsconfig_warned: &tsconfig_warned,
        };
        let result = lookup_internal_file_id(&ctx, &target);
        assert_eq!(
            result,
            Some(FileId(99)),
            "should fall back from raw_path_to_id to path_to_id"
        );
    }

    // --- try_scss_partial_fallback: colon guard (line 91) ---

    #[test]
    fn try_scss_partial_fallback_rejects_colon_specifier() {
        // A specifier containing ':' (e.g. a Sass built-in like "sass:math")
        // must return None immediately.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root.clone(), None, pj, &[], |ctx, _manifest, _r| {
            let from_file = root.join("src/main.scss");
            let result = try_scss_partial_fallback(ctx, &from_file, "sass:math");
            assert!(
                result.is_none(),
                "specifier with ':' should short-circuit to None"
            );
        });
    }

    #[test]
    fn try_scss_partial_fallback_rejects_already_partial_filename() {
        // A specifier whose filename already starts with '_' (i.e. it's already a
        // partial path) must return None immediately.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root.clone(), None, pj, &[], |ctx, _manifest, _r| {
            let from_file = root.join("src/main.scss");
            let result = try_scss_partial_fallback(ctx, &from_file, "./_variables");
            assert!(
                result.is_none(),
                "already-partial filename should short-circuit to None"
            );
        });
    }

    // --- try_workspace_package_fallback: bare-specifier guard (lines 967-968) ---

    #[test]
    fn try_workspace_package_fallback_rejects_relative_specifier() {
        // Relative specifiers (starting with "./" or "../") are not bare specifiers
        // and must return None without touching any manifest.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, None, pj, &[], |ctx, _manifest, _r| {
            let result = try_workspace_package_fallback(ctx, "./local/module");
            assert!(
                result.is_none(),
                "relative specifier should return None from workspace fallback"
            );
        });
    }

    #[test]
    fn try_workspace_package_fallback_rejects_absolute_path() {
        // Absolute paths are not bare specifiers either.
        let root = PathBuf::from("/project");
        let pj = fallow_config::PackageJson::default();
        with_package_map_ctx(root, None, pj, &[], |ctx, _manifest, _r| {
            let result = try_workspace_package_fallback(ctx, "/absolute/path");
            assert!(
                result.is_none(),
                "absolute path should return None from workspace fallback"
            );
        });
    }
}
