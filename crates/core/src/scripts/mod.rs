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

mod argument_entries;
pub use argument_entries::{ArgumentEntries, argument_entries, is_glob, is_script_runtime};
mod borp;
pub mod ci;
#[cfg(test)]
mod command_forms_tests;
mod command_wrappers;
mod config_tasks;
mod file_target_tools;
mod flag_credits;
pub mod hooks;
mod node_test;
pub mod nx_targets;
mod resolve;
mod shell;
mod shell_helpers;
pub mod vite_plus;
mod workspace_selection;

#[expect(
    clippy::disallowed_types,
    reason = "package.json scripts are deserialized as std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use command_wrappers::{command_wrapper_child_index, skip_command_wrappers};
pub use config_tasks::config_task_scripts;
use file_target_tools::file_target_tool;
pub use resolve::{
    DependencyBinaries, all_dependency_names, build_bin_to_package_map, credit_npm_aliases,
    project_bin_map, resolve_binary_to_packages, resolve_known_dependency_binary,
};
use shell::is_builtin_command;
pub use shell::{command_substitutions, split_shell_operators};
pub use workspace_selection::WorkspacePackages;
use workspace_selection::{PackageSelector, WorkspacePackage, relative_dir};

/// Environment variable wrapper commands to strip before the actual binary.
const ENV_WRAPPERS: &[&str] = &["cross-env", "dotenv", "env"];

/// Node.js runners whose first non-flag argument is a file path, not a binary name.
const NODE_RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node", "bun"];

/// Return `true` when a script bundles with every package left external:
/// `bun build --packages=external` or `esbuild --packages=external`, also with
/// the value as a separate argument.
///
/// Such a build does not inline the source of a workspace sibling, so the
/// sibling's own packages are not resolved from the bundling workspace.
#[must_use]
pub fn script_externalizes_packages(script: &str) -> bool {
    shell::split_shell_operators(script)
        .into_iter()
        .any(|segment| {
            let words = shell::split_words(segment);
            let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
            command_externalizes_packages(&tokens)
        })
}

/// Return the program files that a script runs with a node runner:
/// `build.mjs` in `node build.mjs`, `scripts/build.ts` in
/// `npx tsx scripts/build.ts`, or `build.ts` in `bun run build.ts`.
///
/// The program file is the first positional argument of the runner. A runner
/// in test mode (`node --test`, `bun test`) runs test files, not a program,
/// so it gives no file. A program file that looks like a test
/// (`tsx test/a.test.ts`, `node test/run.mjs`) gives no file either. Other
/// commands give no file: a test runner such as `vitest run test/a.test.ts`
/// names test files, not a program.
#[must_use]
pub fn script_program_files(script: &str) -> Vec<String> {
    shell::split_shell_operators(script)
        .into_iter()
        .filter_map(|segment| {
            let words = shell::split_words(segment);
            let tokens: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
            command_program_file(&tokens)
        })
        .collect()
}

/// Node runner flags that run code from the command line instead of a file.
const NODE_INLINE_CODE_FLAGS: &[&str] = &["-e", "--eval", "-p", "--print"];

/// Node runner flags that take the next token as their value.
const NODE_RUNNER_VALUE_FLAGS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "--env-file",
    "--env-file-if-exists",
    "--input-type",
    "--tsconfig",
];

/// Path segments that hold test files.
const TEST_DIRECTORY_SEGMENTS: &[&str] = &["test", "tests", "__tests__"];

/// Whether a program file looks like a test: `.test.` or `.spec.` in the file
/// name, or a `test`, `tests` or `__tests__` directory in the path. A
/// `node:test` file often runs without `--test`, as in `tsx test/a.test.ts`.
fn looks_like_test_file(path: &str) -> bool {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    let file_name = segments.next_back().unwrap_or_default();
    file_name.contains(".test.")
        || file_name.contains(".spec.")
        || segments.any(|segment| TEST_DIRECTORY_SEGMENTS.contains(&segment))
}

fn command_program_file(tokens: &[&str]) -> Option<String> {
    let idx = shell::skip_initial_wrappers(tokens, 0)?;
    // `bun build.ts` and `bun --watch build.ts` run a file with Bun itself.
    // `bun x tsx build.ts` runs a binary, as `npx tsx build.ts` does.
    let runs_binary = tokens[idx] != "bun" || matches!(tokens.get(idx + 1), Some(&("x" | "exec")));
    let runner_idx = if runs_binary {
        shell::advance_past_package_manager(tokens, idx)?
    } else {
        idx
    };
    let runner = tool_name(tokens[runner_idx]);
    if !NODE_RUNNERS.contains(&runner) {
        return None;
    }
    let mut i = runner_idx + 1;
    while let Some(&token) = tokens.get(i) {
        if NODE_INLINE_CODE_FLAGS.contains(&token) || token.starts_with("--test") {
            return None;
        }
        if NODE_RUNNER_VALUE_FLAGS.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') || (runner == "bun" && matches!(token, "run" | "run-script")) {
            i += 1;
            continue;
        }
        return (looks_like_file_path(token) && !looks_like_test_file(token))
            .then(|| token.to_string());
    }
    None
}

fn command_externalizes_packages(tokens: &[&str]) -> bool {
    let Some(idx) = shell::skip_initial_wrappers(tokens, 0) else {
        return false;
    };
    let args_start = if let Some(build_idx) = bun_build_subcommand(tokens, idx) {
        build_idx + 1
    } else {
        let Some(binary_idx) = shell::advance_past_package_manager(tokens, idx) else {
            return false;
        };
        if tool_name(tokens[binary_idx]) != "esbuild" {
            return false;
        }
        binary_idx + 1
    };
    let args = tokens.get(args_start..).unwrap_or_default();
    args.iter().enumerate().any(|(i, arg)| {
        *arg == "--packages=external"
            || (*arg == "--packages" && args.get(i + 1) == Some(&"external"))
    })
}

/// The index of `build` in `bun [runtime flags] build`, or `None` when the
/// command at `idx` is not a `bun build` call.
fn bun_build_subcommand(tokens: &[&str], idx: usize) -> Option<usize> {
    if tokens.get(idx) != Some(&"bun") {
        return None;
    }
    let mut next = idx + 1;
    while tokens
        .get(next)
        .is_some_and(|token| shell::BUN_RUNTIME_FLAGS.contains(token))
    {
        next += 1;
    }
    (tokens.get(next) == Some(&"build")).then_some(next)
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
/// `concurrently` also takes full commands (`concurrently "tsx watch src/api.ts"`),
/// see [`concurrently_inline_commands`].
const SCRIPT_MULTIPLEXERS: &[&str] = &[
    "concurrently",
    "npm-run-all",
    "npm-run-all2",
    "run-s",
    "run-p",
    "run-s2",
    "run-p2",
];

/// The multiplexer that runs a positional argument as a shell command,
/// unless the argument is a package-manager shortcut such as `npm:dev`.
const CONCURRENTLY: &str = "concurrently";

/// `concurrently` argument prefixes that name a package.json script.
const CONCURRENTLY_SCRIPT_SHORTCUTS: &[&str] =
    &["npm:", "pnpm:", "yarn:", "bun:", "node:", "deno:"];

/// `concurrently` options whose value is the next token.
const CONCURRENTLY_VALUE_FLAGS: &[&str] = &[
    "-m",
    "--max-processes",
    "-n",
    "--names",
    "--name-separator",
    "-s",
    "--success",
    "--hide",
    "-p",
    "--prefix",
    "-c",
    "--prefix-colors",
    "-l",
    "--prefix-length",
    "-t",
    "--timestamp-format",
    "--kill-signal",
    "--ks",
    "--kill-timeout",
    "--restart-tries",
    "--restart-after",
    "--default-input-target",
];

/// The `concurrently` option whose value is a command that runs after the others.
const CONCURRENTLY_TEARDOWN_FLAG: &str = "--teardown";

/// The `concurrently` options that make the arguments after `--` placeholder
/// values instead of commands.
const CONCURRENTLY_PASSTHROUGH_FLAGS: &[&str] = &["-P", "--passthrough-arguments"];

/// Return the shell commands that `concurrently` runs, from the arguments
/// after the binary. A package-manager shortcut such as `npm:dev` is a script
/// name and not a command, and an option value is not a command, except the
/// value of `--teardown`.
fn concurrently_inline_commands<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let passthrough = args
        .iter()
        .any(|arg| CONCURRENTLY_PASSTHROUGH_FLAGS.contains(arg));
    let mut commands = Vec::new();
    let mut options_done = false;
    let mut iter = args.iter().copied();
    while let Some(arg) = iter.next() {
        if !options_done && arg == "--" {
            if passthrough {
                break;
            }
            options_done = true;
            continue;
        }
        if !options_done && arg.starts_with('-') {
            if arg == CONCURRENTLY_TEARDOWN_FLAG {
                commands.extend(iter.next());
            } else if let Some(value) = arg.strip_prefix("--teardown=") {
                commands.push(value);
            } else if CONCURRENTLY_VALUE_FLAGS.contains(&arg) {
                iter.next();
            }
            continue;
        }
        let is_script_shortcut = CONCURRENTLY_SCRIPT_SHORTCUTS
            .iter()
            .any(|prefix| arg.starts_with(prefix));
        if !is_script_shortcut && !arg.trim().is_empty() {
            commands.push(arg);
        }
    }
    commands
}

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

/// Maximum nesting of `$(...)` and backtick command substitutions that is
/// parsed. Guards the recursion against pathological input.
pub const MAX_SUBSTITUTION_DEPTH: usize = 8;

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
    /// The directory in which the commands run, for the shell helpers they name.
    helper_base: shell_helpers::HelperBase,
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
        helper_base: shell_helpers::HelperBase::Package,
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
    /// Command substitutions on the current path, bounded by
    /// [`MAX_SUBSTITUTION_DEPTH`].
    substitution_depth: usize,
}

impl ScriptExpansion {
    fn new() -> Self {
        Self {
            active: Vec::new(),
            expansions: 0,
            local_paths: true,
            substitution_depth: 0,
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
    /// The same config file paths, each with the binary that received it.
    /// A plugin reads a config file only when its own binary received it.
    pub binary_config_files: Vec<BinaryConfigFile>,
    /// File paths extracted as positional arguments (entry point candidates).
    pub entry_files: Vec<String>,
    /// The subset of `entry_files` that a built-in test runner runs as tests
    /// (`node --test`, `bun test`, `deno test`). These are test entry points.
    pub test_entry_files: Vec<String>,
}

impl ScriptAnalysis {
    /// Drop repeated config and entry paths, keeping first-seen order.
    ///
    /// A body reached both as its own script and through package-manager
    /// indirection is scanned more than once, and every consumer turns these
    /// lists into patterns one by one.
    fn dedupe_paths(&mut self) {
        retain_first_seen(&mut self.config_files);
        retain_first_seen(&mut self.binary_config_files);
        retain_first_seen(&mut self.entry_files);
        retain_first_seen(&mut self.test_entry_files);
    }
}

/// A config file argument and the binary that received it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BinaryConfigFile {
    /// The binary name (e.g., "vitest").
    pub binary: String,
    /// The config path as it appeared in the script, relative to the package.
    pub path: String,
}

fn retain_first_seen<T: Clone + Eq + std::hash::Hash>(values: &mut Vec<T>) {
    let mut seen: FxHashSet<T> = FxHashSet::default();
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
    /// The command that receives `file_args` and `config_args`. It differs
    /// from `binary` for a command wrapper (`varlock run -- npx eslint`
    /// records `eslint`) and for a Vite+ subcommand (`vp test` records
    /// `vitest`). `ignoreCommandEntries` matches this name.
    pub file_args_command: String,
    /// Packages this command names through a flag value rather than by
    /// importing them or invoking them as the binary.
    pub flag_packages: Vec<String>,
    /// A package whose CLI subcommand runs the production application.
    pub production_runtime_host: Option<&'static str>,
    /// The subset of `file_args` that `file_args_command` runs as tests with
    /// the built-in test runner of a runtime (`node --test`, `bun test`,
    /// `deno test`). These files are test entry points.
    pub test_file_args: Vec<String>,
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

    /// Return the test file arguments that become test entry points: none
    /// when `ignored` lists the command that receives them.
    #[must_use]
    pub fn test_entry_files(&self, ignored: IgnoredCommandEntries<'_>) -> &[String] {
        if ignored.contains(&self.file_args_command) {
            &[]
        } else {
            &self.test_file_args
        }
    }

    /// Apply `map` to every path argument of the command.
    fn map_paths(&mut self, map: impl Fn(Vec<String>) -> Vec<String>) {
        self.config_args = map(std::mem::take(&mut self.config_args));
        self.file_args = map(std::mem::take(&mut self.file_args));
        self.test_file_args = map(std::mem::take(&mut self.test_file_args));
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

/// Runtime hosts reached from production startup scripts, including local
/// script indirection and environment/package-manager wrappers.
pub fn production_runtime_hosts(pkg: &fallow_config::PackageJson) -> FxHashSet<String> {
    let Some(scripts) = &pkg.scripts else {
        return FxHashSet::default();
    };
    let catalog = ScriptCatalog::from_scripts(scripts);
    let declared = pkg.all_dependency_names();
    let mut queue: Vec<String> = scripts
        .keys()
        .filter(|name| {
            let name = name.split(':').next().unwrap_or(name);
            let base = name
                .strip_prefix("pre")
                .or_else(|| name.strip_prefix("post"))
                .unwrap_or(name);
            matches!(base, "start" | "serve")
        })
        .cloned()
        .collect();
    let mut visited = FxHashSet::default();
    let mut hosts = FxHashSet::default();
    while let Some(name) = queue.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(script) = scripts.get(&name) else {
            continue;
        };
        queue.extend(referenced_package_scripts(script, &catalog));
        hosts.extend(
            parse_script_with_catalog(script, &catalog)
                .into_iter()
                .filter_map(|command| command.production_runtime_host)
                .filter(|package| declared.iter().any(|name| name == package))
                .map(str::to_string),
        );
    }
    hosts
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
/// lists are not entry files. A command that runs a project-local shell helper
/// (`bash scripts/build.sh`) also analyzes the commands of that helper.
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
        shell_helpers::accumulate_command_and_helpers(
            command,
            root,
            bin_map,
            &context,
            &mut result,
        );
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

fn accumulate_parsed_commands(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    parsed: Vec<ScriptCommand>,
    ignored: IgnoredCommandEntries<'_>,
    result: &mut ScriptAnalysis,
) {
    for wrapper in ENV_WRAPPERS {
        if command.split_whitespace().any(|token| token == *wrapper) && !is_builtin_command(wrapper)
        {
            result
                .used_packages
                .extend(resolve_binary_to_packages(wrapper, root, bin_map));
        }
    }

    for cmd in parsed {
        if !cmd.binary.is_empty() && !is_builtin_command(&cmd.binary) {
            if NODE_RUNNERS.contains(&cmd.binary.as_str()) {
                if cmd.binary != "node" && cmd.binary != "bun" {
                    result.used_packages.extend(resolve_binary_to_packages(
                        &cmd.binary,
                        root,
                        bin_map,
                    ));
                }
            } else {
                result
                    .used_packages
                    .extend(resolve_binary_to_packages(&cmd.binary, root, bin_map));
            }
        }

        result
            .test_entry_files
            .extend_from_slice(cmd.test_entry_files(ignored));
        result
            .entry_files
            .extend_from_slice(cmd.entry_files(ignored));
        result.used_packages.extend(cmd.flag_packages);
        result
            .binary_config_files
            .extend(cmd.config_args.iter().map(|path| BinaryConfigFile {
                binary: cmd.file_args_command.clone(),
                path: path.clone(),
            }));
        result.config_files.extend(cmd.config_args);
    }
}

/// Whether `cmd` is a shell builtin or a core shell utility rather than a
/// command that a package provides.
#[must_use]
pub fn is_shell_builtin(cmd: &str) -> bool {
    is_builtin_command(cmd)
}

/// Package-manager commands that download and run a package that the project
/// does not need to install (`npx foo`, `pnpm dlx foo`).
fn downloads_its_command(segment: &str) -> bool {
    let mut tokens = segment.split_whitespace().skip_while(|token| {
        ENV_WRAPPERS.contains(token)
            || *token == "--"
            || (token.contains('=') && !token.starts_with('-'))
    });
    let Some(manager) = tokens.next() else {
        return false;
    };
    // Flags such as `--filter web` can come before the subcommand, so the
    // subcommand is the first token that is not a flag or a flag value.
    let mut subcommands = tokens.filter(|token| !token.starts_with('-'));
    match manager {
        "npx" | "pnpx" | "bunx" => true,
        "pnpm" | "yarn" => subcommands.any(|token| token == "dlx"),
        // `npm x` and `bun x` are the aliases of `npm exec` and `bunx`.
        "npm" | "bun" => subcommands.next().is_some_and(|token| token == "x"),
        _ => false,
    }
}

/// The binary names that the commands of one script body run, after
/// environment, package-manager and wrapper prefixes. A command that a
/// package manager downloads on demand (`npx`, `pnpm dlx`) is not included.
#[must_use]
pub fn script_binaries(script: &str) -> Vec<String> {
    let mut binaries = Vec::new();
    for segment in shell::split_shell_operators(script) {
        if downloads_its_command(segment) {
            continue;
        }
        for command in parse_script(segment) {
            if !command.binary.is_empty() && !binaries.contains(&command.binary) {
                binaries.push(command.binary);
            }
        }
    }
    binaries
}

/// The binary names that the `bin` field of `pkg` declares.
#[must_use]
pub fn package_bin_names(pkg: &fallow_config::PackageJson) -> Vec<String> {
    let (Some(name), Some(bin)) = (pkg.name.as_deref(), pkg.bin.as_ref()) else {
        return Vec::new();
    };
    resolve::bin_names(name, bin)
        .into_iter()
        .map(str::to_string)
        .collect()
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
            bun_file_runner_target(tokens, idx, catalog)
                .or_else(|| script_invocation_target(tokens, idx, catalog))
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
        let Some(idx) = skip_command_wrappers(&tokens) else {
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
            if binary == CONCURRENTLY {
                for inline in concurrently_inline_commands(&tokens[idx + 1..]) {
                    names.extend(referenced_package_scripts(inline, catalog));
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
        if tokens[idx] == CONCURRENTLY {
            for inline in concurrently_inline_commands(&tokens[idx + 1..]) {
                references.extend(referenced_workspace_scripts(inline, catalog));
            }
            continue;
        }
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
                        cmd.test_file_args.clear();
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
                            cmd.map_paths(|paths| rebase_all(std::slice::from_ref(&dir), &paths));
                        }
                    }
                }
                SegmentOutcome::InlineCommand { command, location } => {
                    let start = commands.len();
                    parse_script_internal(
                        &command,
                        advance_package_manager,
                        catalog,
                        state,
                        commands,
                    );
                    for cmd in &mut commands[start..] {
                        cmd.map_paths(|paths| location.resolve_all(paths, catalog));
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
    if state.substitution_depth >= MAX_SUBSTITUTION_DEPTH {
        return;
    }
    for body in shell::command_substitutions(script) {
        state.substitution_depth += 1;
        parse_script_internal(body, advance_package_manager, catalog, state, commands);
        state.substitution_depth -= 1;
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

        if is_node_runner && node_test::takes_non_module_value(token) {
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
    // `bun test` runs the built-in test runner also when a script named
    // `test` exists, so the Bun forms come before the script calls.
    if let Some(target) = bun_file_runner_target(tokens, idx, catalog) {
        return Some(target);
    }

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

/// Recognize `bun <file>`, `bun run <file>`, and `bun --watch <file>`, where
/// Bun runs a script file. Bun runs a declared script with that name first, so
/// a declared name is not a file. The target is `bun` itself, which then takes
/// the file as a node-runner argument. `bun test` always runs the built-in
/// test runner, also when a script named `test` exists, so its target is
/// `bun` too.
fn bun_file_runner_target(
    tokens: &[&str],
    idx: usize,
    catalog: &ScriptCatalog,
) -> Option<PackageManagerTarget> {
    if tokens.get(idx) != Some(&"bun") {
        return None;
    }
    let skip_runtime_flags = |mut i: usize| {
        while tokens
            .get(i)
            .is_some_and(|token| shell::BUN_RUNTIME_FLAGS.contains(token))
        {
            i += 1;
        }
        i
    };
    let mut next = skip_runtime_flags(idx + 1);
    if tokens.get(next) == Some(&"test") {
        return Some(PackageManagerTarget::Binary(idx, RunLocation::Here));
    }
    if matches!(tokens.get(next), Some(&("run" | "run-script"))) {
        next = skip_runtime_flags(next + 1);
    }
    let target = *tokens.get(next)?;
    (looks_like_script_file(target) && !catalog.declares_script_at(target, &RunLocation::Here))
        .then_some(PackageManagerTarget::Binary(idx, RunLocation::Here))
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
    let idx = skip_command_wrappers(tokens)?;
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
        "bun" | "vp" | vite_plus::VPR => idx + 1,
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

    let (name_idx, explicit) = if manager == vite_plus::VPR {
        (vite_plus::skip_run_flags(tokens, next, &mut location), true)
    } else if matches!(subcmd, "run" | "run-script") {
        let name_idx = match manager {
            "npm" => {
                npm_run_location(tokens, next + 1, &mut location);
                npm_include_workspace_root(&tokens[idx + 1..], &mut location);
                skip_npm_config_flags(tokens, next + 1)
            }
            "pnpm" => skip_pnpm_flags(tokens, next + 1, &mut location),
            "yarn" => skip_yarn_silent_flags(tokens, next + 1),
            "vp" => vite_plus::skip_run_flags(tokens, next + 1, &mut location),
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
    /// A full command that a multiplexer receives as one argument
    /// (`concurrently "tsx watch src/api.ts"`), parsed like a script body.
    InlineCommand {
        command: String,
        /// Where the multiplexer runs the command.
        location: RunLocation,
    },
}

fn command_runtime_host(
    binary: &str,
    args: &[&str],
    location: &RunLocation,
) -> Option<&'static str> {
    (location == &RunLocation::Here && binary == "next" && args.first() == Some(&"start"))
        .then_some("next")
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
    for _ in 0..command_wrappers::MAX_COMMAND_WRAPPER_DEPTH {
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
        let wrapped =
            wrapped_command_args(&tokens, command_start, advance_package_manager, catalog);
        outcomes.push(SegmentOutcome::Command(ScriptCommand {
            binary: tokens[idx].to_string(),
            config_args: location.resolve_all(wrapped.config_args, catalog),
            file_args: location.resolve_all(wrapped.file_args, catalog),
            file_args_command: wrapped.command.to_string(),
            flag_packages: Vec::new(),
            production_runtime_host: None,
            test_file_args: location.resolve_all(wrapped.test_file_args, catalog),
        }));
        let Some(next) = shell::skip_initial_wrappers(&tokens, command_start) else {
            return outcomes;
        };
        idx = next;
    }

    let binary = tokens[idx].to_string();

    if SCRIPT_MULTIPLEXERS.contains(&binary.as_str()) {
        let inline_commands = if binary == CONCURRENTLY {
            concurrently_inline_commands(&tokens[idx + 1..])
        } else {
            Vec::new()
        };
        outcomes.push(SegmentOutcome::Command(ScriptCommand {
            file_args_command: binary.clone(),
            binary,
            config_args: Vec::new(),
            file_args: Vec::new(),
            flag_packages: Vec::new(),
            production_runtime_host: None,
            test_file_args: Vec::new(),
        }));
        outcomes.extend(
            inline_commands
                .into_iter()
                .map(|command| SegmentOutcome::InlineCommand {
                    command: command.to_string(),
                    location: location.clone(),
                }),
        );
        return outcomes;
    }

    let (args_command, (file_args, config_args)) = match vite_plus::subcommand_tool(&tokens, idx) {
        Some((Some(tool), sub_idx)) => (tool.to_string(), binary_path_args(tool, &tokens, sub_idx)),
        Some((None, _)) => (binary.clone(), (Vec::new(), Vec::new())),
        None => (binary.clone(), binary_path_args(&binary, &tokens, idx)),
    };
    let flag_packages = flag_credits::flag_referenced_packages(&binary, &tokens[idx + 1..]);

    let production_runtime_host = command_runtime_host(&binary, &tokens[idx + 1..], &location);
    let test_file_args = node_test::test_file_args(&binary, &tokens[idx + 1..], &file_args);
    outcomes.push(SegmentOutcome::Command(ScriptCommand {
        test_file_args: location.resolve_all(test_file_args, catalog),
        file_args_command: args_command,
        binary,
        config_args: location.resolve_all(config_args, catalog),
        file_args: location.resolve_all(file_args, catalog),
        flag_packages,
        production_runtime_host,
    }));
    outcomes
}

/// Return the file and config arguments of `binary` at `idx` in `tokens`.
fn binary_path_args(binary: &str, tokens: &[&str], idx: usize) -> (Vec<String>, Vec<String>) {
    let args = &tokens[idx + 1..];
    let (mut file_args, mut config_args) =
        extract_args_for_binary(tokens, idx + 1, NODE_RUNNERS.contains(&binary));
    file_args = argument_entries::entry_file_args(binary, args, file_args);
    if is_task_runner(binary) {
        file_args.clear();
        config_args.clear();
    }
    let file_args = node_test::with_default_test_patterns(binary, args, file_args);
    (file_args, config_args)
}

/// The file and config arguments that a command wrapper records for the
/// command it runs.
struct WrappedCommandArgs<'a> {
    file_args: Vec<String>,
    config_args: Vec<String>,
    /// The name of the command that the wrapper runs.
    command: &'a str,
    /// The subset of `file_args` that the command runs as tests.
    test_file_args: Vec<String>,
}

impl<'a> WrappedCommandArgs<'a> {
    const fn without_args(command: &'a str) -> Self {
        Self {
            file_args: Vec::new(),
            config_args: Vec::new(),
            command,
            test_file_args: Vec::new(),
        }
    }
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
) -> WrappedCommandArgs<'a> {
    let (child_idx, runs_here) =
        match wrapped_command_target(tokens, command_start, advance_package_manager, catalog) {
            Some(PackageManagerTarget::Binary(child_idx, location)) => {
                (child_idx, location == RunLocation::Here)
            }
            Some(PackageManagerTarget::Script { .. }) => {
                return WrappedCommandArgs::without_args(tokens[command_start]);
            }
            None => invoked_command(tokens, command_start, &ScriptCatalog::default())
                .map_or((command_start, true), |invoked| {
                    (invoked.index, invoked.base_dirs.is_none())
                }),
        };
    let child = tokens[child_idx];
    if !runs_here {
        return WrappedCommandArgs::without_args(child);
    }
    let (mut file_args, mut config_args) = extract_args_for_binary(tokens, command_start, false);
    file_args = argument_entries::entry_file_args(child, &tokens[child_idx + 1..], file_args);
    if is_task_runner(child) {
        file_args.clear();
        config_args.clear();
    }
    let file_args =
        node_test::with_default_test_patterns(child, &tokens[child_idx + 1..], file_args);
    WrappedCommandArgs {
        test_file_args: node_test::test_file_args(child, &tokens[child_idx + 1..], &file_args),
        file_args,
        config_args,
        command: child,
    }
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
        // A path separator inside one bracket group is a JS array literal
        // (`['a/b']` after quote stripping), not a route segment like `[id]`.
        if close_offset.is_some_and(|offset| after_open[..offset].contains('/')) {
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
        ".toml", ".sh", ".bash",
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

/// Check if a token looks like a standalone script file reference (must have a
/// script-like source extension and a path-like structure, not a bare command
/// name).
#[must_use]
pub fn looks_like_script_file(token: &str) -> bool {
    if !could_be_file_path(token) {
        return false;
    }
    let extensions = [
        ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".gts", ".gjs",
    ];
    if !extensions.iter().any(|ext| token.ends_with(ext)) {
        return false;
    }
    token.contains('/') || token.starts_with("./") || token.starts_with("../")
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "test assertions use std HashMap for readability"
)]
#[path = "scripts_tests.rs"]
mod tests;
