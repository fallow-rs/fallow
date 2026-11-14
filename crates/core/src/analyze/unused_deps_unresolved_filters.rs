//! The checks that silence an unresolvable specifier in
//! `find_unresolved_imports`.

use std::path::Path;

use fallow_config::ResolvedConfig;

use super::super::predicates::{is_builtin_module, is_virtual_module};
use super::matches_virtual_prefix;
use crate::plugins::CheckedAliasPrefix;

/// Plumbing for the per-spec skip checks in `find_unresolved_imports`.
pub(super) struct UnresolvedImportFilters<'a> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) virtual_prefixes: &'a [&'a str],
    pub(super) generated_patterns: &'a [&'a str],
    pub(super) generated_type_prefixes: &'a [&'a str],
    pub(super) checked_alias_prefixes: &'a [CheckedAliasPrefix],
}

/// Whether a virtual module prefix of a plugin covers `spec` in `from_file`.
///
/// A checked alias points at a directory that a config names, so a missing
/// target under it is a broken import. Such a specifier is not virtual, also
/// when a broad virtual prefix such as the Nuxt `#` matches it.
fn is_plugin_virtual_spec(
    spec: &str,
    from_file: &Path,
    filters: &UnresolvedImportFilters<'_>,
) -> bool {
    filters
        .virtual_prefixes
        .iter()
        .any(|prefix| matches_virtual_prefix(prefix, spec))
        && !filters
            .checked_alias_prefixes
            .iter()
            .any(|checked| checked.covers(&filters.config.root, from_file, spec))
}

/// Return `true` when an unresolvable specifier should be silenced (builtin,
/// virtual, generated artifact, type-only generated prefix, or config-ignored).
pub(super) fn unresolved_spec_is_silenced(
    spec: &str,
    from_file: &Path,
    is_type_only: bool,
    filters: &UnresolvedImportFilters<'_>,
) -> bool {
    if is_builtin_module(spec) || is_virtual_module(spec) {
        return true;
    }
    if is_plugin_virtual_spec(spec, from_file, filters) {
        return true;
    }
    if !filters.generated_patterns.is_empty() {
        let bare = spec
            .strip_suffix(".js")
            .or_else(|| spec.strip_suffix(".ts"))
            .unwrap_or(spec);
        if filters
            .generated_patterns
            .iter()
            .any(|pat| bare.ends_with(pat))
        {
            return true;
        }
    }
    if is_type_only
        && filters
            .generated_type_prefixes
            .iter()
            .any(|prefix| spec.starts_with(prefix))
    {
        return true;
    }
    // Config compilation strips a single leading "./" from
    // ignoreUnresolvedImports globs (see #1385), so the specifier must be
    // stripped the same way or exact-path entries like "./generated/x.js"
    // never match. The raw form is still tried so wildcard patterns that
    // count the "./" segment keep matching.
    let normalized_spec = spec.strip_prefix("./").unwrap_or(spec);
    filters
        .config
        .ignore_unresolved_imports
        .iter()
        .any(|matcher| matcher.is_match(spec) || matcher.is_match(normalized_spec))
}
