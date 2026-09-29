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
        self.packages.push(WorkspacePackage {
            name: name.to_string(),
            dir: normalize_dir(dir).unwrap_or_default(),
            scripts: scripts.cloned().unwrap_or_default(),
        });
    }

    /// The package in `dir`, relative to the project root.
    pub fn find_dir(&self, dir: &str) -> Option<&WorkspacePackage> {
        self.packages.iter().find(|package| package.dir == dir)
    }

    /// The packages that `selectors` select, for a command in the package in
    /// `package_dir` (relative to the project root). A package that an
    /// excluding selector matches is left out. A selector that this module
    /// does not support selects nothing.
    pub fn select(
        &self,
        selectors: &[PackageSelector],
        package_dir: &str,
    ) -> Vec<&WorkspacePackage> {
        let matches = |selector: &PackageSelector, package: &WorkspacePackage| {
            selector.kind.matches(package, package_dir)
        };
        self.packages
            .iter()
            .filter(|package| {
                selectors
                    .iter()
                    .any(|selector| !selector.exclude && matches(selector, package))
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
    /// A package name, or a glob of package names.
    Name(String),
    /// A directory glob, relative to the package that contains the command.
    Dir(String),
    /// A package name, or a directory that contains the packages (npm
    /// `--workspace`).
    NameOrDirPrefix(String),
    /// A selection that this module does not resolve, such as the pnpm
    /// dependency forms (`web...`) and the changed-since form
    /// (`[origin/main]`).
    Unsupported,
}

impl SelectorKind {
    fn matches(&self, package: &WorkspacePackage, package_dir: &str) -> bool {
        match self {
            Self::Name(pattern) => glob_matches(pattern, &package.name, false),
            Self::Dir(pattern) => join_dir(package_dir, pattern)
                .is_some_and(|pattern| glob_matches(&pattern, &package.dir, true)),
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
            Self::Unsupported => false,
        }
    }
}

impl PackageSelector {
    /// A `yarn workspace <name>` selection: an exact package name.
    pub fn yarn_workspace(name: &str) -> Self {
        Self {
            kind: SelectorKind::Name(escape_glob(strip_quotes(name))),
            exclude: false,
        }
    }

    /// An npm `--workspace <value>` selection: a package name, or the
    /// directory of a package or of several packages.
    pub fn npm_workspace(value: &str) -> Self {
        Self {
            kind: SelectorKind::NameOrDirPrefix(strip_quotes(value).to_string()),
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
        for unsupported in ["web...", "...web", "web^...", "[origin/main]", "!web"] {
            assert!(dirs(&[filter(unsupported)], "").is_empty(), "{unsupported}");
        }
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
    }

    #[test]
    fn yarn_workspace_selects_an_exact_name() {
        assert_eq!(
            dirs(&[PackageSelector::yarn_workspace("@acme/api")], ""),
            ["packages/api"]
        );
        assert!(dirs(&[PackageSelector::yarn_workspace("*")], "").is_empty());
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
