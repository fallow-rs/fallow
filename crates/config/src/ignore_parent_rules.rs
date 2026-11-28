//! The ignore rules of the directories between a walk root and the top level
//! of its repository.
//!
//! A walk inside a repository sets `parents(false)` on the `ignore` crate, so
//! the crate applies no rule from a directory above the walk root. These
//! rules then apply the ignore files of the directories from the parent of
//! the root up to and including the top level, with the precedence of the
//! crate: for each kind of ignore file the closest match wins, and a match of
//! `.ignore` rules wins over `.gitignore` rules, which win over the exclude
//! file. The `.gitignore` and exclude rules of the repository do not apply
//! below a nested repository. No file above the top level applies.
//!
//! The crate drops an entry that its own rules ignore before the walk filter
//! sees it. A parent `.ignore` file that re-includes (`!`) such an entry
//! therefore does not bring it back.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustc_hash::FxHashMap;

use super::{DirReads, check, first_line};

/// The ignore files of one directory inside the walk.
struct DirRules {
    dot_ignore: Gitignore,
    git_ignore: Gitignore,
    has_git: bool,
}

pub(super) struct ParentRules {
    root: PathBuf,
    canonical_root: PathBuf,
    scope: PathBuf,
    reads: DirReads,
    /// The rules of each parent directory, closest first.
    dot_ignore: Vec<Gitignore>,
    git_ignore: Vec<Gitignore>,
    git_exclude: Option<Gitignore>,
    walked_dirs: Mutex<FxHashMap<PathBuf, Arc<DirRules>>>,
}

impl ParentRules {
    /// The rules of the parent directories of `canonical_root` up to and
    /// including `scope`. `None` when no parent file has a rule.
    pub(super) fn load(
        root: &Path,
        canonical_root: &Path,
        scope: &Path,
        reads: DirReads,
    ) -> Option<Self> {
        let parents: Vec<&Path> = canonical_root
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(scope))
            .collect();
        let dot_ignore = rules_of(&parents, reads.dot_ignore, ".ignore", scope);
        let git_ignore = rules_of(&parents, reads.sources.git_ignore, ".gitignore", scope);
        let git_exclude = reads
            .sources
            .git_exclude
            .then(|| git_exclude_rules(scope))
            .flatten();
        if dot_ignore.is_empty() && git_ignore.is_empty() && git_exclude.is_none() {
            return None;
        }
        Some(Self {
            root: root.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            scope: scope.to_path_buf(),
            reads,
            dot_ignore,
            git_ignore,
            git_exclude,
            walked_dirs: Mutex::default(),
        })
    }

    /// True when a parent rule ignores the walk entry at `entry_path`.
    pub(super) fn ignores(&self, entry_path: &Path, is_dir: bool) -> bool {
        let Ok(relative) = entry_path.strip_prefix(&self.root) else {
            return false;
        };
        if relative.as_os_str().is_empty() {
            return false;
        }
        let path = self.canonical_root.join(relative);
        let parent_dot_ignore = closest(self.dot_ignore.iter(), &path, is_dir);
        let parent_git_ignore = closest(self.git_ignore.iter(), &path, is_dir);
        let parent_exclude = self
            .git_exclude
            .as_ref()
            .map_or(Match::None, |rules| matched(rules, &path, is_dir));
        if parent_dot_ignore.is_none() && parent_git_ignore.is_none() && parent_exclude.is_none() {
            return false;
        }
        let walked = self.walked_dirs_of(&path);
        let walked_dot_ignore = closest(walked.iter().map(|dir| &dir.dot_ignore), &path, is_dir);
        let dot_ignore = walked_dot_ignore.or(parent_dot_ignore);
        if !dot_ignore.is_none() {
            return dot_ignore.is_ignore();
        }
        if walked.iter().any(|dir| dir.has_git) {
            return false;
        }
        let walked_git_ignore = closest(walked.iter().map(|dir| &dir.git_ignore), &path, is_dir);
        let git_ignore = walked_git_ignore.or(parent_git_ignore);
        if !git_ignore.is_none() {
            return git_ignore.is_ignore();
        }
        parent_exclude.is_ignore()
    }

    /// The rules of each walked directory that holds `path`, closest first:
    /// from the parent of `path` up to and including the walk root.
    fn walked_dirs_of(&self, path: &Path) -> Vec<Arc<DirRules>> {
        path.ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&self.canonical_root))
            .map(|dir| self.walked_dir(dir))
            .collect()
    }

    fn walked_dir(&self, dir: &Path) -> Arc<DirRules> {
        if let Ok(cache) = self.walked_dirs.lock()
            && let Some(rules) = cache.get(dir)
        {
            return Arc::clone(rules);
        }
        let rules = Arc::new(DirRules {
            dot_ignore: dir_rules(dir, self.reads.dot_ignore, ".ignore", &self.scope),
            git_ignore: dir_rules(
                dir,
                self.reads.sources.git_ignore,
                ".gitignore",
                &self.scope,
            ),
            has_git: dir != self.canonical_root && dir.join(".git").exists(),
        });
        if let Ok(mut cache) = self.walked_dirs.lock() {
            cache.insert(dir.to_path_buf(), Arc::clone(&rules));
        }
        rules
    }
}

/// The rules of the file `name` in each of `dirs` that has one.
fn rules_of(dirs: &[&Path], read: bool, name: &str, scope: &Path) -> Vec<Gitignore> {
    dirs.iter()
        .map(|dir| dir_rules(dir, read, name, scope))
        .filter(|rules| !rules.is_empty())
        .collect()
}

/// The rules of the file `name` in `dir`, rooted at `dir`. A file that fails
/// the check gives no rules: the walk already turned off its kind or skips
/// its directory.
fn dir_rules(dir: &Path, read: bool, name: &str, scope: &Path) -> Gitignore {
    let path = dir.join(name);
    if !read || check(&path, Some(scope)) != Ok(true) {
        return Gitignore::empty();
    }
    build(dir, &path)
}

/// The rules of the exclude file of the repository at `top`, rooted at
/// `top`. The file is found the same way as `resolve_git_commondir` in the
/// `ignore` crate finds it.
fn git_exclude_rules(top: &Path) -> Option<Gitignore> {
    let git = top.join(".git");
    let metadata = std::fs::metadata(&git).ok()?;
    let common = if metadata.is_dir() {
        git
    } else {
        let git_dir = PathBuf::from(first_line(&git)?.strip_prefix("gitdir: ")?);
        let commondir = first_line(&git_dir.join("commondir"))?;
        if commondir.starts_with('.') {
            git_dir.join(commondir)
        } else {
            PathBuf::from(commondir)
        }
    };
    let exclude = common.join("info").join("exclude");
    if check(&exclude, None) != Ok(true) {
        return None;
    }
    Some(build(top, &exclude)).filter(|rules| !rules.is_empty())
}

fn build(root: &Path, file: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    let _ = builder.add(file);
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn matched(rules: &Gitignore, path: &Path, is_dir: bool) -> Match<()> {
    match rules.matched(path, is_dir) {
        Match::None => Match::None,
        Match::Ignore(_) => Match::Ignore(()),
        Match::Whitelist(_) => Match::Whitelist(()),
    }
}

/// The first match of `rules`, which are ordered closest first.
fn closest<'a>(rules: impl Iterator<Item = &'a Gitignore>, path: &Path, is_dir: bool) -> Match<()> {
    rules
        .map(|rules| matched(rules, path, is_dir))
        .find(|found| !found.is_none())
        .unwrap_or(Match::None)
}
