//! The ignore rules of a walk from a root below the top level of a
//! repository.
//!
//! The `ignore` crate applies the `.ignore` file of each parent directory of
//! the walk root, also above the top level of the repository. Git applies no
//! ignore file from above a repository. When the crate cannot keep its parent
//! rules (see `checked_walk_builder`), a walk below the top level reads no
//! parent ignore file through the crate. These rules apply each kind with the
//! precedence of the crate, over the directories of the walk and the parent
//! directories up to and including the top level:
//!
//! - the closest match of each kind wins;
//! - a `.ignore` match wins over a `.gitignore` match, which wins over an
//!   exclude match, which wins over a match of the global excludes file;
//! - `.gitignore` and exclude rules stop at the nearest directory with
//!   `.git`, so a nested repository keeps only its own rules.
//!
//! A walked directory is read the way the walker of the crate reads it. The
//! serial walker opens the ignore files of a directory by path, so on a file
//! system that ignores case `.GitIgnore` counts. The parallel walker takes
//! the names from the directory listing, so a name must match exactly. A
//! parent directory is read by path, as both walkers read it.
//!
//! The crate reads the ignore files of the walk itself, so it keeps that
//! work when it can:
//!
//! - Without a rule in a parent directory, the crate applies every rule and
//!   [`ParentRules::load`] gives `None`.
//! - Otherwise, when no parent `.ignore` rule re-includes (`!`), no parent
//!   rule can bring back an entry that a rule of the walk ignores. The crate
//!   applies the rules of the walk except the global excludes file, because
//!   a parent `.gitignore` match wins over a global match. Only an entry
//!   that a parent rule or the global file ignores needs the full
//!   precedence, so only its directories are probed again.
//! - A parent `.ignore` re-include wins over each other kind inside the
//!   walk, so the crate then reads no ignore file ([`ParentRules::full`])
//!   and each entry gets the full precedence.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustc_hash::FxHashMap;

use super::{DirReads, WalkKind, check, checked_global_gitignore, first_line};

/// A distinct number for each [`ParentRules`], the key of the chain cache of
/// a thread.
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// The last chain that this thread used: the rules id, the directory and
    /// the chain. Entries of one directory come one after another, so most
    /// lookups hit it.
    static LAST_CHAIN: RefCell<Option<(usize, PathBuf, Arc<DirChain>)>> =
        const { RefCell::new(None) };
}

/// The ignore files of one directory.
struct DirRules {
    dot_ignore: Gitignore,
    git_ignore: Gitignore,
    git_exclude: Gitignore,
    has_git: bool,
}

impl DirRules {
    fn is_empty(&self) -> bool {
        self.dot_ignore.is_empty() && self.git_ignore.is_empty() && self.git_exclude.is_empty()
    }
}

/// The rules of a walked directory and of each walked directory above it.
struct DirChain {
    rules: DirRules,
    parent: Option<Arc<DirChain>>,
    /// Some directory of this chain has a rule.
    has_rules: bool,
    /// Some directory of this chain is a repository.
    has_git: bool,
}

impl DirChain {
    fn walked(&self) -> impl Iterator<Item = &DirRules> {
        std::iter::successors(Some(self), |chain| chain.parent.as_deref()).map(|chain| &chain.rules)
    }
}

pub(super) struct ParentRules {
    id: usize,
    kind: WalkKind,
    root: PathBuf,
    canonical_root: PathBuf,
    /// The walk spells paths as canonical paths, so a parent rule matches
    /// the entry path as it is.
    root_is_canonical: bool,
    scope: PathBuf,
    reads: DirReads,
    /// The rules of each parent directory up to the top level, closest first.
    parents: Vec<DirRules>,
    parents_have_rules: bool,
    parents_have_git: bool,
    /// A parent `.ignore` rule re-includes.
    full: bool,
    global: Gitignore,
    /// The chain of each walked directory, by its path as the walk spells it.
    chains: RwLock<FxHashMap<PathBuf, Arc<DirChain>>>,
}

impl ParentRules {
    /// The rules of the parent directories of the walk root up to the top
    /// level `scope`. `None` when no parent directory has a rule.
    pub(super) fn load(
        root: &Path,
        canonical_root: &Path,
        scope: &Path,
        reads: DirReads,
        kind: WalkKind,
    ) -> Option<Self> {
        let parents: Vec<DirRules> = canonical_root
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(scope))
            .map(|dir| parent_rules(dir, reads, scope))
            .collect();
        if parents.iter().all(DirRules::is_empty) {
            return None;
        }
        let global = if reads.sources.git_global {
            checked_global_gitignore()
        } else {
            Gitignore::empty()
        };
        Some(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            kind,
            root: root.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            root_is_canonical: root == canonical_root,
            scope: scope.to_path_buf(),
            reads,
            parents_have_rules: parents.iter().any(|rules| !rules.is_empty()),
            parents_have_git: parents.iter().any(|rules| rules.has_git),
            full: parents
                .iter()
                .any(|rules| rules.dot_ignore.num_whitelists() > 0),
            parents,
            global,
            chains: RwLock::default(),
        })
    }

    /// The crate reads no ignore file of the walk, and each entry gets the
    /// full precedence here.
    pub(super) const fn full(&self) -> bool {
        self.full
    }

    /// True when the rules ignore the walk entry at `entry_path`.
    pub(super) fn ignores(&self, entry_path: &Path, is_dir: bool) -> bool {
        let Some(dir) = entry_path.parent().filter(|_| entry_path != self.root) else {
            return false;
        };
        let joined;
        let canonical = if self.root_is_canonical {
            Some(entry_path)
        } else {
            joined = entry_path
                .strip_prefix(&self.root)
                .ok()
                .map(|relative| self.canonical_root.join(relative));
            joined.as_deref()
        };
        if !self.full && !self.may_ignore(entry_path, canonical, is_dir) {
            return false;
        }
        self.with_chain(dir, |chain| {
            self.ignored_in(chain, entry_path, canonical, is_dir)
        })
    }

    /// The full precedence for an entry of the walked directory of `chain`.
    fn ignored_in(
        &self,
        chain: &DirChain,
        entry_path: &Path,
        canonical: Option<&Path>,
        is_dir: bool,
    ) -> bool {
        let any_git = chain.has_git || self.parents_have_git;
        let global_applies = any_git && !self.global.is_empty();
        if !chain.has_rules && !self.parents_have_rules && !global_applies {
            return false;
        }
        let walked = chain
            .walked()
            .map(|dir| matched(&dir.dot_ignore, entry_path, is_dir));
        let parents = self.parents.iter().map(|dir| {
            canonical.map_or(Match::None, |path| matched(&dir.dot_ignore, path, is_dir))
        });
        let dot_ignore = first_match(walked.chain(parents));
        if !dot_ignore.is_none() {
            return dot_ignore.is_ignore();
        }
        if any_git {
            let git = self.git_match(chain, entry_path, canonical, is_dir);
            if !git.is_none() {
                return git.is_ignore();
            }
        }
        global_applies && matched(&self.global, entry_path, is_dir).is_ignore()
    }

    /// Without a parent `.ignore` re-include, the crate already applied the
    /// rules of the walk and kept the entry. Only a parent rule or the global
    /// excludes file can then ignore it.
    fn may_ignore(&self, entry_path: &Path, canonical: Option<&Path>, is_dir: bool) -> bool {
        let parent = canonical.map_or(Match::None, |path| {
            let dot_ignore = first_match(
                self.parents
                    .iter()
                    .map(|dir| matched(&dir.dot_ignore, path, is_dir)),
            );
            let git_ignore = first_match(
                self.parents
                    .iter()
                    .map(|dir| matched(&dir.git_ignore, path, is_dir)),
            );
            dot_ignore.or(git_ignore).or(first_match(
                self.parents
                    .iter()
                    .map(|dir| matched(&dir.git_exclude, path, is_dir)),
            ))
        });
        parent.is_ignore() || matched(&self.global, entry_path, is_dir).is_ignore()
    }

    /// The `.gitignore` match, else the exclude match, over the directories
    /// up to and including the nearest repository.
    fn git_match(
        &self,
        chain: &DirChain,
        entry_path: &Path,
        canonical: Option<&Path>,
        is_dir: bool,
    ) -> Match<()> {
        let (mut git_ignore, mut git_exclude) = (Match::None, Match::None);
        let mut visit = |dir: &DirRules, path: Option<&Path>| {
            if let Some(path) = path {
                if git_ignore.is_none() {
                    git_ignore = matched(&dir.git_ignore, path, is_dir);
                }
                if git_exclude.is_none() {
                    git_exclude = matched(&dir.git_exclude, path, is_dir);
                }
            }
            dir.has_git
        };
        let reached_git = chain.walked().any(|dir| visit(dir, Some(entry_path)));
        if !reached_git {
            for dir in &self.parents {
                if visit(dir, canonical) {
                    break;
                }
            }
        }
        git_ignore.or(git_exclude)
    }

    /// Run `f` with the chain of the walked directory `dir`.
    fn with_chain<R>(&self, dir: &Path, f: impl FnOnce(&DirChain) -> R) -> R {
        LAST_CHAIN.with(|last| {
            let mut last = last.borrow_mut();
            if let Some((id, last_dir, chain)) = last.as_ref()
                && *id == self.id
                && last_dir == dir
            {
                return f(chain);
            }
            let chain = self.chain(dir);
            let result = f(&chain);
            *last = Some((self.id, dir.to_path_buf(), chain));
            result
        })
    }

    /// The chain of the walked directory `dir`, built once.
    fn chain(&self, dir: &Path) -> Arc<DirChain> {
        if let Ok(chains) = self.chains.read()
            && let Some(chain) = chains.get(dir)
        {
            return Arc::clone(chain);
        }
        let parent = (dir != self.root && dir.starts_with(&self.root))
            .then(|| dir.parent().map(|parent| self.chain(parent)))
            .flatten();
        let rules = walked_rules(dir, self.reads, &self.scope, self.kind);
        let chain = Arc::new(DirChain {
            has_rules: !rules.is_empty() || parent.as_ref().is_some_and(|chain| chain.has_rules),
            has_git: rules.has_git || parent.as_ref().is_some_and(|chain| chain.has_git),
            rules,
            parent,
        });
        if let Ok(mut chains) = self.chains.write() {
            chains.insert(dir.to_path_buf(), Arc::clone(&chain));
        }
        chain
    }
}

/// The ignore files that the walk reads, by kind.
#[derive(Default)]
struct Found {
    dot_ignore: bool,
    git_ignore: bool,
    git: bool,
    jj: bool,
}

/// The rules of a walked directory, read the way the walker `kind` reads
/// them. One `lstat` for each name finds the files. The parallel walker
/// takes the names from the listing, so only when a name exists does the
/// listing confirm its exact spelling.
fn walked_rules(dir: &Path, reads: DirReads, scope: &Path, kind: WalkKind) -> DirRules {
    let vcs = reads.sources.git_ignore || reads.sources.git_exclude;
    let exists = |read: bool, name: &str| read && std::fs::symlink_metadata(dir.join(name)).is_ok();
    let mut found = Found {
        dot_ignore: exists(reads.dot_ignore, ".ignore"),
        git_ignore: exists(reads.sources.git_ignore, ".gitignore"),
        git: exists(vcs, ".git"),
        jj: false,
    };
    found.jj = !found.git && exists(vcs, ".jj");
    let any = found.dot_ignore || found.git_ignore || found.git || found.jj;
    if any && kind == WalkKind::Parallel {
        found = listed_names(dir);
    }
    dir_rules(dir, reads, scope, &found)
}

/// The ignore file names of `dir` that its listing spells exactly.
fn listed_names(dir: &Path) -> Found {
    let mut found = Found::default();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            match entry.file_name().to_str() {
                Some(".ignore") => found.dot_ignore = true,
                Some(".gitignore") => found.git_ignore = true,
                Some(".git") => found.git = true,
                Some(".jj") => found.jj = true,
                _ => {}
            }
        }
    }
    found
}

/// The rules of a parent directory, read by path as the crate reads it.
fn parent_rules(dir: &Path, reads: DirReads, scope: &Path) -> DirRules {
    let found = Found {
        dot_ignore: true,
        git_ignore: true,
        git: true,
        jj: true,
    };
    dir_rules(dir, reads, scope, &found)
}

/// The ignore files of `dir` that the walk reads, rooted at `dir`. A
/// directory counts as a repository the way the crate counts it: it has a
/// `.git` or `.jj` entry, and the walk reads `.gitignore` or exclude files.
fn dir_rules(dir: &Path, reads: DirReads, scope: &Path, found: &Found) -> DirRules {
    let vcs = reads.sources.git_ignore || reads.sources.git_exclude;
    let has_git = vcs
        && ((found.git && dir.join(".git").metadata().is_ok())
            || (found.jj && dir.join(".jj").exists()));
    let git_exclude = if reads.sources.git_exclude && found.git && has_git {
        git_exclude_rules(dir)
    } else {
        Gitignore::empty()
    };
    DirRules {
        dot_ignore: file_rules(dir, reads.dot_ignore && found.dot_ignore, ".ignore", scope),
        git_ignore: file_rules(
            dir,
            reads.sources.git_ignore && found.git_ignore,
            ".gitignore",
            scope,
        ),
        git_exclude,
        has_git,
    }
}

/// The rules of the file `name` in `dir`. A file that fails the check gives
/// no rules: the walk already turned off its kind or skips its directory.
fn file_rules(dir: &Path, read: bool, name: &str, scope: &Path) -> Gitignore {
    if !read {
        return Gitignore::empty();
    }
    let path = dir.join(name);
    if check(&path, Some(scope)) != Ok(true) {
        return Gitignore::empty();
    }
    build(dir, &path)
}

/// The rules of the exclude file of the repository at `top`, rooted at
/// `top`. The file is found the same way as `resolve_git_commondir` in the
/// `ignore` crate finds it.
fn git_exclude_rules(top: &Path) -> Gitignore {
    let git = top.join(".git");
    let Ok(metadata) = std::fs::metadata(&git) else {
        return Gitignore::empty();
    };
    let common = if metadata.is_dir() {
        Some(git)
    } else {
        first_line(&git)
            .and_then(|line| line.strip_prefix("gitdir: ").map(PathBuf::from))
            .and_then(|git_dir| {
                let commondir = first_line(&git_dir.join("commondir"))?;
                Some(if commondir.starts_with('.') {
                    git_dir.join(commondir)
                } else {
                    PathBuf::from(commondir)
                })
            })
    };
    let Some(exclude) = common.map(|common| common.join("info").join("exclude")) else {
        return Gitignore::empty();
    };
    if check(&exclude, None) != Ok(true) {
        return Gitignore::empty();
    }
    build(top, &exclude)
}

fn build(root: &Path, file: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    let _ = builder.add(file);
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn matched(rules: &Gitignore, path: &Path, is_dir: bool) -> Match<()> {
    if rules.is_empty() {
        return Match::None;
    }
    match rules.matched(path, is_dir) {
        Match::None => Match::None,
        Match::Ignore(_) => Match::Ignore(()),
        Match::Whitelist(_) => Match::Whitelist(()),
    }
}

/// The first match of a list of matches ordered closest first.
fn first_match(matches: impl Iterator<Item = Match<()>>) -> Match<()> {
    matches
        .into_iter()
        .find(|found| !found.is_none())
        .unwrap_or(Match::None)
}
