//! Match file paths against the `files`, `include` and `exclude` entries of a
//! tsconfig, relative to the directory of that tsconfig.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use serde_json::Value;

use super::fallbacks::normalize_path_lexically;

pub(super) fn glob_values_match(base_dir: &Path, values: &[Value], path: &Path) -> bool {
    let mut builder = GlobSetBuilder::new();
    let mut has_patterns = false;
    for value in values.iter().filter_map(Value::as_str) {
        let Ok(glob) = Glob::new(&tsconfig_entry_glob(base_dir, value)) else {
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
/// directory such as `app/[id]` then matches only itself.
fn tsconfig_entry_glob(base_dir: &Path, value: &str) -> String {
    let glob_start = value
        .find(['*', '?'])
        .map(|wildcard| value[..wildcard].rfind('/').map_or(0, |slash| slash + 1));
    let (literal, rest) = value.split_at(glob_start.unwrap_or(value.len()));
    let literal = resolve_tsconfig_relative_path(base_dir, literal);
    let rest = if glob_start.is_none() && literal.is_dir() {
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
pub(super) fn resolve_tsconfig_relative_path(base_dir: &Path, path: &str) -> PathBuf {
    normalize_path_lexically(&base_dir.join(path))
}
