//! The `source-map-cache` of a V8 coverage dump: its entries, and the rules
//! that turn a source of a map into a file path. The health coverage reader
//! and the runtime-coverage remapper of the CLI both use these rules.

use std::path::{Path, PathBuf};

use fallow_types::path_util::{is_absolute_path_any_platform, looks_like_windows_absolute_path};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use srcmap_sourcemap::SourceMap;
use url::Url;

/// One `source-map-cache` entry of a V8 dump.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceMapCacheEntry {
    /// The URL of an external map. Node writes `null` when it loaded the map.
    #[serde(default)]
    pub url: Option<String>,
    /// The source map as JSON.
    pub data: serde_json::Value,
    /// UTF-16 length of each line of the generated code.
    #[serde(default, rename = "lineLengths")]
    pub line_lengths: Vec<u32>,
}

impl SourceMapCacheEntry {
    /// Parse the source map of the entry. The generated script has the lines
    /// of `lineLengths`, or of `generated_source` when that text is known and
    /// has more lines. The map parser sizes its line table from the largest
    /// generated line, and a section of an indexed map can start at any line.
    /// So a map with a section that starts past the last generated line gives
    /// `None`: it does not describe the script, and its table could be far
    /// larger than the script.
    #[must_use]
    pub fn parse_map(&self, generated_source: Option<&str>) -> Option<SourceMap> {
        let generated_lines = generated_source
            .map_or(0, generated_line_bound)
            .max(self.line_lengths.len());
        let sections = self
            .data
            .get("sections")
            .and_then(serde_json::Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let past_the_end = sections.iter().any(|section| {
            section
                .pointer("/offset/line")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|line| line >= generated_lines as u64)
        });
        if past_the_end {
            return None;
        }
        SourceMap::from_json(&self.data.to_string()).ok()
    }
}

/// An upper bound on the lines of a generated script. JavaScript ends a line
/// at `\n`, `\r`, U+2028 and U+2029, and `\r\n` counts twice here.
fn generated_line_bound(source: &str) -> usize {
    source
        .chars()
        .filter(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        .count()
        + 1
}

/// Decodes each entry of a `source-map-cache` object on its own, keyed by
/// script URL. An entry that does not decode is left out, so one bad entry
/// does not remove the other maps. A value that is not an object gives an
/// empty map.
pub fn parse_source_map_cache(raw: &serde_json::Value) -> FxHashMap<String, SourceMapCacheEntry> {
    let Some(entries) = raw.as_object() else {
        return FxHashMap::default();
    };
    entries
        .iter()
        .filter_map(|(url, entry)| {
            Some((url.clone(), SourceMapCacheEntry::deserialize(entry).ok()?))
        })
        .collect()
}

/// The path of a `file://` URL or of an absolute path. `None` for each other
/// URL and for a relative path.
pub fn file_url_to_path(value: &str) -> Option<PathBuf> {
    if let Ok(url) = Url::parse(value) {
        return if url.scheme() == "file" {
            url.to_file_path().ok()
        } else {
            None
        };
    }
    let path = PathBuf::from(value);
    (is_absolute_path_any_platform(&path) || looks_like_windows_absolute_path(value))
        .then_some(path)
}

/// The directory that relative and virtual sources resolve from: the
/// directory of the external map when the entry names one, else the
/// directory of the generated script. `None` when the script is not a local
/// file or the map is remote.
pub fn source_map_base_dir(generated_url: &str, source_map_url: Option<&str>) -> Option<PathBuf> {
    let generated_path = file_url_to_path(generated_url)?;
    let generated_dir = generated_path.parent()?.to_path_buf();
    let Some(source_map_url) = source_map_url.filter(|url| !url.is_empty()) else {
        return Some(generated_dir);
    };
    if let Some(path) = file_url_to_path(source_map_url) {
        return path.parent().map(Path::to_path_buf);
    }
    let candidate = PathBuf::from(source_map_url);
    if is_absolute_path_any_platform(&candidate) || looks_like_windows_absolute_path(source_map_url)
    {
        return candidate.parent().map(Path::to_path_buf);
    }
    if Url::parse(source_map_url).is_ok() {
        return None;
    }
    generated_dir
        .join(candidate)
        .parent()
        .map(Path::to_path_buf)
}

/// The file path of one source of a map. A `file://` URL and an absolute
/// path resolve without a base directory. A relative path joins the base
/// directory. A `webpack://` or `vite://` source resolves to the first
/// existing file in the base directory or one of its ancestors, up to the
/// project root `root`. The walk does not leave the project root, and a
/// virtual source without a root gives `None`. Each other URL gives `None`.
pub fn resolve_original_source(
    raw_source: &str,
    base_dir: Option<&Path>,
    root: Option<&Path>,
) -> Option<PathBuf> {
    if raw_source.is_empty() {
        return None;
    }
    if let Some(path) = file_url_to_path(raw_source) {
        return Some(path);
    }
    let source_path = PathBuf::from(raw_source);
    // `C:/repo/a.ts` also parses as a URL with the scheme `c`.
    if is_absolute_path_any_platform(&source_path) || looks_like_windows_absolute_path(raw_source) {
        return Some(source_path);
    }
    if let Ok(url) = Url::parse(raw_source) {
        return match url.scheme() {
            "webpack" | "vite" => {
                resolve_virtual_candidate(&virtual_source_candidates(&url), base_dir?, root?)
            }
            _ => None,
        };
    }
    Some(base_dir?.join(source_path))
}

/// The relative paths that a virtual source can name, in lookup order: the
/// namespace plus the path, then the path alone. The decoded form of the
/// path comes first. The raw form follows, for a file name that holds a
/// literal `%XX`.
fn virtual_source_candidates(url: &Url) -> Vec<PathBuf> {
    let raw = url.path().trim_start_matches('/');
    let namespace = url
        .host_str()
        .map(|host| host.trim_matches('/'))
        .filter(|host| !host.is_empty() && !matches!(*host, "." | "_N_E"));
    let decoded = crate::url_path::decode_path(raw);
    let mut candidates = Vec::new();
    for path in [decoded.as_deref(), Some(raw)].into_iter().flatten() {
        if let Some(namespace) = namespace {
            push_candidate(&mut candidates, PathBuf::from(namespace).join(path));
        }
        push_candidate(&mut candidates, PathBuf::from(path));
    }
    candidates
}

fn push_candidate(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidate.as_os_str().is_empty() && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

/// The first candidate that is a file in `base_dir` or one of its
/// ancestors. A missing directory is skipped. A `..` in `base_dir` and in a
/// candidate applies by text first, so the ancestors do not depend on whether
/// a directory before the `..` exists. The walk stops at the first directory
/// that resolves outside `root`, and each candidate is checked against `root`
/// before the probe, so no probe reaches a path outside the root.
fn resolve_virtual_candidate(
    candidates: &[PathBuf],
    base_dir: &Path,
    root: &Path,
) -> Option<PathBuf> {
    let read_root = fallow_types::read_guard::ReadRoot::new(root);
    let base_dir = fallow_types::path_guard::lexically_normalized(base_dir);
    for base in base_dir.ancestors() {
        if base.symlink_metadata().is_err() {
            continue;
        }
        if !read_root.is_inside(base) {
            return None;
        }
        for candidate in candidates {
            let resolved = fallow_types::path_guard::lexically_normalized(&base.join(candidate));
            if read_root.is_inside(&resolved) && resolved.is_file() {
                return Some(resolved);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_url(path: &Path) -> String {
        Url::from_file_path(path).unwrap().to_string()
    }

    fn resolve(source: &str, generated: &str, map_url: Option<&str>) -> Option<PathBuf> {
        resolve_original_source(
            source,
            source_map_base_dir(generated, map_url).as_deref(),
            None,
        )
    }

    /// A tempdir with an empty `dist` directory and each file of `files`.
    fn project(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("dist")).unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "export const a = 1;\n").unwrap();
        }
        dir
    }

    fn resolve_virtual(dir: &tempfile::TempDir, source: &str) -> Option<PathBuf> {
        resolve_original_source(source, Some(&dir.path().join("dist")), Some(dir.path()))
    }

    #[test]
    fn preserves_windows_absolute_source_map_sources() {
        let generated = "file:///C:/repo/dist/bundle.js";
        assert_eq!(
            resolve("C:/repo/src/app.ts", generated, Some("bundle.js.map")),
            Some(PathBuf::from("C:/repo/src/app.ts"))
        );
        assert_eq!(
            resolve(r"C:\repo\src\app.ts", generated, Some("bundle.js.map")),
            Some(PathBuf::from(r"C:\repo\src\app.ts"))
        );
    }

    #[test]
    fn file_url_to_path_accepts_file_urls_and_absolute_paths_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.js");
        assert_eq!(file_url_to_path(&file_url(&path)), Some(path));
        assert_eq!(file_url_to_path("https://example.com/app.js"), None);
        assert_eq!(file_url_to_path("relative/app.js"), None);
    }

    #[test]
    fn resolve_source_map_base_handles_inline_relative_and_remote_urls() {
        let dir = tempfile::tempdir().unwrap();
        let generated = file_url(&dir.path().join("app.js"));
        assert_eq!(
            source_map_base_dir(&generated, None),
            Some(dir.path().to_path_buf())
        );
        assert_eq!(
            source_map_base_dir(&generated, Some("app.js.map")),
            Some(dir.path().to_path_buf())
        );
        assert_eq!(
            source_map_base_dir(&generated, Some("https://cdn.example.com/app.js.map")),
            None
        );
    }

    #[test]
    fn resolve_original_source_path_handles_file_absolute_and_relative_sources() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("src/app.ts");
        let generated = file_url(&dir.path().join("dist/app.js"));
        assert_eq!(resolve("", &generated, None), None);
        assert_eq!(
            resolve(&file_url(&source), &generated, None),
            Some(source.clone())
        );
        assert_eq!(
            resolve(source.to_str().unwrap(), &generated, None),
            Some(source)
        );
        assert_eq!(
            resolve("src/app.ts", &generated, None),
            Some(dir.path().join("dist/src/app.ts"))
        );
    }

    #[test]
    fn resolve_original_source_path_resolves_virtual_sources_from_ancestors() {
        let dir = project(&["src/app.ts"]);
        let generated = file_url(&dir.path().join("dist/chunks/app.js"));
        let base = source_map_base_dir(&generated, None);
        assert_eq!(
            resolve_original_source(
                "webpack://app/src/app.ts",
                base.as_deref(),
                Some(dir.path())
            ),
            Some(dir.path().join("src/app.ts"))
        );
        assert_eq!(resolve("webpack://app/src/app.ts", &generated, None), None);
    }

    #[test]
    fn virtual_source_walk_stops_at_the_project_root() {
        let sandbox = project(&["src/app.ts", "project/dist/chunks/app.js"]);
        let root = sandbox.path().join("project");
        let base = root.join("dist/chunks");
        assert_eq!(
            resolve_original_source("webpack://app/src/app.ts", Some(&base), Some(&root)),
            None
        );
        assert_eq!(
            resolve_original_source("webpack://app/../../src/app.ts", Some(&base), Some(&root)),
            None
        );
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/app.ts"), "").unwrap();
        assert_eq!(
            resolve_original_source("webpack://app/src/app.ts", Some(&base), Some(&root)),
            Some(root.join("src/app.ts"))
        );
    }

    #[test]
    fn virtual_source_candidates_strip_known_pseudo_hosts() {
        let url = Url::parse("webpack://_N_E/./src/app.ts").unwrap();
        let candidates = virtual_source_candidates(&url);
        // The `_N_E` pseudo-host is dropped and the url crate normalizes the
        // `/.` segment, leaving the path-only candidate.
        assert!(
            candidates.contains(&PathBuf::from("src/app.ts")),
            "got: {candidates:?}"
        );
        assert!(!candidates.iter().any(|c| c.starts_with("_N_E")));
    }

    #[test]
    fn resolve_virtual_source_path_ignores_non_virtual_schemes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_original_source("https://example.com/app.ts", Some(dir.path()), None),
            None
        );
    }

    #[test]
    fn parse_keeps_good_entries_beside_a_bad_one() {
        let raw = serde_json::json!({
            "file:///good.js": { "data": { "version": 3 }, "lineLengths": [1] },
            "file:///string.js": "oops",
            "file:///lengths.js": { "data": {}, "lineLengths": "x" },
        });
        let cache = parse_source_map_cache(&raw);
        let keys: Vec<&String> = cache.keys().collect();
        assert_eq!(keys, vec!["file:///good.js"]);
        assert!(parse_source_map_cache(&serde_json::json!("not a cache object")).is_empty());
    }

    #[test]
    fn entry_without_line_lengths_or_url_decodes() {
        let bare: SourceMapCacheEntry =
            serde_json::from_value(serde_json::json!({ "data": { "version": 3 } })).unwrap();
        assert!(bare.line_lengths.is_empty());
        assert_eq!(bare.url, None);
        let full: SourceMapCacheEntry = serde_json::from_value(
            serde_json::json!({ "url": null, "data": {}, "lineLengths": [1] }),
        )
        .unwrap();
        assert_eq!(full.line_lengths, vec![1]);
    }

    #[test]
    fn base_dir_follows_an_external_map_in_another_directory() {
        let dir = tempfile::tempdir().unwrap();
        let generated = file_url(&dir.path().join("dist/app.js"));
        let base = source_map_base_dir(&generated, Some("../maps/app.js.map"));
        let expected = dir.path().join("dist").join("../maps");
        assert_eq!(base.as_deref(), Some(expected.as_path()));
        assert_eq!(
            resolve_original_source("../src/a.ts", base.as_deref(), None),
            Some(expected.join("../src/a.ts"))
        );
    }

    /// A base directory with `..` finds the same virtual source whether or
    /// not the directory before the `..` exists.
    #[test]
    fn virtual_source_lookup_does_not_depend_on_a_dir_before_a_parent_step() {
        for probe_exists in [true, false] {
            let dir = project(&["dist/a.ts", "a.ts"]);
            if probe_exists {
                std::fs::create_dir_all(dir.path().join("outside/probe")).unwrap();
            }
            let base = dir.path().join("outside/probe/../../dist");
            let found =
                resolve_original_source("webpack://app/./a.ts", Some(&base), Some(dir.path()))
                    .unwrap();
            assert_eq!(
                std::fs::canonicalize(found).unwrap(),
                std::fs::canonicalize(dir.path().join("dist/a.ts")).unwrap(),
                "probe exists: {probe_exists}"
            );
        }
    }

    #[test]
    fn virtual_source_with_an_escaped_space_resolves() {
        let dir = project(&["src dir/my file.ts"]);
        assert_eq!(
            resolve_virtual(&dir, "webpack://app/src%20dir/my%20file.ts"),
            Some(dir.path().join("src dir/my file.ts"))
        );
    }

    #[test]
    fn virtual_source_with_a_plain_space_resolves() {
        let dir = project(&["src dir/my file.ts"]);
        assert_eq!(
            resolve_virtual(&dir, "webpack://app/./src dir/my file.ts"),
            Some(dir.path().join("src dir/my file.ts"))
        );
    }

    #[test]
    fn virtual_source_with_a_literal_percent_resolves() {
        let dir = project(&["src/100%.ts", "src/a%20b.ts"]);
        for file in ["src/100%.ts", "src/a%20b.ts"] {
            assert_eq!(
                resolve_virtual(&dir, &format!("webpack://app/{file}")),
                Some(dir.path().join(file))
            );
        }
    }

    #[test]
    fn virtual_source_with_an_escaped_separator_is_not_resolved() {
        let dir = project(&["src/a.ts"]);
        assert_eq!(resolve_virtual(&dir, "webpack://app/src%2Fa.ts"), None);
    }
}
