//! Gitignore rules for the missing targets of unresolved imports.
//!
//! A relative import of a build output (`../dist/out.js`) or of generated code
//! (`./generated/client`) does not resolve before the build runs. The same
//! applies to a package subpath (`store/enums`) whose `exports` entry names
//! generated code. When the repository ignores the target path, the import is
//! not a defect of the source, so the unresolved-import check does not report
//! it.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustc_hash::FxHashMap;

use crate::plugins::config_parser::lexical_normalize;

/// The ignore files that apply to the directory that holds them. A later file
/// overrides an earlier one, so `.ignore` overrides `.gitignore`, as in source
/// discovery.
const DIR_IGNORE_FILES: [&str; 2] = [".gitignore", ".ignore"];

/// Matches the missing target of an unresolved import against the ignore
/// files of the git repository that holds the project root.
///
/// The machine global excludes file is not read, because a rule there would
/// change the findings from one machine to another. The repository root and
/// each matcher are found on the first query that needs them, so a project
/// without such unresolved imports reads no ignore file.
pub(super) struct GitignoredTargets<'a> {
    root: &'a Path,
    repo_root: OnceCell<Option<PathBuf>>,
    dir_matchers: FxHashMap<PathBuf, Gitignore>,
    exclude: Option<Gitignore>,
}

impl<'a> GitignoredTargets<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            root,
            repo_root: OnceCell::new(),
            dir_matchers: FxHashMap::default(),
            exclude: None,
        }
    }

    /// Return `true` when `spec` is relative, its target path does not exist,
    /// and an ignore rule of the repository ignores the target path or one of
    /// its parent directories. Outside a git repository this is always
    /// `false`, the same as source discovery, which applies gitignore rules
    /// only inside a git repository.
    pub(super) fn ignores_missing_target(&mut self, importer: &Path, spec: &str) -> bool {
        if !spec.starts_with("./") && !spec.starts_with("../") {
            return false;
        }
        let Some(importer_dir) = importer.parent() else {
            return false;
        };
        self.ignores_missing_path(&lexical_normalize(&importer_dir.join(spec)))
    }

    /// Return `true` when `paths` is not empty and each path is a missing
    /// path that an ignore rule of the repository ignores. The resolver
    /// records these paths for a bare specifier whose package `exports` entry
    /// names only missing paths.
    pub(super) fn ignores_missing_paths(&mut self, paths: &[PathBuf]) -> bool {
        !paths.is_empty() && paths.iter().all(|path| self.ignores_missing_path(path))
    }

    /// Return `true` when `target` does not exist and an ignore rule of the
    /// repository ignores it or one of its parent directories.
    fn ignores_missing_path(&mut self, target: &Path) -> bool {
        if target.exists() {
            return false;
        }
        let Some(repo_root) = self.repo_root() else {
            return false;
        };
        if target == repo_root || !target.starts_with(&repo_root) {
            return false;
        }

        // The deepest directory decides first, because a nested ignore file
        // overrides the rules of its parent directories.
        for dir in target.ancestors().skip(1) {
            if !dir.starts_with(&repo_root) {
                break;
            }
            if !dir.is_dir() {
                continue;
            }
            match self
                .dir_matcher(dir)
                .matched_path_or_any_parents(target, false)
            {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        self.exclude_matcher(&repo_root)
            .matched_path_or_any_parents(target, false)
            .is_ignore()
    }

    fn repo_root(&self) -> Option<PathBuf> {
        self.repo_root
            .get_or_init(|| {
                self.root
                    .ancestors()
                    .find(|dir| dir.join(".git").exists())
                    .map(Path::to_path_buf)
            })
            .clone()
    }

    fn dir_matcher(&mut self, dir: &Path) -> &Gitignore {
        self.dir_matchers
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                build_matcher(dir, DIR_IGNORE_FILES.iter().map(|name| dir.join(name)))
            })
    }

    fn exclude_matcher(&mut self, repo_root: &Path) -> &Gitignore {
        self.exclude.get_or_insert_with(|| {
            build_matcher(
                repo_root,
                std::iter::once(repo_root.join(".git").join("info").join("exclude")),
            )
        })
    }
}

/// Build one matcher rooted at `dir` from the ignore files that exist. A line
/// that does not parse is skipped, so the other lines of the file still apply.
fn build_matcher(dir: &Path, files: impl Iterator<Item = PathBuf>) -> Gitignore {
    let mut builder = GitignoreBuilder::new(dir);
    for file in files.filter(|file| file.is_file()) {
        let _partial_error = builder.add(file);
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}
