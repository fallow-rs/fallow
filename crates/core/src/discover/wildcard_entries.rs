//! Expansion of wildcard subpath `exports` targets to entry points.

use std::path::{Path, PathBuf};

use fallow_graph::resolve::{MISSING_ONLY_OUTPUT_DIRS, OUTPUT_DIRS};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
use rustc_hash::FxHashMap;

use crate::plugins::config_parser::lexical_normalize;

/// Expand a wildcard subpath export target to the discovered files it matches.
///
/// Follows the Node subpath pattern rule: every `*` in the target stands for
/// the same substring, and that substring can contain `/`. So the target
/// `./src/*.ts` matches `src/top.ts` and also `src/nested/deep.ts`.
///
/// A target in build output also matches its source files, with the rule of
/// [`wildcard_source_patterns`].
pub(super) fn expand_wildcard_entries(
    base: &Path,
    pattern: &str,
    files: &[DiscoveredFile],
    entries: &mut Vec<EntryPoint>,
) {
    let before = entries.len();
    push_wildcard_matches(base, pattern, files, entries);
    let has_direct_match = entries.len() > before;
    for source_pattern in wildcard_source_patterns(pattern, has_direct_match) {
        push_wildcard_matches(base, &source_pattern, files, entries);
    }
}

/// JavaScript output file suffixes that a build makes from a source file.
const WILDCARD_OUTPUT_SUFFIXES: &[&str] =
    &[".d.ts", ".d.mts", ".d.cts", ".js", ".mjs", ".cjs", ".jsx"];

/// Source extensions that a JavaScript output file can come from.
const WILDCARD_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// Map a wildcard export target in build output to source patterns.
///
/// The rule is the same as for a package entry without a wildcard:
///
/// - The last output directory (`dist`, `esm`, ...) before the first `*`
///   becomes `src`, so `./esm/*.mjs` maps to `./src/*.ts`.
/// - A `lib` directory becomes `src` only when the target matches no file.
/// - A target at the package root, such as `./*.js`, maps into `src` only
///   when it matches no file. A package that publishes its build directory
///   as the package root has this shape.
///
/// A JavaScript output suffix (`.js`, `.d.ts`, ...) becomes each source
/// extension. Other suffixes stay. The result is empty when no rule applies.
fn wildcard_source_patterns(pattern: &str, has_direct_match: bool) -> Vec<String> {
    let relative = pattern.trim_start_matches("./");
    let segments: Vec<&str> = relative.split('/').collect();
    let literal_count = segments
        .iter()
        .position(|segment| segment.contains('*'))
        .unwrap_or(segments.len());
    let literal = &segments[..literal_count];
    let source_segments: Vec<&str> = if let Some(position) = literal
        .iter()
        .rposition(|segment| OUTPUT_DIRS.contains(segment))
    {
        replace_segment_with_src(&segments, position)
    } else if has_direct_match {
        return Vec::new();
    } else if let Some(position) = literal
        .iter()
        .rposition(|segment| MISSING_ONLY_OUTPUT_DIRS.contains(segment))
    {
        replace_segment_with_src(&segments, position)
    } else if literal_count == 0 {
        std::iter::once("src")
            .chain(segments.iter().copied())
            .collect()
    } else {
        return Vec::new();
    };
    let source_pattern = format!("./{}", source_segments.join("/"));

    let Some(star) = source_pattern.rfind('*') else {
        return Vec::new();
    };
    let (head, suffix) = source_pattern.split_at(star + 1);
    match WILDCARD_OUTPUT_SUFFIXES
        .iter()
        .find_map(|output| suffix.strip_suffix(output))
    {
        Some(stem) => WILDCARD_SOURCE_EXTENSIONS
            .iter()
            .map(|extension| format!("{head}{stem}.{extension}"))
            .collect(),
        None => vec![source_pattern],
    }
}

fn replace_segment_with_src<'a>(segments: &[&'a str], position: usize) -> Vec<&'a str> {
    let mut source = segments.to_vec();
    source[position] = "src";
    source
}

/// Push the discovered files that match one wildcard target.
fn push_wildcard_matches(
    base: &Path,
    pattern: &str,
    files: &[DiscoveredFile],
    entries: &mut Vec<EntryPoint>,
) {
    let target = lexical_normalize(&base.join(pattern));
    let literal_dir: PathBuf = target
        .components()
        .take_while(|component| !component.as_os_str().to_string_lossy().contains('*'))
        .collect();
    let target = target.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = target.split('*').collect();
    let package_root = lexical_normalize(base);
    let mut nested_packages = FxHashMap::default();
    for file in files {
        if !file.path.starts_with(&literal_dir) {
            continue;
        }
        let path = file.path.to_string_lossy().replace('\\', "/");
        if matches_subpath_pattern(&path, &parts)
            && !is_in_nested_package(&file.path, &package_root, &mut nested_packages)
        {
            entries.push(EntryPoint {
                path: file.path.clone(),
                source: EntryPointSource::PackageJsonExports,
            });
        }
    }
}

/// Whether `file` belongs to a package below `package_root`: a directory
/// between the two has its own `package.json`. A `*` can match `/`, so
/// `./*.js` of the root package also matches the files of nested workspace
/// packages, which the root package does not export.
fn is_in_nested_package(
    file: &Path,
    package_root: &Path,
    cache: &mut FxHashMap<PathBuf, bool>,
) -> bool {
    let mut dir = file.parent();
    while let Some(current) = dir {
        if current == package_root || !current.starts_with(package_root) {
            return false;
        }
        let is_package = *cache
            .entry(current.to_path_buf())
            .or_insert_with(|| current.join("package.json").is_file());
        if is_package {
            return true;
        }
        dir = current.parent();
    }
    false
}

/// Whether `path` matches a target split at its `*` characters, with one
/// non-empty substring in place of every `*`.
fn matches_subpath_pattern(path: &str, parts: &[&str]) -> bool {
    let [prefix, .., suffix] = parts else {
        return false;
    };
    if !path.starts_with(prefix) || !path.ends_with(suffix) {
        return false;
    }
    let literal_len: usize = parts.iter().map(|part| part.len()).sum();
    let stars = parts.len() - 1;
    let Some(captured_len) = path.len().checked_sub(literal_len) else {
        return false;
    };
    if captured_len == 0 || captured_len % stars != 0 {
        return false;
    }
    let Some(capture) = path.get(prefix.len()..prefix.len() + captured_len / stars) else {
        return false;
    };
    stars == 1 || parts.join(capture) == path
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::discover::FileId;

    fn discovered_files_under(root: &Path) -> Vec<DiscoveredFile> {
        let mut paths = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        paths
            .into_iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).unwrap()),
                path,
                size_bytes: 1,
            })
            .collect()
    }

    fn entry_names(entries: &[EntryPoint], root: &Path) -> Vec<String> {
        let mut names: Vec<String> = entries
            .iter()
            .map(|ep| {
                ep.path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn expands_wildcard_css_entries() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let themes = dir.path().join("src").join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("dark.css"), ":root { --bg: #000; }").unwrap();
        std::fs::write(themes.join("light.css"), ":root { --bg: #fff; }").unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/themes/*.css", &files, &mut entries);

        assert_eq!(entries.len(), 2, "should expand wildcard to 2 CSS files");
        let paths: Vec<String> = entries
            .iter()
            .map(|ep| ep.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"dark.css".to_string()));
        assert!(paths.contains(&"light.css".to_string()));
        assert!(
            entries
                .iter()
                .all(|ep| matches!(ep.source, EntryPointSource::PackageJsonExports))
        );
    }

    #[test]
    fn wildcard_does_not_match_nonexistent_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("src/themes")).unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/themes/*.css", &files, &mut entries);

        assert!(
            entries.is_empty(),
            "should return empty when no files match the wildcard"
        );
    }

    #[test]
    fn wildcard_only_matches_specified_extension() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let themes = dir.path().join("src").join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("dark.css"), ":root {}").unwrap();
        std::fs::write(themes.join("index.ts"), "export {};").unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/themes/*.css", &files, &mut entries);

        assert_eq!(entries.len(), 1, "should only match CSS files");
        assert!(
            entries[0]
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(".css")
        );
    }

    #[test]
    fn wildcard_matches_files_in_nested_directories() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("nested/deeper")).unwrap();
        std::fs::write(src.join("top.ts"), "export {};").unwrap();
        std::fs::write(src.join("nested/deep.ts"), "export {};").unwrap();
        std::fs::write(src.join("nested/deeper/leaf.ts"), "export {};").unwrap();
        std::fs::write(src.join("nested/helper.js"), "export {};").unwrap();
        std::fs::write(dir.path().join("outside.ts"), "export {};").unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/*.ts", &files, &mut entries);

        assert_eq!(
            entry_names(&entries, dir.path()),
            vec![
                "src/nested/deep.ts",
                "src/nested/deeper/leaf.ts",
                "src/top.ts"
            ],
        );
    }

    #[test]
    fn wildcard_without_suffix_matches_every_nested_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let src = dir.path().join("src");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("top.ts"), "export {};").unwrap();
        std::fs::write(src.join("a/b/orphan.tsx"), "export {};").unwrap();
        std::fs::write(dir.path().join("index.ts"), "export {};").unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/*", &files, &mut entries);

        assert_eq!(
            entry_names(&entries, dir.path()),
            vec!["src/a/b/orphan.tsx", "src/top.ts"],
        );
    }

    #[test]
    fn wildcard_source_patterns_map_output_dirs_to_src() {
        assert_eq!(
            wildcard_source_patterns("./dist/*.d.ts", true)[0],
            "./src/*.ts"
        );
        assert_eq!(
            wildcard_source_patterns("./esm/*.mjs", false),
            WILDCARD_SOURCE_EXTENSIONS
                .iter()
                .map(|extension| format!("./src/*.{extension}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            wildcard_source_patterns("./packages/ui/dist/*", true),
            vec!["./packages/ui/src/*"]
        );
    }

    #[test]
    fn wildcard_source_patterns_keep_present_lib_and_root_targets() {
        assert!(wildcard_source_patterns("./lib/*.js", true).is_empty());
        assert_eq!(
            wildcard_source_patterns("./lib/*.js", false)[0],
            "./src/*.ts"
        );
        assert!(wildcard_source_patterns("./*.js", true).is_empty());
        assert_eq!(wildcard_source_patterns("./*.js", false)[0], "./src/*.ts");
        assert!(wildcard_source_patterns("./themes/*.css", false).is_empty());
    }

    #[test]
    fn wildcard_with_several_stars_requires_one_substring() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("src/button")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/card")).unwrap();
        std::fs::write(dir.path().join("src/button/button.ts"), "export {};").unwrap();
        std::fs::write(dir.path().join("src/card/other.ts"), "export {};").unwrap();

        let files = discovered_files_under(dir.path());
        let mut entries = Vec::new();
        expand_wildcard_entries(dir.path(), "./src/*/*.ts", &files, &mut entries);

        assert_eq!(
            entry_names(&entries, dir.path()),
            vec!["src/button/button.ts"],
        );
    }
}
