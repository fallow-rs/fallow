use std::path::{Path, PathBuf};

use super::diagnostics::{
    WorkspaceDiagnostic, WorkspaceDiagnosticKind, is_ignored_workspace_dir, is_skip_listed_dir,
};

/// Parse `tsconfig.json` at the project root and extract `references[].path` directories.
///
/// Returns directories that exist on disk. tsconfig.json is JSONC (comments + trailing commas).
///
/// Test-only wrapper around [`parse_tsconfig_references_with_diagnostics`] that drops
/// any emitted diagnostics. Production callers use the diagnostics-aware variant.
#[cfg(test)]
pub(super) fn parse_tsconfig_references(root: &Path) -> Vec<PathBuf> {
    let mut diagnostics = Vec::new();
    parse_tsconfig_references_with_diagnostics(root, &globset::GlobSet::empty(), &mut diagnostics)
}

/// Parse `tsconfig.json` at the project root and extract `references[].path` directories,
/// surfacing parse errors and missing reference directories as workspace diagnostics.
///
/// Severity policy (mirrors what tsc itself does):
/// - `tsconfig.json` missing: silent (many JS-only projects have none).
/// - `tsconfig.json` exists but fails to parse as JSONC: emit
///   [`WorkspaceDiagnosticKind::MalformedTsconfig`].
/// - `references[].path` points to a directory that does not exist: emit
///   [`WorkspaceDiagnosticKind::TsconfigReferenceDirMissing`], filtered through
///   `ignore_patterns` so user-excluded paths stay quiet.
pub(super) fn parse_tsconfig_references_with_diagnostics(
    root: &Path,
    ignore_patterns: &globset::GlobSet,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) -> Vec<PathBuf> {
    let tsconfig_path = root.join("tsconfig.json");
    let Ok(content) = std::fs::read_to_string(&tsconfig_path) else {
        // Missing tsconfig is not an error. Many JS-only projects have none.
        return Vec::new();
    };

    // Strip UTF-8 BOM if present (common in Windows-authored tsconfig files)
    let content = content.trim_start_matches('\u{FEFF}');

    let value: serde_json::Value = match crate::jsonc::parse_to_value(content) {
        Ok(v) => v,
        Err(error) => {
            let diag = WorkspaceDiagnostic::new(
                root,
                tsconfig_path,
                WorkspaceDiagnosticKind::MalformedTsconfig {
                    error: error.to_string(),
                },
            );
            diagnostics.push(diag);
            return Vec::new();
        }
    };

    let Some(refs) = value.get("references").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for r in refs {
        let Some(raw_path) = r.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        // strip_prefix removes exactly one leading "./" (unlike trim_start_matches
        // which would strip repeatedly).
        let cleaned = raw_path.strip_prefix("./").unwrap_or(raw_path);
        let candidate = root.join(cleaned);
        if candidate.is_dir() {
            results.push(candidate);
            continue;
        }

        // Reference points to a missing directory. Filter through
        // ignore_patterns so paths the user already excluded do not trigger
        // a redundant diagnostic.
        let relative = candidate
            .strip_prefix(root)
            .unwrap_or(candidate.as_path())
            .to_path_buf();
        if is_ignored_workspace_dir(&relative, ignore_patterns) {
            continue;
        }

        let diag = WorkspaceDiagnostic::new(
            root,
            candidate,
            WorkspaceDiagnosticKind::TsconfigReferenceDirMissing,
        );
        diagnostics.push(diag);
    }
    results
}

/// Parse `tsconfig.json` at the project root and extract `compilerOptions.rootDir`.
///
/// Returns `None` if the file is missing, malformed, or has no `rootDir` set.
pub fn parse_tsconfig_root_dir(root: &Path) -> Option<String> {
    let tsconfig_path = root.join("tsconfig.json");
    let content = std::fs::read_to_string(&tsconfig_path).ok()?;
    let content = content.trim_start_matches('\u{FEFF}');

    let value: serde_json::Value = crate::jsonc::parse_to_value(content).ok()?;

    value
        .get("compilerOptions")
        .and_then(|opts| opts.get("rootDir"))
        .and_then(|v| v.as_str())
        .map(|s| {
            s.strip_prefix("./")
                .unwrap_or(s)
                .trim_end_matches('/')
                .to_owned()
        })
}

/// Strip trailing commas before `]` and `}` in JSON-like content.
///
/// tsconfig.json commonly uses trailing commas which are valid JSONC but not valid JSON.
/// This strips them so `serde_json` can parse the content.
#[cfg(test)]
pub(super) fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut result = Vec::with_capacity(len);
    let mut in_string = false;
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        if in_string {
            result.push(b);
            if b == b'\\' && i + 1 < len {
                // Push escaped character and skip it
                i += 1;
                result.push(bytes[i]);
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if b == b'"' {
            in_string = true;
            result.push(b);
            i += 1;
            continue;
        }

        if b == b',' {
            // Look ahead past whitespace for ] or }
            let mut j = i + 1;
            while j < len && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && (bytes[j] == b']' || bytes[j] == b'}') {
                // Skip the trailing comma
                i += 1;
                continue;
            }
        }

        result.push(b);
        i += 1;
    }

    // We only removed ASCII commas and preserved all other bytes unchanged,
    // so the result is valid UTF-8 if the input was. Use from_utf8 to be safe.
    String::from_utf8(result).unwrap_or_else(|_| input.to_string())
}

/// Expand a workspace glob pattern to matching directories.
///
/// Returns `(original_path, canonical_path)` tuples so callers can skip redundant
/// `canonicalize()` calls. Only directories containing a `package.json` are
/// canonicalized; this avoids expensive syscalls on the many non-workspace
/// directories that globs like `packages/*` or `**` can match.
///
/// `canonical_root` is pre-computed to avoid repeated `canonicalize()` syscalls.
///
/// Test-only wrapper around [`expand_workspace_glob_with_diagnostics`] that
/// drops any glob-matched-no-package.json diagnostics. Production callers use
/// the diagnostics-aware variant.
#[cfg(test)]
pub(super) fn expand_workspace_glob(
    root: &Path,
    pattern: &str,
    canonical_root: &Path,
) -> Vec<(PathBuf, PathBuf)> {
    let mut diagnostics = Vec::new();
    expand_workspace_glob_with_diagnostics(
        root,
        pattern,
        pattern,
        canonical_root,
        &globset::GlobSet::empty(),
        &mut diagnostics,
    )
}

/// Diagnostics-aware variant of `expand_workspace_glob` (the test-only
/// back-compat wrapper above).
///
/// Emits [`WorkspaceDiagnosticKind::GlobMatchedNoPackageJson`] when a glob match
/// resolves to a directory that contains no `package.json`, with two filters
/// applied first:
/// 1. The directory's leaf name is checked against [`is_skip_listed_dir`]
///    (build artifacts, tooling caches, hidden directories). pnpm/npm/yarn
///    silently filter the same set; fallow follows suit.
/// 2. The project-root-relative path is checked against `ignore_patterns`.
///    User-excluded paths produce no diagnostic.
///
/// `raw_pattern` is the user-supplied glob (e.g. `packages/*`) and goes into the
/// diagnostic's message; `expanded_pattern` is the normalized glob string used
/// for matching (e.g. `packages/*` after trailing-slash expansion).
pub(super) fn expand_workspace_glob_with_diagnostics(
    root: &Path,
    raw_pattern: &str,
    expanded_pattern: &str,
    canonical_root: &Path,
    ignore_patterns: &globset::GlobSet,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) -> Vec<(PathBuf, PathBuf)> {
    // For patterns with `**`, use a manual walk that prunes node_modules
    // during traversal. The glob crate walks into node_modules before
    // filtering, which is catastrophic with pnpm's deep symlink trees
    // (50,000+ entries for `packages/**/*` in starlight).
    if expanded_pattern.contains("**") {
        return expand_recursive_workspace_pattern(
            root,
            raw_pattern,
            expanded_pattern,
            canonical_root,
            ignore_patterns,
            diagnostics,
        );
    }

    let full_pattern = root.join(expanded_pattern).to_string_lossy().to_string();
    match glob::glob(&full_pattern) {
        Ok(paths) => {
            let mut results = Vec::new();
            for path in paths.filter_map(Result::ok) {
                if !path.is_dir() {
                    continue;
                }
                if path.components().any(|c| c.as_os_str() == "node_modules") {
                    continue;
                }
                if path.join("package.json").exists() {
                    if let Some(cp) = dunce::canonicalize(&path)
                        .ok()
                        .filter(|cp| cp.starts_with(canonical_root))
                    {
                        results.push((path, cp));
                    }
                    continue;
                }
                maybe_emit_glob_no_pkg_diag(root, raw_pattern, &path, ignore_patterns, diagnostics);
            }
            results
        }
        Err(e) => {
            tracing::warn!("invalid workspace glob pattern '{raw_pattern}': {e}");
            Vec::new()
        }
    }
}

/// Emit a `glob-matched-no-package-json` diagnostic if the path is neither
/// in the conventional skip list nor in the user `ignorePatterns`.
///
/// Path normalisation: macOS canonicalises `/tmp/<repo>` to
/// `/private/tmp/<repo>`. If `root` was supplied as the canonical form (the
/// CLI prints `/private/...` in `loaded config:` confirming this) but the
/// `glob::glob` paths use the symlinked `/tmp/...` form, a naive
/// `path.strip_prefix(root)` falls through to the full absolute path and the
/// `ignorePatterns` check misses. Canonicalise both before stripping so the
/// suppression contract holds end-to-end.
fn maybe_emit_glob_no_pkg_diag(
    root: &Path,
    raw_pattern: &str,
    path: &Path,
    ignore_patterns: &globset::GlobSet,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) {
    let leaf = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if is_skip_listed_dir(&leaf) {
        return;
    }
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let canonical_path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical_path
        .strip_prefix(&canonical_root)
        .unwrap_or(canonical_path.as_path())
        .to_path_buf();
    if is_ignored_workspace_dir(&relative, ignore_patterns) {
        return;
    }
    let diag = WorkspaceDiagnostic::new(
        root,
        path.to_path_buf(),
        WorkspaceDiagnosticKind::GlobMatchedNoPackageJson {
            pattern: raw_pattern.to_string(),
        },
    );
    diagnostics.push(diag);
}

/// Expand a recursive workspace glob pattern (containing `**`) by walking the
/// directory tree manually, pruning `node_modules` during traversal.
///
/// This avoids the `glob` crate's O(n) expansion where n includes all files
/// inside `node_modules/` (catastrophic with pnpm's deep symlink trees).
fn expand_recursive_workspace_pattern(
    root: &Path,
    raw_pattern: &str,
    expanded_pattern: &str,
    canonical_root: &Path,
    ignore_patterns: &globset::GlobSet,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) -> Vec<(PathBuf, PathBuf)> {
    let full_pattern = root.join(expanded_pattern).to_string_lossy().to_string();
    let Ok(matcher) = glob::Pattern::new(&full_pattern) else {
        tracing::warn!("invalid workspace glob pattern '{raw_pattern}'");
        return Vec::new();
    };

    // Extract the base directory before the first `*` to avoid scanning from root
    let base_dir = match expanded_pattern.find('*') {
        Some(idx) => root.join(&expanded_pattern[..idx]),
        None => root.join(expanded_pattern),
    };

    let mut results = Vec::new();
    walk_workspace_dirs(
        root,
        &base_dir,
        raw_pattern,
        &matcher,
        canonical_root,
        ignore_patterns,
        &mut results,
        diagnostics,
    );
    results
}

/// Recursively walk directories, skipping `node_modules` and `.git`, collecting
/// directories that match the glob pattern and contain a `package.json`.
///
/// Glob-matched directories without `package.json` are surfaced as
/// `glob-matched-no-package-json` diagnostics unless they are in the
/// conventional skip list or covered by `ignore_patterns`.
#[expect(
    clippy::too_many_arguments,
    reason = "internal recursion that threads diagnostic accumulator + ignore globset; refactoring into a context struct would obscure the recursive call site"
)]
fn walk_workspace_dirs(
    root: &Path,
    dir: &Path,
    raw_pattern: &str,
    matcher: &glob::Pattern,
    canonical_root: &Path,
    ignore_patterns: &globset::GlobSet,
    results: &mut Vec<(PathBuf, PathBuf)>,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        // Prune node_modules and hidden directories during traversal
        if name == "node_modules" || name == ".git" {
            continue;
        }
        // Check if this directory matches the pattern.
        if matcher.matches_path(&path) {
            if path.join("package.json").exists() {
                if let Ok(cp) = dunce::canonicalize(&path)
                    && cp.starts_with(canonical_root)
                {
                    results.push((path.clone(), cp));
                }
            } else {
                maybe_emit_glob_no_pkg_diag(root, raw_pattern, &path, ignore_patterns, diagnostics);
            }
        }
        // Continue recursing into subdirectories
        walk_workspace_dirs(
            root,
            &path,
            raw_pattern,
            matcher,
            canonical_root,
            ignore_patterns,
            results,
            diagnostics,
        );
    }
}

/// Parse pnpm-workspace.yaml to extract package patterns.
pub(super) fn parse_pnpm_workspace_yaml(content: &str) -> Vec<String> {
    // Simple YAML parsing for the common format:
    // packages:
    //   - 'packages/*'
    //   - 'apps/*'
    let mut patterns = Vec::new();
    let mut in_packages = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if trimmed.starts_with("- ") {
                let value = trimmed
                    .strip_prefix("- ")
                    .unwrap_or(trimmed)
                    .trim_matches('\'')
                    .trim_matches('"');
                patterns.push(value.to_string());
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                break; // New top-level key
            }
        }
    }

    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pnpm_workspace_basic() {
        let yaml = "packages:\n  - 'packages/*'\n  - 'apps/*'\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_double_quotes() {
        let yaml = "packages:\n  - \"packages/*\"\n  - \"apps/*\"\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_no_quotes() {
        let yaml = "packages:\n  - packages/*\n  - apps/*\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_empty() {
        let yaml = "";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert!(patterns.is_empty());
    }

    #[test]
    fn parse_pnpm_workspace_no_packages_key() {
        let yaml = "other:\n  - something\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert!(patterns.is_empty());
    }

    #[test]
    fn parse_pnpm_workspace_with_comments() {
        let yaml = "packages:\n  # Comment\n  - 'packages/*'\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_stops_at_next_key() {
        let yaml = "packages:\n  - 'packages/*'\ncatalog:\n  react: ^18\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*"]);
    }

    #[test]
    fn strip_trailing_commas_basic() {
        assert_eq!(
            strip_trailing_commas(r#"{"a": 1, "b": 2,}"#),
            r#"{"a": 1, "b": 2}"#
        );
    }

    #[test]
    fn strip_trailing_commas_array() {
        assert_eq!(strip_trailing_commas(r"[1, 2, 3,]"), r"[1, 2, 3]");
    }

    #[test]
    fn strip_trailing_commas_with_whitespace() {
        assert_eq!(
            strip_trailing_commas("{\n  \"a\": 1,\n}"),
            "{\n  \"a\": 1\n}"
        );
    }

    #[test]
    fn strip_trailing_commas_preserves_strings() {
        // Commas inside strings should NOT be stripped
        assert_eq!(
            strip_trailing_commas(r#"{"a": "hello,}"}"#),
            r#"{"a": "hello,}"}"#
        );
    }

    #[test]
    fn strip_trailing_commas_nested() {
        let input = r#"{"refs": [{"path": "./a",}, {"path": "./b",},],}"#;
        let expected = r#"{"refs": [{"path": "./a"}, {"path": "./b"}]}"#;
        assert_eq!(strip_trailing_commas(input), expected);
    }

    #[test]
    fn strip_trailing_commas_escaped_quotes() {
        assert_eq!(
            strip_trailing_commas(r#"{"a": "he\"llo,}",}"#),
            r#"{"a": "he\"llo,}"}"#
        );
    }

    #[test]
    fn tsconfig_references_from_dir() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-refs");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/core")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/ui")).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{
                // Root tsconfig with project references
                "references": [
                    {"path": "./packages/core"},
                    {"path": "./packages/ui"},
                ],
            }"#,
        )
        .unwrap();

        let refs = parse_tsconfig_references(&temp_dir);
        assert_eq!(refs.len(), 2);
        assert!(refs.iter().any(|p| p.ends_with("packages/core")));
        assert!(refs.iter().any(|p| p.ends_with("packages/ui")));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_references_no_file() {
        let refs = parse_tsconfig_references(std::path::Path::new("/nonexistent"));
        assert!(refs.is_empty());
    }

    #[test]
    fn tsconfig_references_no_references_field() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-no-refs");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{"compilerOptions": {"strict": true}}"#,
        )
        .unwrap();

        let refs = parse_tsconfig_references(&temp_dir);
        assert!(refs.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_references_skips_nonexistent_dirs() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-missing-dir");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/core")).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{"references": [{"path": "./packages/core"}, {"path": "./packages/missing"}]}"#,
        )
        .unwrap();

        let refs = parse_tsconfig_references(&temp_dir);
        assert_eq!(refs.len(), 1);
        assert!(refs[0].ends_with("packages/core"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn strip_trailing_commas_no_commas() {
        let input = r#"{"a": 1, "b": [2, 3]}"#;
        assert_eq!(strip_trailing_commas(input), input);
    }

    #[test]
    fn strip_trailing_commas_empty_input() {
        assert_eq!(strip_trailing_commas(""), "");
    }

    #[test]
    fn strip_trailing_commas_nested_objects() {
        let input = "{\n  \"a\": {\n    \"b\": 1,\n    \"c\": 2,\n  },\n  \"d\": 3,\n}";
        let expected = "{\n  \"a\": {\n    \"b\": 1,\n    \"c\": 2\n  },\n  \"d\": 3\n}";
        assert_eq!(strip_trailing_commas(input), expected);
    }

    #[test]
    fn strip_trailing_commas_array_of_objects() {
        let input = r#"[{"a": 1,}, {"b": 2,},]"#;
        let expected = r#"[{"a": 1}, {"b": 2}]"#;
        assert_eq!(strip_trailing_commas(input), expected);
    }

    #[test]
    fn tsconfig_references_malformed_json() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-malformed");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r"{ this is not valid json at all",
        )
        .unwrap();

        let refs = parse_tsconfig_references(&temp_dir);
        assert!(refs.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_references_empty_array() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-empty-refs");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(temp_dir.join("tsconfig.json"), r#"{"references": []}"#).unwrap();

        let refs = parse_tsconfig_references(&temp_dir);
        assert!(refs.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_pnpm_workspace_malformed() {
        // Garbage input should return empty, not panic
        let patterns = parse_pnpm_workspace_yaml(":::not yaml at all:::");
        assert!(patterns.is_empty());
    }

    #[test]
    fn parse_pnpm_workspace_packages_key_empty_list() {
        let yaml = "packages:\nother:\n  - something\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert!(patterns.is_empty());
    }

    #[test]
    fn expand_workspace_glob_exact_path() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-exact");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/core")).unwrap();
        std::fs::write(
            temp_dir.join("packages/core/package.json"),
            r#"{"name": "core"}"#,
        )
        .unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/core", &canonical_root);
        assert_eq!(results.len(), 1);
        assert!(results[0].0.ends_with("packages/core"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn expand_workspace_glob_star() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-star");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/a")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/b")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/c")).unwrap();
        std::fs::write(temp_dir.join("packages/a/package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(temp_dir.join("packages/b/package.json"), r#"{"name": "b"}"#).unwrap();
        // c has no package.json — should be excluded

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/*", &canonical_root);
        assert_eq!(results.len(), 2);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn expand_workspace_glob_nested() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-nested");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/scope/a")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/scope/b")).unwrap();
        std::fs::write(
            temp_dir.join("packages/scope/a/package.json"),
            r#"{"name": "@scope/a"}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("packages/scope/b/package.json"),
            r#"{"name": "@scope/b"}"#,
        )
        .unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/**/*", &canonical_root);
        assert_eq!(results.len(), 2);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── parse_tsconfig_root_dir ──────────────────────────────────

    #[test]
    fn tsconfig_root_dir_extracted() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{ "compilerOptions": { "rootDir": "./src" } }"#,
        )
        .unwrap();

        assert_eq!(parse_tsconfig_root_dir(&temp_dir), Some("src".to_string()));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_root_dir_lib() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir-lib");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{ "compilerOptions": { "rootDir": "lib/" } }"#,
        )
        .unwrap();

        assert_eq!(parse_tsconfig_root_dir(&temp_dir), Some("lib".to_string()));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_root_dir_missing_field() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir-nofield");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{ "compilerOptions": { "strict": true } }"#,
        )
        .unwrap();

        assert_eq!(parse_tsconfig_root_dir(&temp_dir), None);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_root_dir_no_file() {
        assert_eq!(parse_tsconfig_root_dir(Path::new("/nonexistent")), None);
    }

    #[test]
    fn tsconfig_root_dir_with_comments() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir-comments");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            "{\n  // Root directory\n  \"compilerOptions\": { \"rootDir\": \"app\" }\n}",
        )
        .unwrap();

        assert_eq!(parse_tsconfig_root_dir(&temp_dir), Some("app".to_string()));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_root_dir_dot_value() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir-dot");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{ "compilerOptions": { "rootDir": "." } }"#,
        )
        .unwrap();

        // "." is returned as-is — caller filters it out
        assert_eq!(parse_tsconfig_root_dir(&temp_dir), Some(".".to_string()));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_root_dir_parent_traversal() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-rootdir-parent");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{ "compilerOptions": { "rootDir": "../other" } }"#,
        )
        .unwrap();

        // Returned as-is — caller filters it out
        assert_eq!(
            parse_tsconfig_root_dir(&temp_dir),
            Some("../other".to_string())
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn expand_workspace_glob_no_matches() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-nomatch");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "nonexistent/*", &canonical_root);
        assert!(results.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── parse_pnpm_workspace_yaml edge cases ────────────────────────

    #[test]
    fn parse_pnpm_workspace_with_empty_lines_between_entries() {
        let yaml = "packages:\n  - 'packages/*'\n\n  - 'apps/*'\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        // Empty lines between entries should be tolerated (they're skipped)
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_mixed_quotes() {
        let yaml = "packages:\n  - 'single/*'\n  - \"double/*\"\n  - bare/*\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["single/*", "double/*", "bare/*"]);
    }

    #[test]
    fn parse_pnpm_workspace_with_negation() {
        let yaml = "packages:\n  - 'packages/*'\n  - '!packages/test-*'\n";
        let patterns = parse_pnpm_workspace_yaml(yaml);
        assert_eq!(patterns, vec!["packages/*", "!packages/test-*"]);
    }

    // ── strip_trailing_commas advanced ───────────────────────────────

    #[test]
    fn strip_trailing_commas_string_with_closing_brackets() {
        // String containing "]" and "}" should not affect comma stripping
        let input = r#"{"key": "value with ] and }",}"#;
        let expected = r#"{"key": "value with ] and }"}"#;
        assert_eq!(strip_trailing_commas(input), expected);
    }

    #[test]
    fn strip_trailing_commas_multiple_levels() {
        let input = r#"{"a": {"b": [1, 2,], "c": 3,},}"#;
        let expected = r#"{"a": {"b": [1, 2], "c": 3}}"#;
        assert_eq!(strip_trailing_commas(input), expected);
    }

    // ── tsconfig_root_dir edge cases ────────────────────────────────

    #[test]
    fn tsconfig_root_dir_with_trailing_commas() {
        let temp_dir = std::env::temp_dir().join("fallow-test-rootdir-trailing-comma");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        std::fs::write(
            temp_dir.join("tsconfig.json"),
            "{\n  \"compilerOptions\": {\n    \"rootDir\": \"app\",\n  },\n}",
        )
        .unwrap();

        assert_eq!(parse_tsconfig_root_dir(&temp_dir), Some("app".to_string()));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── expand_workspace_glob with trailing slash ────────────────────

    #[test]
    fn expand_workspace_glob_trailing_slash() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-trailing");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/a")).unwrap();
        std::fs::write(temp_dir.join("packages/a/package.json"), r#"{"name": "a"}"#).unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        // Trailing slash pattern gets `*` appended -> `packages/*`
        let results = expand_workspace_glob(&temp_dir, "packages/*", &canonical_root);
        assert_eq!(results.len(), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── expand_workspace_glob excludes node_modules ──────────────────

    #[test]
    fn expand_workspace_glob_excludes_node_modules() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-no-nodemod");
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Nested node_modules package — should be excluded
        let nm_pkg = temp_dir.join("packages/foo/node_modules/bar");
        std::fs::create_dir_all(&nm_pkg).unwrap();
        std::fs::write(nm_pkg.join("package.json"), r#"{"name":"bar"}"#).unwrap();

        // Legitimate workspace package — should be included
        let ws_pkg = temp_dir.join("packages/foo");
        std::fs::write(ws_pkg.join("package.json"), r#"{"name":"foo"}"#).unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/**", &canonical_root);

        assert!(results.iter().any(|(_orig, canon)| {
            canon
                .to_string_lossy()
                .replace('\\', "/")
                .contains("packages/foo")
                && !canon.to_string_lossy().contains("node_modules")
        }));
        assert!(
            !results
                .iter()
                .any(|(_, cp)| cp.to_string_lossy().contains("node_modules"))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── expand_workspace_glob skips dirs without package.json ────────

    #[test]
    fn expand_workspace_glob_skips_dirs_without_pkg() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-no-pkg");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/with-pkg")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/without-pkg")).unwrap();
        std::fs::write(
            temp_dir.join("packages/with-pkg/package.json"),
            r#"{"name": "with"}"#,
        )
        .unwrap();
        // packages/without-pkg has no package.json

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/*", &canonical_root);
        assert_eq!(results.len(), 1);
        assert!(
            results[0]
                .0
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("packages/with-pkg")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── expand_workspace_glob prunes node_modules with ** patterns ───

    #[test]
    fn expand_recursive_glob_prunes_node_modules() {
        // When using `packages/**/*` the manual walk should prune
        // `node_modules` during traversal, so a package.json inside
        // `packages/app/node_modules/dep/` is never returned.
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-recursive-prune");
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Legitimate workspace packages
        std::fs::create_dir_all(temp_dir.join("packages/app")).unwrap();
        std::fs::write(
            temp_dir.join("packages/app/package.json"),
            r#"{"name": "app"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/lib")).unwrap();
        std::fs::write(
            temp_dir.join("packages/lib/package.json"),
            r#"{"name": "lib"}"#,
        )
        .unwrap();

        // Nested node_modules dependency (should be pruned)
        let nm_dep = temp_dir.join("packages/app/node_modules/dep");
        std::fs::create_dir_all(&nm_dep).unwrap();
        std::fs::write(nm_dep.join("package.json"), r#"{"name": "dep"}"#).unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/**/*", &canonical_root);

        // Should find exactly the two legitimate workspace packages
        let found_names: Vec<String> = results
            .iter()
            .map(|(orig, _)| orig.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            found_names.contains(&"app".to_string()),
            "should find packages/app"
        );
        assert!(
            found_names.contains(&"lib".to_string()),
            "should find packages/lib"
        );
        assert!(
            !results
                .iter()
                .any(|(_, cp)| cp.to_string_lossy().contains("node_modules")),
            "should NOT include packages inside node_modules"
        );
        assert_eq!(
            results.len(),
            2,
            "should find exactly 2 workspace packages (node_modules pruned)"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn expand_recursive_glob_preserves_nested_workspace_roots() {
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-recursive-workspace-prune");
        let _ = std::fs::remove_dir_all(&temp_dir);

        std::fs::create_dir_all(temp_dir.join("apps/app/packages/nested")).unwrap();
        std::fs::write(temp_dir.join("apps/app/package.json"), r#"{"name":"app"}"#).unwrap();
        std::fs::write(
            temp_dir.join("apps/app/packages/nested/package.json"),
            r#"{"name":"nested"}"#,
        )
        .unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "apps/**", &canonical_root);
        let mut paths: Vec<_> = results
            .iter()
            .map(|(path, _)| path.strip_prefix(&temp_dir).unwrap().to_path_buf())
            .collect();
        paths.sort();

        assert_eq!(
            paths,
            vec![
                PathBuf::from("apps/app"),
                PathBuf::from("apps/app/packages/nested")
            ]
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn expand_recursive_glob_prunes_deeply_nested_node_modules() {
        // Even deeply nested node_modules (e.g., pnpm's deep symlink trees)
        // should be pruned during the walk.
        let temp_dir = std::env::temp_dir().join("fallow-test-expand-deep-prune");
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Legitimate workspace package
        std::fs::create_dir_all(temp_dir.join("packages/core")).unwrap();
        std::fs::write(
            temp_dir.join("packages/core/package.json"),
            r#"{"name": "core"}"#,
        )
        .unwrap();

        // Deeply nested node_modules (simulates pnpm virtual store)
        let deep_nm = temp_dir.join("packages/core/node_modules/.pnpm/react@18/node_modules/react");
        std::fs::create_dir_all(&deep_nm).unwrap();
        std::fs::write(deep_nm.join("package.json"), r#"{"name": "react"}"#).unwrap();

        let canonical_root = dunce::canonicalize(&temp_dir).unwrap();
        let results = expand_workspace_glob(&temp_dir, "packages/**/*", &canonical_root);

        assert_eq!(
            results.len(),
            1,
            "should find exactly 1 workspace package, pruning deep node_modules"
        );
        assert!(
            results[0]
                .0
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("packages/core"),
            "the single result should be packages/core"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
