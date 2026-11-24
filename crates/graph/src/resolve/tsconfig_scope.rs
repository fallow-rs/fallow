//! Match file paths against the `files`, `include` and `exclude` entries of a
//! tsconfig, relative to the directory of that tsconfig.

use std::path::{Path, PathBuf};

use globset::GlobSetBuilder;
use serde_json::Value;

use fallow_config::{TsconfigFileSet, TsconfigPattern};

use super::fallbacks::normalize_path_lexically;
use super::specifier::{merged_tsconfig_cached, same_path};
use super::types::ResolveContext;
use crate::read_trace::TracedPath;

pub(super) fn glob_values_match(base_dir: &Path, values: &[Value], path: &Path) -> bool {
    let mut builder = GlobSetBuilder::new();
    let mut has_patterns = false;
    for value in values.iter().filter_map(Value::as_str) {
        let Ok(glob) = fallow_config::compile_path_glob(&tsconfig_entry_glob(base_dir, value))
        else {
            continue;
        };
        builder.add(glob);
        has_patterns = true;
    }
    has_patterns && builder.build().is_ok_and(|set| set.is_match(path))
}

/// Build the glob for one `include` or `exclude` entry. tsc reads only `*`
/// and `?` as wildcards, so the tsconfig directory, the entry segments before
/// the first wildcard and any bracket or brace are literal path text. A
/// directory such as `app/[id]` then matches only itself. tsc keeps `*` and
/// `?` inside one path segment, and only `**/` crosses directories.
fn tsconfig_entry_glob(base_dir: &Path, value: &str) -> String {
    let glob_start = value
        .find(['*', '?'])
        .map(|wildcard| value[..wildcard].rfind('/').map_or(0, |slash| slash + 1));
    let (literal, rest) = value.split_at(glob_start.unwrap_or(value.len()));
    let literal = resolve_tsconfig_relative_path(base_dir, literal);
    let rest = if glob_start.is_none() && literal.traced_is_dir() {
        "**/*"
    } else {
        rest
    };
    let mut glob = globset::escape(&literal.to_string_lossy().replace('\\', "/"));
    if !rest.is_empty() && !glob.ends_with('/') {
        glob.push('/');
    }
    for symbol in rest.chars() {
        if matches!(symbol, '[' | ']' | '{' | '}') {
            glob.extend(['[', symbol, ']']);
        } else {
            glob.push(symbol);
        }
    }
    glob
}

/// Join a tsconfig `files`, `include` or `exclude` entry to its directory
/// without `./` and `..`, as the globs match path text (issue #3946).
fn resolve_tsconfig_relative_path(base_dir: &Path, path: &str) -> PathBuf {
    normalize_path_lexically(&base_dir.join(path))
}

/// True when the tsconfig at `tsconfig_path`, after its `extends` chain,
/// contains `from_file` through `files`, `include` and `exclude`.
pub(super) fn tsconfig_applies_to_file(
    ctx: &ResolveContext<'_>,
    tsconfig_path: &Path,
    from_file: &Path,
) -> bool {
    let root = ctx.root;
    let Some(merged) = merged_tsconfig_cached(ctx, tsconfig_path) else {
        return false;
    };
    // `references` paths are joined verbatim, so a folder can carry `./`
    // and `..` segments that a normalized file path never has.
    let tsconfig_dir = normalize_path_lexically(tsconfig_path.parent().unwrap_or(root));
    // `files` and `include` add up, as in TypeScript. A config with `files`
    // and no `include` contains only the listed files. An `exclude` does not
    // remove a listed file.
    let files = merged.file_set(TsconfigFileSet::Files);
    if files.is_some_and(|files| {
        let base_dir = normalize_path_lexically(files.origin());
        files
            .strings()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|file| merged.resolve_path(file, &base_dir))
            .any(|file| same_path(&file, from_file))
    }) {
        return true;
    }

    let patterns_match = |patterns: &[TsconfigPattern]| {
        patterns.iter().any(|pattern| {
            let value = [Value::String(pattern.pattern.clone())];
            glob_values_match(&normalize_path_lexically(&pattern.base), &value, from_file)
        })
    };
    let include_matches = match merged.file_set_patterns(TsconfigFileSet::Include) {
        Some(include) => patterns_match(&include),
        None => files.is_none() && from_file.starts_with(&tsconfig_dir),
    };
    if !include_matches {
        return false;
    }

    !merged
        .file_set_patterns(TsconfigFileSet::Exclude)
        .is_some_and(|exclude| patterns_match(&exclude))
}
