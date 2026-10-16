//! Expansion of a glob from a plugin config, such as a `test.projects`
//! entry, against the file system.
//!
//! The walk does not enter `node_modules`, `.git`, or a gitignored path.
//! A pnpm workspace links each dependency, and each workspace package, into
//! `node_modules`, so a walk that follows these links reads the dependency
//! store once for each workspace, and a workspace cycle has no end. Source
//! discovery does not analyze a gitignored file, so the walk skips the same
//! paths, for example a local build output. The walk follows a directory
//! symlink outside `node_modules` when its target is inside the project root,
//! and it stops at a symlink loop. The walk never reads a directory outside
//! the project root.

use std::path::{Path, PathBuf};

use globset::GlobBuilder;

/// Directory names that the walk never enters.
const PRUNED_DIRS: &[&str] = &["node_modules", ".git"];

/// The maximum number of entries (files and directories) that one expansion
/// reads.
const MAX_VISITED_ENTRIES: usize = 200_000;

/// The result of one glob expansion.
#[derive(Debug, Default)]
pub struct GlobExpansion {
    /// The files and directories that match, in path order.
    pub paths: Vec<PathBuf>,
    /// The number of entries (files and directories) that the walk read.
    pub visited: usize,
    /// Whether the walk stopped at the entry limit before its end.
    pub truncated: bool,
}

/// Expand `pattern`, relative to `base`, against the file system.
///
/// The pattern uses `/` as separator, and also `\` on Windows. An absolute
/// pattern replaces `base`.
/// `*` and `?` do not match `/`, and `**` matches zero or more directories.
/// The walk starts at the literal directory before the first glob segment.
/// A pattern without `**` limits the walk depth to its segment count.
/// The walk does not start, and does not descend, into a directory whose
/// canonical path is outside `root`.
#[must_use]
pub fn expand(base: &Path, pattern: &str, root: &Path) -> GlobExpansion {
    let expansion = expand_with_limit(base, pattern, root, MAX_VISITED_ENTRIES);
    if expansion.truncated {
        tracing::warn!(
            "the glob '{pattern}' in {} reads more than {MAX_VISITED_ENTRIES} entries; \
             the walk stopped and later matches are not used",
            base.display()
        );
    }
    expansion
}

/// Whether `path` has a `node_modules` component below `root`. A test
/// project inside a dependency is not a project of the analyzed repository.
#[must_use]
pub fn is_in_node_modules(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str() == "node_modules")
}

fn expand_with_limit(base: &Path, pattern: &str, root: &Path, max_visited: usize) -> GlobExpansion {
    let mut expansion = GlobExpansion::default();
    // Windows also accepts `\` as separator.
    let pattern = if cfg!(windows) {
        std::borrow::Cow::Owned(pattern.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(pattern)
    };
    let pattern = pattern.strip_prefix("./").unwrap_or(&pattern);
    let literal_len = pattern
        .split_inclusive('/')
        .take_while(|segment| !is_glob_segment(segment))
        .map(str::len)
        .sum();
    let (literal, glob) = pattern.split_at(literal_len);
    let walk_root = if literal.is_empty() {
        base.to_path_buf()
    } else {
        base.join(literal.trim_end_matches('/'))
    };
    let rest: Vec<&str> = glob
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    if rest.is_empty() {
        if walk_root.exists() {
            expansion.paths.push(walk_root);
        }
        return expansion;
    }
    if !walk_root.is_dir() {
        return expansion;
    }
    // Resolve the root once. A symlink in the literal base, or a symlinked
    // directory in the walk, can point out of the project.
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if !resolves_inside(&walk_root, &root) {
        return expansion;
    }
    let Ok(glob) = GlobBuilder::new(&rest.join("/"))
        .literal_separator(true)
        .build()
    else {
        return expansion;
    };
    let matcher = glob.compile_matcher();
    let max_depth = (!rest.contains(&"**")).then_some(rest.len());

    let mut builder = fallow_config::source_walk_builder(&walk_root);
    builder
        .follow_links(true)
        .max_depth(max_depth)
        .sort_by_file_name(std::cmp::Ord::cmp)
        .filter_entry(move |entry| {
            if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                return true;
            }
            if PRUNED_DIRS
                .iter()
                .any(|name| entry.file_name() == std::ffi::OsStr::new(name))
            {
                return false;
            }
            // A real directory below an accepted directory is inside the
            // root, so only a symlink needs the canonical check.
            !entry.path_is_symlink() || resolves_inside(entry.path(), &root)
        });
    for entry in builder.build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        expansion.visited += 1;
        if expansion.visited > max_visited {
            expansion.truncated = true;
            break;
        }
        let Ok(relative) = entry.path().strip_prefix(&walk_root) else {
            continue;
        };
        if matcher.is_match(relative) {
            expansion.paths.push(entry.into_path());
        }
    }
    expansion.paths.sort();
    expansion
}

/// Whether the canonical form of `path` is inside `root`, which is already
/// canonical. This is the check of `config_parser::is_inside_root`, without
/// a second canonicalization of the root for each entry.
fn resolves_inside(path: &Path, root: &Path) -> bool {
    std::fs::canonicalize(path).is_ok_and(|resolved| resolved.starts_with(root))
}

/// Whether a pattern segment holds a glob character.
fn is_glob_segment(segment: &str) -> bool {
    segment.contains(['*', '?', '[', '{'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, file: &str) {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, "").expect("write file");
    }

    fn relative(root: &Path, expansion: &GlobExpansion) -> Vec<String> {
        expansion
            .paths
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .expect("inside root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn single_star_matches_one_segment() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "apps/a/vitest.config.ts");
        touch(root, "apps/a/deep/vitest.config.ts");
        touch(root, "apps/.hidden/vitest.config.ts");
        let expansion = expand(root, "apps/*/vitest.config.ts", root);
        assert_eq!(
            relative(root, &expansion),
            vec!["apps/.hidden/vitest.config.ts", "apps/a/vitest.config.ts"]
        );
    }

    #[test]
    fn double_star_matches_zero_or_more_directories() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "apps/vitest.config.ts");
        touch(root, "apps/a/vitest.config.ts");
        touch(root, "apps/a/b/vitest.config.ts");
        touch(root, "libs/c/vitest.config.ts");
        let expansion = expand(root, "./apps/**/vitest.config.ts", root);
        assert_eq!(
            relative(root, &expansion),
            vec![
                "apps/a/b/vitest.config.ts",
                "apps/a/vitest.config.ts",
                "apps/vitest.config.ts",
            ]
        );
    }

    #[test]
    fn directories_match_and_a_literal_pattern_gives_itself() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "packages/a/package.json");
        touch(root, "packages/b/package.json");
        assert_eq!(
            relative(root, &expand(root, "packages/*", root)),
            vec!["packages/a", "packages/b"]
        );
        assert_eq!(
            relative(root, &expand(root, "packages/a/package.json", root)),
            vec!["packages/a/package.json"]
        );
        assert!(expand(root, "missing/*", root).paths.is_empty());
    }

    #[test]
    fn walk_does_not_enter_node_modules_or_git() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "apps/a/vitest.config.ts");
        for index in 0..20 {
            touch(
                root,
                &format!("apps/a/node_modules/dep{index}/lib/x/vitest.config.ts"),
            );
            touch(
                root,
                &format!("apps/a/.git/objects/{index}/vitest.config.ts"),
            );
        }
        let expansion = expand(root, "apps/**/vitest.config.ts", root);
        assert_eq!(relative(root, &expansion), vec!["apps/a/vitest.config.ts"]);
        // Only `apps/a` and its config: the walk reads nothing in the two
        // pruned directories.
        assert_eq!(expansion.visited, 2);
    }

    /// A local build output is gitignored and can hold many thousands of
    /// files. The walk must not read it, so a project after it still matches.
    #[test]
    fn walk_does_not_enter_gitignored_directories() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        std::fs::create_dir(root.join(".git")).expect("create git dir");
        std::fs::write(root.join(".gitignore"), ".next/\n").expect("write gitignore");
        for index in 0..20 {
            touch(
                root,
                &format!("apps/a/.next/cache/{index}/vitest.e2e.config.ts"),
            );
        }
        touch(root, "apps/b/vitest.e2e.config.ts");
        let expansion = expand_with_limit(root, "apps/**/vitest.e2e.config.ts", root, 3);
        assert!(!expansion.truncated);
        assert_eq!(
            relative(root, &expansion),
            vec!["apps/b/vitest.e2e.config.ts"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn walk_follows_directory_symlinks_and_stops_at_a_loop() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "real/b/vitest.e2e.config.ts");
        touch(root, "apps/a/vitest.e2e.config.ts");
        std::os::unix::fs::symlink("../real/b", root.join("apps/b")).expect("create symlink");
        std::os::unix::fs::symlink("..", root.join("apps/a/cycle")).expect("create symlink");
        let expected = vec!["apps/a/vitest.e2e.config.ts", "apps/b/vitest.e2e.config.ts"];
        assert_eq!(
            relative(root, &expand(root, "apps/*/vitest.e2e.config.ts", root)),
            expected
        );
        assert_eq!(
            relative(root, &expand(root, "apps/**/vitest.e2e.config.ts", root)),
            expected
        );
    }

    /// A symlink inside the root can point out of the project, for example
    /// to `/`. The walk must not read the target, so it cannot list the
    /// file system above the project.
    #[cfg(unix)]
    #[test]
    fn walk_does_not_enter_a_symlink_that_points_out_of_the_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        touch(root, "apps/a/vitest.config.ts");
        for index in 0..20 {
            touch(outside.path(), &format!("x{index}/vitest.config.ts"));
        }
        std::os::unix::fs::symlink(outside.path(), root.join("apps/out")).expect("create symlink");
        let expansion = expand(root, "apps/**/vitest.config.ts", root);
        assert_eq!(relative(root, &expansion), vec!["apps/a/vitest.config.ts"]);
        // Only `apps/a` and its config: the walk reads nothing in the target.
        assert_eq!(expansion.visited, 2);
    }

    /// One directory can hold very many files, so the limit counts files too.
    #[test]
    fn walk_limit_counts_files_in_one_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        for index in 0..20 {
            touch(root, &format!("apps/a/file{index:02}.ts"));
        }
        let expansion = expand_with_limit(root, "apps/**/vitest.config.ts", root, 10);
        assert!(expansion.truncated);
        assert_eq!(expansion.visited, 11);
    }

    #[test]
    fn walk_stops_at_the_entry_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        for index in 0..10 {
            touch(root, &format!("apps/a{index}/vitest.config.ts"));
        }
        // Each project gives two entries: the directory and its config.
        let expansion = expand_with_limit(root, "apps/**/vitest.config.ts", root, 8);
        assert!(expansion.truncated);
        assert_eq!(expansion.visited, 9);
        assert_eq!(
            relative(root, &expansion),
            vec![
                "apps/a0/vitest.config.ts",
                "apps/a1/vitest.config.ts",
                "apps/a2/vitest.config.ts",
                "apps/a3/vitest.config.ts",
            ]
        );
    }

    #[test]
    fn node_modules_component_is_checked_below_the_root() {
        let root = Path::new("/work/node_modules/app");
        assert!(!is_in_node_modules(
            &root.join("apps/a/vitest.config.ts"),
            root
        ));
        assert!(is_in_node_modules(
            &root.join("node_modules/pkg/vitest.config.ts"),
            root
        ));
    }
}
