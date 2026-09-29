//! Workspace packages that a package-manager command selects by name or by
//! directory, such as `yarn workspace web`, `npm -w web`, or
//! `pnpm --filter './packages/*'`.
//!
//! A command in a selected package resolves its file arguments against the
//! directory of that package. The selection resolves to those directories,
//! relative to the package that contains the command.

#[expect(
    clippy::disallowed_types,
    reason = "package.json scripts are deserialized as std HashMap"
)]
use std::collections::HashMap;

/// One workspace package: its name, its directory relative to the project
/// root, and its `scripts` map.
#[derive(Debug)]
pub struct WorkspacePackage {
    name: String,
    dir: String,
    /// `true` for the root package of the project (the workspace root).
    root: bool,
    #[expect(
        clippy::disallowed_types,
        reason = "package.json scripts are deserialized as std HashMap"
    )]
    scripts: HashMap<String, String>,
}

impl WorkspacePackage {
    /// The directory of the package, relative to the project root.
    pub fn dir(&self) -> &str {
        &self.dir
    }

    /// The `scripts` map of the package.
    #[expect(
        clippy::disallowed_types,
        reason = "package.json scripts are deserialized as std HashMap"
    )]
    pub const fn scripts(&self) -> &HashMap<String, String> {
        &self.scripts
    }
}

/// The workspace packages of a project, so that a command that selects a
/// package (`yarn workspace web node scripts/a.ts`) resolves its file
/// arguments in the directory of that package.
#[derive(Debug, Default)]
pub struct WorkspacePackages {
    packages: Vec<WorkspacePackage>,
}

impl WorkspacePackages {
    /// Add a workspace package. `dir` is relative to the project root.
    #[expect(
        clippy::disallowed_types,
        reason = "package.json scripts are deserialized as std HashMap"
    )]
    pub fn add(&mut self, name: &str, dir: &str, scripts: Option<&HashMap<String, String>>) {
        self.push(name, dir, scripts, false);
    }

    /// Add the root package of the project. `name` is empty for a root
    /// package without a name. The package managers select the root package
    /// by name, by directory, with `yarn workspaces foreach -A`, and with
    /// `pnpm --include-workspace-root` or `pnpm -w`, but not with the other
    /// selections of every package.
    #[expect(
        clippy::disallowed_types,
        reason = "package.json scripts are deserialized as std HashMap"
    )]
    pub fn add_root(&mut self, name: &str, scripts: Option<&HashMap<String, String>>) {
        self.push(name, "", scripts, true);
    }

    #[expect(
        clippy::disallowed_types,
        reason = "package.json scripts are deserialized as std HashMap"
    )]
    fn push(
        &mut self,
        name: &str,
        dir: &str,
        scripts: Option<&HashMap<String, String>>,
        root: bool,
    ) {
        self.packages.push(WorkspacePackage {
            name: name.to_string(),
            dir: normalize_dir(dir).unwrap_or_default(),
            root,
            scripts: scripts.cloned().unwrap_or_default(),
        });
    }

    /// Every package, the root package included.
    pub fn iter(&self) -> impl Iterator<Item = &WorkspacePackage> {
        self.packages.iter()
    }

    /// The package in `dir`, relative to the project root. An empty `dir`
    /// is the root package.
    pub fn find_dir(&self, dir: &str) -> Option<&WorkspacePackage> {
        self.packages.iter().find(|package| package.dir == dir)
    }

    /// The packages that `selectors` select, for a command in the package in
    /// `package_dir` (relative to the project root). Without an including
    /// selector other than [`PackageSelector::all`], every workspace package
    /// is included, as with `pnpm -r` or `pnpm --filter '!web'`. The root
    /// package is then included only with
    /// [`PackageSelector::include_root`]. A package that an excluding
    /// selector matches is left out. A selector that this module does not
    /// support selects nothing.
    pub fn select(
        &self,
        selectors: &[PackageSelector],
        package_dir: &str,
    ) -> Vec<&WorkspacePackage> {
        let matches = |selector: &PackageSelector, package: &WorkspacePackage| {
            selector.kind.matches(package, package_dir)
        };
        let including: Vec<&PackageSelector> = selectors
            .iter()
            .filter(|selector| {
                !selector.exclude
                    && !matches!(selector.kind, SelectorKind::All | SelectorKind::IncludeRoot)
            })
            .collect();
        let include_root = selectors
            .iter()
            .any(|selector| !selector.exclude && selector.kind == SelectorKind::IncludeRoot);
        self.packages
            .iter()
            .filter(|package| {
                let included = if including.is_empty() {
                    !package.root || include_root
                } else {
                    including.iter().any(|selector| matches(selector, package))
                };
                included
                    && !selectors
                        .iter()
                        .any(|selector| selector.exclude && matches(selector, package))
            })
            .collect()
    }
}

/// One package selection of a package-manager command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSelector {
    kind: SelectorKind,
    /// `true` for a pnpm filter that excludes packages (`--filter '!web'`).
    exclude: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SelectorKind {
    /// Every workspace package (`pnpm -r`, `yarn workspaces foreach -A`).
    /// Another including selector narrows the selection.
    All,
    /// The root package is one of every package
    /// (`yarn workspaces foreach -A`, `pnpm -r --include-workspace-root`).
    /// Another including selector narrows the selection.
    IncludeRoot,
    /// The root package only (`pnpm -w`).
    Root,
    /// A package name, or a glob of package names.
    Name(String),
    /// A glob of package names or of package directories relative to the
    /// project root, where `.` is the root package
    /// (`yarn workspaces foreach --include`).
    NameOrProjectDir(String),
    /// A directory glob, relative to the package that contains the command.
    Dir(String),
    /// A package name, or a directory that contains the packages, relative
    /// to the package that contains the command (npm `--workspace`,
    /// `--workspaces`). `--workspaces` in a workspace package selects that
    /// package.
    NameOrDirPrefix(String),
    /// A selection that this module does not resolve, such as the pnpm
    /// dependency forms (`web...`) and the changed-since form
    /// (`[origin/main]`).
    Unsupported,
}

impl SelectorKind {
    fn matches(&self, package: &WorkspacePackage, package_dir: &str) -> bool {
        match self {
            Self::Name(pattern) => {
                !package.name.is_empty() && glob_matches(pattern, &package.name, false)
            }
            Self::NameOrProjectDir(pattern) => {
                let dir = if package.dir.is_empty() {
                    "."
                } else {
                    &package.dir
                };
                (!package.name.is_empty() && glob_matches(pattern, &package.name, false))
                    || glob_matches(pattern, dir, true)
            }
            Self::Dir(pattern) => join_dir(package_dir, pattern)
                .is_some_and(|pattern| glob_matches(&pattern, &package.dir, true)),
            // npm never runs a command in the root package for a workspace
            // selection.
            Self::NameOrDirPrefix(_) if package.root => false,
            Self::NameOrDirPrefix(value) => {
                package.name == *value
                    || join_dir(package_dir, value).is_some_and(|dir| {
                        package.dir == dir
                            || dir.is_empty()
                            || package
                                .dir
                                .strip_prefix(&dir)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
            }
            Self::All | Self::IncludeRoot => true,
            Self::Root => package.root,
            Self::Unsupported => false,
        }
    }
}

impl PackageSelector {
    /// Every workspace package without the root package: `pnpm -r`, and
    /// `yarn workspaces run` (yarn classic).
    pub const fn all() -> Self {
        Self {
            kind: SelectorKind::All,
            exclude: false,
        }
    }

    /// Add the root package to a selection of every package:
    /// `yarn workspaces foreach -A` (the root is a workspace in yarn berry)
    /// and `pnpm -r --include-workspace-root`.
    pub const fn include_root() -> Self {
        Self {
            kind: SelectorKind::IncludeRoot,
            exclude: false,
        }
    }

    /// The root package: `pnpm -w` (`--workspace-root`).
    pub const fn root() -> Self {
        Self {
            kind: SelectorKind::Root,
            exclude: false,
        }
    }

    /// The package in a directory, relative to the package that contains
    /// the command (`pnpm -C packages/web run gen`).
    pub fn directory(dir: &str) -> Self {
        Self {
            kind: SelectorKind::Dir(escape_glob(strip_quotes(dir))),
            exclude: false,
        }
    }

    /// A `yarn workspaces foreach --include <glob>` selection, or with
    /// `exclude`, an `--exclude <glob>` selection: a glob of package names
    /// or of package directories relative to the project root.
    pub fn yarn_foreach_glob(glob: &str, exclude: bool) -> Self {
        Self {
            kind: SelectorKind::NameOrProjectDir(strip_quotes(glob).to_string()),
            exclude,
        }
    }

    /// A `yarn workspace <name>` selection: an exact package name.
    pub fn yarn_workspace(name: &str) -> Self {
        Self {
            kind: SelectorKind::Name(escape_glob(strip_quotes(name))),
            exclude: false,
        }
    }

    /// An npm `--workspace <value>` selection: a package name, or the
    /// directory of a package or of several packages, relative to the
    /// package that contains the command.
    pub fn npm_workspace(value: &str) -> Self {
        Self {
            kind: SelectorKind::NameOrDirPrefix(strip_quotes(value).to_string()),
            exclude: false,
        }
    }

    /// An npm `--workspaces` selection: every workspace from the root, else
    /// the calling package. npm sets the package that contains the working
    /// directory as the default `--workspace`.
    pub fn npm_workspaces() -> Self {
        Self {
            kind: SelectorKind::NameOrDirPrefix(".".to_string()),
            exclude: false,
        }
    }

    /// A pnpm `--filter <value>` selection: a package name or name glob, or
    /// a directory glob in the `./dir` or `{dir}` form. A leading `!`
    /// excludes the packages.
    pub fn pnpm_filter(value: &str) -> Self {
        let value = strip_quotes(value);
        let (exclude, value) = value
            .strip_prefix('!')
            .map_or((false, value), |rest| (true, rest));
        let kind = if value.is_empty()
            || value.contains("...")
            || value.contains('^')
            || value.contains('[')
        {
            SelectorKind::Unsupported
        } else if let Some(dir) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
            SelectorKind::Dir(dir.to_string())
        } else if value.starts_with("./") || value.starts_with("../") || value == "." {
            SelectorKind::Dir(value.to_string())
        } else {
            SelectorKind::Name(value.to_string())
        };
        Self { kind, exclude }
    }
}

fn strip_quotes(value: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Escape the glob characters of an exact name.
fn escape_glob(name: &str) -> String {
    globset::escape(name)
}

/// Match a glob. pnpm matches a `*` in a package name across `/`, so
/// `*` selects `@acme/api`, but not across `/` in a directory.
fn glob_matches(pattern: &str, value: &str, literal_separator: bool) -> bool {
    globset::GlobBuilder::new(pattern)
        .literal_separator(literal_separator)
        .build()
        .is_ok_and(|glob| glob.compile_matcher().is_match(value))
}

/// Join `path` to `base` and normalize `.` and `..` segments. Return `None`
/// for a path above the project root.
fn join_dir(base: &str, path: &str) -> Option<String> {
    if base.is_empty() {
        normalize_dir(path)
    } else {
        normalize_dir(&format!("{base}/{path}"))
    }
}

/// Normalize `.` and `..` segments and drop empty ones. Return `None` for a
/// path above the root.
fn normalize_dir(path: &str) -> Option<String> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

/// The path of `to` relative to `from`. Both are relative to the project
/// root (`packages/api` and `packages/web` give `../web`).
pub fn relative_dir(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend_from_slice(&to[common..]);
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packages() -> WorkspacePackages {
        let mut packages = WorkspacePackages::default();
        packages.add("web", "packages/web", None);
        packages.add("@acme/api", "./packages/api/", None);
        packages.add("docs", "apps/docs", None);
        packages
    }

    fn dirs(selectors: &[PackageSelector], package_dir: &str) -> Vec<String> {
        packages()
            .select(selectors, package_dir)
            .iter()
            .map(|package| package.dir().to_string())
            .collect()
    }

    #[test]
    fn pnpm_filters_select_by_name_glob_and_directory() {
        let filter = PackageSelector::pnpm_filter;
        assert_eq!(dirs(&[filter("web")], ""), ["packages/web"]);
        assert_eq!(dirs(&[filter("'@acme/*'")], ""), ["packages/api"]);
        assert_eq!(
            dirs(&[filter("./packages/*")], ""),
            ["packages/web", "packages/api"]
        );
        assert_eq!(dirs(&[filter("{apps/docs}")], ""), ["apps/docs"]);
        assert_eq!(dirs(&[filter("../web")], "packages/api"), ["packages/web"]);
        assert_eq!(
            dirs(&[filter("./packages/*"), filter("!web")], ""),
            ["packages/api"]
        );
        assert_eq!(
            dirs(&[filter("!web")], ""),
            ["packages/api", "apps/docs"],
            "an excluding filter alone selects every other package"
        );
        for unsupported in ["web...", "...web", "web^...", "[origin/main]"] {
            assert!(dirs(&[filter(unsupported)], "").is_empty(), "{unsupported}");
        }
    }

    #[test]
    fn every_package_is_narrowed_by_other_selectors() {
        let all = PackageSelector::all;
        assert_eq!(
            dirs(&[all()], ""),
            ["packages/web", "packages/api", "apps/docs"]
        );
        assert_eq!(
            dirs(&[all(), PackageSelector::pnpm_filter("web")], ""),
            ["packages/web"]
        );
        assert_eq!(
            dirs(
                &[all(), PackageSelector::yarn_foreach_glob("*o*", true)],
                ""
            ),
            ["packages/web", "packages/api"]
        );
        assert!(dirs(&[all(), PackageSelector::pnpm_filter("web...")], "").is_empty());
    }

    #[test]
    fn a_directory_selects_the_package_in_it() {
        let directory = PackageSelector::directory;
        assert_eq!(dirs(&[directory("packages/web")], ""), ["packages/web"]);
        assert_eq!(
            dirs(&[directory("../../apps/docs")], "packages/api"),
            ["apps/docs"]
        );
        assert!(dirs(&[directory("packages")], "").is_empty());
        assert!(dirs(&[directory("packages/*")], "").is_empty());
    }

    #[test]
    fn npm_workspaces_select_by_name_or_directory() {
        let workspace = PackageSelector::npm_workspace;
        assert_eq!(dirs(&[workspace("web")], ""), ["packages/web"]);
        assert_eq!(dirs(&[workspace("packages/web")], ""), ["packages/web"]);
        assert_eq!(
            dirs(&[workspace("packages")], ""),
            ["packages/web", "packages/api"]
        );
        assert!(dirs(&[workspace("pack")], "").is_empty());
        assert!(
            dirs(&[workspace("packages/web")], "packages/api").is_empty(),
            "npm resolves a workspace path against the calling package"
        );
        assert_eq!(
            dirs(&[workspace("../web")], "packages/api"),
            ["packages/web"],
            "npm resolves a workspace path against the calling package"
        );
        assert_eq!(
            dirs(&[PackageSelector::npm_workspaces()], "packages/api"),
            ["packages/api"],
            "`--workspaces` in a workspace package selects that package"
        );
    }

    #[test]
    fn yarn_workspace_selects_an_exact_name() {
        assert_eq!(
            dirs(&[PackageSelector::yarn_workspace("@acme/api")], ""),
            ["packages/api"]
        );
        assert!(dirs(&[PackageSelector::yarn_workspace("*")], "").is_empty());
    }

    fn dirs_with_root(selectors: &[PackageSelector], package_dir: &str) -> Vec<String> {
        let mut packages = packages();
        packages.add_root("monorepo", None);
        packages
            .select(selectors, package_dir)
            .iter()
            .map(|package| package.dir().to_string())
            .collect()
    }

    #[test]
    fn the_root_package_is_selected_where_the_package_manager_selects_it() {
        let all = PackageSelector::all;
        let workspaces = ["packages/web", "packages/api", "apps/docs"];
        assert_eq!(
            dirs_with_root(&[all()], ""),
            workspaces,
            "`pnpm -r`, `npm -ws`, and `yarn workspaces run` leave out the root"
        );
        assert_eq!(
            dirs_with_root(&[all(), PackageSelector::include_root()], ""),
            ["packages/web", "packages/api", "apps/docs", ""],
            "`yarn workspaces foreach -A` includes the root workspace"
        );
        assert_eq!(
            dirs_with_root(
                &[
                    all(),
                    PackageSelector::include_root(),
                    PackageSelector::yarn_foreach_glob("web", false)
                ],
                ""
            ),
            ["packages/web"]
        );
        assert_eq!(
            dirs_with_root(
                &[
                    all(),
                    PackageSelector::include_root(),
                    PackageSelector::yarn_foreach_glob(".", true)
                ],
                ""
            ),
            workspaces,
            "yarn matches `--exclude` against the directory too"
        );
        assert_eq!(
            dirs_with_root(&[PackageSelector::root()], "packages/web"),
            [""]
        );
        assert_eq!(
            dirs_with_root(&[PackageSelector::yarn_workspace("monorepo")], ""),
            [""]
        );
        assert_eq!(
            dirs_with_root(&[PackageSelector::directory("../..")], "packages/web"),
            [""]
        );
        assert_eq!(
            dirs_with_root(&[PackageSelector::npm_workspaces()], ""),
            ["packages/web", "packages/api", "apps/docs"],
            "npm never selects the root for a workspace selection"
        );
    }

    #[test]
    fn yarn_foreach_globs_match_names_or_directories() {
        let include = |glob| PackageSelector::yarn_foreach_glob(glob, false);
        assert_eq!(
            dirs(&[PackageSelector::all(), include("packages/*")], ""),
            ["packages/web", "packages/api"]
        );
        assert_eq!(
            dirs(&[PackageSelector::all(), include("@acme/*")], ""),
            ["packages/api"]
        );
    }

    #[test]
    fn relative_dirs_climb_to_the_common_parent() {
        assert_eq!(relative_dir("", "packages/web"), "packages/web");
        assert_eq!(relative_dir("packages/api", "packages/web"), "../web");
        assert_eq!(
            relative_dir("apps/docs", "packages/web"),
            "../../packages/web"
        );
        assert_eq!(relative_dir("packages/web", "packages/web"), ".");
        assert_eq!(relative_dir("packages/web", ""), "../..");
    }
}
