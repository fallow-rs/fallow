//! One `oxc_resolver` file resolution with the fallbacks that keep it usable:
//! a retry without tsconfig after a tsconfig load error, and a retry without
//! Metro platform extensions after a platform self-resolution.

use std::path::Path;

use oxc_resolver::{Resolution, ResolveError, Resolver};

use super::react_native::{may_be_platform_self_resolution, without_platform_extensions};
use super::specifier::{is_style_file, same_path};
use super::types::ResolveContext;

/// Return `true` for errors raised while loading a tsconfig file (as opposed to
/// errors about the specifier itself). When `resolve_file` fails with one of these,
/// a broken sibling tsconfig is poisoning resolution for the current file, retrying
/// via `resolve(dir, specifier)` bypasses `TsconfigDiscovery::Auto` and restores
/// resolution for everything that does not need path aliases (relative, absolute,
/// bare package specifiers).
///
/// `IOError` and `Json` are included because a malformed or unreadable tsconfig
/// surfaces as one of these. The variants are shared with package.json parsing,
/// but a retry is still safe: if the error really came from the specifier's own
/// resolution, `resolve()` will fail the same way and we fall through to the
/// existing error handling.
pub(super) const fn is_tsconfig_error(err: &ResolveError) -> bool {
    matches!(
        err,
        ResolveError::TsconfigNotFound(_)
            | ResolveError::TsconfigCircularExtend(_)
            | ResolveError::TsconfigSelfReference(_)
            | ResolveError::Json(_)
            | ResolveError::IOError(_)
    )
}

pub(super) enum ResolveFileAttempt {
    Resolved {
        resolution: Resolution,
        used_tsconfig_fallback: bool,
    },
    Failed {
        used_tsconfig_fallback: bool,
    },
}

/// Try `resolve_file` first (honors per-file tsconfig discovery); on a
/// tsconfig-loading failure, retry with `resolve(dir, specifier)` which skips
/// tsconfig entirely. Emits a single `tracing::warn!` per unique error message
/// so users get one actionable hint per broken tsconfig without log spam.
pub(super) fn resolve_file_with_tsconfig_fallback(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    from_style: bool,
) -> ResolveFileAttempt {
    if from_style || is_style_file(from_file) {
        return resolve_file_with_resolver_and_tsconfig_fallback(
            ctx,
            ctx.resolver,
            from_file,
            specifier,
        );
    }
    // Most Node-protocol requests resolve externally. One broad attempt
    // preserves that path and tsconfig remaps, including implicit assets.
    // Only successful remaps need the script lookup for directory priority.
    if specifier.starts_with("node:") {
        let broad = resolve_file_with_resolver_and_tsconfig_fallback(
            ctx,
            ctx.resolver,
            from_file,
            specifier,
        );
        if matches!(broad, ResolveFileAttempt::Failed { .. }) {
            return broad;
        }
        let attempt = resolve_file_with_resolver_and_tsconfig_fallback(
            ctx,
            ctx.script_resolver,
            from_file,
            specifier,
        );
        if matches!(attempt, ResolveFileAttempt::Resolved { .. }) {
            return attempt;
        }
        return broad;
    }
    let attempt = resolve_file_with_resolver_and_tsconfig_fallback(
        ctx,
        ctx.script_resolver,
        from_file,
        specifier,
    );
    if matches!(attempt, ResolveFileAttempt::Resolved { .. }) {
        return attempt;
    }
    resolve_file_with_resolver_and_tsconfig_fallback(ctx, ctx.resolver, from_file, specifier)
}

pub(super) fn resolve_file_with_resolver_and_tsconfig_fallback(
    ctx: &ResolveContext<'_>,
    resolver: &Resolver,
    from_file: &Path,
    specifier: &str,
) -> ResolveFileAttempt {
    super::work::note_oxc_resolve();
    match resolver.resolve_file(from_file, specifier) {
        Ok(resolution) => ResolveFileAttempt::Resolved {
            resolution,
            used_tsconfig_fallback: false,
        },
        Err(err) if is_tsconfig_error(&err) => {
            warn_once_tsconfig(ctx, &err);
            let dir = from_file.parent().unwrap_or(from_file);
            super::work::note_oxc_resolve();
            match resolver.resolve(dir, specifier) {
                Ok(resolution) => ResolveFileAttempt::Resolved {
                    resolution,
                    used_tsconfig_fallback: true,
                },
                Err(_) => ResolveFileAttempt::Failed {
                    used_tsconfig_fallback: true,
                },
            }
        }
        Err(_) => ResolveFileAttempt::Failed {
            used_tsconfig_fallback: false,
        },
    }
}

/// Warn the first time a tsconfig error message is seen. The shared set in the
/// resolver context dedupes across all threads for one analysis run.
fn warn_once_tsconfig(ctx: &ResolveContext<'_>, err: &ResolveError) {
    let message = err.to_string();
    let should_warn = {
        let Ok(mut seen) = ctx.tsconfig_warned.lock() else {
            return;
        };
        seen.insert(message.clone())
    };
    if should_warn {
        tracing::warn!(
            target: fallow_types::ANALYSIS_DIAGNOSTIC_LOG_TARGET,
            "tsconfig chain not fully loaded: {message}. Falling back to resolver-less \
             resolution for the affected files. Relative imports, bare imports, and path \
             aliases declared in a discovered root or workspace tsconfig still resolve (fallow \
             applies those project-wide), so this is usually harmless and is common when \
             analyzing before dependencies are installed. Only aliases declared solely in the \
             unreadable inherited config are affected: if imports look misreported, install \
             dependencies so the referenced config resolves, or fix the extends/references \
             chain."
        );
    }
}

/// Resolve a specifier again without Metro platform extensions when the
/// platform-extension resolution returned the importing file itself.
///
/// `UserMenu.ios.tsx` often re-exports the types of `UserMenu.tsx` with
/// `export type { Props } from "./UserMenu"`. Metro platform extensions turn
/// `./UserMenu` into `UserMenu.ios.tsx`, which is a self-import that the
/// author did not intend. TypeScript without `moduleSuffixes` resolves the
/// specifier to the base file, so the retry does the same. When no base
/// module exists, the original resolution stays.
pub(super) fn skip_platform_self_resolution(
    ctx: &ResolveContext<'_>,
    from_file: &Path,
    specifier: &str,
    resolution: Resolution,
    used_tsconfig_fallback: bool,
) -> Resolution {
    if !may_be_platform_self_resolution(from_file, specifier, resolution.path())
        || !same_path(from_file, resolution.path())
    {
        return resolution;
    }
    let resolver = ctx
        .script_resolver
        .clone_with_options(without_platform_extensions(
            ctx.script_resolver.options().clone(),
        ));
    super::work::note_oxc_resolve();
    let retry = if used_tsconfig_fallback {
        resolver.resolve(from_file.parent().unwrap_or(from_file), specifier)
    } else {
        resolver.resolve_file(from_file, specifier)
    };
    retry.unwrap_or(resolution)
}
