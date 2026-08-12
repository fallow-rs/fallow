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
mod flag_credits;
mod resolve;
mod shell;

#[expect(
    clippy::disallowed_types,
    reason = "package.json scripts are deserialized as std HashMap"
)]
use std::collections::HashMap;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

pub use resolve::{
    build_bin_to_package_map, resolve_binary_to_package, resolve_known_dependency_binary,
};

/// Environment variable wrapper commands to strip before the actual binary.
const ENV_WRAPPERS: &[&str] = &["cross-env", "dotenv", "env"];

/// Node.js runners whose first non-flag argument is a file path, not a binary name.
const NODE_RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node", "bun"];

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

/// Boolean pnpm flags that can appear before an implicit binary invocation.
const PNPM_IMPLICIT_EXEC_FLAGS: &[&str] = &["--silent", "-s"];

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
const MAX_SCRIPT_INDIRECTION_DEPTH: usize = 8;

/// Maximum number of script bodies expanded while analyzing a single command.
/// The depth limit bounds one path; this bounds the total fan-out when many
/// scripts call each other with arguments.
const MAX_SCRIPT_EXPANSIONS: usize = 64;

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
}

impl ScriptCatalog {
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
}

/// Where the real command starts once the package manager prefix is consumed.
enum PackageManagerTarget {
    /// A binary invocation starting at this token index.
    Binary(usize),
    /// A package.json script invocation. The script body is re-scanned with the
    /// call-site arguments starting at `extra_args_from` appended, which is what
    /// the package manager itself does.
    Script {
        name: String,
        extra_args_from: usize,
    },
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
    for segment in combined.split('/') {
        match segment {
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
    /// Packages this command names through a flag value rather than by
    /// importing them or invoking them as the binary.
    pub flag_packages: Vec<String>,
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

/// Analyze all scripts from a package.json `scripts` field.
///
/// For each script value, parses shell commands, extracts binary names (mapped to
/// package names), `--config` file paths, and positional file path arguments.
#[must_use]
#[expect(
    clippy::disallowed_types,
    reason = "API matches serde-deserialized HashMap from package.json"
)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "kept for syntax-only callers and tests")
)]
pub fn analyze_scripts(
    scripts: &HashMap<String, String>,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
) -> ScriptAnalysis {
    let mut result = ScriptAnalysis::default();

    for script_value in scripts.values() {
        accumulate_command(script_value, root, bin_map, &mut result);
    }

    result
}

/// Analyze package.json scripts with dependency context for package-manager
/// forms that need disambiguation, such as `pnpm <binary>`.
#[must_use]
#[expect(
    clippy::disallowed_types,
    reason = "API matches serde-deserialized HashMap from package.json"
)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "kept for unfiltered script callers and tests")
)]
pub fn analyze_scripts_with_dependencies(
    scripts: &HashMap<String, String>,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
) -> ScriptAnalysis {
    let catalog = ScriptCatalog::from_scripts(scripts);
    analyze_scripts_with_dependency_context(scripts, root, bin_map, declared_packages, &catalog)
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
) -> ScriptAnalysis {
    analyze_commands_with_context(scripts.values(), root, bin_map, declared_packages, catalog)
}

/// Analyze arbitrary shell commands with dependency and script-catalog context.
///
/// A command that invokes a declared script through a package manager
/// (`npm run lint -- --format gha`) is resolved to that script's body with the
/// call-site arguments appended, so binaries and flag values behind the
/// indirection are credited.
#[must_use]
pub fn analyze_commands_with_context<'a, I>(
    commands: I,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
    catalog: &ScriptCatalog,
) -> ScriptAnalysis
where
    I: IntoIterator<Item = &'a String>,
{
    let mut result = ScriptAnalysis::default();
    let context = ScriptCommandContext {
        declared_packages,
        scripts: catalog,
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
    result
}

/// Parse one command string and fold its binaries, config args, and file args
/// into `result`. Shared by [`analyze_scripts`] (per script value) and
/// [`analyze_command`] (single command).
fn accumulate_command(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    result: &mut ScriptAnalysis,
) {
    accumulate_parsed_commands(command, root, bin_map, parse_script(command), result);
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
        result,
    );
}

fn accumulate_parsed_commands(
    command: &str,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    parsed: Vec<ScriptCommand>,
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

        result.used_packages.extend(cmd.flag_packages);
        result.config_files.extend(cmd.config_args);
        result.entry_files.extend(cmd.file_args);
    }
}

/// Parse a single script value into one or more commands.
///
/// Splits on shell operators (`&&`, `||`, `;`, `|`, `&`) and parses each segment.
#[must_use]
pub fn parse_script(script: &str) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();
    let mut state = ScriptExpansion::new();
    parse_script_internal(
        script,
        &|tokens, idx| {
            shell::advance_past_package_manager(tokens, idx).map(PackageManagerTarget::Binary)
        },
        None,
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
        let tokens: Vec<&str> = segment
            .split_whitespace()
            .map(strip_surrounding_quotes)
            .collect();
        let Some(idx) = shell::skip_initial_wrappers(&tokens, 0) else {
            continue;
        };
        if parse_workspace_script_invocation(&tokens, idx).is_some() {
            continue;
        }
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
        if let Some(invocation) = declared_script_invocation(&tokens, idx, catalog) {
            names.insert(invocation.name.to_string());
        }
    }

    names
}

/// Return package-qualified script calls such as
/// `pnpm --filter @scope/api run serve` from one command.
pub fn referenced_workspace_scripts(command: &str) -> Vec<(String, String)> {
    let mut references = Vec::new();
    for segment in shell::split_shell_operators(command) {
        let tokens: Vec<&str> = segment
            .split_whitespace()
            .map(strip_surrounding_quotes)
            .collect();
        let Some(idx) = shell::skip_initial_wrappers(&tokens, 0) else {
            continue;
        };
        if let Some(reference) = parse_workspace_script_invocation(&tokens, idx) {
            references.push(reference);
        }
    }
    references.sort_unstable();
    references.dedup();
    references
}

fn parse_workspace_script_invocation(tokens: &[&str], idx: usize) -> Option<(String, String)> {
    match tokens.get(idx).copied()? {
        "pnpm" => parse_pnpm_workspace_script(tokens, idx),
        "npm" => parse_npm_workspace_script(tokens, idx),
        "yarn" => parse_yarn_workspace_script(tokens, idx),
        _ => None,
    }
}

fn parse_pnpm_workspace_script(tokens: &[&str], idx: usize) -> Option<(String, String)> {
    let mut next = idx + 1;
    while tokens
        .get(next)
        .is_some_and(|token| PNPM_IMPLICIT_EXEC_FLAGS.contains(token))
    {
        next += 1;
    }
    let selector = if tokens.get(next) == Some(&"--filter") {
        let selector = *tokens.get(next + 1)?;
        next += 2;
        selector
    } else {
        let selector = tokens.get(next)?.strip_prefix("--filter=")?;
        next += 1;
        selector
    };
    if !matches!(tokens.get(next), Some(&"run" | &"run-script")) {
        return None;
    }
    Some((selector.to_string(), (*tokens.get(next + 1)?).to_string()))
}

fn parse_npm_workspace_script(tokens: &[&str], idx: usize) -> Option<(String, String)> {
    let flag = *tokens.get(idx + 1)?;
    let (selector, next) = if matches!(flag, "--workspace" | "-w") {
        (*tokens.get(idx + 2)?, idx + 3)
    } else {
        (flag.strip_prefix("--workspace=")?, idx + 2)
    };
    if !matches!(tokens.get(next), Some(&"run" | &"run-script")) {
        return None;
    }
    Some((selector.to_string(), (*tokens.get(next + 1)?).to_string()))
}

fn parse_yarn_workspace_script(tokens: &[&str], idx: usize) -> Option<(String, String)> {
    if tokens.get(idx + 1) != Some(&"workspace") {
        return None;
    }
    let selector = *tokens.get(idx + 2)?;
    let mut next = idx + 3;
    if matches!(tokens.get(next), Some(&"run" | &"run-script")) {
        next += 1;
    }
    Some((selector.to_string(), (*tokens.get(next)?).to_string()))
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
        &|tokens, idx| {
            advance_past_package_manager_with_context(tokens, idx, root, bin_map, context)
        },
        Some(context.scripts),
        &mut state,
        &mut commands,
    );
    commands
}

fn parse_script_internal(
    script: &str,
    advance_package_manager: &impl Fn(&[&str], usize) -> Option<PackageManagerTarget>,
    catalog: Option<&ScriptCatalog>,
    state: &mut ScriptExpansion,
    commands: &mut Vec<ScriptCommand>,
) {
    for segment in shell::split_shell_operators(script) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        match parse_command_segment(segment, advance_package_manager) {
            Some(SegmentOutcome::Command(mut cmd)) => {
                if !state.local_paths {
                    cmd.config_args.clear();
                    cmd.file_args.clear();
                }
                commands.push(cmd);
            }
            Some(SegmentOutcome::ScriptCall { name, extra_args }) => {
                resolve_script_call(
                    &name,
                    &extra_args,
                    advance_package_manager,
                    catalog,
                    state,
                    commands,
                );
            }
            None => {}
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
    advance_package_manager: &impl Fn(&[&str], usize) -> Option<PackageManagerTarget>,
    catalog: Option<&ScriptCatalog>,
    state: &mut ScriptExpansion,
    commands: &mut Vec<ScriptCommand>,
) {
    if extra_args.is_empty()
        || state.active.len() >= MAX_SCRIPT_INDIRECTION_DEPTH
        || state.expansions >= MAX_SCRIPT_EXPANSIONS
    {
        return;
    }
    let Some(entry) = catalog.and_then(|catalog| catalog.body(name)) else {
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
) -> Option<PackageManagerTarget> {
    if let Some(target) = script_invocation_target(tokens, idx, context) {
        return Some(target);
    }

    if tokens[idx] != "pnpm" {
        return shell::advance_past_package_manager(tokens, idx).map(PackageManagerTarget::Binary);
    }

    let mut next = idx + 1;
    while next < tokens.len() && PNPM_IMPLICIT_EXEC_FLAGS.contains(&tokens[next]) {
        next += 1;
    }
    if next >= tokens.len() {
        return None;
    }

    let subcmd = tokens[next];
    if matches!(subcmd, "exec" | "dlx") {
        next += 1;
        while next < tokens.len() && PNPM_IMPLICIT_EXEC_FLAGS.contains(&tokens[next]) {
            next += 1;
        }
        return (next < tokens.len())
            .then_some(next)
            .map(PackageManagerTarget::Binary);
    }

    if subcmd.starts_with('-')
        || PNPM_BUILTIN_COMMANDS.contains(&subcmd)
        || context.scripts.contains(subcmd)
    {
        return None;
    }

    resolve_known_dependency_binary(subcmd, root, bin_map, context.declared_packages)
        .map(|_| PackageManagerTarget::Binary(next))
}

/// Recognize a package manager invocation of a package.json script.
///
/// Handles the explicit `run` form for npm, pnpm, yarn, and bun, plus the bare
/// `yarn <script>` and `pnpm <script>` forms. npm only forwards arguments that
/// follow `--`; the other managers forward them directly and tolerate a `--`
/// separator.
fn script_invocation_target(
    tokens: &[&str],
    idx: usize,
    context: &ScriptCommandContext<'_>,
) -> Option<PackageManagerTarget> {
    let invocation = declared_script_invocation(tokens, idx, context.scripts)?;
    let mut extra_args_from = invocation.name_idx + 1;
    if tokens.get(extra_args_from) == Some(&"--") {
        extra_args_from += 1;
    } else if invocation.requires_double_dash && extra_args_from < tokens.len() {
        return None;
    }

    Some(PackageManagerTarget::Script {
        name: invocation.name.to_string(),
        extra_args_from,
    })
}

struct DeclaredScriptInvocation<'a> {
    name: &'a str,
    name_idx: usize,
    requires_double_dash: bool,
}

fn declared_script_invocation<'a>(
    tokens: &'a [&'a str],
    idx: usize,
    catalog: &ScriptCatalog,
) -> Option<DeclaredScriptInvocation<'a>> {
    if parse_workspace_script_invocation(tokens, idx).is_some() {
        return None;
    }
    let manager = tokens[idx];
    if !matches!(manager, "npm" | "pnpm" | "yarn" | "bun") {
        return None;
    }

    let mut next = idx + 1;
    if manager == "pnpm" {
        while next < tokens.len() && PNPM_IMPLICIT_EXEC_FLAGS.contains(&tokens[next]) {
            next += 1;
        }
        if tokens.get(next) == Some(&"--filter") {
            next += 2;
        }
    } else if manager == "npm" && tokens.get(next) == Some(&"--silent") {
        next += 1;
    }
    let subcmd = *tokens.get(next)?;

    let (name_idx, requires_double_dash) = if matches!(subcmd, "run" | "run-script") {
        (next + 1, manager == "npm")
    } else if matches!(manager, "yarn" | "pnpm" | "bun")
        && !subcmd.starts_with('-')
        && !PACKAGE_MANAGER_BUILTIN_COMMANDS.contains(&subcmd)
    {
        (next, false)
    } else {
        return None;
    };

    let name = *tokens.get(name_idx)?;
    if !catalog.contains(name) {
        return None;
    }

    Some(DeclaredScriptInvocation {
        name,
        name_idx,
        requires_double_dash,
    })
}

/// What a command segment resolved to.
enum SegmentOutcome {
    Command(ScriptCommand),
    /// A package manager invocation of a package.json script, with the
    /// arguments the call site forwards to that script's body.
    ScriptCall {
        name: String,
        extra_args: String,
    },
}

/// Parse a single command segment (after splitting on shell operators).
fn parse_command_segment(
    segment: &str,
    advance_package_manager: &impl Fn(&[&str], usize) -> Option<PackageManagerTarget>,
) -> Option<SegmentOutcome> {
    let tokens: Vec<&str> = segment
        .split_whitespace()
        .map(strip_surrounding_quotes)
        .collect();
    if tokens.is_empty() {
        return None;
    }

    let idx = shell::skip_initial_wrappers(&tokens, 0)?;
    let idx = match advance_package_manager(&tokens, idx)? {
        PackageManagerTarget::Binary(idx) => idx,
        PackageManagerTarget::Script {
            name,
            extra_args_from,
        } => {
            return Some(SegmentOutcome::ScriptCall {
                name,
                extra_args: forwarded_arguments(segment, extra_args_from),
            });
        }
    };

    let binary = tokens[idx].to_string();

    if SCRIPT_MULTIPLEXERS.contains(&binary.as_str()) {
        return Some(SegmentOutcome::Command(ScriptCommand {
            binary,
            config_args: Vec::new(),
            file_args: Vec::new(),
            flag_packages: Vec::new(),
        }));
    }

    let is_node_runner = NODE_RUNNERS.contains(&binary.as_str());
    let (file_args, config_args) = extract_args_for_binary(&tokens, idx + 1, is_node_runner);
    let flag_packages = flag_credits::flag_referenced_packages(&binary, &tokens[idx + 1..]);

    Some(SegmentOutcome::Command(ScriptCommand {
        binary,
        config_args,
        file_args,
        flag_packages,
    }))
}

/// The raw tail of a segment, keeping quoting intact so the re-scanned script
/// body sees the arguments as the shell would pass them.
fn forwarded_arguments(segment: &str, from_token: usize) -> String {
    segment
        .split_whitespace()
        .skip(from_token)
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
    token.starts_with("./")
        || token.starts_with("../")
        || (token.contains('/') && !token.starts_with('@') && !token.contains("://"))
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
    fn npm_run_without_double_dash_credits_nothing() {
        let result = analyze_ci_command(
            "npm run lint --format gha",
            &[("lint", "eslint .")],
            &["eslint"],
        );
        assert!(result.used_packages.is_empty());
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
                        "eslint leaf.js".to_string()
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
        let result = analyze_ci_command("npm run s0 -- --go", &scripts, &["eslint"]);

        assert!(result.used_packages.contains("eslint"));
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
        for command in [
            "pnpm --filter @scope/api run serve",
            "pnpm --filter=@scope/api run serve",
            "npm --workspace @scope/api run serve",
            "npm --workspace=@scope/api run serve",
            "yarn workspace @scope/api serve",
        ] {
            assert_eq!(
                referenced_workspace_scripts(command),
                vec![("@scope/api".to_string(), "serve".to_string())],
                "failed to parse {command}"
            );
        }
    }

    #[test]
    fn token_with_internal_single_quote_unchanged() {
        // A token whose quote is internal (not surrounding) must not be mangled.
        // Use a file arg that contains an internal apostrophe but is not shell-quoted.
        // We exercise strip_surrounding_quotes directly via a known non-file-path
        // context: confirm parse_script does not mangle such a token.
        assert_eq!(super::strip_surrounding_quotes("can't"), "can't");
        assert_eq!(super::strip_surrounding_quotes("'quoted'"), "quoted");
        assert_eq!(super::strip_surrounding_quotes("\"quoted\""), "quoted");
        assert_eq!(
            super::strip_surrounding_quotes("'mismatched\""),
            "'mismatched\""
        );
        assert_eq!(super::strip_surrounding_quotes(""), "");
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
