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

    /// Return `true` when `target`, or a file that the resolver tries for
    /// it, is a missing path that an ignore rule of the repository ignores.
    ///
    /// An import usually leaves out the extension (`./keys.generated` for
    /// `keys.generated.ts`), names a directory index, or names the `.js`
    /// output of a `.ts` file. The ignore rule names the file on disk, so
    /// each of these file names is tested too. When one of them exists, a
    /// missing generated file does not explain the unresolved import.
    ///
    /// A rule such as `*.js` or `index.js` ignores a file name that fallow
    /// built for any import, so it would hide a typo. A tried file name counts
    /// only when the rule does not also ignore the same file name built from
    /// another import name.
    fn ignores_missing_path(&mut self, target: &Path) -> bool {
        let candidates = module_file_candidates(target);
        if candidates.iter().any(|candidate| candidate.path.exists()) {
            return false;
        }
        candidates.iter().any(|candidate| {
            self.ignores_missing_file(&candidate.path)
                && candidate
                    .name_probe
                    .as_deref()
                    .is_none_or(|probe| !self.ignores_missing_file(probe))
        })
    }

    /// Return `true` when `target` does not exist and an ignore rule of the
    /// repository ignores it or one of its parent directories.
    fn ignores_missing_file(&mut self, target: &Path) -> bool {
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
                .dir_matcher(dir, &repo_root)
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

    fn dir_matcher(&mut self, dir: &Path, repo_root: &Path) -> &Gitignore {
        self.dir_matchers
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                build_matcher(
                    dir,
                    repo_root,
                    DIR_IGNORE_FILES.iter().map(|name| dir.join(name)),
                )
            })
    }

    fn exclude_matcher(&mut self, repo_root: &Path) -> &Gitignore {
        self.exclude.get_or_insert_with(|| {
            build_matcher(
                repo_root,
                repo_root,
                std::iter::once(repo_root.join(".git").join("info").join("exclude")),
            )
        })
    }
}

/// The module extensions that the resolver appends to a specifier.
const MODULE_EXTENSIONS: [&str; 9] = ["ts", "tsx", "d.ts", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The source extensions that a TypeScript import of an output extension
/// names, for example `./api.js` for `api.ts`.
const OUTPUT_TO_SOURCE_EXTENSIONS: [(&str, &[&str]); 4] = [
    ("js", &["ts", "tsx", "d.ts"]),
    ("jsx", &["tsx"]),
    ("mjs", &["mts", "d.mts"]),
    ("cjs", &["cts", "d.cts"]),
];

/// A file name that no import uses. It takes the place of the import name in
/// the name probe of a candidate.
const NAME_PROBE: &str = "fallow-name-probe";

/// A file that the resolver tries for the missing target of an import.
struct Candidate {
    path: PathBuf,
    /// The same file name built from [`NAME_PROBE`] in place of the import
    /// name. `None` for the target itself, which fallow does not build.
    name_probe: Option<PathBuf>,
}

/// `target` itself, then the files that the resolver tries for it: each
/// module extension appended, each directory index, and the source files of
/// an output extension.
fn module_file_candidates(target: &Path) -> Vec<Candidate> {
    let mut candidates = vec![Candidate {
        path: target.to_path_buf(),
        name_probe: None,
    }];
    let Some(name) = target.file_name().and_then(|name| name.to_str()) else {
        return candidates;
    };
    let probe_target = target.with_file_name(NAME_PROBE);
    for extension in MODULE_EXTENSIONS {
        candidates.push(Candidate {
            path: target.with_file_name(format!("{name}.{extension}")),
            name_probe: Some(probe_target.with_file_name(format!("{NAME_PROBE}.{extension}"))),
        });
    }
    for extension in MODULE_EXTENSIONS {
        let index = format!("index.{extension}");
        candidates.push(Candidate {
            path: target.join(&index),
            name_probe: Some(probe_target.join(&index)),
        });
    }
    if let Some((stem, extension)) = name.rsplit_once('.')
        && let Some((_, sources)) = OUTPUT_TO_SOURCE_EXTENSIONS
            .iter()
            .find(|(output, _)| *output == extension)
    {
        for source in *sources {
            candidates.push(Candidate {
                path: target.with_file_name(format!("{stem}.{source}")),
                name_probe: Some(probe_target.with_file_name(format!("{NAME_PROBE}.{source}"))),
            });
        }
    }
    candidates
}

/// Build one matcher rooted at `dir` from the ignore files that exist. A line
/// that does not parse is skipped, so the other lines of the file still apply.
/// A file that is not a regular file, a link out of `repo_root`, or a file
/// that is too large is not read.
fn build_matcher(dir: &Path, repo_root: &Path, files: impl Iterator<Item = PathBuf>) -> Gitignore {
    let mut builder = GitignoreBuilder::new(dir);
    for file in files.filter(|file| fallow_config::readable_ignore_file(file, repo_root)) {
        let _partial_error = builder.add(file);
    }
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}
