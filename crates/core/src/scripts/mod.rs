//! Lightweight shell command parser for package.json scripts.
//!
//! Extracts:
//! - **Binary names** → mapped to npm package names for dependency usage detection
//! - **`--config` arguments** → file paths for entry point discovery
//! - **Positional file arguments** → file paths for entry point discovery
//!
//! Handles env var prefixes (`cross-env`, `dotenv`, `KEY=value`), package manager
//! runners (`npx`, `pnpm exec`, `yarn dlx`), and Node.js runners (`node`, `tsx`,
//! `ts-node`). Shell operators (`&&`, `||`, `;`, `|`, `&`) are split correctly.

pub mod ci;
#[cfg(test)]
mod command_forms_tests;
mod flag_credits;
mod node_test;
mod resolve;
mod shell;
mod workspace_selection;

#[expect(
    clippy::disallowed_types,
    reason = "package.json scripts are deserialized as std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

pub use resolve::{
    DependencyBinaries, build_bin_to_package_map, resolve_binary_to_package,
    resolve_known_dependency_binary,
};
pub use workspace_selection::WorkspacePackages;
use workspace_selection::{PackageSelector, WorkspacePackage, relative_dir};

/// Environment variable wrapper commands to strip before the actual binary.
const ENV_WRAPPERS: &[&str] = &["cross-env", "dotenv", "env"];

struct CommandWrapperSpec {
    binary: &'static str,
    prefix: &'static [&'static str],
    separator: &'static str,
}

/// Known wrappers; `--` alone does not imply that a CLI executes another command.
const COMMAND_WRAPPERS: &[CommandWrapperSpec] = &[CommandWrapperSpec {
    // `varlock run [options] -- <command>`: https://varlock.dev/reference/cli/load-and-run/
    binary: "varlock",
    prefix: &["run"],
    separator: "--",
}];

/// Node.js runners whose first non-flag argument is a file path, not a binary name.
const NODE_RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node", "bun"];

/// A tool that reads its file arguments but never executes them: a formatter,
/// linter, spell checker, or code-quality checker (issue #2954). Its positional
/// arguments are not entry points. The tool itself still counts as a used
/// dependency, and its `--config` argument still counts as a config file.
struct FileTargetTool {
    /// Binary names of the tool.
    names: &'static [&'static str],
    /// Flags whose value names a module that the tool loads and runs, such as
    /// a custom formatter, plugin, or parser. A path value of such a flag stays
    /// reachable.
    loading_flags: &'static [&'static str],
    /// Flags whose value names a directory of modules that the tool loads,
    /// such as a rule directory. The source files directly in that directory
    /// stay reachable.
    directory_flags: &'static [&'static str],
}

/// Every [`FileTargetTool`], sorted by first name.
const FILE_TARGET_TOOLS: &[FileTargetTool] = &[
    FileTargetTool {
        names: &["alex"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["biome", "rome"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["cspell"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["depcruise", "dependency-cruiser"],
        loading_flags: &["--webpack-config"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["dprint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["editorconfig-checker", "eclint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["ember-template-lint"],
        loading_flags: &["--config-path"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["eslint", "eslint_d"],
        loading_flags: &["-f", "--format", "--parser"],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["htmlhint"],
        loading_flags: &[],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["jscpd"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["jshint"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["madge"],
        loading_flags: &["--webpack-config", "--require-config"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markdownlint"],
        loading_flags: &["-r", "--rules"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markdownlint-cli2"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["markuplint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["oxfmt", "oxlint"],
        loading_flags: &[],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["prettier"],
        loading_flags: &["--plugin"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["remark"],
        loading_flags: &["-u", "--use", "-r", "--rc-path"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["secretlint"],
        loading_flags: &["--secretlintrc"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["standard", "semistandard", "ts-standard"],
        loading_flags: &["--parser"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["stylelint"],
        loading_flags: &["--custom-formatter", "--custom-syntax"],
        directory_flags: &[],
    },
    FileTargetTool {
        names: &["textlint"],
        loading_flags: &["-f", "--format", "--rule", "--plugin", "--preset"],
        directory_flags: &["--rulesdir"],
    },
    FileTargetTool {
        names: &["tslint"],
        loading_flags: &[],
        directory_flags: &["-r", "--rules-dir", "-s", "--formatters-dir"],
    },
    FileTargetTool {
        names: &["xo"],
        loading_flags: &["--reporter"],
        directory_flags: &[],
    },
];

/// Return the [`FileTargetTool`] that `binary` invokes. A path such as
/// `./node_modules/.bin/eslint` resolves to its file name.
fn file_target_tool(binary: &str) -> Option<&'static FileTargetTool> {
    let name = tool_name(binary);
    FILE_TARGET_TOOLS
        .iter()
        .find(|tool| tool.names.contains(&name))
}

/// Return `true` when `binary` only reads its file arguments (a formatter,
/// linter, or checker), so those arguments must not become entry points.
#[must_use]
pub fn is_file_target_tool(binary: &str) -> bool {
    file_target_tool(binary).is_some()
}

/// Return the file name of a binary path (`./node_modules/.bin/eslint` is `eslint`).
fn tool_name(binary: &str) -> &str {
    binary.rsplit('/').next().unwrap_or(binary)
}

/// Command names from the `ignoreCommandEntries` config key: the file
/// arguments of these commands never become entry points. `*` matches every
/// command. A name matches the command that receives the arguments, after
/// environment, package-manager, and wrapper prefixes, by its file name.
#[derive(Debug, Clone, Copy, Default)]
pub struct IgnoredCommandEntries<'a> {
    names: &'a [String],
}

impl<'a> IgnoredCommandEntries<'a> {
    /// No command is ignored.
    pub const NONE: Self = Self { names: &[] };

    /// Wrap the configured command names.
    #[must_use]
    pub const fn new(names: &'a [String]) -> Self {
        Self { names }
    }

    /// Whether the file arguments of `command` are ignored.
    #[must_use]
    pub fn contains(&self, command: &str) -> bool {
        let name = tool_name(command);
        self.names
            .iter()
            .any(|ignored| ignored == "*" || ignored == name)
    }
}

/// Source extensions a tool loads from a rule or formatter directory.
const DIRECTORY_MODULE_GLOB: &str = "*.{js,cjs,mjs,ts,cts,mts}";

/// Return the modules that a file-target tool loads through its flags
/// (`eslint -f ./tools/fmt.js src` yields `./tools/fmt.js`). A directory flag
/// yields a glob for the source files directly in that directory
/// (`eslint --rulesdir ./rules` yields `./rules/*.{js,cjs,mjs,ts,cts,mts}`).
/// Positional targets are not returned. Both `--flag value` and
/// `--flag=value` forms are recognized, and surrounding quotes are removed.
#[must_use]
pub fn file_target_tool_loaded_files(binary: &str, args: &[&str]) -> Vec<String> {
    let Some(tool) = file_target_tool(binary) else {
        return Vec::new();
    };
    let takes_value =
        |flag: &str| tool.loading_flags.contains(&flag) || tool.directory_flags.contains(&flag);
    let mut files = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        let token = args[idx];
        idx += 1;
        let (flag, value) = if takes_value(token) {
            let Some(&next) = args.get(idx) else { break };
            idx += 1;
            (token, next)
        } else if let Some((flag, value)) = token.split_once('=')
            && takes_value(flag)
        {
            (flag, value)
        } else {
            continue;
        };
        let value = strip_surrounding_quotes(value);
        if tool.directory_flags.contains(&flag) {
            files.extend(directory_module_glob(value));
        } else if looks_like_file_path(value) {
            files.push(value.to_string());
        }
    }
    files
}

/// Return the glob for the modules directly in `dir`, or `None` when the value
/// cannot be a local directory.
fn directory_module_glob(dir: &str) -> Option<String> {
    if dir.is_empty()
        || dir.starts_with('-')
        || dir.starts_with('@')
        || dir.contains("://")
        || dir.contains(char::is_whitespace)
        || !could_be_file_path(dir)
    {
        return None;
    }
    let dir = dir.trim_end_matches('/');
    Some(match dir.trim_start_matches("./") {
        "" | "." => DIRECTORY_MODULE_GLOB.to_string(),
        _ => format!("{dir}/{DIRECTORY_MODULE_GLOB}"),
    })
}

/// Script multiplexer commands whose positional arguments are script names, not binaries.
/// `concurrently "npm:dev"` and `run-p server worker` reference other package.json scripts.
const SCRIPT_MULTIPLEXERS: &[&str] = &[
    "concurrently",
    "npm-run-all",
    "npm-run-all2",
    "run-s",
    "run-p",
    "run-s2",
    "run-p2",
];

/// pnpm commands and shorthands whose next token is not a dependency binary.
const PNPM_BUILTIN_COMMANDS: &[&str] = &[
    "add",
    "audit",
    "bin",
    "catalog",
    "ci",
    "config",
    "dedupe",
    "deploy",
    "env",
    "exec",
    "fetch",
    "import",
    "init",
    "install",
    "licenses",
    "link",
    "list",
    "outdated",
    "pack",
    "patch",
    "prune",
    "publish",
    "rebuild",
    "remove",
    "root",
    "run",
    "run-script",
    "setup",
    "start",
    "stop",
    "store",
    "test",
    "unlink",
    "update",
    "why",
];

/// npm config flags that take a value and select where a command runs or
/// where npm reads its files (`npm run build -w web`).
const NPM_LOCATION_VALUE_FLAGS: &[&str] = &[
    "-w",
    "--workspace",
    "-C",
    "--prefix",
    "-L",
    "--location",
    "--userconfig",
    "--globalconfig",
    "--cache",
    "--logs-dir",
    "--pack-destination",
];

/// npm config flags that take a value and configure the registry, the
/// network, or authentication (`npm run release --otp 123456`).
const NPM_REGISTRY_VALUE_FLAGS: &[&str] = &[
    "--registry",
    "--reg",
    "--replace-registry-host",
    "--scope",
    "--otp",
    "--auth-type",
    "--access",
    "--ca",
    "--cafile",
    "--cert",
    "--key",
    "--proxy",
    "--https-proxy",
    "--noproxy",
    "--local-address",
    "--cidr",
    "--user-agent",
    "--maxsockets",
    "--fetch-retries",
    "--fetch-retry-factor",
    "--fetch-retry-maxtimeout",
    "--fetch-retry-mintimeout",
    "--fetch-timeout",
    "--cache-max",
    "--cache-min",
];

/// npm config flags that take a value and select dependencies, versions, or
/// platforms (`npm run build --omit dev`).
const NPM_DEPENDENCY_VALUE_FLAGS: &[&str] = &[
    "--tag",
    "--before",
    "--enjoy-by",
    "--include",
    "--omit",
    "--only",
    "--also",
    "--install-strategy",
    "--save-prefix",
    "--lockfile-version",
    "--depth",
    "--cpu",
    "--os",
    "--libc",
    "--package",
];

/// npm config flags that take a value and set how npm runs a command
/// (`npm run test --node-options=--inspect`).
const NPM_RUNTIME_VALUE_FLAGS: &[&str] = &[
    "--node-options",
    "--script-shell",
    "--shell",
    "-c",
    "--call",
    "--editor",
    "--viewer",
    "--umask",
    "--which",
];

/// npm config flags that take a value and set the output, the audit, or the
/// report format (`npm run lint --loglevel warn`).
const NPM_OUTPUT_VALUE_FLAGS: &[&str] = &[
    "--loglevel",
    "--logs-max",
    "--heading",
    "--audit-level",
    "--diff",
    "--diff-dst-prefix",
    "--diff-src-prefix",
    "--diff-unified",
    "--sbom-format",
    "--sbom-type",
    "--searchexclude",
    "--searchlimit",
    "--searchopts",
    "--searchstaleness",
    "--expect-result-count",
];

/// npm config flags that take a value and set version, publish, or `npm
/// init` details (`npm run release --preid beta`).
const NPM_PUBLISH_VALUE_FLAGS: &[&str] = &[
    "-m",
    "--message",
    "--preid",
    "--tag-version-prefix",
    "--git",
    "--provenance-file",
    "--init-author-email",
    "--init-author-name",
    "--init-author-url",
    "--init-license",
    "--init-module",
    "--init-version",
];

/// Every group of npm config flags that take a value as the next argument.
/// npm consumes the flag and the value. The groups follow the npm config
/// definitions: each definition whose type does not accept a boolean. npm
/// parses an unknown flag as a boolean, so `npm run lint --fix src/a.ts`
/// forwards `src/a.ts`.
const NPM_CONFIG_VALUE_FLAG_GROUPS: &[&[&str]] = &[
    NPM_LOCATION_VALUE_FLAGS,
    NPM_REGISTRY_VALUE_FLAGS,
    NPM_DEPENDENCY_VALUE_FLAGS,
    NPM_RUNTIME_VALUE_FLAGS,
    NPM_OUTPUT_VALUE_FLAGS,
    NPM_PUBLISH_VALUE_FLAGS,
];

/// Whether npm reads the next argument as the value of `flag`.
fn npm_flag_takes_value(flag: &str) -> bool {
    NPM_CONFIG_VALUE_FLAG_GROUPS
        .iter()
        .any(|group| group.contains(&flag))
}

/// yarn `workspaces foreach` flags without a value
/// (`yarn workspaces foreach -A run lint`). `--since` takes a value only in
/// the `--since=<ref>` form.
const YARN_FOREACH_BOOLEAN_FLAGS: &[&str] = &[
    "-A",
    "--all",
    "-R",
    "--recursive",
    "-W",
    "--worktree",
    "-v",
    "--verbose",
    "-p",
    "--parallel",
    "-i",
    "--interlaced",
    "-t",
    "--topological",
    "--topological-dev",
    "--no-private",
    "-n",
    "--dry-run",
    "--since",
];

/// yarn `workspaces foreach` flags that take a value.
const YARN_FOREACH_VALUE_FLAGS: &[&str] = &["-j", "--jobs", "--include", "--exclude", "--from"];

/// Monorepo task runners. Their positional arguments are task names, and
/// the arguments after `--` go to the task scripts of other workspace
/// packages (`turbo run lint -- src/a.ts`), so no argument is an entry
/// point here.
const TASK_RUNNERS: &[&str] = &["turbo", "nx", "lerna"];

/// pnpm flags that select other workspace packages and take a value
/// (`pnpm --filter web lint`).
const PNPM_FILTER_FLAGS: &[&str] = &["--filter", "-F", "--filter-prod"];

/// Boolean pnpm flags that select workspace packages or set how a command
/// runs. They can appear before and after `exec`
/// (`pnpm -r exec prettier --check src`).
const PNPM_EXEC_BOOLEAN_FLAGS: &[&str] = &[
    "--silent",
    "-s",
    "-r",
    "--recursive",
    "--parallel",
    "--stream",
    "-w",
    "--workspace-root",
    "--include-workspace-root",
    "--no-bail",
    "--sequential",
    "--reverse",
    "--report-summary",
];

/// pnpm flags that select workspace packages or a directory and take a value
/// (`pnpm --filter web exec eslint src`).
const PNPM_EXEC_VALUE_FLAGS: &[&str] = &[
    "--filter",
    "-F",
    "--filter-prod",
    "-C",
    "--dir",
    "--workspace-concurrency",
    "--resume-from",
];

/// Package manager subcommands that never name a package.json script, even when
/// a script with the same name exists. `yarn install` runs the installer, not a
/// script called `install`.
const PACKAGE_MANAGER_BUILTIN_COMMANDS: &[&str] = &[
    "add",
    "audit",
    "bin",
    "cache",
    "config",
    "create",
    "dedupe",
    "dlx",
    "exec",
    "global",
    "import",
    "info",
    "init",
    "install",
    "link",
    "list",
    "login",
    "logout",
    "ls",
    "node",
    "outdated",
    "pack",
    "patch",
    "publish",
    "remove",
    "run",
    "run-script",
    "set",
    "unlink",
    "up",
    "upgrade",
    "version",
    "why",
    "workspace",
    "workspaces",
];

/// Maximum depth of `npm run <script>` indirection that is followed. Guards
/// against pathological nesting on top of the cycle guard.
pub const MAX_SCRIPT_INDIRECTION_DEPTH: usize = 8;

/// Maximum number of script bodies expanded while analyzing a single command.
/// The depth limit bounds one path; this bounds the total fan-out when many
/// scripts call each other with arguments.
pub const MAX_SCRIPT_EXPANSIONS: usize = 64;

/// A script body in the catalog, plus whether its file arguments are relative
/// to the root the analysis resolves paths against.
#[derive(Debug, Clone)]
struct CatalogBody {
    body: String,
    /// `false` for a body merged from another workspace package: its positional
    /// file arguments are relative to that package, not to the analysis root.
    local: bool,
}

/// Script names declared by a project, with the bodies that a package manager
/// invocation such as `npm run lint -- --format gha` resolves to.
///
/// A name whose body is ambiguous (declared by several packages with different
/// bodies) keeps its name but permanently loses its body, so the indirection is
/// not followed and nothing is credited from the wrong package. Ambiguity is
/// sticky: a later package re-declaring one of the conflicting bodies does not
/// restore it.
#[derive(Debug, Default, Clone)]
pub struct ScriptCatalog {
    names: FxHashSet<String>,
    bodies: FxHashMap<String, CatalogBody>,
    ambiguous: FxHashSet<String>,
    /// The workspace packages of the project, so that a command that selects
    /// a package by name resolves its file arguments in that package.
    workspaces: Option<Arc<WorkspacePackages>>,
    /// The directory of the package whose commands this catalog resolves,
    /// relative to the project root. Empty for the root package.
    package_dir: String,
}

impl ScriptCatalog {
    /// A catalog without scripts: no script call resolves.
    #[cfg(test)]
    pub const EMPTY: &'static Self = &Self {
        names: FxHashSet::with_hasher(rustc_hash::FxBuildHasher),
        bodies: FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
        ambiguous: FxHashSet::with_hasher(rustc_hash::FxBuildHasher),
        workspaces: None,
        package_dir: String::new(),
    };

    /// Attach the workspace packages of the project. `package_dir` is the
    /// directory of the package whose commands the catalog resolves,
    /// relative to the project root (empty for the root package).
    #[must_use]
    pub fn with_workspaces(
        mut self,
        workspaces: Arc<WorkspacePackages>,
        package_dir: &str,
    ) -> Self {
        self.workspaces = Some(workspaces);
        package_dir
            .trim_matches('/')
            .clone_into(&mut self.package_dir);
        self
    }

    /// The workspace packages that `selectors` select for a command of this
    /// catalog's package. Empty without workspace packages.
    fn selected_packages(&self, selectors: &[PackageSelector]) -> Vec<&WorkspacePackage> {
        self.workspaces
            .as_deref()
            .map(|workspaces| workspaces.select(selectors, &self.package_dir))
            .unwrap_or_default()
    }

    /// The workspace packages in which a command at `location` runs: the
    /// selected packages, or the package in the directory. `None` for a
    /// location that selects no workspace package.
    fn location_packages(&self, location: &RunLocation) -> Option<Vec<&WorkspacePackage>> {
        match location {
            RunLocation::Packages(selectors) => Some(self.selected_packages(selectors)),
            RunLocation::Directory(dir) => {
                Some(self.selected_packages(&[PackageSelector::directory(dir)]))
            }
            RunLocation::Here | RunLocation::OtherPackages => None,
        }
    }

    /// The location of a script call. A directory that holds a workspace
    /// package selects that package, so that `pnpm -C packages/web run gen`
    /// runs the `gen` script of that package.
    fn script_call_location(&self, location: RunLocation) -> RunLocation {
        if let RunLocation::Directory(dir) = &location {
            let selector = PackageSelector::directory(dir);
            if !self
                .selected_packages(std::slice::from_ref(&selector))
                .is_empty()
            {
                return RunLocation::Packages(vec![selector]);
            }
        }
        location
    }

    /// The directory of a selected package, relative to this catalog's package.
    fn relative_package_dir(&self, package: &WorkspacePackage) -> String {
        relative_dir(&self.package_dir, package.dir())
    }

    /// The catalog of the package in which a script command of a
    /// [`DeclaredScriptCall::InPackages`] call runs.
    #[must_use]
    pub fn for_package_command(&self, command: &PackageScriptCommand) -> Self {
        self.workspaces
            .as_deref()
            .and_then(|workspaces| workspaces.find_dir(&command.package_dir))
            .map_or_else(Self::default, |package| self.package_catalog(package))
    }

    /// The catalog of a selected workspace package: its own scripts, with the
    /// same workspace packages.
    fn package_catalog(&self, package: &WorkspacePackage) -> Self {
        let catalog = Self::from_scripts(package.scripts());
        match &self.workspaces {
            Some(workspaces) => catalog.with_workspaces(Arc::clone(workspaces), package.dir()),
            None => catalog,
        }
    }

    /// Whether `name` is a script that a command at `location` calls. For a
    /// selection of workspace packages, or the directory of one, the scripts
    /// of those packages decide. Otherwise, and when the location matches no
    /// known package, the scripts of this catalog decide.
    fn declares_script_at(&self, name: &str, location: &RunLocation) -> bool {
        if let Some(packages) = self.location_packages(location)
            && !packages.is_empty()
        {
            return packages
                .iter()
                .any(|package| package.scripts().contains_key(name));
        }
        self.contains(name)
    }

    /// Build a catalog from one package's `scripts` map.
    #[must_use]
    #[expect(
        clippy::disallowed_types,
        reason = "API matches serde-deserialized HashMap from package.json"
    )]
    pub fn from_scripts(scripts: &HashMap<String, String>) -> Self {
        let mut catalog = Self::default();
        catalog.merge_scripts(scripts);
        catalog
    }

    /// Build a catalog whose names come from `all` but whose bodies are limited
    /// to `analyzed`.
    ///
    /// Production runs analyze only production-relevant scripts. Names and
    /// bodies must be filtered separately: a package manager resolves
    /// `pnpm <name>` to the declared script and never to a same-named binary,
    /// so dropping a filtered script's name would credit a dependency that
    /// shares the name. The body of a filtered script must stay unreachable, so
    /// argument-bearing indirection cannot enter a script the filter skipped.
    #[must_use]
    #[expect(
        clippy::disallowed_types,
        reason = "API matches serde-deserialized HashMap from package.json"
    )]
    pub fn from_scripts_with_bodies(
        all: &HashMap<String, String>,
        analyzed: &HashMap<String, String>,
    ) -> Self {
        let mut catalog = Self::from_scripts(analyzed);
        catalog.names.extend(all.keys().cloned());
        catalog
    }

    /// Fold the analyzed package's own `scripts` map into the catalog.
    #[expect(
        clippy::disallowed_types,
        reason = "API matches serde-deserialized HashMap from package.json"
    )]
    pub fn merge_scripts(&mut self, scripts: &HashMap<String, String>) {
        self.merge(scripts, true);
    }

    /// Fold another workspace package's `scripts` map into the catalog. Bodies
    /// merged this way still credit dependencies, but their file arguments are
    /// dropped because they are relative to that package's own root.
    #[expect(
        clippy::disallowed_types,
        reason = "API matches serde-deserialized HashMap from package.json"
    )]
    pub fn merge_workspace_scripts(&mut self, scripts: &HashMap<String, String>) {
        self.merge(scripts, false);
    }

    #[expect(
        clippy::disallowed_types,
        reason = "API matches serde-deserialized HashMap from package.json"
    )]
    fn merge(&mut self, scripts: &HashMap<String, String>, local: bool) {
        for (name, body) in scripts {
            self.names.insert(name.clone());
            if self.ambiguous.contains(name) {
                continue;
            }
            match self.bodies.get_mut(name) {
                Some(existing) if existing.body == *body => {
                    existing.local = existing.local || local;
                }
                Some(_) => {
                    self.bodies.remove(name);
                    self.ambiguous.insert(name.clone());
                }
                None => {
                    self.bodies.insert(
                        name.clone(),
                        CatalogBody {
                            body: body.clone(),
                            local,
                        },
                    );
                }
            }
        }
    }

    /// Whether a script with this name is declared anywhere in the project.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    fn body(&self, name: &str) -> Option<&CatalogBody> {
        if self.ambiguous.contains(name) {
            return None;
        }
        self.bodies.get(name)
    }
}

/// Bookkeeping for one top-level command while script indirection is followed.
struct ScriptExpansion {
    /// Script names on the current expansion path, for the cycle guard.
    active: Vec<String>,
    /// Bodies expanded so far, bounded by [`MAX_SCRIPT_EXPANSIONS`].
    expansions: usize,
    /// `false` once a body from another workspace package has been entered.
    local_paths: bool,
}

impl ScriptExpansion {
    fn new() -> Self {
        Self {
            active: Vec::new(),
            expansions: 0,
            local_paths: true,
        }
    }
}

struct ScriptCommandContext<'a> {
    declared_packages: &'a FxHashSet<String>,
    scripts: &'a ScriptCatalog,
    ignored: IgnoredCommandEntries<'a>,
}

/// Where the real command starts once the package manager prefix is consumed.
enum PackageManagerTarget {
    /// A binary invocation starting at this token index, and where it runs.
    Binary(usize, RunLocation),
    /// A package.json script invocation. The script body is re-scanned with the
    /// call-site arguments at the `forwarded` token indices appended, which is
    /// what the package manager itself does.
    Script {
        name: String,
        forwarded: Vec<usize>,
        location: RunLocation,
    },
}

/// Where a package-manager command runs, relative to the package whose
/// script or CI step contains the command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunLocation {
    /// The same package: file arguments resolve against it.
    Here,
    /// A directory relative to that package
    /// (`pnpm -C docs exec tsx scripts/a.ts`). File arguments resolve
    /// against the directory.
    Directory(String),
    /// Workspace packages selected by name, by directory, or all of them
    /// (`yarn workspace web node scripts/a.ts`, `pnpm -r exec tsx
    /// scripts/a.ts`). File arguments resolve against the directory of each
    /// selected package.
    Packages(Vec<PackageSelector>),
    /// Workspace packages that this module does not resolve, such as a
    /// directory inside each selected package or
    /// `yarn workspaces foreach --since`. No file argument is an entry
    /// point of the calling package.
    OtherPackages,
}

impl RunLocation {
    /// Record a flag that selects every workspace package. A named
    /// selection narrows it (`pnpm -r --filter web`).
    fn select_all_packages(&mut self) {
        self.select_package(PackageSelector::all());
    }

    /// Record a selection of workspace packages that this module does not
    /// resolve.
    fn select_unresolved_packages(&mut self) {
        *self = Self::OtherPackages;
    }

    /// Record a flag that selects workspace packages by name or directory.
    fn select_package(&mut self, selector: PackageSelector) {
        match self {
            Self::Packages(selectors) => selectors.push(selector),
            _ => *self = Self::Packages(vec![selector]),
        }
    }

    /// Record a flag that selects a directory. A package selection wins.
    fn select_directory(&mut self, dir: &str) {
        if *self == Self::Here {
            *self = Self::Directory(strip_surrounding_quotes(dir).to_string());
        }
    }

    /// The location of a command that `self` runs through a command wrapper.
    fn nest(self, inner: Self) -> Self {
        match (self, inner) {
            // A directory inside each selected package is not resolved.
            (Self::OtherPackages, _)
            | (_, Self::OtherPackages)
            | (Self::Packages(_), Self::Directory(_)) => Self::OtherPackages,
            (Self::Here, inner) => inner,
            (outer, Self::Here) => outer,
            (Self::Directory(outer), Self::Directory(inner)) => {
                Self::Directory(rebase_path(&outer, &inner))
            }
            // A package selection names packages of the whole workspace.
            (_, Self::Packages(selectors)) => Self::Packages(selectors),
        }
    }

    /// The directories, relative to the package of `catalog`, that file
    /// arguments resolve against. `None` means the calling package itself,
    /// and an empty list means that no file argument is a file of a known
    /// package.
    fn base_dirs(&self, catalog: &ScriptCatalog) -> Option<Vec<String>> {
        match self {
            Self::Here => None,
            Self::Directory(dir) => Some(vec![dir.clone()]),
            Self::Packages(selectors) => Some(
                catalog
                    .selected_packages(selectors)
                    .into_iter()
                    .map(|package| catalog.relative_package_dir(package))
                    .collect(),
            ),
            Self::OtherPackages => Some(Vec::new()),
        }
    }

    /// Resolve every path against each directory that the command runs in.
    fn resolve_all(&self, paths: Vec<String>, catalog: &ScriptCatalog) -> Vec<String> {
        match self.base_dirs(catalog) {
            None => paths,
            Some(dirs) => rebase_all(&dirs, &paths),
        }
    }
}

/// Join every path to every directory.
fn rebase_all(dirs: &[String], paths: &[String]) -> Vec<String> {
    dirs.iter()
        .flat_map(|dir| paths.iter().map(|path| rebase_path(dir, path)))
        .collect()
}

/// Join a path argument to the directory a command runs in
/// (`packages/web` and `./src/a.ts` give `packages/web/src/a.ts`).
#[must_use]
pub fn rebase_path(dir: &str, path: &str) -> String {
    let dir = dir.trim_end_matches('/');
    let path = path.trim_start_matches("./");
    match dir.trim_start_matches("./") {
        "" | "." => path.to_string(),
        _ => format!("{dir}/{path}"),
    }
}

/// The command that a command segment invokes, after environment,
/// package-manager, and wrapper prefixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokedCommand {
    /// The token index of the command.
    pub index: usize,
    /// The directories the command runs in, relative to the calling package,
    /// or `None` for the calling package itself.
    base_dirs: Option<Vec<String>>,
}

impl InvokedCommand {
    /// Return a file argument of the command relative to the calling
    /// package, once for each directory that the command runs in. The list
    /// is empty when the command runs in workspace packages that no name
    /// selects (`pnpm -r eslint src/a.ts`).
    #[must_use]
    pub fn file_refs(&self, path: &str) -> Vec<String> {
        match &self.base_dirs {
            None => vec![path.to_string()],
            Some(dirs) => dirs.iter().map(|dir| rebase_path(dir, path)).collect(),
        }
    }

    /// Whether no file argument of the command is a file of the calling
    /// package or of a known workspace package.
    #[must_use]
    pub fn runs_in_other_packages(&self) -> bool {
        self.base_dirs.as_ref().is_some_and(Vec::is_empty)
    }
}

/// Result of analyzing all package.json scripts.
#[derive(Debug, Default)]
pub struct ScriptAnalysis {
    /// Package names used as binaries in scripts (mapped from binary → package name).
    pub used_packages: FxHashSet<String>,
    /// Config file paths extracted from `--config` / `-c` arguments.
    pub config_files: Vec<String>,
    /// File paths extracted as positional arguments (entry point candidates).
    pub entry_files: Vec<String>,
}

impl ScriptAnalysis {
    /// Drop repeated config and entry paths, keeping first-seen order.
    ///
    /// A body reached both as its own script and through package-manager
    /// indirection is scanned more than once, and every consumer turns these
    /// lists into patterns one by one.
    fn dedupe_paths(&mut self) {
        retain_first_seen(&mut self.config_files);
        retain_first_seen(&mut self.entry_files);
    }
}

fn retain_first_seen(values: &mut Vec<String>) {
    let mut seen: FxHashSet<String> = FxHashSet::default();
    values.retain(|value| seen.insert(value.clone()));
}

/// Normalize a script-extracted file path into a project-relative entry pattern.
///
/// `ws_prefix` is the workspace package's path relative to the project root
/// (empty string for root-level package.json scripts). `raw` is the path as it
/// appeared in the script (e.g., `./scripts/deploy.ts`, `scripts/deploy.ts`).
///
/// Returns `None` when:
/// - The path is absolute or escapes the project root. Parent segments may
///   resolve above the workspace package as long as they stay inside the
///   project root (e.g., `apps/api/../../top.ts` becomes `top.ts`).
///
/// Matches existing behaviour for `config_files` (workspace-prefix join) but
/// additionally normalizes `..` segments via [`Path::components`] so paths like
/// `apps/api/../shared/scripts/deploy.ts` collapse to `apps/shared/scripts/deploy.ts`
/// instead of being passed verbatim to globset (which does not normalize).
#[must_use]
pub fn normalize_script_entry_pattern(ws_prefix: &str, raw: &str) -> Option<String> {
    let trimmed = raw.trim_start_matches("./");
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    let combined = if ws_prefix.is_empty() {
        trimmed.to_string()
    } else {
        format!("{}/{}", ws_prefix.trim_end_matches('/'), trimmed)
    };

    let mut stack: Vec<&str> = Vec::new();
    let mut segments = combined.split('/').peekable();
    while let Some(segment) = segments.next() {
        match segment {
            // A trailing slash is tolerated (`scripts/deploy/`), but an
            // internal empty segment means a doubled separator that is not
            // real path syntax (issue #2592: a jq filter's `//` operator
            // misclassified as a file path must not have it silently
            // collapsed into a single `/` here).
            "" if segments.peek().is_some() => return None,
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            other => stack.push(other),
        }
    }

    if stack.is_empty() {
        None
    } else {
        Some(stack.join("/"))
    }
}

/// A parsed command segment from a script value.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptCommand {
    /// The binary/command name (e.g., "webpack", "eslint", "tsc").
    pub binary: String,
    /// Config file arguments (from `--config`, `-c`).
    pub config_args: Vec<String>,
    /// File path arguments (positional args that look like file paths).
    pub file_args: Vec<String>,
    /// The command that receives `file_args`. It differs from `binary` for a
    /// command wrapper (`varlock run -- npx eslint` records `eslint`).
    /// `ignoreCommandEntries` matches this name.
    pub file_args_command: String,
    /// Packages this command names through a flag value rather than by
    /// importing them or invoking them as the binary.
    pub flag_packages: Vec<String>,
}

impl ScriptCommand {
    /// Return the file arguments that become entry points: none when
    /// `ignored` lists the command that receives them.
    #[must_use]
    pub fn entry_files(&self, ignored: IgnoredCommandEntries<'_>) -> &[String] {
        if ignored.contains(&self.file_args_command) {
            &[]
        } else {
            &self.file_args
        }
    }
}

/// Filter scripts to only production-relevant ones (start, build, and their pre/post hooks).
///
/// In production mode, dev/test/lint scripts are excluded since they only affect
/// devDependency usage, not the production dependency graph.
#[must_use]
#[expect(
    clippy::disallowed_types,
    reason = "API matches serde-deserialized HashMap from package.json"
)]
pub fn filter_production_scripts(scripts: &HashMap<String, String>) -> HashMap<String, String> {
    scripts
        .iter()
        .filter(|(name, _)| is_production_script(name))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Check if a script name is production-relevant.
///
/// Production scripts: `start`, `build`, `serve`, `preview`, `prepare`, `prepublishOnly`,
/// and their `pre`/`post` lifecycle hooks, plus namespaced variants like `build:prod`.
fn is_production_script(name: &str) -> bool {
    let root_name = name.split(':').next().unwrap_or(name);

    if matches!(
        root_name,
        "start" | "build" | "serve" | "preview" | "prepare" | "prepublishOnly" | "postinstall"
    ) {
        return true;
    }

    let base = root_name
        .strip_prefix("pre")
        .or_else(|| root_name.strip_prefix("post"));

    base.is_some_and(|base| matches!(base, "start" | "build" | "serve" | "install"))
}

/// Analyze scripts with dependency context and the project-wide script catalog.
#[must_use]
#[expect(
    clippy::disallowed_types,
    reason = "API matches serde-deserialized HashMap from package.json"
)]
pub fn analyze_scripts_with_dependency_context(
    scripts: &HashMap<String, String>,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
    catalog: &ScriptCatalog,
    ignored: IgnoredCommandEntries<'_>,
) -> ScriptAnalysis {
    analyze_commands_with_context(
        scripts.values(),
        root,
        bin_map,
        declared_packages,
        catalog,
        ignored,
    )
}

/// Analyze arbitrary shell commands with dependency and script-catalog context.
///
/// A command that invokes a declared script through a package manager
/// (`npm run lint -- --format gha`) is resolved to that script's body with the
/// call-site arguments appended, so binaries and flag values behind the
/// indirection are credited. The file arguments of a command that `ignored`
/// lists are not entry files.
#[must_use]
pub fn analyze_commands_with_context<'a, I>(
    commands: I,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
    catalog: &ScriptCatalog,
    ignored: IgnoredCommandEntries<'_>,
) -> ScriptAnalysis
where
    I: IntoIterator<Item = &'a String>,
{
    let mut result = ScriptAnalysis::default();
    let context = ScriptCommandContext {
        declared_packages,
        scripts: catalog,
        ignored,
    };

    for command in commands {
        accumulate_command_with_context(command, root, bin_map, &context, &mut result);
    }

    result.dedupe_paths();
    result
}

/// Analyze a single shell command string into used packages, config files, and
/// entry files.
///
/// Shares the exact binary-to-package mapping, builtin filtering, node-runner
/// handling, and config/file argument extraction used for package.json scripts.
/// Lets non-script command sources (e.g. a Playwright `webServer.command`) credit
/// invoked binaries as referenced dependencies and seed local file arguments as
/// entry/setup files identically to how the same command would behave in a script.
#[must_use]
pub fn analyze_command(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
) -> ScriptAnalysis {
    let mut result = ScriptAnalysis::default();
    accumulate_command(command, root, bin_map, &mut result);
    result.dedupe_paths();
    result
}

/// Parse one command string and fold its binaries, config args, and file args
/// into `result`. [`analyze_command`] calls it for a single command.
fn accumulate_command(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    result: &mut ScriptAnalysis,
) {
    accumulate_parsed_commands(
        command,
        root,
        bin_map,
        parse_script(command),
        IgnoredCommandEntries::NONE,
        result,
    );
}

fn accumulate_command_with_context(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
    result: &mut ScriptAnalysis,
) {
    accumulate_parsed_commands(
        command,
        root,
        bin_map,
        parse_script_with_context(command, root, bin_map, context),
        context.ignored,
        result,
    );
}

fn accumulate_parsed_commands(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    parsed: Vec<ScriptCommand>,
    ignored: IgnoredCommandEntries<'_>,
    result: &mut ScriptAnalysis,
) {
    for wrapper in ENV_WRAPPERS {
        if command.split_whitespace().any(|token| token == *wrapper) {
            let pkg = resolve_binary_to_package(wrapper, root, bin_map);
            if !is_builtin_command(wrapper) {
                result.used_packages.insert(pkg);
            }
        }
    }

    for cmd in parsed {
        if !cmd.binary.is_empty() && !is_builtin_command(&cmd.binary) {
            if NODE_RUNNERS.contains(&cmd.binary.as_str()) {
                if cmd.binary != "node" && cmd.binary != "bun" {
                    let pkg = resolve_binary_to_package(&cmd.binary, root, bin_map);
                    result.used_packages.insert(pkg);
                }
            } else {
                let pkg = resolve_binary_to_package(&cmd.binary, root, bin_map);
                result.used_packages.insert(pkg);
            }
        }

        result
            .entry_files
            .extend_from_slice(cmd.entry_files(ignored));
        result.used_packages.extend(cmd.flag_packages);
        result.config_files.extend(cmd.config_args);
    }
}

/// Parse a single script value into one or more commands.
///
/// Splits on shell operators (`&&`, `||`, `;`, `|`, `&`) and parses each segment.
#[must_use]
pub fn parse_script(script: &str) -> Vec<ScriptCommand> {
    parse_script_with_catalog(script, &ScriptCatalog::default())
}

/// Parse a single script value into one or more commands, and resolve a call
/// of a script that `catalog` declares.
///
/// `npm run lint -- src/a.ts` with the script `lint: eslint` parses as
/// `eslint src/a.ts`, which is what the package manager runs. So the rules for
/// formatter and linter targets and for `ignoreCommandEntries` apply to the
/// command behind the script name.
#[must_use]
pub fn parse_script_with_catalog(script: &str, catalog: &ScriptCatalog) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();
    let mut state = ScriptExpansion::new();
    parse_script_internal(
        script,
        &|tokens, idx, catalog| {
            script_invocation_target(tokens, idx, catalog)
                .or_else(|| {
                    package_manager_exec_binary(tokens, idx).map(|(binary_idx, location)| {
                        PackageManagerTarget::Binary(binary_idx, location)
                    })
                })
                .or_else(|| {
                    shell::advance_past_package_manager(tokens, idx).map(|binary_idx| {
                        PackageManagerTarget::Binary(binary_idx, RunLocation::Here)
                    })
                })
        },
        catalog,
        &mut state,
        &mut commands,
    );
    commands
}

/// Return declared package scripts invoked by `command` through npm, pnpm,
/// yarn, or bun. Used by entry-point discovery to propagate lifecycle roles
/// through script indirection without expanding or executing script bodies.
pub fn referenced_package_scripts(command: &str, catalog: &ScriptCatalog) -> FxHashSet<String> {
    let mut names = FxHashSet::default();

    for segment in shell::split_shell_operators(command) {
        let words = shell::split_words(segment);
        let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
        let Some(idx) = shell::skip_initial_wrappers(&tokens, 0) else {
            continue;
        };
        if let Some(binary) = tokens.get(idx).copied()
            && SCRIPT_MULTIPLEXERS.contains(&binary)
        {
            let mut skip_next = false;
            for token in &tokens[idx + 1..] {
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if matches!(*token, "--names" | "--prefix" | "--max-parallel") {
                    skip_next = true;
                    continue;
                }
                let name = token.strip_prefix("npm:").unwrap_or(token);
                if !name.starts_with('-') && catalog.contains(name) {
                    names.insert(name.to_string());
                }
            }
            continue;
        }
        if let Some(invocation) = declared_script_invocation(&tokens, idx, catalog)
            && invocation.location == RunLocation::Here
        {
            names.insert(invocation.name.to_string());
        }
    }

    names
}

/// Return the scripts of workspace packages that `command` calls, as
/// `(package directory, script name)` pairs. The directory is relative to the
/// project root, and empty for the root package. `catalog` is the catalog of
/// the calling package, with the workspace packages attached. Every form that
/// selects packages counts: `pnpm --filter web run serve`,
/// `pnpm -r run serve`, `pnpm -C packages/web run serve`,
/// `npm --prefix packages/web run serve`, `yarn --cwd packages/web serve`,
/// and `yarn workspaces foreach -A run serve`. A selected package that does
/// not declare the script adds nothing.
pub fn referenced_workspace_scripts(
    command: &str,
    catalog: &ScriptCatalog,
) -> Vec<(String, String)> {
    let mut references = Vec::new();
    for segment in shell::split_shell_operators(command) {
        let words = shell::split_words(segment);
        let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
        let Some(idx) = shell::skip_initial_wrappers(&tokens, 0) else {
            continue;
        };
        let Some(invocation) = declared_script_invocation(&tokens, idx, catalog) else {
            continue;
        };
        let RunLocation::Packages(selectors) = &invocation.location else {
            continue;
        };
        references.extend(
            catalog
                .selected_packages(selectors)
                .into_iter()
                .filter(|package| package.scripts().contains_key(invocation.name))
                .map(|package| (package.dir().to_string(), invocation.name.to_string())),
        );
    }
    references.sort_unstable();
    references.dedup();
    references
}

fn parse_script_with_context(
    script: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();
    let mut state = ScriptExpansion::new();
    parse_script_internal(
        script,
        &|tokens, idx, catalog| {
            advance_past_package_manager_with_context(tokens, idx, root, bin_map, context, catalog)
        },
        context.scripts,
        &mut state,
        &mut commands,
    );
    commands
}

fn parse_script_internal(
    script: &str,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
    state: &mut ScriptExpansion,
    commands: &mut Vec<ScriptCommand>,
) {
    for segment in shell::split_shell_operators(script) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        for outcome in parse_command_segment(segment, advance_package_manager, catalog) {
            match outcome {
                SegmentOutcome::Command(mut cmd) => {
                    if !state.local_paths {
                        cmd.config_args.clear();
                        cmd.file_args.clear();
                    }
                    commands.push(cmd);
                }
                SegmentOutcome::ScriptCall {
                    name,
                    extra_args,
                    location: RunLocation::Packages(selectors),
                } => {
                    for package in catalog.selected_packages(&selectors) {
                        let dir = catalog.relative_package_dir(package);
                        let package_catalog = catalog.package_catalog(package);
                        let start = commands.len();
                        resolve_script_call(
                            &name,
                            &extra_args,
                            advance_package_manager,
                            &package_catalog,
                            state,
                            commands,
                        );
                        for cmd in &mut commands[start..] {
                            cmd.config_args =
                                rebase_all(std::slice::from_ref(&dir), &cmd.config_args);
                            cmd.file_args = rebase_all(std::slice::from_ref(&dir), &cmd.file_args);
                        }
                    }
                }
                SegmentOutcome::ScriptCall {
                    name, extra_args, ..
                } => {
                    resolve_script_call(
                        &name,
                        &extra_args,
                        advance_package_manager,
                        catalog,
                        state,
                        commands,
                    );
                }
            }
        }
    }
}

/// Re-scan the body of a script invoked through a package manager, with the
/// call-site arguments appended.
///
/// Only follows the indirection when the call site adds arguments: without them
/// the body is already analyzed as a script of its own package, and following it
/// anyway would add nothing.
///
/// Reachability is exactly what the caller put in the catalog. A production run
/// that analyzes filtered scripts must build the catalog with
/// [`ScriptCatalog::from_scripts_with_bodies`]: the names stay complete so the
/// package-manager form still resolves to the script rather than to a
/// same-named binary, while only the analyzed bodies are reachable, so
/// `npm run lint -- --fix` cannot enter a dev-only body that script filtering
/// deliberately skipped.
///
/// Expansion is bounded twice: [`MAX_SCRIPT_INDIRECTION_DEPTH`] bounds a single
/// path, [`MAX_SCRIPT_EXPANSIONS`] bounds the total number of bodies expanded
/// for one command, because the cycle guard only rejects names on the current
/// path and mutually calling scripts otherwise fan out per path.
fn resolve_script_call(
    name: &str,
    extra_args: &str,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
    state: &mut ScriptExpansion,
    commands: &mut Vec<ScriptCommand>,
) {
    if extra_args.is_empty()
        || state.active.len() >= MAX_SCRIPT_INDIRECTION_DEPTH
        || state.expansions >= MAX_SCRIPT_EXPANSIONS
    {
        return;
    }
    let Some(entry) = catalog.body(name) else {
        return;
    };
    if state.active.iter().any(|active_name| active_name == name) {
        return;
    }

    let expanded = format!("{} {extra_args}", entry.body);
    let outer_local_paths = state.local_paths;
    state.local_paths = state.local_paths && entry.local;
    state.expansions += 1;
    state.active.push(name.to_string());
    parse_script_internal(&expanded, advance_package_manager, catalog, state, commands);
    state.active.pop();
    state.local_paths = outer_local_paths;
}

/// Extract file path arguments and `--config`/`-c` arguments from the remaining tokens.
/// When `is_node_runner` is true, flags like `-e`/`--eval`/`-r`/`--require` that consume
/// the next argument are skipped.
fn extract_args_for_binary(
    tokens: &[&str],
    mut idx: usize,
    is_node_runner: bool,
) -> (Vec<String>, Vec<String>) {
    let mut file_args = Vec::new();
    let mut config_args = Vec::new();

    while idx < tokens.len() {
        let token = tokens[idx];

        if is_node_runner
            && matches!(
                token,
                "-e" | "--eval" | "-p" | "--print" | "-r" | "--require"
            )
        {
            idx += 2;
            continue;
        }

        if let Some(config) = extract_config_arg(token, tokens.get(idx + 1).copied()) {
            config_args.push(config);
            if token.contains('=') || token.starts_with("--config=") || token.starts_with("-c=") {
                idx += 1;
            } else {
                idx += 2;
            }
            continue;
        }

        if token.starts_with('-') {
            idx += 1;
            continue;
        }

        if looks_like_file_path(token) {
            file_args.push(token.to_string());
        }
        idx += 1;
    }

    (file_args, config_args)
}

/// Strip a matching pair of surrounding single or double quotes from a token.
///
/// Only strips when the token both starts and ends with the same quote character.
/// A token with a single internal quote (e.g. `can't`) is returned unchanged.
fn strip_surrounding_quotes(token: &str) -> &str {
    if token.len() >= 2 {
        let first = token.as_bytes()[0];
        let last = token.as_bytes()[token.len() - 1];
        if (first == b'\'' || first == b'"') && first == last {
            return &token[1..token.len() - 1];
        }
    }
    token
}

fn advance_past_package_manager_with_context(
    tokens: &[&str],
    idx: usize,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    context: &ScriptCommandContext<'_>,
    catalog: &ScriptCatalog,
) -> Option<PackageManagerTarget> {
    if let Some(target) = script_invocation_target(tokens, idx, catalog) {
        return Some(target);
    }

    if let Some((binary_idx, location)) = package_manager_exec_binary(tokens, idx) {
        return Some(PackageManagerTarget::Binary(binary_idx, location));
    }

    // `yarn eslint`, `yarn run eslint`, `pnpm eslint`, and `bun run eslint` run
    // the binary when no script has that name. So do the forms that run in
    // other workspace packages, such as `yarn workspace web eslint` and
    // `pnpm --filter web eslint`. Only a binary of a declared dependency
    // counts, so a typo or a missing script credits nothing.
    if let Some(run) = package_manager_run(tokens, idx)
        && run.runs_binary_without_script()
        && !catalog.declares_script_at(run.name, &run.location)
    {
        return resolve_known_dependency_binary(run.name, root, bin_map, context.declared_packages)
            .map(|_| PackageManagerTarget::Binary(run.name_idx, run.location));
    }

    shell::advance_past_package_manager(tokens, idx)
        .map(|binary_idx| PackageManagerTarget::Binary(binary_idx, RunLocation::Here))
}

/// Recognize a package manager invocation of a package.json script.
///
/// Handles the explicit `run` form for npm, pnpm, yarn, and bun, plus the bare
/// `yarn <script>` and `pnpm <script>` forms. npm 7 and later forward the
/// positional arguments after the script name, and parse the `-`-prefixed
/// arguments before `--` as npm config. The other managers forward all
/// arguments directly and tolerate a `--` separator.
fn script_invocation_target(
    tokens: &[&str],
    idx: usize,
    catalog: &ScriptCatalog,
) -> Option<PackageManagerTarget> {
    let call = script_call_arguments(tokens, idx, catalog)?;
    Some(PackageManagerTarget::Script {
        name: call.name.to_string(),
        forwarded: call.forwarded,
        location: call.location,
    })
}

/// A call of a declared script: its name, the indices of the call-site
/// arguments that the package manager forwards, and where the script runs.
struct ScriptCallArguments<'a> {
    name: &'a str,
    forwarded: Vec<usize>,
    location: RunLocation,
}

/// Return the name of the declared script that `tokens` call at `idx`, the
/// indices of the call-site arguments that the package manager forwards to
/// it, and where the script runs. A call in selected workspace packages, or
/// in the directory of one, forwards its arguments to the script of each
/// package. A call in another directory, or in packages that no selection
/// resolves, forwards nothing that counts here: the script there resolves
/// the arguments against its own directory and can declare another body.
fn script_call_arguments<'a>(
    tokens: &'a [&'a str],
    idx: usize,
    catalog: &ScriptCatalog,
) -> Option<ScriptCallArguments<'a>> {
    let invocation = declared_script_invocation(tokens, idx, catalog)?;
    let first = invocation.name_idx + 1;
    let forwarded = if matches!(
        invocation.location,
        RunLocation::Directory(_) | RunLocation::OtherPackages
    ) {
        Vec::new()
    } else if tokens.get(first) == Some(&"--") {
        (first + 1..tokens.len()).collect()
    } else if !invocation.npm_config_flags {
        (first..tokens.len()).collect()
    } else {
        npm_forwarded_arguments(tokens, first)
    };
    Some(ScriptCallArguments {
        name: invocation.name,
        forwarded,
        location: invocation.location,
    })
}

/// Return the indices of the arguments from `first` on that npm forwards to
/// a script: the positional arguments before `--` and every argument after
/// it. npm consumes the `-`-prefixed arguments before `--` as its own config.
fn npm_forwarded_arguments(tokens: &[&str], first: usize) -> Vec<usize> {
    let mut forwarded = Vec::new();
    let mut i = first;
    while let Some(&token) = tokens.get(i) {
        if token == "--" {
            forwarded.extend(i + 1..tokens.len());
            break;
        }
        if npm_flag_takes_value(token) {
            i += 2;
            continue;
        }
        if !token.starts_with('-') {
            forwarded.push(i);
        }
        i += 1;
    }
    forwarded
}

/// How a command segment that calls a declared package.json script through a
/// package manager resolves.
#[derive(Debug, PartialEq, Eq)]
pub enum DeclaredScriptCall {
    /// The call adds no file references. It forwards no arguments, so the
    /// body is analyzed as a script of its own, or the body belongs to another
    /// workspace package, so its file arguments are relative to that package.
    NoFileRefs,
    /// The body is not known: several packages declare the name with
    /// different bodies, or script filtering skipped it.
    UnknownBody,
    /// The command that the package manager runs: the script body with the
    /// forwarded call-site arguments appended.
    Command(String),
    /// The commands that the package manager runs in workspace packages
    /// selected by name (`yarn workspace web gen scripts/a.ts`): for each
    /// selected package that declares the script, its body with the
    /// forwarded call-site arguments appended.
    InPackages(Vec<PackageScriptCommand>),
}

/// A script command that runs in a selected workspace package.
#[derive(Debug, PartialEq, Eq)]
pub struct PackageScriptCommand {
    /// The directory of the package, relative to the calling package. The
    /// file arguments of `command` are relative to it.
    pub dir: String,
    /// The script body with the forwarded call-site arguments appended.
    pub command: String,
    /// The directory of the package, relative to the project root.
    package_dir: String,
}

/// Resolve a command segment that calls a declared package.json script
/// (`npm run lint -- src/a.ts`, `yarn lint src/a.ts`, `pnpm fmt src/a.ts`).
/// Return `None` when the segment does not call a declared script.
///
/// `tokens` are the whitespace-separated words of one segment. Environment
/// assignments, env wrappers, and command wrappers at the start are skipped
/// (`varlock run -- yarn lint src/a.ts` calls the `lint` script).
#[must_use]
pub fn declared_script_call(
    tokens: &[&str],
    catalog: &ScriptCatalog,
) -> Option<DeclaredScriptCall> {
    let mut idx = shell::skip_initial_wrappers(tokens, 0)?;
    while let Some(child_idx) = command_wrapper_child_index(tokens, idx) {
        idx = shell::skip_initial_wrappers(tokens, child_idx)?;
    }
    let ScriptCallArguments {
        name,
        forwarded,
        location,
    } = script_call_arguments(tokens, idx, catalog)?;
    if forwarded.is_empty() {
        return Some(DeclaredScriptCall::NoFileRefs);
    }
    let extra_args: Vec<&str> = forwarded.iter().map(|&i| tokens[i]).collect();
    let extra_args = extra_args.join(" ");
    if let RunLocation::Packages(selectors) = &location {
        let commands: Vec<PackageScriptCommand> = catalog
            .selected_packages(selectors)
            .into_iter()
            .filter_map(|package| {
                let body = package.scripts().get(name)?;
                Some(PackageScriptCommand {
                    dir: catalog.relative_package_dir(package),
                    command: format!("{body} {extra_args}"),
                    package_dir: package.dir().to_string(),
                })
            })
            .collect();
        return Some(if commands.is_empty() {
            DeclaredScriptCall::NoFileRefs
        } else {
            DeclaredScriptCall::InPackages(commands)
        });
    }
    let Some(entry) = catalog.body(name) else {
        return Some(DeclaredScriptCall::UnknownBody);
    };
    if !entry.local {
        return Some(DeclaredScriptCall::NoFileRefs);
    }
    Some(DeclaredScriptCall::Command(format!(
        "{} {extra_args}",
        entry.body
    )))
}

struct DeclaredScriptInvocation<'a> {
    name: &'a str,
    name_idx: usize,
    /// `true` for `npm run`: npm parses `-`-prefixed call-site arguments
    /// before `--` as its own config and does not forward them.
    npm_config_flags: bool,
    /// Where the call runs the script.
    location: RunLocation,
}

fn declared_script_invocation<'a>(
    tokens: &'a [&'a str],
    idx: usize,
    catalog: &ScriptCatalog,
) -> Option<DeclaredScriptInvocation<'a>> {
    let run = package_manager_run(tokens, idx)?;
    if !catalog.declares_script_at(run.name, &run.location) {
        return None;
    }

    Some(DeclaredScriptInvocation {
        name: run.name,
        name_idx: run.name_idx,
        npm_config_flags: run.explicit && run.manager == "npm",
        location: catalog.script_call_location(run.location),
    })
}

/// A package manager form that runs a script by name: `npm run <name>`,
/// `yarn [run] <name>`, `pnpm [run] <name>`, or `bun [run] <name>`, plus the
/// forms that run it in other workspace packages, such as
/// `yarn workspace web <name>` and `pnpm -r run <name>`.
struct PackageManagerRun<'a> {
    manager: &'a str,
    name: &'a str,
    name_idx: usize,
    /// `true` for the explicit `run` or `run-script` subcommand.
    explicit: bool,
    /// Where the script or binary runs.
    location: RunLocation,
}

impl PackageManagerRun<'_> {
    /// Whether the package manager runs a binary with this name when no
    /// script has it. yarn does for both forms, pnpm for the bare form, and
    /// bun for `bun run`. npm never does.
    fn runs_binary_without_script(&self) -> bool {
        match self.manager {
            "yarn" => true,
            "pnpm" => !self.explicit && !PNPM_BUILTIN_COMMANDS.contains(&self.name),
            "bun" => self.explicit,
            _ => false,
        }
    }
}

/// A package manager, the index of its subcommand after the flags and the
/// workspace selection that precede it, and where the subcommand runs.
struct ManagerPrefix<'a> {
    manager: &'a str,
    subcmd_idx: usize,
    location: RunLocation,
}

/// Parse the package manager at `idx` and the flags before its subcommand:
/// `pnpm -r --filter web`, `npm -w web`, `yarn workspace web`,
/// `yarn workspaces foreach -A`, or `yarn --cwd docs`.
fn package_manager_prefix<'a>(tokens: &[&'a str], idx: usize) -> Option<ManagerPrefix<'a>> {
    let manager = *tokens.get(idx)?;
    let mut location = RunLocation::Here;
    let subcmd_idx = match manager {
        "pnpm" => skip_pnpm_flags(tokens, idx + 1, &mut location),
        "npm" => skip_npm_flags(tokens, idx + 1, &mut location),
        "yarn" => skip_yarn_selection(tokens, idx + 1, &mut location),
        "bun" => idx + 1,
        _ => return None,
    };
    Some(ManagerPrefix {
        manager,
        subcmd_idx,
        location,
    })
}

fn package_manager_run<'a>(tokens: &'a [&'a str], idx: usize) -> Option<PackageManagerRun<'a>> {
    let ManagerPrefix {
        manager,
        subcmd_idx: next,
        mut location,
    } = package_manager_prefix(tokens, idx)?;
    let subcmd = *tokens.get(next)?;
    // `yarn node` is a yarn command that runs Node.js, not a script call.
    if manager == "yarn" && subcmd == "node" {
        return None;
    }

    let (name_idx, explicit) = if matches!(subcmd, "run" | "run-script") {
        let name_idx = match manager {
            "npm" => {
                npm_run_location(tokens, next + 1, &mut location);
                npm_include_workspace_root(&tokens[idx + 1..], &mut location);
                skip_npm_config_flags(tokens, next + 1)
            }
            "pnpm" => skip_pnpm_flags(tokens, next + 1, &mut location),
            "yarn" => skip_yarn_silent_flags(tokens, next + 1),
            _ => next + 1,
        };
        (name_idx, true)
    } else if matches!(manager, "yarn" | "pnpm" | "bun")
        && !subcmd.starts_with('-')
        && !PACKAGE_MANAGER_BUILTIN_COMMANDS.contains(&subcmd)
    {
        (next, false)
    } else {
        return None;
    };

    let name = *tokens.get(name_idx)?;
    Some(PackageManagerRun {
        manager,
        name,
        name_idx,
        explicit,
        location,
    })
}

/// Return the index of the first token from `idx` that is not a pnpm
/// selection or output flag (or the value of such a flag), and record in
/// `location` the packages or the directory that the flags select.
fn skip_pnpm_flags(tokens: &[&str], mut idx: usize, location: &mut RunLocation) -> usize {
    let mut include_root = false;
    while let Some(&token) = tokens.get(idx) {
        if PNPM_EXEC_BOOLEAN_FLAGS.contains(&token) {
            match token {
                "-r" | "--recursive" => location.select_all_packages(),
                "-w" | "--workspace-root" => location.select_package(PackageSelector::root()),
                "--include-workspace-root" => include_root = true,
                _ => {}
            }
            idx += 1;
            continue;
        }
        let (flag, value, width) = if PNPM_EXEC_VALUE_FLAGS.contains(&token) {
            (token, tokens.get(idx + 1).copied(), 2)
        } else if let Some((flag, value)) = token.split_once('=')
            && PNPM_EXEC_VALUE_FLAGS.contains(&flag)
        {
            (flag, Some(value), 1)
        } else {
            break;
        };
        if PNPM_FILTER_FLAGS.contains(&flag) {
            location.select_package(value.map_or_else(
                || PackageSelector::pnpm_filter(""),
                PackageSelector::pnpm_filter,
            ));
        } else if matches!(flag, "-C" | "--dir")
            && let Some(dir) = value
        {
            location.select_directory(dir);
        }
        idx += width;
    }
    // `--include-workspace-root` adds the root package to a selection of
    // every package: `-r`, or a filter that only excludes packages. An
    // including filter keeps the root out (`--filter web`), and
    // `WorkspacePackages::select` applies that rule.
    if include_root && matches!(location, RunLocation::Packages(_)) {
        location.select_package(PackageSelector::include_root());
    }
    idx.min(tokens.len())
}

/// Return the index of the first token from `idx` that is not an npm config
/// flag (or the value of such a flag), and record in `location` the
/// workspaces or the directory that the flags select.
fn skip_npm_flags(tokens: &[&str], mut idx: usize, location: &mut RunLocation) -> usize {
    while let Some(&token) = tokens.get(idx) {
        if token == "--" || !token.starts_with('-') {
            break;
        }
        idx += apply_npm_flag(tokens, idx, location);
    }
    idx.min(tokens.len())
}

/// Record the workspaces or the directory that the npm config flags from
/// `from` up to `--` select (`npm run lint -w web`).
fn npm_run_location(tokens: &[&str], from: usize, location: &mut RunLocation) {
    let mut idx = from;
    while let Some(&token) = tokens.get(idx) {
        if token == "--" {
            break;
        }
        idx += if token.starts_with('-') {
            apply_npm_flag(tokens, idx, location)
        } else {
            1
        };
    }
}

/// Add the root package to an npm workspace selection when the npm flags in
/// `tokens`, up to `--`, set `--include-workspace-root` (`-iwr`). npm adds
/// the root to every workspace selection, `-w <name>` included. The last
/// value of the flag wins, and the flag alone selects no workspace.
fn npm_include_workspace_root(tokens: &[&str], location: &mut RunLocation) {
    let include_root = tokens.iter().take_while(|token| **token != "--").fold(
        false,
        |include, token| match *token {
            "-iwr" | "--include-workspace-root" | "--include-workspace-root=true" => true,
            "--include-workspace-root=false" | "--no-include-workspace-root" => false,
            _ => include,
        },
    );
    if include_root && matches!(location, RunLocation::Packages(_)) {
        location.select_package(PackageSelector::root());
    }
}

/// Record what the npm config flag at `idx` selects, and return the number
/// of tokens it takes: two for a flag with a separate value, else one.
fn apply_npm_flag(tokens: &[&str], idx: usize, location: &mut RunLocation) -> usize {
    let token = tokens[idx];
    let (flag, value, width) = if npm_flag_takes_value(token) {
        (token, tokens.get(idx + 1).copied(), 2)
    } else if let Some((flag, value)) = token.split_once('=') {
        (flag, Some(value), 1)
    } else {
        (token, None, 1)
    };
    match flag {
        "-w" | "--workspace" => {
            location.select_package(PackageSelector::npm_workspace(value.unwrap_or_default()));
        }
        "-ws" => location.select_package(PackageSelector::npm_workspaces()),
        "--workspaces" if value.is_none_or(|value| value == "true") => {
            location.select_package(PackageSelector::npm_workspaces());
        }
        "-C" | "--prefix" => {
            if let Some(dir) = value {
                location.select_directory(dir);
            }
        }
        _ => {}
    }
    width
}

/// Return the index of the yarn subcommand after `yarn --cwd <dir>`,
/// `yarn workspace <name>`, `yarn workspaces foreach [flags]`, or
/// `yarn workspaces run` (yarn classic), and record in `location` the directory or the packages that they select.
fn skip_yarn_selection(tokens: &[&str], mut idx: usize, location: &mut RunLocation) -> usize {
    while let Some(&token) = tokens.get(idx) {
        if token == "--cwd" {
            if let Some(dir) = tokens.get(idx + 1) {
                location.select_directory(dir);
            }
            idx += 2;
        } else if let Some(dir) = token.strip_prefix("--cwd=") {
            location.select_directory(dir);
            idx += 1;
        } else if matches!(token, "-s" | "--silent") {
            idx += 1;
        } else {
            break;
        }
    }
    let idx = idx.min(tokens.len());
    match tokens.get(idx..idx + 2) {
        Some(["workspace", name]) => {
            location.select_package(PackageSelector::yarn_workspace(name));
            idx + 2
        }
        Some(["workspaces", "foreach"]) => skip_yarn_foreach_flags(tokens, idx + 2, location),
        // Yarn classic runs `yarn run <cmd>` in every workspace. Point at
        // `run` so the caller parses the next token as an explicit run.
        Some(["workspaces", "run"]) => {
            location.select_all_packages();
            idx + 1
        }
        _ => idx,
    }
}

/// Return the index of the first token from `idx` that is not a yarn silent
/// flag. Yarn classic accepts `-s` after `run`, also in the form
/// `yarn workspaces run -s <script>`.
fn skip_yarn_silent_flags(tokens: &[&str], mut idx: usize) -> usize {
    while tokens
        .get(idx)
        .is_some_and(|token| matches!(*token, "-s" | "--silent"))
    {
        idx += 1;
    }
    idx
}

/// Return the index of the first token from `idx` that is not a
/// `yarn workspaces foreach` flag (or the value of such a flag), and record
/// in `location` the packages that the flags select. Only `-A` (`--all`),
/// narrowed by `--include` and `--exclude`, resolves to packages. The other
/// selections, such as `--since`, `--recursive`, and `--no-private`, need
/// facts that the workspace map does not hold.
fn skip_yarn_foreach_flags(tokens: &[&str], mut idx: usize, location: &mut RunLocation) -> usize {
    let mut all = false;
    let mut resolved = true;
    let mut selectors = Vec::new();
    while let Some(&token) = tokens.get(idx) {
        let (flag, value, width) = if YARN_FOREACH_BOOLEAN_FLAGS.contains(&token) {
            (token, None, 1)
        } else if YARN_FOREACH_VALUE_FLAGS.contains(&token) {
            (token, tokens.get(idx + 1).copied(), 2)
        } else if let Some((flag, value)) = token.split_once('=')
            && (YARN_FOREACH_VALUE_FLAGS.contains(&flag) || flag == "--since")
        {
            (flag, Some(value), 1)
        } else {
            break;
        };
        match (flag, value) {
            ("-A" | "--all", _) => all = true,
            ("--include", Some(glob)) => {
                selectors.push(PackageSelector::yarn_foreach_glob(glob, false));
            }
            ("--exclude", Some(glob)) => {
                selectors.push(PackageSelector::yarn_foreach_glob(glob, true));
            }
            (
                "-R" | "--recursive" | "-W" | "--worktree" | "--since" | "--from" | "--no-private",
                _,
            ) => resolved = false,
            _ => {}
        }
        idx += width;
    }
    if all && resolved {
        // Yarn berry lists the root package as a workspace, so `-A` runs in
        // it too.
        location.select_all_packages();
        location.select_package(PackageSelector::include_root());
        for selector in selectors {
            location.select_package(selector);
        }
    } else {
        location.select_unresolved_packages();
    }
    idx.min(tokens.len())
}

/// Skip the npm config flags between `npm run` and the script name, as in
/// `npm run -s lint`.
fn skip_npm_config_flags(tokens: &[&str], idx: usize) -> usize {
    skip_npm_flags(tokens, idx, &mut RunLocation::Here)
}

/// Return the binary index of an explicit package-manager exec form, and
/// where the binary runs: `pnpm [flags] exec|dlx [flags] [--] <binary>`,
/// `npm [flags] exec|x [flags] [--] <binary>`, and
/// `yarn [selection] exec|dlx <binary>`. The flags include the workspace
/// selection, such as `pnpm -r`, `npm -w web`, and
/// `yarn workspaces foreach -A`.
fn package_manager_exec_binary(tokens: &[&str], idx: usize) -> Option<(usize, RunLocation)> {
    let ManagerPrefix {
        manager,
        subcmd_idx,
        mut location,
    } = package_manager_prefix(tokens, idx)?;
    let subcmd = *tokens.get(subcmd_idx)?;
    let mut next = match (manager, subcmd) {
        ("pnpm", "exec" | "dlx") => skip_pnpm_flags(tokens, subcmd_idx + 1, &mut location),
        ("npm", "exec" | "x") => {
            let next = skip_npm_flags(tokens, subcmd_idx + 1, &mut location);
            npm_include_workspace_root(&tokens[idx + 1..next], &mut location);
            next
        }
        ("yarn", "exec" | "dlx") => subcmd_idx + 1,
        // `yarn node <file>` runs Node.js with the yarn environment.
        ("yarn", "node") => return Some((subcmd_idx, location)),
        _ => return None,
    };
    if tokens.get(next) == Some(&"--") {
        next += 1;
    }
    (next < tokens.len()).then_some((next, location))
}

/// Return the command that a segment invokes, after environment assignments,
/// env wrappers, package-manager prefixes, and command wrappers.
///
/// A call of a script that `catalog` or a selected workspace package declares
/// has no binary: the package manager runs the script, also when a binary
/// has the same name. The result is then `None`, or a command without file
/// references at the script name when the call selects other workspace
/// packages or another directory. Otherwise `yarn <name>`, `yarn run <name>`,
/// `pnpm <name>`, and `bun run <name>` count as the binary `<name>`, because
/// the package manager runs that binary when no script has the name.
#[must_use]
pub fn invoked_command(
    tokens: &[&str],
    mut idx: usize,
    catalog: &ScriptCatalog,
) -> Option<InvokedCommand> {
    let mut location = RunLocation::Here;
    loop {
        idx = shell::skip_initial_wrappers(tokens, idx)?;
        let run = package_manager_run(tokens, idx);
        if let Some(run) = &run
            && (catalog.declares_script_at(run.name, &run.location)
                || !run.runs_binary_without_script())
        {
            if run.location == RunLocation::Here {
                return None;
            }
            // A script in other packages or another directory: its body and
            // its arguments belong to that location.
            return Some(InvokedCommand {
                index: run.name_idx,
                base_dirs: Some(Vec::new()),
            });
        }
        let (binary_idx, binary_location) =
            if let Some(exec) = package_manager_exec_binary(tokens, idx) {
                exec
            } else if let Some(run) = run
                && run.runs_binary_without_script()
            {
                (run.name_idx, run.location)
            } else {
                (
                    shell::advance_past_package_manager(tokens, idx)?,
                    RunLocation::Here,
                )
            };
        location = location.nest(binary_location);
        match command_wrapper_child_index(tokens, binary_idx) {
            Some(child_idx) => idx = child_idx,
            None => {
                return Some(InvokedCommand {
                    index: binary_idx,
                    base_dirs: location.base_dirs(catalog),
                });
            }
        }
    }
}

/// Return the target of the command that a command wrapper runs from `idx`,
/// through nested wrappers, as `advance_package_manager` resolves it.
fn wrapped_command_target(
    tokens: &[&str],
    mut idx: usize,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
) -> Option<PackageManagerTarget> {
    let mut location = RunLocation::Here;
    loop {
        idx = shell::skip_initial_wrappers(tokens, idx)?;
        match advance_package_manager(tokens, idx, catalog)? {
            PackageManagerTarget::Binary(binary_idx, binary_location) => {
                location = location.nest(binary_location);
                match command_wrapper_child_index(tokens, binary_idx) {
                    Some(child_idx) => idx = child_idx,
                    None => return Some(PackageManagerTarget::Binary(binary_idx, location)),
                }
            }
            script @ PackageManagerTarget::Script { .. } => return Some(script),
        }
    }
}

/// What a command segment resolved to.
enum SegmentOutcome {
    Command(ScriptCommand),
    /// A package manager invocation of a package.json script, with the
    /// arguments the call site forwards to that script's body.
    ScriptCall {
        name: String,
        extra_args: String,
        /// Where the package manager runs the script.
        location: RunLocation,
    },
}

/// Return the first token of the child command for a known command wrapper.
fn command_wrapper_child_index(tokens: &[&str], idx: usize) -> Option<usize> {
    let wrapper = COMMAND_WRAPPERS.iter().find(|wrapper| {
        tokens.get(idx) == Some(&wrapper.binary)
            && tokens.get(idx + 1..idx + 1 + wrapper.prefix.len()) == Some(wrapper.prefix)
    })?;
    let args_start = idx + 1 + wrapper.prefix.len();
    let separator = tokens[args_start..]
        .iter()
        .position(|token| *token == wrapper.separator)?;
    let child_idx = args_start + separator + 1;

    (child_idx < tokens.len()).then_some(child_idx)
}

/// Parse a single command segment (after splitting on shell operators).
fn parse_command_segment(
    segment: &str,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
) -> Vec<SegmentOutcome> {
    let mut outcomes = Vec::new();
    let words = shell::split_words(segment);
    let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
    let Some(mut idx) = shell::skip_initial_wrappers(&tokens, 0) else {
        return outcomes;
    };
    let mut location = RunLocation::Here;
    loop {
        let Some(target) = advance_package_manager(&tokens, idx, catalog) else {
            return outcomes;
        };
        idx = match target {
            PackageManagerTarget::Binary(binary_idx, binary_location) => {
                location = location.nest(binary_location);
                binary_idx
            }
            PackageManagerTarget::Script {
                name,
                forwarded,
                location: script_location,
            } => {
                outcomes.push(SegmentOutcome::ScriptCall {
                    name,
                    extra_args: forwarded_arguments(segment, &words, &forwarded),
                    location: location.nest(script_location),
                });
                return outcomes;
            }
        };

        let Some(command_start) = command_wrapper_child_index(&tokens, idx) else {
            break;
        };
        let (file_args, config_args, child) =
            wrapped_command_args(&tokens, command_start, advance_package_manager, catalog);
        outcomes.push(SegmentOutcome::Command(ScriptCommand {
            binary: tokens[idx].to_string(),
            config_args: location.resolve_all(config_args, catalog),
            file_args: location.resolve_all(file_args, catalog),
            file_args_command: child.to_string(),
            flag_packages: Vec::new(),
        }));
        let Some(next) = shell::skip_initial_wrappers(&tokens, command_start) else {
            return outcomes;
        };
        idx = next;
    }

    let binary = tokens[idx].to_string();

    if SCRIPT_MULTIPLEXERS.contains(&binary.as_str()) {
        outcomes.push(SegmentOutcome::Command(ScriptCommand {
            file_args_command: binary.clone(),
            binary,
            config_args: Vec::new(),
            file_args: Vec::new(),
            flag_packages: Vec::new(),
        }));
        return outcomes;
    }

    let is_node_runner = NODE_RUNNERS.contains(&binary.as_str());
    let (mut file_args, mut config_args) =
        extract_args_for_binary(&tokens, idx + 1, is_node_runner);
    if is_file_target_tool(&binary) {
        file_args = file_target_tool_loaded_files(&binary, &tokens[idx + 1..]);
    }
    if is_task_runner(&binary) {
        file_args.clear();
        config_args.clear();
    }
    file_args.extend(node_test::default_test_patterns(
        &binary,
        &tokens[idx + 1..],
    ));
    let flag_packages = flag_credits::flag_referenced_packages(&binary, &tokens[idx + 1..]);

    outcomes.push(SegmentOutcome::Command(ScriptCommand {
        file_args_command: binary.clone(),
        binary,
        config_args: location.resolve_all(config_args, catalog),
        file_args: location.resolve_all(file_args, catalog),
        flag_packages,
    }));
    outcomes
}

/// Return the file and config arguments that a command wrapper records for
/// the command it runs from `command_start`, plus the name of that command.
///
/// The arguments stay with the wrapper only when the command runs in the
/// same package as a binary. Then an executable path or a package-manager
/// form that does not resolve to a dependency keeps its entry references.
/// A declared script call and a command in another location record their
/// arguments when the segment parses the command itself.
fn wrapped_command_args<'a>(
    tokens: &[&'a str],
    command_start: usize,
    advance_package_manager: &impl Fn(&[&str], usize, &ScriptCatalog) -> Option<PackageManagerTarget>,
    catalog: &ScriptCatalog,
) -> (Vec<String>, Vec<String>, &'a str) {
    let (child_idx, runs_here) =
        match wrapped_command_target(tokens, command_start, advance_package_manager, catalog) {
            Some(PackageManagerTarget::Binary(child_idx, location)) => {
                (child_idx, location == RunLocation::Here)
            }
            Some(PackageManagerTarget::Script { .. }) => {
                return (Vec::new(), Vec::new(), tokens[command_start]);
            }
            None => invoked_command(tokens, command_start, &ScriptCatalog::default())
                .map_or((command_start, true), |invoked| {
                    (invoked.index, invoked.base_dirs.is_none())
                }),
        };
    let child = tokens[child_idx];
    if !runs_here {
        return (Vec::new(), Vec::new(), child);
    }
    let (mut file_args, mut config_args) = extract_args_for_binary(tokens, command_start, false);
    if is_file_target_tool(child) {
        file_args = file_target_tool_loaded_files(child, &tokens[child_idx + 1..]);
    }
    if is_task_runner(child) {
        file_args.clear();
        config_args.clear();
    }
    file_args.extend(node_test::default_test_patterns(
        child,
        &tokens[child_idx + 1..],
    ));
    (file_args, config_args, child)
}

/// Return `true` when `binary` is a monorepo task runner such as `turbo`,
/// whose arguments are never entry points of the calling package.
#[must_use]
pub fn is_task_runner(binary: &str) -> bool {
    TASK_RUNNERS.contains(&tool_name(binary))
}

/// The source text of the forwarded words, with quoting intact, so the
/// re-scanned script body sees the arguments as the shell would pass them.
fn forwarded_arguments(
    segment: &str,
    words: &[shell::ShellWord<'_>],
    forwarded: &[usize],
) -> String {
    forwarded
        .iter()
        .filter_map(|&i| {
            let start = words.get(i)?.start;
            let end = words.get(i + 1).map_or(segment.len(), |next| next.start);
            Some(segment[start..end].trim_end())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract a config file path from a `--config` or `-c` flag.
fn extract_config_arg(token: &str, next: Option<&str>) -> Option<String> {
    if let Some(value) = token.strip_prefix("--config=")
        && !value.is_empty()
    {
        return Some(value.to_string());
    }
    if let Some(value) = token.strip_prefix("-c=")
        && !value.is_empty()
    {
        return Some(value.to_string());
    }
    if matches!(token, "--config" | "-c")
        && let Some(next_token) = next
        && !next_token.starts_with('-')
    {
        return Some(next_token.to_string());
    }
    None
}

/// Check if a token is an environment variable assignment (`KEY=value`).
fn is_env_assignment(token: &str) -> bool {
    token.find('=').is_some_and(|eq_pos| {
        let name = &token[..eq_pos];
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// Reject tokens whose syntax precludes a Unix path (GHA expressions,
/// backslash escapes, malformed `[...]`). Used as a pre-filter before
/// globset compilation and as a shared single-source-of-truth negative
/// guard for sibling script extractors. Lenient: passes bare names
/// without extensions (e.g. `deploy.log`, `Makefile`).
pub fn could_be_file_path(token: &str) -> bool {
    if token.contains("${{") || (token.contains("}}") && !token.contains("{{")) {
        return false;
    }

    if token.contains('\\') {
        return false;
    }

    if let Some(open) = token.find('[') {
        let after_open = &token[open + 1..];
        let close_offset = after_open.find(']');
        if !matches!(close_offset, Some(offset) if offset > 0) {
            return false;
        }
    }

    true
}

/// Check if a token looks like a file path (has a known extension or path separator).
/// Stricter than `could_be_file_path` — used by CI command extractors to recognize
/// definitely-path-shaped tokens.
fn looks_like_file_path(token: &str) -> bool {
    if !could_be_file_path(token) {
        return false;
    }

    const EXTENSIONS: &[&str] = &[
        ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".json", ".yaml", ".yml",
        ".toml",
    ];
    if EXTENSIONS.iter().any(|ext| token.ends_with(ext)) {
        return true;
    }
    if token.starts_with("./") || token.starts_with("../") {
        return true;
    }
    // A bare positional token is never whitespace-internal: the shell word
    // splitter only produces a multi-word value when the source was quoted
    // (issue #2592: a quoted jq filter such as `.proposals // {}` contains a
    // path separator but is shell/filter syntax, not a file path).
    token.contains('/')
        && !token.contains(char::is_whitespace)
        && !token.starts_with('@')
        && !token.contains("://")
}

/// Check if a command is a shell built-in (not an npm package).
fn is_builtin_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "echo"
            | "cat"
            | "cp"
            | "mv"
            | "rm"
            | "mkdir"
            | "rmdir"
            | "ls"
            | "cd"
            | "pwd"
            | "test"
            | "true"
            | "false"
            | "exit"
            | "export"
            | "source"
            | "which"
            | "chmod"
            | "chown"
            | "touch"
            | "find"
            | "grep"
            | "sed"
            | "awk"
            | "xargs"
            | "tee"
            | "sort"
            | "uniq"
            | "wc"
            | "head"
            | "tail"
            | "sleep"
            | "wait"
            | "kill"
            | "sh"
            | "bash"
            | "zsh"
    )
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "test assertions use std HashMap for readability"
)]
mod tests {
    use super::*;

    /// Analyze every script value without dependency context.
    fn analyze_scripts(
        scripts: &HashMap<String, String>,
        root: &Path,
        bin_map: &FxHashMap<String, String>,
    ) -> ScriptAnalysis {
        let mut result = ScriptAnalysis::default();
        for script_value in scripts.values() {
            accumulate_command(script_value, root, bin_map, &mut result);
        }
        result.dedupe_paths();
        result
    }

    /// Analyze every script value with dependency context and a catalog built
    /// from the same scripts.
    fn analyze_scripts_with_dependencies(
        scripts: &HashMap<String, String>,
        root: &Path,
        bin_map: &FxHashMap<String, String>,
        declared_packages: &FxHashSet<String>,
    ) -> ScriptAnalysis {
        let catalog = ScriptCatalog::from_scripts(scripts);
        analyze_scripts_with_dependency_context(
            scripts,
            root,
            bin_map,
            declared_packages,
            &catalog,
            IgnoredCommandEntries::NONE,
        )
    }

    fn package_set(packages: &[&str]) -> FxHashSet<String> {
        packages.iter().map(|pkg| (*pkg).to_string()).collect()
    }

    /// Analyze a CI-style command against a project whose package.json declares
    /// `scripts` and every package in `declared`.
    fn analyze_ci_command(
        command: &str,
        scripts: &[(&str, &str)],
        declared: &[&str],
    ) -> ScriptAnalysis {
        let scripts: HashMap<String, String> = scripts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect();
        let commands = vec![command.to_string()];
        analyze_commands_with_context(
            &commands,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(declared),
            &ScriptCatalog::from_scripts(&scripts),
            IgnoredCommandEntries::NONE,
        )
    }

    #[test]
    fn normalize_root_level_strips_dot_slash() {
        assert_eq!(
            normalize_script_entry_pattern("", "./scripts/deploy.ts").as_deref(),
            Some("scripts/deploy.ts")
        );
    }

    #[test]
    fn normalize_root_level_keeps_already_relative() {
        assert_eq!(
            normalize_script_entry_pattern("", "scripts/deploy.ts").as_deref(),
            Some("scripts/deploy.ts")
        );
    }

    #[test]
    fn normalize_workspace_prefix_joins_path() {
        assert_eq!(
            normalize_script_entry_pattern("apps/api", "./scripts/deploy.ts").as_deref(),
            Some("apps/api/scripts/deploy.ts")
        );
    }

    #[test]
    fn normalize_workspace_prefix_collapses_parent_segment() {
        assert_eq!(
            normalize_script_entry_pattern("apps/api", "../shared/scripts/deploy.ts").as_deref(),
            Some("apps/shared/scripts/deploy.ts")
        );
    }

    #[test]
    fn normalize_workspace_prefix_collapses_two_parent_segments_to_root() {
        assert_eq!(
            normalize_script_entry_pattern("apps/api", "../../top.ts").as_deref(),
            Some("top.ts")
        );
    }

    #[test]
    fn normalize_path_escaping_project_root_skipped() {
        assert_eq!(normalize_script_entry_pattern("", "../outside.ts"), None);
        assert_eq!(
            normalize_script_entry_pattern("apps/api", "../../../outside.ts"),
            None
        );
    }

    #[test]
    fn normalize_absolute_path_skipped() {
        assert_eq!(normalize_script_entry_pattern("", "/etc/passwd"), None);
    }

    /// Regression test for issue #2592: an internal doubled separator (`//`)
    /// must not be silently collapsed into a single `/`, which is exactly
    /// what turned a jq filter's `//` alternative operator into what looked
    /// like a truncated, malformed path in the reported warning.
    #[test]
    fn normalize_rejects_internal_double_slash_instead_of_silently_collapsing_it() {
        assert_eq!(
            normalize_script_entry_pattern(
                "",
                "[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]"
            ),
            None
        );
    }

    #[test]
    fn normalize_tolerates_trailing_slash() {
        assert_eq!(
            normalize_script_entry_pattern("", "scripts/deploy/").as_deref(),
            Some("scripts/deploy")
        );
    }

    #[test]
    fn normalize_empty_path_skipped() {
        assert_eq!(normalize_script_entry_pattern("", ""), None);
        assert_eq!(normalize_script_entry_pattern("apps/api", "./"), None);
    }

    #[test]
    fn normalize_workspace_prefix_with_trailing_slash() {
        assert_eq!(
            normalize_script_entry_pattern("apps/api/", "./scripts/deploy.ts").as_deref(),
            Some("apps/api/scripts/deploy.ts")
        );
    }

    #[test]
    fn simple_binary() {
        let cmds = parse_script("webpack");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "webpack");
    }

    #[test]
    fn binary_with_args() {
        let cmds = parse_script("eslint src --ext .ts,.tsx");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "eslint");
    }

    #[test]
    fn chained_commands() {
        let cmds = parse_script("tsc --noEmit && eslint src");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "tsc");
        assert_eq!(cmds[1].binary, "eslint");
    }

    #[test]
    fn semicolon_separator() {
        let cmds = parse_script("tsc; eslint src");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "tsc");
        assert_eq!(cmds[1].binary, "eslint");
    }

    #[test]
    fn or_chain() {
        let cmds = parse_script("tsc --noEmit || echo failed");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "tsc");
        assert_eq!(cmds[1].binary, "echo");
    }

    #[test]
    fn pipe_operator() {
        let cmds = parse_script("jest --json | tee results.json");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "jest");
        assert_eq!(cmds[1].binary, "tee");
    }

    #[test]
    fn npx_prefix() {
        let cmds = parse_script("npx eslint src");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "eslint");
    }

    #[test]
    fn pnpx_prefix() {
        let cmds = parse_script("pnpx vitest run");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "vitest");
    }

    #[test]
    fn npx_with_flags() {
        let cmds = parse_script("npx --yes --package @scope/tool eslint src");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "eslint");
    }

    #[test]
    fn yarn_exec() {
        let cmds = parse_script("yarn exec jest");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "jest");
    }

    #[test]
    fn pnpm_exec() {
        let cmds = parse_script("pnpm exec vitest run");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "vitest");
    }

    #[test]
    fn pnpm_dlx() {
        let cmds = parse_script("pnpm dlx create-react-app my-app");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "create-react-app");
    }

    #[test]
    fn npm_run_skipped() {
        let cmds = parse_script("npm run build");
        assert!(cmds.is_empty());
    }

    #[test]
    fn yarn_run_skipped() {
        let cmds = parse_script("yarn run test");
        assert!(cmds.is_empty());
    }

    #[test]
    fn bare_yarn_skipped() {
        let cmds = parse_script("yarn build");
        assert!(cmds.is_empty());
    }

    #[test]
    fn cross_env_prefix() {
        let cmds = parse_script("cross-env NODE_ENV=production webpack");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "webpack");
    }

    #[test]
    fn dotenv_prefix() {
        let cmds = parse_script("dotenv -- next build");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "next");
    }

    #[test]
    fn env_var_assignment_prefix() {
        let cmds = parse_script("NODE_ENV=production webpack --mode production");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "webpack");
    }

    #[test]
    fn multiple_env_vars() {
        let cmds = parse_script("NODE_ENV=test CI=true jest");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "jest");
    }

    #[test]
    fn formatter_and_linter_targets_are_not_file_args() {
        for script in [
            "oxfmt --check \"**/*.ts\"",
            "oxfmt --check src/dead.ts",
            "eslint src/dead.ts",
            "oxlint src/",
            "prettier --write src/a.ts src/b.ts",
            "biome check src/dead.ts",
            "stylelint \"src/**/*.css\"",
            "npx eslint src/dead.ts",
            "pnpm exec prettier --check src/dead.ts",
            "./node_modules/.bin/eslint src/dead.ts",
            "varlock run -- eslint src/dead.ts",
            "varlock run -- CI=1 eslint src/dead.ts",
        ] {
            let cmds = parse_script(script);
            assert!(
                cmds.iter().all(|cmd| cmd.file_args.is_empty()),
                "`{script}` must not produce file args, got: {cmds:?}"
            );
        }
    }

    #[test]
    fn package_manager_forms_of_a_linter_credit_it_without_entries() {
        for command in [
            "yarn run eslint src/dead.ts",
            "yarn eslint src/dead.ts",
            "bun run eslint src/dead.ts",
            "pnpm eslint src/dead.ts",
            "pnpm --silent exec eslint src/dead.ts",
            "npm exec -- eslint src/dead.ts",
            "npx -- eslint src/dead.ts",
            "cross-env CI=1 npx eslint src/dead.ts",
            "varlock run -- npx eslint src/dead.ts",
            "varlock run -- yarn run eslint src/dead.ts",
        ] {
            let result = analyze_ci_command(command, &[], &["eslint", "varlock"]);
            assert!(
                result.used_packages.contains("eslint"),
                "`{command}` did not credit eslint: {:?}",
                result.used_packages
            );
            assert!(
                result.entry_files.is_empty(),
                "`{command}` produced entries: {:?}",
                result.entry_files
            );
        }
    }

    #[test]
    fn workspace_and_env_wrapper_forms_of_a_linter_credit_it_without_entries() {
        for command in [
            "pnpm --filter web exec eslint src/dead.ts",
            "pnpm --filter=web exec eslint src/dead.ts",
            "pnpm -F web exec eslint src/dead.ts",
            "pnpm -r exec eslint src/dead.ts",
            "pnpm --recursive --parallel exec eslint src/dead.ts",
            "pnpm -C packages/web exec eslint src/dead.ts",
            "pnpm exec -r -- eslint src/dead.ts",
            "dotenv -e .env.ci -- eslint src/dead.ts",
            "dotenv -e .env.ci -e .env -- eslint src/dead.ts",
            "dotenv -c production -- eslint src/dead.ts",
            "dotenv -c -- eslint src/dead.ts",
            "dotenv -v CI=1 --override -- eslint src/dead.ts",
            "env -u HOME eslint src/dead.ts",
            "env -i CI=1 eslint src/dead.ts",
        ] {
            let result = analyze_ci_command(command, &[], &["eslint"]);
            assert!(
                result.used_packages.contains("eslint"),
                "`{command}` did not credit eslint: {:?}",
                result.used_packages
            );
            assert!(
                result.entry_files.is_empty(),
                "`{command}` produced entries: {:?}",
                result.entry_files
            );
        }
    }

    #[test]
    fn env_wrapper_forms_of_a_runner_keep_entries() {
        for command in [
            "dotenv -e .env.ci -- tsx scripts/run.ts",
            "pnpm exec tsx scripts/run.ts",
        ] {
            let result = analyze_ci_command(command, &[], &["tsx"]);
            assert_eq!(result.entry_files, vec!["scripts/run.ts"], "`{command}`");
        }
    }

    #[test]
    fn parse_script_with_catalog_resolves_a_call_of_a_linter_script() {
        let scripts: HashMap<String, String> = HashMap::from([
            ("lint".to_string(), "eslint".to_string()),
            ("gen".to_string(), "my-codegen".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);
        for script in [
            "npm run lint -- src/a.ts",
            "yarn lint src/b.ts",
            "pnpm lint src/c.ts",
            "pnpm run lint src/d.ts",
        ] {
            let commands = parse_script_with_catalog(script, &catalog);
            assert!(
                commands.iter().all(|command| command.file_args.is_empty()),
                "`{script}`: {commands:?}"
            );
        }
        let commands = parse_script_with_catalog("npm run gen -- src/gen-input.ts", &catalog);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].file_args, vec!["src/gen-input.ts"]);
        assert_eq!(commands[0].file_args_command, "my-codegen");
        let ignored = vec!["my-codegen".to_string()];
        assert!(
            commands[0]
                .entry_files(IgnoredCommandEntries::new(&ignored))
                .is_empty()
        );
    }

    #[test]
    fn declared_script_call_resolves_only_local_bodies_with_arguments() {
        let mut catalog = ScriptCatalog::from_scripts(&HashMap::from([(
            "lint".to_string(),
            "eslint".to_string(),
        )]));
        catalog.merge_workspace_scripts(&HashMap::from([(
            "gen".to_string(),
            "my-codegen".to_string(),
        )]));
        let tokens = |command: &'static str| command.split_whitespace().collect::<Vec<_>>();
        assert_eq!(
            declared_script_call(&tokens("CI=1 npm run lint -- src/a.ts"), &catalog),
            Some(DeclaredScriptCall::Command("eslint src/a.ts".to_string()))
        );
        assert_eq!(
            declared_script_call(&tokens("npm run lint"), &catalog),
            Some(DeclaredScriptCall::NoFileRefs)
        );
        assert_eq!(
            declared_script_call(&tokens("npm run lint --fix src/a.ts -- --quiet"), &catalog),
            Some(DeclaredScriptCall::Command(
                "eslint src/a.ts --quiet".to_string()
            ))
        );
        assert_eq!(
            declared_script_call(&tokens("npm run -s lint --cache .cache src/a.ts"), &catalog),
            Some(DeclaredScriptCall::Command("eslint src/a.ts".to_string()))
        );
        for other_package in [
            "npm run -s lint -w web src/a.ts",
            "npm run lint --workspace=web src/a.ts",
            "npm run lint --workspaces -- src/a.ts",
            "pnpm --filter web lint src/a.ts",
            "pnpm -F web lint src/a.ts",
            "pnpm --filter=web lint src/a.ts",
        ] {
            assert_eq!(
                declared_script_call(&tokens(other_package), &catalog),
                Some(DeclaredScriptCall::NoFileRefs),
                "{other_package} runs the script in another package"
            );
        }
        assert_eq!(
            declared_script_call(&tokens("npm run lint --fix"), &catalog),
            Some(DeclaredScriptCall::NoFileRefs)
        );
        assert_eq!(
            declared_script_call(&tokens("npm run gen -- src/a.ts"), &catalog),
            Some(DeclaredScriptCall::NoFileRefs)
        );
        assert_eq!(
            declared_script_call(&tokens("npm run other -- src/a.ts"), &catalog),
            None
        );
    }

    #[test]
    fn package_manager_run_of_an_undeclared_binary_credits_nothing() {
        let result = analyze_ci_command("yarn run eslint src/dead.ts", &[], &[]);
        assert!(
            result.used_packages.is_empty(),
            "{:?}",
            result.used_packages
        );
        assert!(result.entry_files.is_empty(), "{:?}", result.entry_files);
    }

    #[test]
    fn package_manager_run_prefers_the_declared_script() {
        let result = analyze_ci_command(
            "yarn run eslint -- --fix",
            &[("eslint", "node scripts/lint.js")],
            &["eslint"],
        );
        assert_eq!(result.entry_files, vec!["scripts/lint.js"]);
    }

    #[test]
    fn npm_run_never_runs_a_binary() {
        let result = analyze_ci_command("npm run eslint src/dead.ts", &[], &["eslint"]);
        assert!(!result.used_packages.contains("eslint"));
    }

    #[test]
    fn wrapped_package_manager_linter_keeps_loaded_modules_only() {
        let result = analyze_ci_command(
            "varlock run -- npx eslint -f ./tools/fmt.js src/dead.ts",
            &[],
            &["eslint", "varlock"],
        );
        assert_eq!(result.entry_files, vec!["./tools/fmt.js"]);
    }

    #[test]
    fn wrapped_runner_keeps_its_entry() {
        let result =
            analyze_ci_command("varlock run -- pnpm tsx scripts/seed.ts", &[], &["varlock"]);
        assert_eq!(result.entry_files, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn additional_formatters_linters_and_checkers_have_no_target_entries() {
        for script in [
            "textlint \"docs/**/*.md\" src/dead.ts",
            "eslint_d src/dead.ts",
            "semistandard src/dead.ts",
            "remark . src/dead.ts",
            "secretlint \"**/*\" src/dead.ts",
            "htmlhint src/dead.ts",
            "markuplint src/dead.ts",
            "alex src/dead.ts",
            "jscpd src/dead.ts",
            "madge --circular src/dead.ts",
            "depcruise src/dead.ts",
            "ember-template-lint src/dead.ts",
            "editorconfig-checker src/dead.ts",
            "rome check src/dead.ts",
        ] {
            let files: Vec<String> = parse_script(script)
                .into_iter()
                .flat_map(|cmd| cmd.file_args)
                .collect();
            assert!(files.is_empty(), "`{script}` produced file args {files:?}");
        }
    }

    #[test]
    fn additional_tools_keep_loaded_module_paths() {
        let cases: [(&str, &str); 12] = [
            (
                "textlint --rulesdir ./rules docs",
                "./rules/*.{js,cjs,mjs,ts,cts,mts}",
            ),
            (
                "eslint . --rulesdir ./devEnv/eslint/rules/",
                "./devEnv/eslint/rules/*.{js,cjs,mjs,ts,cts,mts}",
            ),
            (
                "textlint --rulesdir=. ../src/content",
                "*.{js,cjs,mjs,ts,cts,mts}",
            ),
            (
                "tslint -r tools/rules src/a.ts",
                "tools/rules/*.{js,cjs,mjs,ts,cts,mts}",
            ),
            ("eslint --rulesdir @scope/rules src", ""),
            ("textlint -f ./tools/fmt.js docs", "./tools/fmt.js"),
            ("remark --use ./plugins/lint.mjs .", "./plugins/lint.mjs"),
            (
                "cspell --reporter ./tools/reporter.js .",
                "./tools/reporter.js",
            ),
            ("eslint --parser ./tools/parser.js src", "./tools/parser.js"),
            ("eslint_d -f ./tools/fmt.js src", "./tools/fmt.js"),
            (
                "madge --webpack-config webpack.config.js src",
                "webpack.config.js",
            ),
            (
                "htmlhint --rulesdir ./rules src",
                "./rules/*.{js,cjs,mjs,ts,cts,mts}",
            ),
        ];
        for (script, expected) in cases {
            let files: Vec<String> = parse_script(script)
                .into_iter()
                .flat_map(|cmd| cmd.file_args)
                .collect();
            let expected: Vec<&str> = if expected.is_empty() {
                Vec::new()
            } else {
                vec![expected]
            };
            assert_eq!(files, expected, "`{script}`");
        }
    }

    #[test]
    fn ignored_command_entries_drop_file_args_but_keep_credit_and_config() {
        let scripts: HashMap<String, String> = [
            ("gen", "my-codegen -c codegen.config.ts \"src/**/*.ts\""),
            ("seed", "node scripts/seed.ts"),
            ("fmt", "varlock run -- npx my-codegen src/a.ts"),
        ]
        .into_iter()
        .map(|(name, body)| (name.to_string(), body.to_string()))
        .collect();
        let ignored = vec!["my-codegen".to_string()];
        let result = analyze_scripts_with_dependency_context(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["my-codegen", "varlock"]),
            &ScriptCatalog::from_scripts(&scripts),
            IgnoredCommandEntries::new(&ignored),
        );
        assert!(result.used_packages.contains("my-codegen"));
        assert_eq!(result.config_files, vec!["codegen.config.ts"]);
        assert_eq!(result.entry_files, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn wildcard_ignores_every_command_entry() {
        let ignored = vec!["*".to_string()];
        let commands = vec![
            "node scripts/seed.ts".to_string(),
            "eslint -f ./tools/fmt.js src".to_string(),
        ];
        let result = analyze_commands_with_context(
            &commands,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["eslint"]),
            &ScriptCatalog::default(),
            IgnoredCommandEntries::new(&ignored),
        );
        assert!(result.used_packages.contains("eslint"));
        assert!(result.entry_files.is_empty(), "{:?}", result.entry_files);
    }

    #[test]
    fn formatter_and_linter_keep_config_args() {
        let cmds = parse_script("eslint -c config/eslint.config.js src/dead.ts");
        assert_eq!(cmds[0].binary, "eslint");
        assert_eq!(cmds[0].config_args, vec!["config/eslint.config.js"]);
        assert!(cmds[0].file_args.is_empty());
    }

    #[test]
    fn formatter_and_linter_keep_loaded_module_paths() {
        let cases: [(&str, &[&str]); 10] = [
            ("eslint -f ./tools/fmt.js src", &["./tools/fmt.js"]),
            ("npx eslint --format ./tools/fmt.js .", &["./tools/fmt.js"]),
            (
                "eslint --format=./tools/fmt.js src/a.ts",
                &["./tools/fmt.js"],
            ),
            ("eslint -f stylish src/a.ts", &[]),
            ("prettier --plugin=./p.mjs --check src/a.ts", &["./p.mjs"]),
            (
                "prettier --plugin ./tools/fmt.js --check src",
                &["./tools/fmt.js"],
            ),
            (
                "prettier --plugin prettier-plugin-foo --check src/a.ts",
                &[],
            ),
            (
                "stylelint --custom-formatter ./tools/fmt.js \"**/*.css\"",
                &["./tools/fmt.js"],
            ),
            (
                "varlock run -- eslint -f ./tools/fmt.js src/a.ts",
                &["./tools/fmt.js"],
            ),
            ("oxfmt --check src/dead.ts", &[]),
        ];
        for (script, expected) in cases {
            for cmd in parse_script(script) {
                let file_args: Vec<&str> = cmd.file_args.iter().map(String::as_str).collect();
                assert!(
                    file_args.is_empty() || file_args == expected,
                    "`{script}` produced file args {file_args:?}, expected {expected:?}"
                );
            }
            let all: Vec<String> = parse_script(script)
                .into_iter()
                .flat_map(|cmd| cmd.file_args)
                .collect();
            assert_eq!(all.is_empty(), expected.is_empty(), "`{script}`: {all:?}");
        }
    }

    #[test]
    fn linter_chained_with_node_keeps_node_entry() {
        let cmds = parse_script("eslint src/dead.ts && node scripts/build.js");
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].file_args.is_empty());
        assert_eq!(cmds[1].file_args, vec!["scripts/build.js"]);
    }

    #[test]
    fn node_runner_file_args() {
        let cmds = parse_script("node scripts/build.js");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "node");
        assert_eq!(cmds[0].file_args, vec!["scripts/build.js"]);
    }

    #[test]
    fn tsx_runner_file_args() {
        let cmds = parse_script("tsx scripts/migrate.ts");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "tsx");
        assert_eq!(cmds[0].file_args, vec!["scripts/migrate.ts"]);
    }

    #[test]
    fn node_with_flags() {
        let cmds = parse_script("node --experimental-specifier-resolution=node scripts/run.mjs");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].file_args, vec!["scripts/run.mjs"]);
    }

    #[test]
    fn node_eval_no_file() {
        let cmds = parse_script("node -e \"console.log('hi')\"");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "node");
        assert!(cmds[0].file_args.is_empty());
    }

    #[test]
    fn node_multiple_files() {
        let cmds = parse_script("node --test file1.mjs file2.mjs");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].file_args, vec!["file1.mjs", "file2.mjs"]);
    }

    #[test]
    fn config_equals() {
        let cmds = parse_script("webpack --config=webpack.prod.js");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "webpack");
        assert_eq!(cmds[0].config_args, vec!["webpack.prod.js"]);
    }

    #[test]
    fn config_space() {
        let cmds = parse_script("jest --config jest.config.ts");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "jest");
        assert_eq!(cmds[0].config_args, vec!["jest.config.ts"]);
    }

    #[test]
    fn config_short_flag() {
        let cmds = parse_script("eslint -c .eslintrc.json src");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "eslint");
        assert_eq!(cmds[0].config_args, vec![".eslintrc.json"]);
    }

    #[test]
    fn tsc_maps_to_typescript() {
        let pkg =
            resolve_binary_to_package("tsc", Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(pkg, "typescript");
    }

    #[test]
    fn ng_maps_to_angular_cli() {
        let pkg = resolve_binary_to_package("ng", Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(pkg, "@angular/cli");
    }

    #[test]
    fn biome_maps_to_biomejs() {
        let pkg =
            resolve_binary_to_package("biome", Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(pkg, "@biomejs/biome");
    }

    #[test]
    fn unknown_binary_is_identity() {
        let pkg = resolve_binary_to_package(
            "my-custom-tool",
            Path::new("/nonexistent"),
            &FxHashMap::default(),
        );
        assert_eq!(pkg, "my-custom-tool");
    }

    #[test]
    fn run_s_maps_to_npm_run_all() {
        let pkg =
            resolve_binary_to_package("run-s", Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(pkg, "npm-run-all");
    }

    #[test]
    fn bin_path_regular_package() {
        let path = std::path::Path::new("../webpack/bin/webpack.js");
        assert_eq!(
            resolve::extract_package_from_bin_path(path),
            Some("webpack".to_string())
        );
    }

    #[test]
    fn bin_path_scoped_package() {
        let path = std::path::Path::new("../@babel/cli/bin/babel.js");
        assert_eq!(
            resolve::extract_package_from_bin_path(path),
            Some("@babel/cli".to_string())
        );
    }

    #[test]
    fn builtin_commands_not_tracked() {
        let scripts: HashMap<String, String> =
            std::iter::once(("postinstall".to_string(), "echo done".to_string())).collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.used_packages.is_empty());
    }

    #[test]
    fn analyze_extracts_binaries() {
        let scripts: HashMap<String, String> = [
            ("build".to_string(), "tsc --noEmit && webpack".to_string()),
            ("lint".to_string(), "eslint src".to_string()),
            ("test".to_string(), "jest".to_string()),
        ]
        .into_iter()
        .collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.used_packages.contains("typescript"));
        assert!(result.used_packages.contains("webpack"));
        assert!(result.used_packages.contains("eslint"));
        assert!(result.used_packages.contains("jest"));
    }

    #[test]
    fn analyze_extracts_config_files() {
        let scripts: HashMap<String, String> = std::iter::once((
            "build".to_string(),
            "webpack --config webpack.prod.js".to_string(),
        ))
        .collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.config_files.contains(&"webpack.prod.js".to_string()));
    }

    #[test]
    fn analyze_extracts_entry_files() {
        let scripts: HashMap<String, String> =
            std::iter::once(("seed".to_string(), "ts-node scripts/seed.ts".to_string())).collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.entry_files.contains(&"scripts/seed.ts".to_string()));
        assert!(result.used_packages.contains("ts-node"));
    }

    #[test]
    fn analyze_extracts_k6_run_entry_file_and_binary() {
        let scripts: HashMap<String, String> =
            std::iter::once(("load".to_string(), "k6 run load/smoke.k6.js".to_string())).collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());

        assert!(result.entry_files.contains(&"load/smoke.k6.js".to_string()));
        assert!(result.used_packages.contains("k6"));
    }

    #[test]
    fn analyze_cross_env_with_config() {
        let scripts: HashMap<String, String> = std::iter::once((
            "build".to_string(),
            "cross-env NODE_ENV=production webpack --config webpack.prod.js".to_string(),
        ))
        .collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.used_packages.contains("cross-env"));
        assert!(result.used_packages.contains("webpack"));
        assert!(result.config_files.contains(&"webpack.prod.js".to_string()));
    }

    #[test]
    fn analyze_complex_script() {
        let scripts: HashMap<String, String> = std::iter::once((
            "ci".to_string(),
            "cross-env CI=true npm run build && jest --config jest.ci.js --coverage".to_string(),
        ))
        .collect();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.used_packages.contains("cross-env"));
        assert!(result.used_packages.contains("jest"));
        assert!(!result.used_packages.contains("npm"));
        assert!(result.config_files.contains(&"jest.ci.js".to_string()));
    }

    #[test]
    fn analyze_scripts_with_dependencies_credits_pnpm_bare_declared_binary() {
        let scripts = HashMap::from([(
            "viteinfo".to_string(),
            "pnpm envinfo --system --npmPackages '{vite,@vitejs/*}' --binaries --browsers"
                .to_string(),
        )]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["envinfo"]),
        );
        assert!(result.used_packages.contains("envinfo"));
    }

    #[test]
    fn analyze_scripts_with_dependencies_credits_pnpm_silent_binary() {
        let scripts = HashMap::from([(
            "viteinfo".to_string(),
            "pnpm --silent envinfo --system".to_string(),
        )]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["envinfo"]),
        );
        assert!(result.used_packages.contains("envinfo"));
    }

    #[test]
    fn analyze_scripts_with_dependencies_credits_varlock_run_binary() {
        for command in [
            "varlock run -- is-ci",
            "varlock run -p ./env/ -p ./env/.env.schema -- is-ci",
            "ENVIRONMENT=development varlock run -- is-ci",
            "varlock run -- pnpm is-ci",
            "pnpm exec varlock run -- is-ci",
            "varlock run -- varlock run -- is-ci",
        ] {
            let scripts = HashMap::from([("ci".to_string(), command.to_string())]);
            let result = analyze_scripts_with_dependencies(
                &scripts,
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&["varlock", "is-ci"]),
            );
            assert_eq!(
                result.used_packages,
                package_set(&["varlock", "is-ci"]),
                "{command}"
            );
            assert!(result.entry_files.is_empty(), "{command}");
        }
    }

    #[test]
    fn analyze_scripts_with_dependencies_varlock_preserves_pnpm_exclusions() {
        for command in [
            "varlock run -- pnpm build",
            "varlock run -- pnpm install",
            "varlock run -- pnpm audit",
            "varlock run -- pnpm add lodash",
            "varlock run -- pnpm start",
            "varlock run -- pnpm test",
        ] {
            let scripts = HashMap::from([
                ("build".to_string(), "echo build".to_string()),
                ("check".to_string(), command.to_string()),
            ]);
            let result = analyze_scripts_with_dependencies(
                &scripts,
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&[
                    "varlock", "build", "install", "audit", "add", "lodash", "start", "test",
                ]),
            );
            assert_eq!(result.used_packages, package_set(&["varlock"]), "{command}");
        }
    }

    #[test]
    fn analyze_scripts_with_dependencies_varlock_preserves_executable_file() {
        for command in [
            "varlock run -- './scripts/worker.js'",
            "varlock run -- bun './scripts/worker.js'",
            "varlock run -- node './scripts/worker.js'",
        ] {
            let scripts = HashMap::from([("start".to_string(), command.to_string())]);
            let result = analyze_scripts_with_dependencies(
                &scripts,
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&["varlock"]),
            );
            assert_eq!(result.entry_files, vec!["./scripts/worker.js"], "{command}");
            let result = analyze_command(command, Path::new("/nonexistent"), &FxHashMap::default());
            assert_eq!(result.entry_files, vec!["./scripts/worker.js"], "{command}");
        }
    }

    #[test]
    fn analyze_scripts_with_dependencies_varlock_requires_run_and_separator() {
        for command in [
            "varlock",
            "varlock run",
            "varlock run --",
            "varlock run is-ci",
            "varlock printenv -- is-ci",
        ] {
            let scripts = HashMap::from([("ci".to_string(), command.to_string())]);
            let result = analyze_scripts_with_dependencies(
                &scripts,
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&["varlock", "is-ci"]),
            );
            assert_eq!(result.used_packages, package_set(&["varlock"]), "{command}");
        }
    }

    #[test]
    fn analyze_scripts_with_dependencies_skips_pnpm_script_name_collision() {
        let scripts = HashMap::from([
            ("build".to_string(), "echo build".to_string()),
            ("check".to_string(), "pnpm build".to_string()),
        ]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["build"]),
        );
        assert!(!result.used_packages.contains("build"));
    }

    #[test]
    fn analyze_scripts_with_dependencies_skips_pnpm_builtin_commands() {
        let scripts = HashMap::from([(
            "ci".to_string(),
            "pnpm install && pnpm audit && pnpm add lodash && pnpm start && pnpm test".to_string(),
        )]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["install", "audit", "add", "start", "test"]),
        );
        assert!(result.used_packages.is_empty());
    }

    #[test]
    fn analyze_scripts_with_dependencies_credits_pnpm_divergent_bin_map() {
        let scripts = HashMap::from([(
            "lint".to_string(),
            "pnpm attw --profile esm-only --pack .".to_string(),
        )]);
        let mut bin_map = FxHashMap::default();
        bin_map.insert("attw".to_string(), "@arethetypeswrong/cli".to_string());
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &bin_map,
            &package_set(&["@arethetypeswrong/cli"]),
        );
        assert!(result.used_packages.contains("@arethetypeswrong/cli"));
    }

    #[test]
    fn parse_script_keeps_bare_pnpm_syntax_only_behavior() {
        let cmds = parse_script("pnpm envinfo --system");
        assert!(cmds.is_empty());
    }

    #[test]
    fn env_assignment_valid() {
        assert!(is_env_assignment("NODE_ENV=production"));
        assert!(is_env_assignment("CI=true"));
        assert!(is_env_assignment("PORT=3000"));
    }

    #[test]
    fn env_assignment_invalid() {
        assert!(!is_env_assignment("--config"));
        assert!(!is_env_assignment("webpack"));
        assert!(!is_env_assignment("./scripts/build.js"));
    }

    #[test]
    fn split_respects_quotes() {
        let segments = shell::split_shell_operators("echo 'a && b' && jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn split_double_quotes() {
        let segments = shell::split_shell_operators("echo \"a || b\" || jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn background_operator_splits_commands() {
        let cmds = parse_script("tsc --watch & webpack --watch");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "tsc");
        assert_eq!(cmds[1].binary, "webpack");
    }

    #[test]
    fn double_ampersand_still_works() {
        let cmds = parse_script("tsc --watch && webpack --watch");
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].binary, "tsc");
        assert_eq!(cmds[1].binary, "webpack");
    }

    #[test]
    fn multiple_background_operators() {
        let cmds = parse_script("server & client & proxy");
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].binary, "server");
        assert_eq!(cmds[1].binary, "client");
        assert_eq!(cmds[2].binary, "proxy");
    }

    #[test]
    fn production_script_start() {
        assert!(super::is_production_script("start"));
        assert!(super::is_production_script("prestart"));
        assert!(super::is_production_script("poststart"));
    }

    #[test]
    fn production_script_build() {
        assert!(super::is_production_script("build"));
        assert!(super::is_production_script("prebuild"));
        assert!(super::is_production_script("postbuild"));
        assert!(super::is_production_script("build:prod"));
        assert!(super::is_production_script("build:esm"));
    }

    #[test]
    fn production_script_serve_preview() {
        assert!(super::is_production_script("serve"));
        assert!(super::is_production_script("preview"));
        assert!(super::is_production_script("prepare"));
    }

    #[test]
    fn non_production_scripts() {
        assert!(!super::is_production_script("test"));
        assert!(!super::is_production_script("lint"));
        assert!(!super::is_production_script("dev"));
        assert!(!super::is_production_script("storybook"));
        assert!(!super::is_production_script("typecheck"));
        assert!(!super::is_production_script("format"));
        assert!(!super::is_production_script("e2e"));
    }

    #[test]
    fn mixed_operators_all_binaries_detected() {
        let cmds = parse_script("build && serve & watch || fallback");
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[0].binary, "build");
        assert_eq!(cmds[1].binary, "serve");
        assert_eq!(cmds[2].binary, "watch");
        assert_eq!(cmds[3].binary, "fallback");
    }

    #[test]
    fn background_with_env_vars() {
        let cmds = parse_script("NODE_ENV=production server &");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "server");
    }

    #[test]
    fn trailing_background_operator() {
        let cmds = parse_script("webpack --watch &");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "webpack");
    }

    #[test]
    fn filter_keeps_production_scripts() {
        let scripts: HashMap<String, String> = [
            ("build".to_string(), "webpack".to_string()),
            ("start".to_string(), "node server.js".to_string()),
            ("test".to_string(), "jest".to_string()),
            ("lint".to_string(), "eslint src".to_string()),
            ("dev".to_string(), "next dev".to_string()),
        ]
        .into_iter()
        .collect();

        let filtered = filter_production_scripts(&scripts);
        assert!(filtered.contains_key("build"));
        assert!(filtered.contains_key("start"));
        assert!(!filtered.contains_key("test"));
        assert!(!filtered.contains_key("lint"));
        assert!(!filtered.contains_key("dev"));
    }

    #[test]
    fn npm_run_forwards_call_site_flags_into_script_body() {
        let result = analyze_ci_command(
            "npm run lint -- --format gha",
            &[("lint", "eslint .")],
            &["eslint", "eslint-formatter-gha"],
        );
        assert!(result.used_packages.contains("eslint"));
        assert!(result.used_packages.contains("eslint-formatter-gha"));
    }

    #[test]
    fn yarn_script_without_double_dash_forwards_call_site_flags() {
        let result = analyze_ci_command(
            "yarn lint --format gha",
            &[("lint", "eslint .")],
            &["eslint", "eslint-formatter-gha"],
        );
        assert!(result.used_packages.contains("eslint-formatter-gha"));
    }

    #[test]
    fn pnpm_and_bun_script_forms_forward_call_site_flags() {
        for command in ["pnpm lint --format gha", "bun run lint --format gha"] {
            let result = analyze_ci_command(
                command,
                &[("lint", "eslint .")],
                &["eslint", "eslint-formatter-gha"],
            );
            assert!(
                result.used_packages.contains("eslint-formatter-gha"),
                "{command} credited nothing"
            );
        }
    }

    #[test]
    fn script_body_flags_and_call_site_flags_are_both_credited() {
        let result = analyze_ci_command(
            "npm run lint -- --format gha",
            &[("lint", "eslint . --format json")],
            &["eslint"],
        );
        assert!(result.used_packages.contains("eslint-formatter-json"));
        assert!(result.used_packages.contains("eslint-formatter-gha"));
    }

    #[test]
    fn unknown_script_name_credits_nothing() {
        let result = analyze_ci_command(
            "npm run typecheck -- --format gha",
            &[("lint", "eslint .")],
            &["eslint"],
        );
        assert!(result.used_packages.is_empty());
    }

    #[test]
    fn npm_run_without_double_dash_drops_call_site_flags() {
        let result = analyze_ci_command(
            "npm run lint --format gha",
            &[("lint", "eslint .")],
            &["eslint"],
        );
        assert!(
            !result.used_packages.contains("eslint-formatter-gha"),
            "npm consumes `--format` itself: {:?}",
            result.used_packages
        );
    }

    #[test]
    fn npm_run_forwards_positional_arguments_without_double_dash() {
        let result = analyze_ci_command(
            "npm run gen src/input.ts",
            &[("gen", "node scripts/gen.ts")],
            &[],
        );
        assert!(
            result
                .entry_files
                .iter()
                .any(|file| file.ends_with("src/input.ts")),
            "npm forwards `src/input.ts` to the script: {:?}",
            result.entry_files
        );
    }

    #[test]
    fn package_manager_builtin_is_not_a_script_invocation() {
        let result = analyze_ci_command(
            "yarn install --frozen-lockfile",
            &[("install", "eslint .")],
            &["eslint"],
        );
        assert!(result.used_packages.is_empty());
    }

    #[test]
    fn mutually_recursive_scripts_terminate() {
        let result = analyze_ci_command(
            "npm run a -- --fix",
            &[
                ("a", "npm run b -- --format gha"),
                ("b", "npm run a -- --format json"),
            ],
            &["eslint"],
        );
        assert!(result.used_packages.is_empty());
    }

    #[test]
    fn self_recursive_script_terminates_after_one_expansion() {
        let result = analyze_ci_command(
            "npm run loop -- --fix",
            &[("loop", "eslint . && npm run loop -- --format gha")],
            &["eslint"],
        );
        assert!(result.used_packages.contains("eslint"));
        assert!(!result.used_packages.contains("eslint-formatter-gha"));
    }

    #[test]
    fn deep_script_chain_stops_at_the_depth_limit() {
        let chain: Vec<(String, String)> = (0..12)
            .map(|step| {
                (
                    format!("s{step}"),
                    if step == 11 {
                        "eslint .".to_string()
                    } else {
                        format!("npm run s{} -- --cache", step + 1)
                    },
                )
            })
            .collect();
        let scripts: Vec<(&str, &str)> = chain
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str()))
            .collect();
        let result = analyze_ci_command("npm run s0 -- --fix", &scripts, &["eslint"]);
        assert!(result.used_packages.is_empty());

        let shallow = analyze_ci_command("npm run s9 -- --fix", &scripts, &["eslint"]);
        assert!(shallow.used_packages.contains("eslint"));
    }

    #[test]
    fn ambiguous_script_body_is_not_followed() {
        let mut catalog = ScriptCatalog::from_scripts(&HashMap::from([(
            "lint".to_string(),
            "eslint .".to_string(),
        )]));
        catalog.merge_scripts(&HashMap::from([(
            "lint".to_string(),
            "biome check".to_string(),
        )]));
        let commands = vec!["npm run lint -- --format gha".to_string()];
        let result = analyze_commands_with_context(
            &commands,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["eslint", "biome"]),
            &catalog,
            IgnoredCommandEntries::NONE,
        );
        assert!(catalog.contains("lint"));
        assert!(result.used_packages.is_empty());
    }

    /// A third package restating one of the conflicting bodies must not undo the
    /// ambiguity: which body wins would otherwise depend on workspace order.
    #[test]
    fn ambiguity_survives_a_third_package_restating_the_first_body() {
        let mut catalog = ScriptCatalog::from_scripts(&HashMap::from([(
            "lint".to_string(),
            "eslint .".to_string(),
        )]));
        catalog.merge_scripts(&HashMap::from([(
            "lint".to_string(),
            "biome check".to_string(),
        )]));
        catalog.merge_scripts(&HashMap::from([(
            "lint".to_string(),
            "eslint .".to_string(),
        )]));
        let commands = vec!["npm run lint -- --format gha".to_string()];
        let result = analyze_commands_with_context(
            &commands,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["eslint", "biome", "eslint-formatter-gha"]),
            &catalog,
            IgnoredCommandEntries::NONE,
        );
        assert!(catalog.contains("lint"));
        assert!(result.used_packages.is_empty());
    }

    /// The cycle guard only rejects names on the current path, so a branching
    /// chain fans out per path. The expansion budget bounds the total work.
    #[test]
    fn branching_script_chain_is_bounded_by_the_expansion_budget() {
        let levels = MAX_SCRIPT_INDIRECTION_DEPTH;
        let chain: Vec<(String, String)> = (0..levels)
            .map(|step| {
                (
                    format!("s{step}"),
                    if step + 1 == levels {
                        "tsx leaf.js".to_string()
                    } else {
                        format!(
                            "npm run s{next} -- --a && npm run s{next} -- --b",
                            next = step + 1
                        )
                    },
                )
            })
            .collect();
        let scripts: Vec<(&str, &str)> = chain
            .iter()
            .map(|(name, body)| (name.as_str(), body.as_str()))
            .collect();
        let result = analyze_ci_command("npm run s0 -- --go", &scripts, &["tsx"]);

        assert!(result.used_packages.contains("tsx"));
        assert!(!result.entry_files.is_empty());
        assert!(
            result.entry_files.len() <= MAX_SCRIPT_EXPANSIONS,
            "expansion budget should cap leaf visits, got {}",
            result.entry_files.len()
        );
    }

    /// Under production filtering only the bodies are filtered, so an
    /// argument-bearing call still resolves to the script name but cannot reach
    /// the dev-only body behind it.
    #[test]
    fn production_filtered_catalog_does_not_reach_a_dev_script_body() {
        let scripts = HashMap::from([
            (
                "build".to_string(),
                "npm run lint -- --fix && vite build".to_string(),
            ),
            ("lint".to_string(), "eslint .".to_string()),
        ]);
        let filtered = filter_production_scripts(&scripts);
        let result = analyze_scripts_with_dependency_context(
            &filtered,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["eslint", "vite"]),
            &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
            IgnoredCommandEntries::NONE,
        );
        assert!(result.used_packages.contains("vite"));
        assert!(!result.used_packages.contains("eslint"));
    }

    /// The filtered catalog keeps every declared name, so a body reached
    /// through indirection is unreachable while the name itself still resolves.
    #[test]
    fn filtered_catalog_keeps_names_and_drops_bodies() {
        let scripts = HashMap::from([
            ("build".to_string(), "vite build".to_string()),
            ("lint".to_string(), "eslint .".to_string()),
        ]);
        let filtered = filter_production_scripts(&scripts);
        let catalog = ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered);
        assert!(catalog.contains("lint"));
        assert!(catalog.contains("build"));
        assert!(catalog.body("lint").is_none());
        assert!(catalog.body("build").is_some());
    }

    /// A body reached both as its own script and through indirection must not
    /// contribute the same entry file twice.
    #[test]
    fn repeated_expansion_does_not_duplicate_entry_files() {
        let scripts = HashMap::from([
            (
                "build".to_string(),
                "npm run bundle -- --minify && npm run bundle -- --watch".to_string(),
            ),
            (
                "bundle".to_string(),
                "esbuild scripts/bundle.js".to_string(),
            ),
        ]);
        let result = analyze_scripts_with_dependency_context(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["esbuild"]),
            &ScriptCatalog::from_scripts(&scripts),
            IgnoredCommandEntries::NONE,
        );
        assert_eq!(result.entry_files, vec!["scripts/bundle.js".to_string()]);
    }

    /// A body merged from another workspace package still credits dependencies,
    /// but its file arguments are relative to that package, not to the analysis
    /// root, so they must not become entry files here.
    #[test]
    fn workspace_body_credits_packages_without_leaking_file_arguments() {
        let mut catalog = ScriptCatalog::default();
        catalog.merge_workspace_scripts(&HashMap::from([(
            "build".to_string(),
            "esbuild scripts/bundle.js".to_string(),
        )]));
        let commands = vec!["npm run build -- --mode ci".to_string()];
        let result = analyze_commands_with_context(
            &commands,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["esbuild"]),
            &catalog,
            IgnoredCommandEntries::NONE,
        );
        assert!(result.used_packages.contains("esbuild"));
        assert!(result.entry_files.is_empty());
    }

    #[test]
    fn local_body_still_contributes_file_arguments() {
        let result = analyze_ci_command(
            "npm run build -- --mode ci",
            &[("build", "esbuild scripts/bundle.js")],
            &["esbuild"],
        );
        assert!(result.used_packages.contains("esbuild"));
        assert!(
            result
                .entry_files
                .contains(&"scripts/bundle.js".to_string())
        );
    }

    /// Built the same way as the production callers build it, so the guard this
    /// test covers is the one that actually runs.
    #[test]
    fn production_filtered_context_skips_non_production_script_name() {
        let scripts = HashMap::from([
            ("build".to_string(), "pnpm lint".to_string()),
            ("lint".to_string(), "eslint src".to_string()),
        ]);
        let filtered = filter_production_scripts(&scripts);
        let result = analyze_scripts_with_dependency_context(
            &filtered,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["lint"]),
            &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
            IgnoredCommandEntries::NONE,
        );
        assert!(!result.used_packages.contains("lint"));
    }

    /// `pnpm <name>` runs the declared script, never a same-named binary. A
    /// filtered-out script keeps its name for exactly that reason: dropping it
    /// would credit the dependency and pick up the remaining tokens as files.
    #[test]
    fn filtered_script_name_shadowing_a_dependency_bin_credits_nothing() {
        let scripts = HashMap::from([
            (
                "build".to_string(),
                "pnpm lint --fix src/app.ts".to_string(),
            ),
            ("lint".to_string(), "eslint src".to_string()),
        ]);
        let filtered = filter_production_scripts(&scripts);
        let result = analyze_scripts_with_dependency_context(
            &filtered,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["lint", "eslint"]),
            &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
            IgnoredCommandEntries::NONE,
        );
        assert!(!result.used_packages.contains("lint"));
        assert!(!result.used_packages.contains("eslint"));
        assert!(result.entry_files.is_empty());
    }

    #[test]
    fn looks_like_file_path_with_known_extensions() {
        assert!(super::looks_like_file_path("src/app.ts"));
        assert!(super::looks_like_file_path("config.json"));
        assert!(super::looks_like_file_path("setup.yaml"));
        assert!(super::looks_like_file_path("rollup.config.mjs"));
        assert!(super::looks_like_file_path("test.spec.tsx"));
        assert!(super::looks_like_file_path("file.toml"));
    }

    #[test]
    fn looks_like_file_path_with_relative_prefix() {
        assert!(super::looks_like_file_path("./scripts/build"));
        assert!(super::looks_like_file_path("../shared/utils"));
    }

    #[test]
    fn looks_like_file_path_with_slash_but_not_scope() {
        assert!(super::looks_like_file_path("src/components/Button"));
        assert!(!super::looks_like_file_path("@scope/package")); // scoped package
    }

    #[test]
    fn looks_like_file_path_url_not_file() {
        assert!(!super::looks_like_file_path("https://example.com/path"));
    }

    #[test]
    fn looks_like_file_path_bare_word_not_file() {
        assert!(!super::looks_like_file_path("webpack"));
        assert!(!super::looks_like_file_path("--mode"));
        assert!(!super::looks_like_file_path("production"));
    }

    #[test]
    fn looks_like_file_path_github_actions_expression_not_file() {
        assert!(!super::looks_like_file_path(
            r#""${{ env.ENVIRONMENT_URL }}/api/health/ready""#
        ));
        assert!(!super::looks_like_file_path("}}/api/health/ready\""));
        assert!(!super::looks_like_file_path("${{ env.BASE_URL }}"));
    }

    #[test]
    fn looks_like_file_path_jq_array_iterator_not_file() {
        assert!(!super::looks_like_file_path(".[]"));
        assert!(!super::looks_like_file_path("'.[]'"));
    }

    /// Regression test for issue #2592: a quoted jq filter using the `//`
    /// alternative operator was picked up as a file path candidate purely
    /// because it contains a `/`, and its whitespace-separated words are the
    /// distinguishing signal that it is shell/filter syntax, not a path.
    #[test]
    fn looks_like_file_path_jq_alternative_operator_not_file() {
        assert!(!super::looks_like_file_path(
            "[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]"
        ));
    }

    #[test]
    fn looks_like_file_path_regex_fragment_not_file() {
        assert!(!super::looks_like_file_path(r")\./[^"));
        assert!(!super::looks_like_file_path(r"path\with\backslash"));
        assert!(!super::looks_like_file_path("prefix/[^unclosed"));
    }

    #[test]
    fn looks_like_file_path_valid_nextjs_dynamic_route() {
        assert!(super::looks_like_file_path("app/[id]/page.tsx"));
        assert!(super::looks_like_file_path("pages/[...slug].ts"));
    }

    #[test]
    fn could_be_file_path_passes_bare_names() {
        assert!(super::could_be_file_path("deploy.log"));
        assert!(super::could_be_file_path("Makefile"));
        assert!(super::could_be_file_path("Cargo.lock"));
    }

    #[test]
    fn could_be_file_path_passes_balanced_mustache() {
        assert!(super::could_be_file_path("templates/{{name}}.hbs"));
        assert!(super::could_be_file_path("{{partial}}.html"));
    }

    #[test]
    fn could_be_file_path_rejects_ghs_fragments() {
        assert!(!super::could_be_file_path("${{ env.X }}"));
        assert!(!super::could_be_file_path("}}/path"));
    }

    #[test]
    fn could_be_file_path_rejects_regex_and_jq_fragments() {
        assert!(!super::could_be_file_path(r")\./[^"));
        assert!(!super::could_be_file_path(".[]"));
    }

    #[test]
    fn extract_config_arg_with_equals() {
        assert_eq!(
            super::extract_config_arg("--config=webpack.prod.js", None),
            Some("webpack.prod.js".to_string())
        );
    }

    #[test]
    fn extract_config_arg_short_with_equals() {
        assert_eq!(
            super::extract_config_arg("-c=.eslintrc.json", None),
            Some(".eslintrc.json".to_string())
        );
    }

    #[test]
    fn extract_config_arg_with_next_token() {
        assert_eq!(
            super::extract_config_arg("--config", Some("jest.config.ts")),
            Some("jest.config.ts".to_string())
        );
    }

    #[test]
    fn extract_config_arg_short_with_next_token() {
        assert_eq!(
            super::extract_config_arg("-c", Some(".eslintrc.json")),
            Some(".eslintrc.json".to_string())
        );
    }

    #[test]
    fn extract_config_arg_next_is_flag_returns_none() {
        assert_eq!(
            super::extract_config_arg("--config", Some("--verbose")),
            None
        );
    }

    #[test]
    fn extract_config_arg_no_match() {
        assert_eq!(super::extract_config_arg("--verbose", None), None);
        assert_eq!(super::extract_config_arg("src/index.ts", None), None);
    }

    #[test]
    fn extract_config_arg_empty_equals_returns_none() {
        assert_eq!(super::extract_config_arg("--config=", None), None);
        assert_eq!(super::extract_config_arg("-c=", None), None);
    }

    #[test]
    fn node_require_flag_skips_next_arg() {
        let cmds = parse_script("node -r tsconfig-paths/register ./src/server.ts");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "node");
        assert!(cmds[0].file_args.contains(&"./src/server.ts".to_string()));
        assert!(
            !cmds[0]
                .file_args
                .contains(&"tsconfig-paths/register".to_string())
        );
    }

    #[test]
    fn node_eval_skips_next_arg() {
        let cmds = parse_script("node --eval \"console.log(1)\" scripts/run.js");
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].file_args.contains(&"scripts/run.js".to_string()));
    }

    #[test]
    fn production_script_prepublish_only() {
        assert!(super::is_production_script("prepublishOnly"));
    }

    #[test]
    fn production_script_postinstall() {
        assert!(super::is_production_script("postinstall"));
    }

    #[test]
    fn production_script_preserve_is_not_production() {
        assert!(super::is_production_script("preserve"));
    }

    #[test]
    fn production_script_preinstall() {
        assert!(super::is_production_script("preinstall"));
    }

    #[test]
    fn production_script_namespaced() {
        assert!(super::is_production_script("build:esm"));
        assert!(super::is_production_script("start:dev"));
        assert!(!super::is_production_script("test:unit"));
        assert!(!super::is_production_script("lint:fix"));
    }

    #[test]
    fn env_assignment_empty_value() {
        assert!(is_env_assignment("KEY="));
    }

    #[test]
    fn env_assignment_equals_at_start_is_not_assignment() {
        assert!(!is_env_assignment("=value"));
    }

    #[test]
    fn parse_empty_script() {
        let cmds = parse_script("");
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_whitespace_only_script() {
        let cmds = parse_script("   ");
        assert!(cmds.is_empty());
    }

    #[test]
    fn analyze_scripts_empty_scripts() {
        let scripts: HashMap<String, String> = HashMap::new();
        let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(result.used_packages.is_empty());
        assert!(result.config_files.is_empty());
        assert!(result.entry_files.is_empty());
    }

    #[test]
    fn bun_treated_as_package_manager() {
        let cmds = parse_script("bun scripts/build.ts");
        assert!(
            cmds.is_empty(),
            "bare `bun <arg>` should be treated as running a script (like yarn)"
        );
    }

    #[test]
    fn bun_exec_extracts_binary() {
        let cmds = parse_script("bun exec vitest run");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "vitest");
    }

    #[test]
    fn bun_runtime_flag_extracts_binary() {
        let cmds = parse_script("bun --bun prek install");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "prek");
    }

    #[test]
    fn bun_multiple_runtime_flags_extract_binary() {
        let cmds = parse_script("bun --bun --watch prek");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "prek");
    }

    #[test]
    fn bun_runtime_flag_before_run_is_script() {
        let cmds = parse_script("bun --watch run dev");
        assert!(cmds.is_empty());
    }

    #[test]
    fn bun_unknown_flag_credits_nothing() {
        let cmds = parse_script("bun --filter foo run build");
        assert!(cmds.is_empty());
    }

    #[test]
    fn bun_x_extracts_binary() {
        let cmds = parse_script("bun x cowsay hello");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "cowsay");
    }

    #[test]
    fn concurrently_with_npm_prefix() {
        let scripts = HashMap::from([(
            "dev".to_string(),
            "concurrently \"npm:server\" \"npm:worker\"".to_string(),
        )]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("concurrently"));
        assert!(!result.used_packages.contains("server"));
        assert!(!result.used_packages.contains("worker"));
        assert!(!result.used_packages.contains("npm:server"));
    }

    #[test]
    fn run_p_with_bare_script_names() {
        let scripts = HashMap::from([("dev".to_string(), "run-p server worker".to_string())]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("npm-run-all"));
        assert!(!result.used_packages.contains("server"));
        assert!(!result.used_packages.contains("worker"));
    }

    #[test]
    fn run_s_with_bare_script_names() {
        let scripts = HashMap::from([("build".to_string(), "run-s clean compile".to_string())]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("npm-run-all"));
        assert!(!result.used_packages.contains("clean"));
        assert!(!result.used_packages.contains("compile"));
    }

    #[test]
    fn npm_run_all_with_script_names() {
        let scripts = HashMap::from([(
            "dev".to_string(),
            "npm-run-all --parallel server worker".to_string(),
        )]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("npm-run-all"));
        assert!(!result.used_packages.contains("server"));
        assert!(!result.used_packages.contains("worker"));
    }

    #[test]
    fn concurrently_with_flags_before_args() {
        let scripts = HashMap::from([(
            "dev".to_string(),
            "concurrently --kill-others \"npm:server\" \"npm:worker\"".to_string(),
        )]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("concurrently"));
        assert!(!result.used_packages.contains("server"));
        assert!(!result.used_packages.contains("worker"));
        assert!(!result.used_packages.contains("kill-others"));
    }

    #[test]
    fn concurrently_unquoted_npm_prefix() {
        let scripts = HashMap::from([(
            "dev".to_string(),
            "concurrently npm:dev npm:test".to_string(),
        )]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("concurrently"));
        assert!(!result.used_packages.contains("dev"));
        assert!(!result.used_packages.contains("test"));
        assert!(!result.used_packages.contains("npm:dev"));
    }

    #[test]
    fn run_p_with_npm_prefix() {
        let scripts = HashMap::from([(
            "dev".to_string(),
            "run-p \"npm:server\" \"npm:worker\"".to_string(),
        )]);
        let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
        assert!(result.used_packages.contains("npm-run-all"));
        assert!(!result.used_packages.contains("server"));
    }

    #[test]
    fn node_test_quoted_glob_strips_quotes() {
        // Regression test for issue #841: quoted glob args kept their quotes,
        // causing looks_like_file_path to reject them and the entry pattern to
        // match zero files.
        let cmds = parse_script("node --test --import tsx 'src/**/*.test.ts'");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "node");
        // The surrounding quotes must be stripped from the glob.
        assert!(
            cmds[0].file_args.contains(&"src/**/*.test.ts".to_string()),
            "expected unquoted glob in file_args, got: {:?}",
            cmds[0].file_args
        );
        assert!(
            !cmds[0]
                .file_args
                .iter()
                .any(|f| f.starts_with('\'') || f.ends_with('\'')),
            "file_args must not contain surrounding single quotes"
        );
    }

    #[test]
    fn bare_node_test_script_records_default_test_patterns() {
        for script in [
            "node --test",
            "NODE_ENV=test node --test",
            "node --experimental-strip-types --test",
            "node --test --watch",
            "node --test --import tsx --test-reporter spec",
        ] {
            let cmds = parse_script(script);
            assert_eq!(cmds.len(), 1, "`{script}`");
            assert_eq!(cmds[0].binary, "node", "`{script}`");
            assert!(
                cmds[0]
                    .file_args
                    .contains(&"**/*.test.{js,mjs,cjs,ts,mts,cts}".to_string()),
                "`{script}` produced {:?}",
                cmds[0].file_args
            );
            assert!(
                !cmds[0]
                    .file_args
                    .iter()
                    .any(|arg| arg == "tsx" || arg == "spec"),
                "`{script}` recorded a flag value: {:?}",
                cmds[0].file_args
            );
        }
    }

    #[test]
    fn node_test_with_file_argument_records_only_that_file() {
        let cmds = parse_script("node --test test/only.test.ts");
        assert_eq!(cmds[0].file_args, vec!["test/only.test.ts"]);
    }

    #[test]
    fn node_test_setup_import_keeps_default_test_patterns() {
        let cmds = parse_script("node --import ./setup.ts --test");
        assert!(cmds[0].file_args.contains(&"./setup.ts".to_string()));
        assert!(
            cmds[0]
                .file_args
                .contains(&"**/test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string())
        );
    }

    #[test]
    fn ignored_node_command_drops_default_test_patterns() {
        let ignored = vec!["node".to_string()];
        let cmds = parse_script("node --test");
        assert!(
            cmds[0]
                .entry_files(IgnoredCommandEntries::new(&ignored))
                .is_empty()
        );
    }

    #[test]
    fn node_test_unquoted_glob_still_works() {
        // Unquoted globs must continue to be extracted correctly.
        let cmds = parse_script("node --test src/**/*.test.ts");
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].binary, "node");
        assert!(cmds[0].file_args.contains(&"src/**/*.test.ts".to_string()));
    }

    #[test]
    fn referenced_scripts_include_plain_npm_run_without_forwarded_args() {
        let scripts = HashMap::from([
            ("start".to_string(), "npm run serve".to_string()),
            ("serve".to_string(), "node src/server.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);

        let referenced = referenced_package_scripts(&scripts["start"], &catalog);

        assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
    }

    #[test]
    fn referenced_scripts_honor_pnpm_silent_shorthand() {
        let scripts = HashMap::from([
            ("start".to_string(), "pnpm -s serve".to_string()),
            ("serve".to_string(), "node src/server.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);

        let referenced = referenced_package_scripts(&scripts["start"], &catalog);

        assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
    }

    #[test]
    fn referenced_scripts_honor_package_manager_options() {
        let scripts = HashMap::from([
            ("start".to_string(), "npm --silent run serve".to_string()),
            ("serve".to_string(), "node src/server.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);
        assert_eq!(
            referenced_package_scripts(&scripts["start"], &catalog),
            FxHashSet::from_iter(["serve".to_string()])
        );

        let command = "pnpm --filter app run serve";
        assert_eq!(
            referenced_package_scripts(command, &catalog),
            FxHashSet::default(),
            "workspace-qualified calls must not promote a same-named local script"
        );
    }

    #[test]
    fn referenced_scripts_include_multiplexer_targets() {
        let scripts = HashMap::from([
            (
                "start".to_string(),
                "run-p serve worker && concurrently \"npm:monitor\"".to_string(),
            ),
            ("serve".to_string(), "node src/server.ts".to_string()),
            ("worker".to_string(), "node src/worker.ts".to_string()),
            ("monitor".to_string(), "node src/monitor.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);

        let referenced = referenced_package_scripts(&scripts["start"], &catalog);

        assert_eq!(
            referenced,
            FxHashSet::from_iter([
                "serve".to_string(),
                "worker".to_string(),
                "monitor".to_string(),
            ])
        );
    }

    #[test]
    fn referenced_scripts_skip_multiplexer_option_values() {
        let scripts = HashMap::from([
            (
                "start".to_string(),
                "concurrently --names serve npm:worker npm:api".to_string(),
            ),
            ("serve".to_string(), "node src/server.ts".to_string()),
            ("worker".to_string(), "node src/worker.ts".to_string()),
            ("api".to_string(), "node src/api.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);

        let referenced = referenced_package_scripts(&scripts["start"], &catalog);

        assert_eq!(
            referenced,
            FxHashSet::from_iter(["worker".to_string(), "api".to_string()])
        );
    }

    #[test]
    fn referenced_scripts_include_bun_implicit_target() {
        let scripts = HashMap::from([
            ("start".to_string(), "bun serve".to_string()),
            ("serve".to_string(), "bun src/server.ts".to_string()),
        ]);
        let catalog = ScriptCatalog::from_scripts(&scripts);

        let referenced = referenced_package_scripts(&scripts["start"], &catalog);

        assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
    }

    #[test]
    fn referenced_workspace_scripts_preserve_package_identity() {
        let mut packages = WorkspacePackages::default();
        let serve = std::collections::HashMap::from([(
            "serve".to_string(),
            "node src/server.ts".to_string(),
        )]);
        packages.add("@scope/api", "packages/api", Some(&serve));
        packages.add("@scope/web", "packages/web", None);
        let catalog = ScriptCatalog::default().with_workspaces(std::sync::Arc::new(packages), "");
        for command in [
            "pnpm --filter @scope/api run serve",
            "pnpm --filter=@scope/api run serve",
            "npm --workspace @scope/api run serve",
            "npm --workspace=@scope/api run serve",
            "yarn workspace @scope/api serve",
            "pnpm -r run serve",
            "pnpm --recursive serve",
            "pnpm -C packages/api run serve",
            "pnpm --dir=packages/api serve",
            "npm --prefix packages/api run serve",
            "npm run serve --prefix packages/api",
            "yarn --cwd packages/api serve",
            "yarn --cwd=packages/api run serve",
            "yarn workspaces foreach -A run serve",
            "yarn workspaces run serve",
            "npm -ws run serve",
        ] {
            assert_eq!(
                referenced_workspace_scripts(command, &catalog),
                vec![("packages/api".to_string(), "serve".to_string())],
                "failed to parse {command}"
            );
        }
        for command in [
            "pnpm --filter @scope/web run serve",
            "pnpm -C packages/other run serve",
            "yarn workspaces foreach --since run serve",
            "pnpm run serve",
        ] {
            assert!(
                referenced_workspace_scripts(command, &catalog).is_empty(),
                "{command} calls no script of a workspace package"
            );
        }
    }

    #[test]
    fn varlock_preserves_quoted_argument_boundaries() {
        for command in [
            r#"varlock run -p "./env -- is-ci ignored" -- publint"#,
            "varlock run -p './env -- is-ci ignored' -- publint",
            r"varlock run -p ./env\ --\ is-ci\ ignored -- publint",
            r#"varlock run -p "./env \" -- is-ci ignored" -- publint"#,
        ] {
            let result = analyze_scripts_with_dependencies(
                &HashMap::from([("check".to_string(), command.to_string())]),
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&["varlock", "is-ci", "publint"]),
            );
            assert_eq!(
                result.used_packages,
                package_set(&["varlock", "publint"]),
                "{command}"
            );
        }
    }

    #[test]
    fn varlock_forwards_quoted_arguments_to_package_scripts() {
        let result = analyze_ci_command(
            r#"varlock run -p "./env with spaces" -- npm run child -- -p "./env -- is-ci ignored" -- publint"#,
            &[("child", "varlock run")],
            &["varlock", "is-ci", "publint"],
        );
        assert_eq!(result.used_packages, package_set(&["varlock", "publint"]));
    }

    #[test]
    fn varlock_preserves_quoted_child_paths() {
        let commands = parse_script(r#"varlock run -- node "./scripts/worker with spaces.js""#);
        assert!(commands.iter().any(|command| {
            command
                .file_args
                .contains(&"./scripts/worker with spaces.js".to_string())
        }));
    }

    #[test]
    fn varlock_does_not_guess_through_unbalanced_or_dynamic_words() {
        for command in [
            r#"varlock run -p "./env -- is-ci ignored -- publint"#,
            "varlock run -p './env -- is-ci ignored -- publint",
            "varlock run -p $(echo -- is-ci) -- publint",
            "varlock run -p `echo -- is-ci` -- publint",
        ] {
            let commands = parse_script(command);
            assert_eq!(
                commands
                    .iter()
                    .map(|command| command.binary.as_str())
                    .collect::<Vec<_>>(),
                vec!["varlock"],
                "{command}"
            );
        }
    }

    #[test]
    fn varlock_keeps_known_child_before_dynamic_arguments() {
        for command in [
            "varlock run -- vite --host=$(hostname)",
            "varlock run -- vite $(pwd)",
            "varlock run -- vite `pwd`",
        ] {
            let commands = parse_script(command);
            assert_eq!(
                commands
                    .iter()
                    .map(|command| command.binary.as_str())
                    .collect::<Vec<_>>(),
                vec!["varlock", "vite"],
                "{command}"
            );
        }
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// parse_script should never panic on arbitrary input.
            #[test]
            fn parse_script_no_panic(s in "[a-zA-Z0-9 _./@&|;=\"'-]{1,200}") {
                let _ = parse_script(&s);
            }

            /// split_shell_operators should never panic on arbitrary input.
            #[test]
            fn split_shell_operators_no_panic(s in "[a-zA-Z0-9 _./@&|;=\"'-]{1,200}") {
                let _ = shell::split_shell_operators(&s);
            }

            /// When parse_script returns commands, binary names should be non-empty.
            #[test]
            fn parsed_binaries_are_non_empty(
                binary in "[a-z][a-z0-9-]{0,20}",
                args in "[a-zA-Z0-9 _./=-]{0,50}",
            ) {
                let script = format!("{binary} {args}");
                let commands = parse_script(&script);
                for cmd in &commands {
                    prop_assert!(!cmd.binary.is_empty(), "Binary name should never be empty");
                }
            }

            /// analyze_scripts should never panic on arbitrary script values.
            #[test]
            fn analyze_scripts_no_panic(
                name in "[a-z]{1,10}",
                value in "[a-zA-Z0-9 _./@&|;=-]{1,100}",
            ) {
                let scripts: HashMap<String, String> = std::iter::once((name, value)).collect();
                let _ = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
            }

            /// is_env_assignment should never panic on arbitrary input.
            #[test]
            fn is_env_assignment_no_panic(s in "[a-zA-Z0-9_=./-]{1,50}") {
                let _ = is_env_assignment(&s);
            }

            /// resolve_binary_to_package should always return a non-empty string.
            #[test]
            fn resolve_binary_always_non_empty(binary in "[a-z][a-z0-9-]{0,20}") {
                let result = resolve_binary_to_package(&binary, Path::new("/nonexistent"), &FxHashMap::default());
                prop_assert!(!result.is_empty(), "Package name should never be empty");
            }

            /// Chained scripts should produce at least as many commands as operators + 1
            /// when each segment is a valid binary (excluding package managers and builtins).
            #[test]
            fn chained_binaries_produce_multiple_commands(
                bins in prop::collection::vec("[a-z][a-z0-9]{0,10}", 2..5),
            ) {
                let reserved = ["npm", "npx", "yarn", "pnpm", "pnpx", "bun", "bunx",
                    "node", "env", "cross", "sh", "bash", "exec", "sudo", "nohup"];
                prop_assume!(!bins.iter().any(|b| reserved.contains(&b.as_str())));
                let script = bins.join(" && ");
                let commands = parse_script(&script);
                prop_assert!(
                    commands.len() >= 2,
                    "Chained commands should produce multiple parsed commands, got {}",
                    commands.len()
                );
            }
        }
    }
}
