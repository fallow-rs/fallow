//! Pre-upload check that a source map's `sources[]` point at files in the
//! repository.
//!
//! The cloud resolves each `sources[]` entry against the map's repo-relative
//! path and joins the result to the static inventory. A build setting such as
//! `sourceRoot: "/"` with `rootDir: src` in `tsconfig.json` makes every entry
//! resolve to a path without the `src/` segment, so no runtime function ever
//! maps back to source. The upload still succeeds, and the only later symptom
//! is unresolved coverage (issue #3298). This module reproduces the cloud's
//! path resolution so the CLI can report the problem before the upload.

use std::path::Path;

const MAX_REPORTED_MAPS: usize = 5;

/// One map whose original sources are all absent from the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnresolvedMap {
    pub map_path: String,
    pub source_root: Option<String>,
    pub raw_source: String,
    pub resolved_source: String,
}

/// Checks one parsed map. Returns `Some` only when the map lists at least one
/// project source and none of them exists under `repo_root`. Dependency
/// sources under `node_modules/` are ignored, because a missing dependency
/// file does not stop the project sources from resolving.
pub(super) fn find_unresolved_map(
    repo_root: &Path,
    map_path: &str,
    source_map: &serde_json::Value,
) -> Option<UnresolvedMap> {
    if !is_script_map(map_path) {
        return None;
    }
    let sources = source_map.get("sources")?.as_array()?;
    let source_root = source_map
        .get("sourceRoot")
        .and_then(serde_json::Value::as_str)
        .filter(|root| !root.is_empty());

    let mut first_missing: Option<(String, String)> = None;
    for raw in sources.iter().filter_map(serde_json::Value::as_str) {
        let resolved = resolve_map_source_path(raw, source_root, map_path);
        if is_virtual_source(raw) || !has_file_extension(&resolved) || is_dependency_path(&resolved)
        {
            continue;
        }
        if source_candidates(raw, source_root, map_path)
            .iter()
            .any(|candidate| repo_root.join(candidate).is_file())
        {
            return None;
        }
        first_missing.get_or_insert_with(|| (raw.to_owned(), resolved));
    }
    let (raw_source, resolved_source) = first_missing?;
    Some(UnresolvedMap {
        map_path: map_path.to_owned(),
        source_root: source_root.map(str::to_owned),
        raw_source,
        resolved_source,
    })
}

/// Prints one warning for all maps whose sources do not resolve.
pub(super) fn warn_unresolved_maps(log_prefix: &str, unresolved: &[UnresolvedMap], total: usize) {
    use colored::Colorize as _;

    if unresolved.is_empty() {
        return;
    }
    eprintln!(
        "{log_prefix}: {}: {} of {total} source maps point at original sources that are not in \
         the repository. Runtime coverage for these files will not resolve to source:",
        "warning".yellow().bold(),
        unresolved.len(),
    );
    for map in unresolved.iter().take(MAX_REPORTED_MAPS) {
        let source_root = map
            .source_root
            .as_deref()
            .map(|root| format!(" with sourceRoot \"{root}\""))
            .unwrap_or_default();
        eprintln!(
            "  {}: \"{}\"{source_root} resolves to {}, which does not exist",
            map.map_path, map.raw_source, map.resolved_source
        );
    }
    if unresolved.len() > MAX_REPORTED_MAPS {
        eprintln!("  ... and {} more", unresolved.len() - MAX_REPORTED_MAPS);
    }
    if unresolved.iter().any(|map| map.source_root.is_some()) {
        eprintln!(
            "  For maps that set sourceRoot: remove sourceRoot from the build config (for tsc: \
             tsconfig.json), so each source resolves from the directory of its map."
        );
    }
    if unresolved.iter().any(|map| map.source_root.is_none()) {
        eprintln!(
            "  Run this command from the repository root, and make sure the original sources \
             are present where the build ran."
        );
    }
}

const SCRIPT_MAP_SUFFIXES: [&str; 3] = [".js.map", ".mjs.map", ".cjs.map"];

/// Runtime coverage measures JavaScript, so only script maps decide whether it
/// resolves. Stylesheet maps often list virtual sources, for example
/// `angular:styles/component:css;<hash>;<path>`.
fn is_script_map(map_path: &str) -> bool {
    SCRIPT_MAP_SUFFIXES
        .iter()
        .any(|suffix| map_path.ends_with(suffix))
}

/// Bundler runtime modules such as `webpack/bootstrap`, `(webpack)/...`,
/// `<anon>` or `angular:styles/...` have no file on disk in any repository.
fn is_virtual_source(raw: &str) -> bool {
    let without_scheme = strip_scheme(raw);
    let first_segment = without_scheme.split('/').next().unwrap_or_default();
    let is_drive = first_segment.len() == 2
        && first_segment.ends_with(':')
        && first_segment.starts_with(|c: char| c.is_ascii_alphabetic());
    raw.contains('<')
        || raw.contains('>')
        || without_scheme.starts_with('(')
        || (first_segment.contains(':') && !is_drive)
        || without_scheme
            .split('/')
            .any(|segment| segment == "webpack")
}

fn has_file_extension(resolved: &str) -> bool {
    resolved
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
}

fn is_dependency_path(resolved: &str) -> bool {
    resolved.starts_with("node_modules/") || resolved.contains("/node_modules/")
}

/// Mirrors the cloud's `resolveMapSourcePath`: scheme-prefixed and absolute
/// sources anchor at the repository root, and other sources resolve from the
/// map's directory. Keep the two in step, or this check reports a different
/// path than the one the cloud joins to the inventory.
fn resolve_map_source_path(raw: &str, source_root: Option<&str>, map_path: &str) -> String {
    let value = decode_with_source_root(raw, source_root);
    let scheme_stripped = strip_scheme(&value);
    if scheme_stripped.len() != value.len() || scheme_stripped.starts_with('/') {
        return normalize_source_path(&value);
    }
    let map_dir = map_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    if map_dir.is_empty() {
        canonicalize_posix(&value)
    } else {
        canonicalize_posix(&format!("{map_dir}/{value}"))
    }
}

const WEBPACK_SCHEME: &str = "webpack://";

/// Every repo-relative path the cloud tries for one source, in its order. The
/// first is `resolve_map_source_path`. The others join the relative source,
/// with any webpack namespace removed, to each parent of the map's directory
/// up to the repository root. This covers sources relative to a workspace
/// root (Angular CLI) or to a webpack context. The cloud keeps the first
/// candidate that is a known repository file. Empty candidates are dropped.
/// `tests/fixtures/source-map-path-contract.json` holds the cases that both
/// implementations must pass.
fn source_candidates(raw: &str, source_root: Option<&str>, map_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let resolved = resolve_map_source_path(raw, source_root, map_path);
    if !resolved.is_empty() {
        candidates.push(resolved);
    }
    let decoded = decode_with_source_root(raw, source_root);
    let relative = relative_source(&decoded);
    let map_path = canonicalize_posix(map_path);
    let mut dir = map_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    while !dir.is_empty() {
        dir = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
        let joined = if dir.is_empty() {
            canonicalize_posix(relative)
        } else {
            canonicalize_posix(&format!("{dir}/{relative}"))
        };
        if !joined.is_empty() && !candidates.contains(&joined) {
            candidates.push(joined);
        }
    }
    candidates
}

/// The source without its scheme, webpack namespace, and leading `/` or `./`.
/// A webpack namespace is every segment in front of the first `.` or `..`
/// segment, as in `webpack://@scope/app/./src/x.ts`. A source with a `/` or
/// `.` right after `webpack://` has no namespace.
fn relative_source(decoded: &str) -> &str {
    let mut value = strip_scheme(decoded);
    let has_namespace = decoded
        .strip_prefix(WEBPACK_SCHEME)
        .is_some_and(|rest| !rest.starts_with(['/', '.']));
    if has_namespace {
        let mut offset = 0;
        for segment in value.split('/') {
            if segment == "." || segment == ".." {
                value = &value[offset..];
                break;
            }
            offset += segment.len() + 1;
        }
    }
    value
        .strip_prefix("./")
        .or_else(|| value.strip_prefix('/'))
        .unwrap_or(value)
}

fn normalize_source_path(raw: &str) -> String {
    let decoded = percent_decode(raw);
    let value = strip_scheme(&decoded);
    let value = value
        .strip_prefix("./")
        .or_else(|| value.strip_prefix('/'))
        .unwrap_or(value);
    let value = value.strip_prefix("[project]/").unwrap_or(value);
    canonicalize_posix(value)
}

fn decode_with_source_root(raw: &str, source_root: Option<&str>) -> String {
    let joined = match source_root {
        None | Some("") => raw.to_owned(),
        Some(root) if root.ends_with('/') => format!("{root}{raw}"),
        Some(root) => format!("{root}/{raw}"),
    };
    percent_decode(&joined)
}

/// Strips a leading `scheme://` (with any extra slashes), as the cloud's
/// `^[a-zA-Z][a-zA-Z0-9.+-]*:\/\/+` pattern does.
fn strip_scheme(value: &str) -> &str {
    let mut chars = value.char_indices();
    if !chars.next().is_some_and(|(_, c)| c.is_ascii_alphabetic()) {
        return value;
    }
    let Some(colon) = value.find(':') else {
        return value;
    };
    let scheme_ok = value[..colon]
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'));
    let rest = &value[colon + 1..];
    if !scheme_ok || !rest.starts_with("//") {
        return value;
    }
    rest.trim_start_matches('/')
}

/// Folds `.` and `..` segments and drops a leading `..` that would escape the
/// root, as the cloud's `canonicalizePosix` does.
fn canonicalize_posix(value: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for segment in value.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if stack.last().is_some_and(|top| *top != "..") {
                    stack.pop();
                } else {
                    stack.push("..");
                }
            }
            _ => stack.push(segment),
        }
    }
    let start = stack.iter().take_while(|segment| **segment == "..").count();
    stack[start..].join("/")
}

/// Decodes `%XX` escapes like `decodeURIComponent`, and keeps the input when
/// the escapes do not form valid UTF-8.
fn percent_decode(value: &str) -> String {
    if !value.contains('%') {
        return value.to_owned();
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .filter(|pair| pair.iter().all(u8::is_ascii_hexdigit))
                .and_then(|pair| std::str::from_utf8(pair).ok())
                .and_then(|pair| u8::from_str_radix(pair, 16).ok());
            let Some(byte) = hex else {
                return value.to_owned();
            };
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn repo_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempdir().expect("tempdir");
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, "export {};").expect("write");
        }
        dir
    }

    #[test]
    fn tsc_map_without_source_root_resolves() {
        let repo = repo_with(&["src/consumers/base/index.ts"]);
        let map = json!({ "sources": ["../../../src/consumers/base/index.ts"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/consumers/base/index.js.map", &map),
            None
        );
    }

    #[test]
    fn source_root_slash_with_root_dir_src_is_reported() {
        let repo = repo_with(&["src/index.ts"]);
        let map = json!({ "sourceRoot": "/", "sources": ["index.ts"] });
        let unresolved = find_unresolved_map(repo.path(), "dist/index.js.map", &map)
            .expect("sourceRoot \"/\" drops the src/ segment");
        assert_eq!(unresolved.resolved_source, "index.ts");
        assert_eq!(unresolved.raw_source, "index.ts");
        assert_eq!(unresolved.source_root.as_deref(), Some("/"));
    }

    #[test]
    fn source_root_slash_with_repo_relative_sources_resolves() {
        let repo = repo_with(&["src/consumers/base/index.ts"]);
        let map = json!({ "sourceRoot": "/", "sources": ["../../../src/consumers/base/index.ts"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/consumers/base/index.js.map", &map),
            None
        );
    }

    #[test]
    fn bundler_scheme_sources_anchor_at_repo_root() {
        let repo = repo_with(&["src/app.ts"]);
        let map =
            json!({ "sources": ["webpack://./src/app.ts", "webpack://./node_modules/x/y.js"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/assets/app.js.map", &map),
            None
        );
    }

    #[test]
    fn dependency_only_maps_are_not_reported() {
        let repo = repo_with(&[]);
        let map = json!({ "sources": ["../node_modules/lib/index.js"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/vendor.js.map", &map),
            None
        );
    }

    #[test]
    fn one_present_source_is_enough() {
        let repo = repo_with(&["src/a.ts"]);
        let map = json!({ "sources": ["../src/gone.ts", "../src/a.ts"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/a.js.map", &map),
            None
        );
    }

    #[test]
    fn mirrors_cloud_path_normalization() {
        assert_eq!(
            resolve_map_source_path("../../src/x.ts", None, "dist/assets/app.js.map"),
            "src/x.ts"
        );
        assert_eq!(
            resolve_map_source_path("x.ts", Some("/"), "dist/app.js.map"),
            "x.ts"
        );
        assert_eq!(
            resolve_map_source_path("[project]/src/x.ts", Some("file:///"), "a.js.map"),
            "src/x.ts"
        );
        assert_eq!(
            resolve_map_source_path("my%20file.ts", None, "dist/a.js.map"),
            "dist/my file.ts"
        );
        assert_eq!(
            resolve_map_source_path("../../../../x.ts", None, "dist/a.js.map"),
            "x.ts"
        );
    }

    #[test]
    fn strip_scheme_matches_cloud_pattern() {
        assert_eq!(strip_scheme("C:/x"), "C:/x");
        assert_eq!(strip_scheme("webpack:///src/x.ts"), "src/x.ts");
        assert_eq!(strip_scheme("file://x"), "x");
        assert_eq!(strip_scheme("1http://x"), "1http://x");
        assert_eq!(strip_scheme("a b://x"), "a b://x");
    }

    #[test]
    fn percent_decode_matches_decode_uri_component() {
        assert_eq!(percent_decode("caf%C3%A9.ts"), "café.ts");
        assert_eq!(percent_decode("%zz.ts"), "%zz.ts");
        assert_eq!(percent_decode("x%4"), "x%4");
        assert_eq!(percent_decode("%+1.ts"), "%+1.ts");
        assert_eq!(percent_decode("%FF.ts"), "%FF.ts");
        assert_eq!(percent_decode("plain.ts"), "plain.ts");
    }

    #[test]
    fn absolute_sources_decode_twice_like_the_cloud() {
        assert_eq!(
            resolve_map_source_path("/src/a%2541.ts", None, "dist/a.js.map"),
            "src/aA.ts"
        );
        assert_eq!(
            resolve_map_source_path("a%2541.ts", None, "dist/a.js.map"),
            "dist/a%41.ts"
        );
    }

    #[test]
    fn bundler_runtime_maps_are_not_reported() {
        let repo = repo_with(&[]);
        let map = json!({ "sources": [
            "webpack://app/webpack/bootstrap",
            "webpack://app/webpack/runtime/define property getters",
            "<anon>",
            "(webpack)/buildin/global.js",
        ] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/runtime.js.map", &map),
            None
        );
    }

    #[test]
    fn stylesheet_maps_and_prefixed_virtual_sources_are_not_reported() {
        let repo = repo_with(&[]);
        let virtual_source =
            json!({ "sources": ["angular:styles/component:css;abc;/app/x.component.ts"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/x.component.css.map", &virtual_source),
            None
        );
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/main.js.map", &virtual_source),
            None
        );
        let missing = json!({ "sources": ["../src/gone.ts"] });
        assert!(find_unresolved_map(repo.path(), "dist/main.mjs.map", &missing).is_some());
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/a.css.map", &missing),
            None
        );
    }

    #[test]
    fn workspace_relative_sources_resolve_from_a_parent_directory() {
        let repo = repo_with(&["apps/client/src/app/app.routes.ts"]);
        let map = json!({ "sources": ["apps/client/src/app/app.routes.ts"] });
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/client/browser/main.js.map", &map),
            None
        );
    }

    #[test]
    fn webpack_namespace_sources_resolve_from_the_package_directory() {
        let repo = repo_with(&[
            "packages/app/src/x.component.ts",
            "packages/components/src/button.ts",
        ]);
        for source in [
            "webpack://@scope/app/./src/x.component.ts",
            "webpack://@scope/app/../components/src/button.ts",
        ] {
            let map = json!({ "sources": [source] });
            assert_eq!(
                find_unresolved_map(repo.path(), "packages/app/dist/387.js.map", &map),
                None,
                "{source}"
            );
        }
        assert_eq!(
            relative_source("webpack://@scope/app/./src/x.ts"),
            "src/x.ts"
        );
        assert_eq!(relative_source("webpack:///./src/x.ts"), "src/x.ts");
        assert_eq!(relative_source("webpack://app/src/x.ts"), "app/src/x.ts");
    }

    #[test]
    fn source_root_slash_stays_unresolved_with_parent_candidates() {
        let repo = repo_with(&["src/consumers/base/index.ts"]);
        let map = json!({ "sourceRoot": "/", "sources": ["consumers/base/index.ts"] });
        assert!(
            find_unresolved_map(repo.path(), "dist/consumers/base/index.js.map", &map).is_some()
        );
    }

    /// The cloud runs the same cases against its implementation, so a change
    /// on either side that alters a candidate list fails one of the two suites.
    #[test]
    fn source_candidates_match_the_shared_contract() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/source-map-path-contract.json");
        let text = std::fs::read_to_string(&path).expect("read contract fixture");
        let contract: serde_json::Value = serde_json::from_str(&text).expect("parse contract");
        assert_eq!(contract["schemaVersion"], 1);
        let cases = contract["cases"].as_array().expect("cases array");
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let raw = case["raw"].as_str().expect("raw");
            let source_root = case["sourceRoot"].as_str();
            let map_path = case["mapPath"].as_str().expect("mapPath");
            let expected: Vec<&str> = case["candidates"]
                .as_array()
                .expect("candidates")
                .iter()
                .map(|candidate| candidate.as_str().expect("candidate string"))
                .collect();
            assert_eq!(
                source_candidates(raw, source_root, map_path),
                expected,
                "case: {name}"
            );
        }
    }

    #[test]
    fn map_without_sources_is_not_reported() {
        let repo = repo_with(&[]);
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/a.js.map", &json!({})),
            None
        );
        assert_eq!(
            find_unresolved_map(repo.path(), "dist/a.js.map", &json!({ "sources": [] })),
            None
        );
    }
}
