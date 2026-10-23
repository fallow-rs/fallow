//! Sass load-path lookups: the file index lookup and the plugin path aliases.

use std::path::Path;

use fallow_types::discover::FileId;

use super::{ResolveContext, alias_match_remainder, find_scss_in_dir};

/// Look up an absolute candidate path in the file index, falling back to
/// canonical path lookup for intra-project symlinks.
pub(super) fn lookup_scss_path(candidate: &Path, ctx: &ResolveContext<'_>) -> Option<FileId> {
    if let Some(&file_id) = ctx.raw_path_to_id.get(candidate) {
        return Some(file_id);
    }
    if let Some(canonical) = ctx.canonicalize_cache.get(candidate) {
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

/// Resolve a bare Sass specifier through a plugin path alias, such as a Vite
/// `resolve.alias` entry `src -> <root>/src` for `@use "src/styles/tokens"`.
///
/// Vite's Sass importer runs the URL through the Vite resolver after Sass
/// fails to load it relative to the file, so the alias applies with the Sass
/// partial and directory-index conventions.
pub(super) fn find_scss_through_path_alias(ctx: &ResolveContext<'_>, bare: &str) -> Option<FileId> {
    ctx.path_aliases.iter().find_map(|(prefix, replacement)| {
        let remainder = alias_match_remainder(bare, prefix)?.trim_start_matches('/');
        let target_dir = ctx.root.join(replacement);
        if remainder.is_empty() {
            return lookup_scss_path(&target_dir, ctx);
        }
        find_scss_in_dir(&target_dir, remainder, ctx)
    })
}
