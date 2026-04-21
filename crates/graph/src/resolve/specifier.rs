//! Main resolution engine: creates the oxc_resolver instance and resolves individual specifiers.

use std::path::{Path, PathBuf};

use oxc_resolver::{Resolution, ResolveError, ResolveOptions, Resolver};

use super::fallbacks::{
    extract_package_name_from_node_modules_path, try_path_alias_fallback,
    try_pnpm_workspace_fallback, try_scss_include_path_fallback, try_scss_node_modules_fallback,
    try_scss_partial_fallback, try_source_fallback, try_workspace_package_fallback,
};
use super::path_info::{
    extract_package_name, is_bare_specifier, is_path_alias, is_valid_package_name,
};
use super::react_native::{build_condition_names, build_extensions};
use super::types::{ResolveContext, ResolveResult};

/// Create an `oxc_resolver` instance with standard configuration.
///
/// When React Native or Expo plugins are active, platform-specific extensions
/// (e.g., `.web.tsx`, `.ios.ts`) are prepended to the extension list so that
/// Metro-style platform resolution works correctly. User-supplied
/// `extra_conditions` are prepended to the resolver's `condition_names`
/// list, giving them priority over baseline conditions during package.json
/// `exports` / `imports` matching.
pub(super) fn create_resolver(active_plugins: &[String], extra_conditions: &[String]) -> Resolver {
    let mut options = ResolveOptions {
        extensions: build_extensions(active_plugins),
        // Support TypeScript's node16/nodenext module resolution where .ts files
        // are imported with .js extensions (e.g., `import './api.js'` for `api.ts`).
        extension_alias: vec![
            (
                ".js".into(),
                vec![".ts".into(), ".tsx".into(), ".js".into()],
            ),
            (".jsx".into(), vec![".tsx".into(), ".jsx".into()]),
            (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
            (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
        ],
        condition_names: build_condition_names(active_plugins, extra_conditions),
        main_fields: vec!["module".into(), "main".into()],
        ..Default::default()
    };

    // Always use auto-discovery mode so oxc_resolver finds the nearest tsconfig.json
    // for each file. This is critical for monorepos where workspace packages have
    // their own tsconfig with path aliases (e.g., `~/*` → `./src/*`). Manual mode
    // with a root tsconfig only uses that single tsconfig's paths for ALL files,
    // missing workspace-specific aliases. Auto mode walks up from each file to find
    // the nearest tsconfig.json and follows `extends` chains, so workspace tsconfigs
    // that extend a root tsconfig still inherit root-level paths.
    options.tsconfig = Some(oxc_resolver::TsconfigDiscovery::Auto);

    Resolver::new(options)
}

/// Return `true` for errors raised while loading a tsconfig file (as opposed to
/// errors about the specifier itself). When `resolve_file` fails with one of these,
/// a broken sibling tsconfig is poisoning resolution for the current file — retrying
/// via `resolve(dir, specifier)` bypasses `TsconfigDiscovery::Auto` and restores
/// resolution for everything that does not need path aliases (relative, absolute,
/// bare package specifiers).
///
/// `IOError` and `Json` are included because a malformed or unreadable tsconfig
/// surfaces as one of these — the variants are shared with package.json parsing,
/// but a retry is still safe: if the error really came from the specifier's own
/// resolution, `resolve()` will fail the same way and we fall through to the
/// existing error handling.
const fn is_tsconfig_error(err: &ResolveError) -> bool {
    matches!(
        err,
        ResolveError::TsconfigNotFound(_)
            | ResolveError::TsconfigCircularExtend(_)
            | ResolveError::TsconfigSelfReference(_)
            | ResolveError::Json(_)
            | ResolveError::IOError(_)
    )
}

/// Try `resolve_file` first (honors per-file tsconfig discovery); on a
/// tsconfig-loading failure, retry with `resolve(dir, specifier)` which skips
/// tsconfig entirely. Emits a single `tracing::warn!` per unique error message
/// so users get one actionable hint per broken tsconfig without log spam.
fn resolve_file_with_tsconfig_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Result<Resolution, ResolveError> {
    match ctx.resolver.resolve_file(from_file, specifier) {
        Ok(resolution) => Ok(resolution),
        Err(err) if is_tsconfig_error(&err) => {
            warn_once_tsconfig(ctx, &err);
            let dir = from_file.parent().unwrap_or(from_file);
            ctx.resolver.resolve(dir, specifier)
        }
        Err(err) => Err(err),
    }
}

/// Emit a `tracing::warn!` the first time a given tsconfig error message is
/// observed. The shared `Mutex<FxHashSet<String>>` in the resolver context
/// dedupes across all parallel threads for the lifetime of one analysis run.
fn warn_once_tsconfig(ctx: &ResolveContext<'_>, err: &ResolveError) {
    let message = err.to_string();
    let should_warn = {
        let Ok(mut seen) = ctx.tsconfig_warned.lock() else {
            // Mutex poisoned by a panic on another thread — stay silent rather
            // than poisoning this thread's resolution with another panic.
            return;
        };
        seen.insert(message.clone())
    };
    if should_warn {
        tracing::warn!(
            "Broken tsconfig chain: {message}. Falling back to resolver-less resolution for \
             affected files. Relative and bare imports still work, but tsconfig path aliases \
             (e.g., `@/...`) will not. Fix the extends/references chain to restore alias support."
        );
    }
}

/// Try the SCSS-specific resolution fallbacks in order: local partial,
/// framework-supplied include paths, and `node_modules/`.
///
/// Only applies to `.scss` / `.sass` importers. Returns `None` when the
/// importer is not an SCSS file or when none of the fallbacks produce a hit,
/// so the outer error path continues to the generic alias / bare / workspace
/// fallbacks.
fn try_scss_fallbacks(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> Option<ResolveResult> {
    if !from_file
        .extension()
        .is_some_and(|e| e == "scss" || e == "sass")
    {
        return None;
    }
    // 1. Local partial convention: `@use 'variables'` → `_variables.scss`.
    if let Some(result) = try_scss_partial_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    // 2. Framework-supplied SCSS include paths (Angular's
    //    `stylePreprocessorOptions.includePaths`, Nx equivalent). See #103.
    if let Some(result) = try_scss_include_path_fallback(ctx, from_file, specifier) {
        return Some(result);
    }
    // 3. `node_modules/` search (Sass's own resolution algorithm):
    //    `@import 'bootstrap/scss/functions'` →
    //    `node_modules/bootstrap/scss/_functions.scss`. Returns
    //    `ResolveResult::NpmPackage` so unused-/unlisted-dependency detection
    //    stays accurate. See #125.
    try_scss_node_modules_fallback(ctx, from_file, specifier)
}

/// Resolve a single import specifier to a target.
pub(super) fn resolve_specifier(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
) -> ResolveResult {
    // URL imports (https://, http://, data:) are valid but can't be resolved locally
    if specifier.contains("://") || specifier.starts_with("data:") {
        return ResolveResult::ExternalFile(PathBuf::from(specifier));
    }

    // Root-relative paths (`/src/main.tsx`, `/static/style.css`) are a web
    // convention meaning "relative to the project/workspace root". Vite,
    // Parcel, and other dev servers resolve them this way. In monorepos, each
    // workspace member has its own Vite root, so `site/index.html` referencing
    // `/src/main.tsx` should resolve to `site/src/main.tsx`, not
    // `<monorepo-root>/src/main.tsx`. Use the source file's parent directory as
    // the base, which is correct for both workspace members and single projects.
    //
    // Applied to web-facing source files: HTML, JSX/TSX, and plain JS/TS. The
    // JSX/TSX case covers SSR frameworks like Hono where JSX templates emit
    // `<link href="/static/style.css" />`: these paths cannot be AST-resolved
    // and have the same web-root semantics as HTML. See issue #105 (till's
    // comment). Applied unconditionally to JS/TS too because the JSX visitor
    // emits `ImportInfo` with the raw attribute value, and the file extension
    // after JSX retry may not reflect the original source (`.js` files with
    // JSX still parse as JSX and get their asset refs recorded here).
    if specifier.starts_with('/')
        && from_file.extension().is_some_and(|e| {
            matches!(
                e.to_str(),
                Some("html" | "jsx" | "tsx" | "js" | "ts" | "mjs" | "cjs" | "mts" | "cts")
            )
        })
    {
        let relative = format!(".{specifier}");
        let source_dir = from_file.parent().unwrap_or(ctx.root);
        if let Ok(resolved) = ctx.resolver.resolve(source_dir, &relative) {
            let resolved_path = resolved.path();
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }
            if let Ok(canonical) = dunce::canonicalize(resolved_path) {
                if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                    return ResolveResult::InternalModule(file_id);
                }
                if let Some(fallback) = ctx.canonical_fallback
                    && let Some(file_id) = fallback.get(&canonical)
                {
                    return ResolveResult::InternalModule(file_id);
                }
            }
        }
        // Fall back to project root for non-workspace setups where the source
        // file may be in a subdirectory (e.g., `public/index.html` referencing
        // `/src/main.tsx`, or a Hono JSX layout in `src/` referencing `/static/style.css`).
        if source_dir != ctx.root
            && let Ok(resolved) = ctx.resolver.resolve(ctx.root, &relative)
        {
            let resolved_path = resolved.path();
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }
            if let Ok(canonical) = dunce::canonicalize(resolved_path) {
                if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                    return ResolveResult::InternalModule(file_id);
                }
                if let Some(fallback) = ctx.canonical_fallback
                    && let Some(file_id) = fallback.get(&canonical)
                {
                    return ResolveResult::InternalModule(file_id);
                }
            }
        }
        return ResolveResult::Unresolvable(specifier.to_string());
    }

    // Bare specifier classification (used for fallback logic below).
    let is_bare = is_bare_specifier(specifier);
    let is_alias = is_path_alias(specifier);
    let matches_plugin_alias = ctx
        .path_aliases
        .iter()
        .any(|(prefix, _)| specifier.starts_with(prefix));

    // Use resolve_file instead of resolve so that TsconfigDiscovery::Auto works.
    // oxc_resolver's resolve() ignores Auto tsconfig discovery — only resolve_file()
    // walks up from the importing file to find the nearest tsconfig.json and apply
    // its path aliases (e.g., @/ → src/).
    //
    // If resolve_file returns a tsconfig-related error (e.g., a solution-style
    // tsconfig.json references a sibling with a broken `extends` chain), retry with
    // the directory-only `resolve()` form so a broken sibling config does not poison
    // resolution for files covered by a healthy sibling. See issue #97.
    match resolve_file_with_tsconfig_fallback(ctx, from_file, specifier) {
        Ok(resolved) => {
            let resolved_path = resolved.path();
            // Try raw path lookup first (avoids canonicalize syscall in most cases)
            if let Some(&file_id) = ctx.raw_path_to_id.get(resolved_path) {
                return ResolveResult::InternalModule(file_id);
            }

            // Fast path for bare specifiers resolving to node_modules: if the resolved
            // path is in node_modules (but not pnpm's .pnpm virtual store) and the
            // package is not a workspace package, skip the expensive canonicalize()
            // syscall and go directly to NpmPackage. Workspace packages need the full
            // fallback chain (source fallback, pnpm fallback) to map dist→src.
            // Note: the byte pattern check handles Unix and Windows separators separately.
            // Paths with mixed separators fall through to canonicalize() (perf-only cost).
            if is_bare
                && !resolved_path
                    .as_os_str()
                    .as_encoded_bytes()
                    .windows(7)
                    .any(|w| w == b"/.pnpm/" || w == b"\\.pnpm\\")
                && let Some(pkg_name) = extract_package_name_from_node_modules_path(resolved_path)
                && !ctx.workspace_roots.contains_key(pkg_name.as_str())
            {
                return ResolveResult::NpmPackage(pkg_name);
            }

            // Fall back to canonical path lookup
            match dunce::canonicalize(resolved_path) {
                Ok(canonical) => {
                    if let Some(&file_id) = ctx.path_to_id.get(canonical.as_path()) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(fallback) = ctx.canonical_fallback
                        && let Some(file_id) = fallback.get(&canonical)
                    {
                        // Intra-project symlink: raw path differs from canonical path.
                        // The lazy fallback resolves this without upfront bulk canonicalize.
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) = try_source_fallback(&canonical, ctx.path_to_id) {
                        // Exports map resolved to a built output (e.g., dist/utils.js)
                        // but the source file (e.g., src/utils.ts) is what we track.
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) =
                        try_pnpm_workspace_fallback(&canonical, ctx.path_to_id, ctx.workspace_roots)
                    {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(pkg_name) =
                        extract_package_name_from_node_modules_path(&canonical)
                    {
                        // Workspace package resolved through a node_modules symlink to
                        // a built output (e.g. dist/esm/button/index.js) that has no
                        // src/ mirror. Retry against the workspace root's source tree.
                        // See issue #106.
                        if ctx.workspace_roots.contains_key(pkg_name.as_str())
                            && let Some(result) = try_workspace_package_fallback(ctx, specifier)
                        {
                            return result;
                        }
                        ResolveResult::NpmPackage(pkg_name)
                    } else {
                        ResolveResult::ExternalFile(canonical)
                    }
                }
                Err(_) => {
                    // Path doesn't exist on disk — try source fallback on the raw path
                    if let Some(file_id) = try_source_fallback(resolved_path, ctx.path_to_id) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(file_id) = try_pnpm_workspace_fallback(
                        resolved_path,
                        ctx.path_to_id,
                        ctx.workspace_roots,
                    ) {
                        ResolveResult::InternalModule(file_id)
                    } else if let Some(pkg_name) =
                        extract_package_name_from_node_modules_path(resolved_path)
                    {
                        if ctx.workspace_roots.contains_key(pkg_name.as_str())
                            && let Some(result) = try_workspace_package_fallback(ctx, specifier)
                        {
                            return result;
                        }
                        ResolveResult::NpmPackage(pkg_name)
                    } else {
                        ResolveResult::ExternalFile(resolved_path.to_path_buf())
                    }
                }
            }
        }
        Err(_) => {
            if let Some(result) = try_scss_fallbacks(ctx, from_file, specifier) {
                return result;
            }

            if is_alias || matches_plugin_alias {
                // Try plugin-provided path aliases before giving up.
                // This covers both built-in alias shapes (`~/`, `@/`, `#foo`) and
                // custom prefixes discovered from framework config files such as
                // `@shared/*` or `$utils/*`.
                // Path aliases that fail resolution are unresolvable, not npm packages.
                // Classifying them as NpmPackage would cause false "unlisted dependency" reports.
                try_path_alias_fallback(ctx, specifier)
                    .unwrap_or_else(|| ResolveResult::Unresolvable(specifier.to_string()))
            } else if is_bare && is_valid_package_name(specifier) {
                // Workspace package fallback: self-referencing and cross-workspace
                // imports without node_modules symlinks. Resolves `@org/pkg/sub`
                // against the workspace root's source tree. See issue #106.
                if let Some(result) = try_workspace_package_fallback(ctx, specifier) {
                    return result;
                }
                let pkg_name = extract_package_name(specifier);
                ResolveResult::NpmPackage(pkg_name)
            } else {
                ResolveResult::Unresolvable(specifier.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use oxc_resolver::{JSONError, ResolveError};

    use super::is_tsconfig_error;

    #[test]
    fn tsconfig_not_found_is_tsconfig_error() {
        assert!(is_tsconfig_error(&ResolveError::TsconfigNotFound(
            PathBuf::from("/nonexistent/tsconfig.json")
        )));
    }

    #[test]
    fn tsconfig_self_reference_is_tsconfig_error() {
        assert!(is_tsconfig_error(&ResolveError::TsconfigSelfReference(
            PathBuf::from("/project/tsconfig.json")
        )));
    }

    // `TsconfigCircularExtend(CircularPathBufs)` is part of the matched set but
    // cannot be directly unit-tested: `CircularPathBufs` is not re-exported from
    // `oxc_resolver::lib`, so external crates cannot construct the variant. The
    // `matches!` arm is structural, so adding the variant to `is_tsconfig_error`
    // is guaranteed by the compiler to return `true` regardless of payload.

    #[test]
    fn io_error_is_tsconfig_error() {
        // An IO error (permission denied while reading a tsconfig) must trigger
        // the fallback. The variant is shared with non-tsconfig IO failures, but
        // the retry via `resolve()` is safe in either case.
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert!(is_tsconfig_error(&ResolveError::from(io_err)));
    }

    #[test]
    fn json_error_is_tsconfig_error() {
        // Malformed tsconfig JSON surfaces as ResolveError::Json. Same variant
        // covers malformed package.json; retry via `resolve()` is safe there too.
        assert!(is_tsconfig_error(&ResolveError::Json(JSONError {
            path: PathBuf::from("/project/tsconfig.json"),
            message: "unexpected token".to_string(),
            line: 1,
            column: 1,
        })));
    }

    #[test]
    fn module_not_found_is_not_tsconfig_error() {
        // Regular "module not found" must NOT trigger the fallback —
        // the tsconfig loaded fine, the specifier just doesn't exist.
        assert!(!is_tsconfig_error(&ResolveError::NotFound(
            "./missing-module".to_string()
        )));
    }

    #[test]
    fn ignored_is_not_tsconfig_error() {
        assert!(!is_tsconfig_error(&ResolveError::Ignored(PathBuf::from(
            "/ignored"
        ))));
    }
}
