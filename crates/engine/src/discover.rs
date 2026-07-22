//! Discovery helpers and types exposed through the engine boundary.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{
    PackageJson, ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo, discover_workspaces,
    find_undeclared_workspaces_with_ignores,
};
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use rustc_hash::FxHashSet;

use crate::{EngineError, EngineResult, plugins::PluginRegistry};

const UNDECLARED_WORKSPACE_WARNING_PREVIEW: usize = 5;

pub const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx", "css", "scss", "sass", "less", "html", "graphql", "gql",
];

/// Glob patterns for test/dev/story files excluded in production mode.
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.e2e.*",
    "**/*.e2e-spec.*",
    "**/*.bench.*",
    "**/*.fixture.*",
    "**/*.stories.*",
    "**/*.story.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/__snapshots__/**",
    "**/__fixtures__/**",
    "**/test/**",
    "**/tests/**",
    "*.config.*",
    "**/.*.js",
    "**/.*.ts",
    "**/.*.mjs",
    "**/.*.cjs",
];

const ALLOWED_HIDDEN_DIRS: &[&str] = &[
    ".storybook",
    ".vitepress",
    ".well-known",
    ".changeset",
    ".github",
];

const SCRIPT_SCOPE_DENYLIST: &[&str] = &[
    ".git",
    ".next",
    ".nuxt",
    ".output",
    ".svelte-kit",
    ".turbo",
    ".nx",
    ".cache",
    ".parcel-cache",
    ".vercel",
    ".netlify",
    ".yarn",
    ".pnpm-store",
    ".docusaurus",
    ".vscode",
    ".idea",
    ".fallow",
    ".husky",
];

const ENV_WRAPPERS: &[&str] = &["cross-env", "dotenv", "env"];
const NODE_RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node", "bun"];
const SCRIPT_MULTIPLEXERS: &[&str] = &[
    "concurrently",
    "npm-run-all",
    "npm-run-all2",
    "run-s",
    "run-p",
    "run-s2",
    "run-p2",
];
const BUN_RUNTIME_FLAGS: &[&str] = &["--bun", "--watch", "--hot", "--smol", "--no-clear-screen"];

/// Discover workspace packages through the engine boundary.
///
/// Use this for callers that only need workspace metadata and do not yet own an
/// `AnalysisSession`. Session-backed flows should prefer
/// [`AnalysisSession::workspaces`](crate::session::AnalysisSession::workspaces)
/// so discovery is reused with the rest of the analysis context.
#[must_use]
pub fn discover_workspace_packages(root: &Path) -> Vec<WorkspaceInfo> {
    discover_workspaces(root)
}

/// Discover workspace packages and diagnostics through the engine boundary.
///
/// This is for CLI/API surfaces that need to render workspace diagnostics but
/// do not otherwise need a full [`AnalysisSession`](crate::session::AnalysisSession).
///
/// # Errors
///
/// Returns an engine error when workspace manifest loading fails.
pub fn discover_workspace_packages_with_diagnostics(
    root: &Path,
    ignore_patterns: &globset::GlobSet,
) -> EngineResult<(Vec<WorkspaceInfo>, Vec<WorkspaceDiagnostic>)> {
    fallow_config::discover_workspaces_with_diagnostics(root, ignore_patterns)
        .map_err(|err| EngineError::new(err.to_string()))
}

/// Entry points grouped by reachability role.
#[derive(Debug, Clone, Default)]
pub struct CategorizedEntryPoints {
    pub(crate) all: Vec<EntryPoint>,
    runtime: Vec<EntryPoint>,
    test: Vec<EntryPoint>,
}

impl CategorizedEntryPoints {
    pub(crate) fn push_runtime(&mut self, entry: EntryPoint) {
        self.runtime.push(entry.clone());
        self.all.push(entry);
    }

    pub(crate) fn push_test(&mut self, entry: EntryPoint) {
        self.test.push(entry.clone());
        self.all.push(entry);
    }

    pub(crate) fn push_support(&mut self, entry: EntryPoint) {
        self.all.push(entry);
    }

    #[must_use]
    pub(crate) fn dedup(mut self) -> Self {
        dedup_entry_paths(&mut self.all);
        dedup_entry_paths(&mut self.runtime);
        dedup_entry_paths(&mut self.test);
        self
    }
}

fn dedup_entry_paths(entries: &mut Vec<EntryPoint>) {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries.dedup_by(|a, b| a.path == b.path);
}

/// Package-scoped hidden directories that source discovery should traverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenDirScope {
    root: PathBuf,
    dirs: Vec<String>,
}

impl HiddenDirScope {
    #[must_use]
    const fn new(root: PathBuf, dirs: Vec<String>) -> Self {
        Self { root, dirs }
    }

    #[must_use]
    fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    fn dirs(&self) -> &[String] {
        &self.dirs
    }
}

/// Reusable engine discovery prelude for one resolved project.
#[derive(Debug, Clone)]
pub struct AnalysisDiscovery {
    files: Vec<DiscoveredFile>,
    workspaces: Vec<WorkspaceInfo>,
    root_pkg: Option<PackageJson>,
    config_candidates: Vec<PathBuf>,
    discover_ms: f64,
    workspaces_ms: f64,
}

impl AnalysisDiscovery {
    fn from_parts(
        files: Vec<DiscoveredFile>,
        workspaces: Vec<WorkspaceInfo>,
        root_pkg: Option<PackageJson>,
        config_candidates: Vec<PathBuf>,
        discover_ms: f64,
        workspaces_ms: f64,
    ) -> Self {
        Self {
            files,
            workspaces,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        }
    }

    /// Discovered source files, indexed by stable `FileId` for this session.
    #[must_use]
    pub(crate) fn files(&self) -> &[DiscoveredFile] {
        &self.files
    }

    /// Discovered workspace packages for this session.
    #[must_use]
    pub(crate) fn workspaces(&self) -> &[WorkspaceInfo] {
        &self.workspaces
    }

    pub(crate) fn root_pkg(&self) -> Option<&PackageJson> {
        self.root_pkg.as_ref()
    }

    pub(crate) fn config_candidates(&self) -> &[PathBuf] {
        &self.config_candidates
    }

    pub(crate) fn discover_ms(&self) -> f64 {
        self.discover_ms
    }

    pub(crate) fn workspaces_ms(&self) -> f64 {
        self.workspaces_ms
    }

    /// Consume this discovery prelude and return its source file registry.
    #[must_use]
    pub fn into_files(self) -> Vec<DiscoveredFile> {
        self.files
    }
}

/// Run engine-owned workspace and source discovery for a resolved project.
#[must_use]
pub(crate) fn prepare_analysis_discovery(config: &ResolvedConfig) -> AnalysisDiscovery {
    warn_missing_node_modules(config);

    let workspaces_start = Instant::now();
    let workspaces = discover_workspaces(&config.root);
    let workspaces_ms = workspaces_start.elapsed().as_secs_f64() * 1000.0;
    if !workspaces.is_empty() {
        tracing::info!(count = workspaces.len(), "workspaces discovered");
    }
    warn_undeclared_workspaces(
        &config.root,
        &workspaces,
        &config.ignore_patterns,
        config.quiet,
    );

    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let hidden_dir_scopes = collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces);

    let discover_start = Instant::now();
    let (files, config_candidates) =
        discover_files_and_config_candidates(config, &hidden_dir_scopes);
    let discover_ms = discover_start.elapsed().as_secs_f64() * 1000.0;

    AnalysisDiscovery::from_parts(
        files,
        workspaces,
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    )
}

/// Run source discovery with workspace metadata already resolved by config load.
///
/// This is the normal [`AnalysisSession`](crate::session::AnalysisSession) path:
/// config loading already expanded workspace globs and collected diagnostics, so
/// source discovery can reuse that set instead of walking workspace manifests a
/// second time.
#[must_use]
pub(crate) fn prepare_analysis_discovery_with_workspaces(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    workspaces_ms: f64,
) -> AnalysisDiscovery {
    warn_missing_node_modules(config);

    if !workspaces.is_empty() {
        tracing::info!(count = workspaces.len(), "workspaces discovered");
    }

    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let hidden_dir_scopes = collect_hidden_dir_scopes(config, root_pkg.as_ref(), workspaces);

    let discover_start = Instant::now();
    let (files, config_candidates) =
        discover_files_and_config_candidates(config, &hidden_dir_scopes);
    let discover_ms = discover_start.elapsed().as_secs_f64() * 1000.0;

    AnalysisDiscovery::from_parts(
        files,
        workspaces.to_vec(),
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    )
}

fn warn_missing_node_modules(config: &ResolvedConfig) {
    if config.root.join("node_modules").is_dir() {
        return;
    }

    tracing::warn!(
        "node_modules directory not found. Run `npm install` / `pnpm install` first for accurate results."
    );
}

fn format_undeclared_workspace_warning(
    root: &Path,
    undeclared: &[WorkspaceDiagnostic],
) -> Option<String> {
    if undeclared.is_empty() {
        return None;
    }

    let preview = undeclared
        .iter()
        .take(UNDECLARED_WORKSPACE_WARNING_PREVIEW)
        .map(|diagnostic| {
            diagnostic
                .path
                .strip_prefix(root)
                .unwrap_or(&diagnostic.path)
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    let remaining = undeclared
        .len()
        .saturating_sub(UNDECLARED_WORKSPACE_WARNING_PREVIEW);
    let tail = if remaining > 0 {
        format!(" (and {remaining} more)")
    } else {
        String::new()
    };
    let noun = if undeclared.len() == 1 {
        "directory with package.json is"
    } else {
        "directories with package.json are"
    };
    let guidance = if undeclared.len() == 1 {
        "Add that path to package.json workspaces or pnpm-workspace.yaml if it should be analyzed as a workspace."
    } else {
        "Add those paths to package.json workspaces or pnpm-workspace.yaml if they should be analyzed as workspaces."
    };

    Some(format!(
        "{} {} not declared as {}: {}{}. {}",
        undeclared.len(),
        noun,
        if undeclared.len() == 1 {
            "a workspace"
        } else {
            "workspaces"
        },
        preview.join(", "),
        tail,
        guidance
    ))
}

fn warn_undeclared_workspaces(
    root: &Path,
    workspaces: &[WorkspaceInfo],
    ignore_patterns: &globset::GlobSet,
    quiet: bool,
) {
    let undeclared = find_undeclared_workspaces_with_ignores(root, workspaces, ignore_patterns);
    if undeclared.is_empty() {
        return;
    }

    let existing = fallow_config::workspace_diagnostics_for(root);
    let already_flagged: FxHashSet<PathBuf> = existing
        .iter()
        .map(|diagnostic| {
            dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone())
        })
        .collect();
    let undeclared: Vec<_> = undeclared
        .into_iter()
        .filter(|diagnostic| {
            let canonical =
                dunce::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone());
            !already_flagged.contains(&canonical)
        })
        .collect();
    if undeclared.is_empty() {
        return;
    }

    fallow_config::append_workspace_diagnostics(root, undeclared.clone());

    if !quiet && let Some(message) = format_undeclared_workspace_warning(root, &undeclared) {
        tracing::warn!("{message}");
    }
}

/// Check if a hidden directory name is on the discovery allowlist.
#[must_use]
pub fn is_allowed_hidden_dir(name: &OsStr) -> bool {
    ALLOWED_HIDDEN_DIRS
        .iter()
        .any(|&dir| OsStr::new(dir) == name)
}

/// Collect plugin-derived hidden directory scopes.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let registry = PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
        }
    }

    scopes
}

fn push_plugin_hidden_dir_scope(
    scopes: &mut Vec<HiddenDirScope>,
    registry: &PluginRegistry,
    pkg: &PackageJson,
    root: &Path,
) {
    let dirs = registry.discovery_hidden_dirs(pkg, root);
    if !dirs.is_empty() {
        scopes.push(HiddenDirScope::new(root.to_path_buf(), dirs));
    }
}

/// Collect plugin and script-derived hidden directory scopes.
#[must_use]
pub(crate) fn collect_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let _span = tracing::info_span!("collect_hidden_dir_scopes").entered();
    let registry = PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
        push_script_hidden_dir_scope(&mut scopes, pkg, &config.root);
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
            push_script_hidden_dir_scope(&mut scopes, &pkg, &ws.root);
        }
    }

    scopes
}

fn push_script_hidden_dir_scope(scopes: &mut Vec<HiddenDirScope>, pkg: &PackageJson, root: &Path) {
    if let Some(scope) = build_script_scope(pkg, root) {
        scopes.push(scope);
    }
}

fn build_script_scope(pkg: &PackageJson, root: &Path) -> Option<HiddenDirScope> {
    let scripts = pkg.scripts.as_ref()?;
    let mut seen = FxHashSet::default();
    let mut dirs: Vec<String> = Vec::new();

    for (script_name, script_value) in scripts {
        for cmd in parse_script_value(script_value) {
            for path in cmd.config_args.iter().chain(cmd.file_args.iter()) {
                for hidden in extract_hidden_segments(path) {
                    if SCRIPT_SCOPE_DENYLIST.contains(&hidden.as_str()) {
                        continue;
                    }
                    if seen.insert(hidden.clone()) {
                        tracing::debug!(
                            dir = %hidden,
                            script = %script_name,
                            package_root = %root.display(),
                            "inferred hidden_dir_scope from package.json#scripts"
                        );
                        dirs.push(hidden);
                    }
                }
            }
        }
    }

    if dirs.is_empty() {
        None
    } else {
        Some(HiddenDirScope::new(root.to_path_buf(), dirs))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ScriptCommand {
    config_args: Vec<String>,
    file_args: Vec<String>,
}

fn parse_script_value(script: &str) -> Vec<ScriptCommand> {
    let mut commands = Vec::new();

    for segment in split_shell_operators(script) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if let Some(cmd) = parse_command_segment(segment) {
            commands.push(cmd);
        }
    }

    commands
}

fn parse_command_segment(segment: &str) -> Option<ScriptCommand> {
    let tokens: Vec<&str> = segment
        .split_whitespace()
        .map(strip_surrounding_quotes)
        .collect();
    if tokens.is_empty() {
        return None;
    }

    let idx = skip_initial_wrappers(&tokens, 0)?;
    let idx = advance_past_package_manager(&tokens, idx)?;
    let binary = tokens[idx];

    if SCRIPT_MULTIPLEXERS.contains(&binary) {
        return Some(ScriptCommand {
            config_args: Vec::new(),
            file_args: Vec::new(),
        });
    }

    let is_node_runner = NODE_RUNNERS.contains(&binary);
    let (file_args, config_args) = extract_args_for_binary(&tokens, idx + 1, is_node_runner);

    Some(ScriptCommand {
        config_args,
        file_args,
    })
}

fn split_shell_operators(script: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = script.as_bytes();
    let len = bytes.len();
    let mut index = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while index < len {
        let byte = bytes[index];

        if byte == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            index += 1;
            continue;
        }
        if byte == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            index += 1;
            continue;
        }

        if in_single_quote || in_double_quote {
            index += 1;
            continue;
        }

        if let Some(op_len) = shell_operator_len(bytes, index) {
            segments.push(&script[start..index]);
            index += op_len;
            start = index;
            continue;
        }

        index += 1;
    }

    if start < len {
        segments.push(&script[start..]);
    }

    segments
}

fn shell_operator_len(bytes: &[u8], index: usize) -> Option<usize> {
    let byte = bytes[index];
    let next = bytes.get(index + 1).copied();

    if matches!((byte, next), (b'&', Some(b'&')) | (b'|', Some(b'|'))) {
        return Some(2);
    }

    if byte == b';' {
        return Some(1);
    }
    if byte == b'|' && next != Some(b'|') {
        return Some(1);
    }
    if byte == b'&' && next != Some(b'&') {
        return Some(1);
    }

    None
}

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

fn skip_initial_wrappers(tokens: &[&str], mut index: usize) -> Option<usize> {
    while index < tokens.len() && is_env_assignment(tokens[index]) {
        index += 1;
    }
    if index >= tokens.len() {
        return None;
    }

    while index < tokens.len() && ENV_WRAPPERS.contains(&tokens[index]) {
        index += 1;
        while index < tokens.len() && is_env_assignment(tokens[index]) {
            index += 1;
        }
        if index < tokens.len() && tokens[index] == "--" {
            index += 1;
        }
    }
    if index >= tokens.len() {
        return None;
    }

    Some(index)
}

fn advance_past_package_manager(tokens: &[&str], mut index: usize) -> Option<usize> {
    let token = tokens[index];
    if matches!(token, "npx" | "pnpx" | "bunx") {
        index += 1;
        while index < tokens.len() && tokens[index].starts_with('-') {
            let flag = tokens[index];
            index += 1;
            if matches!(flag, "--package" | "-p") && index < tokens.len() {
                index += 1;
            }
        }
    } else if token == "bun" {
        index += 1;
        let mut saw_runtime_flag = false;
        while index < tokens.len() && BUN_RUNTIME_FLAGS.contains(&tokens[index]) {
            index += 1;
            saw_runtime_flag = true;
        }
        if index >= tokens.len() {
            return None;
        }
        let subcmd = tokens[index];
        if subcmd == "exec" || subcmd == "x" {
            index += 1;
        } else if matches!(subcmd, "run" | "run-script") || !saw_runtime_flag {
            return None;
        }
    } else if matches!(token, "yarn" | "pnpm" | "npm") {
        if index + 1 < tokens.len() {
            let subcmd = tokens[index + 1];
            if subcmd == "exec" || subcmd == "dlx" {
                index += 2;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    if index >= tokens.len() {
        return None;
    }

    Some(index)
}

fn extract_args_for_binary(
    tokens: &[&str],
    mut index: usize,
    is_node_runner: bool,
) -> (Vec<String>, Vec<String>) {
    let mut file_args = Vec::new();
    let mut config_args = Vec::new();

    while index < tokens.len() {
        let token = tokens[index];

        if is_node_runner
            && matches!(
                token,
                "-e" | "--eval" | "-p" | "--print" | "-r" | "--require"
            )
        {
            index += 2;
            continue;
        }

        if let Some(config) = extract_config_arg(token, tokens.get(index + 1).copied()) {
            config_args.push(config);
            if token.contains('=') || token.starts_with("--config=") || token.starts_with("-c=") {
                index += 1;
            } else {
                index += 2;
            }
            continue;
        }

        if token.starts_with('-') {
            index += 1;
            continue;
        }

        if looks_like_file_path(token) {
            file_args.push(token.to_string());
        }
        index += 1;
    }

    (file_args, config_args)
}

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

fn is_env_assignment(token: &str) -> bool {
    token.find('=').is_some_and(|eq_pos| {
        let name = &token[..eq_pos];
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

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

fn could_be_file_path(token: &str) -> bool {
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

fn extract_hidden_segments(path: &str) -> Vec<String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Vec::new();
    }

    let mut hidden = Vec::new();
    let components = path.components().collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::RootDir
        )
    }) {
        return Vec::new();
    }

    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(value) = component else {
            continue;
        };
        let value = value.to_string_lossy();
        if !value.starts_with('.') || value == "." || value == ".." {
            continue;
        }
        if index == components.len().saturating_sub(1) {
            continue;
        }
        hidden.push(value.to_string());
    }

    hidden
}

/// Discover source files and non-source config candidates in one traversal.
#[must_use]
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    let scopes = additional_hidden_dir_scopes
        .iter()
        .map(|scope| {
            crate::discover_walk::HiddenDirScope::new(
                scope.root().to_path_buf(),
                scope.dirs().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    crate::discover_walk::discover_files_and_config_candidates(config, &scopes)
}

/// Discover configured and inferred entry points.
#[must_use]
pub(crate) fn discover_entry_points(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    crate::entry_points::discover_entry_points(config, files)
}

/// Discover entry points for a workspace package.
#[must_use]
pub(crate) fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    crate::entry_points::discover_workspace_entry_points(ws_root, config, all_files)
}

/// Discover entry points from plugin results.
#[must_use]
pub(crate) fn discover_plugin_entry_points(
    plugin_result: &crate::plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    crate::entry_points::discover_plugin_entry_points(plugin_result, config, files)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_config::PackageJson;

    use super::{
        ALLOWED_HIDDEN_DIRS, CategorizedEntryPoints, EntryPoint, EntryPointSource, HiddenDirScope,
        collect_hidden_dir_scopes, collect_plugin_hidden_dir_scopes, extract_hidden_segments,
        is_allowed_hidden_dir,
    };

    #[test]
    fn hidden_dir_scope_exposes_root_and_dirs() {
        let scope = HiddenDirScope::new(PathBuf::from("/repo/packages/app"), vec![".next".into()]);

        assert_eq!(scope.root(), PathBuf::from("/repo/packages/app"));
        assert_eq!(scope.dirs(), [".next"]);
    }

    #[test]
    fn hidden_dir_allowlist_is_engine_owned() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(is_allowed_hidden_dir(std::ffi::OsStr::new(dir)));
        }
        assert!(!is_allowed_hidden_dir(std::ffi::OsStr::new(".git")));
    }

    #[test]
    fn plugin_hidden_dir_scopes_are_engine_owned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = fallow_config::FallowConfig::default().resolve(
            dir.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let pkg: PackageJson = serde_json::from_value(serde_json::json!({
            "devDependencies": {
                "@react-router/dev": "^7.0.0"
            }
        }))
        .expect("valid package fixture");

        let scopes = collect_plugin_hidden_dir_scopes(&config, Some(&pkg), &[]);

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].root(), dir.path());
        assert_eq!(scopes[0].dirs(), [".client", ".server"]);
    }

    #[test]
    fn script_hidden_dir_scopes_are_engine_owned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = fallow_config::FallowConfig::default().resolve(
            dir.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let pkg: PackageJson = serde_json::from_value(serde_json::json!({
            "scripts": {
                "lint": "eslint -c .config/eslint.config.js",
                "build": "tsx ./.scripts/build.ts",
                "cache": "tsx .nx/cache/build.ts"
            }
        }))
        .expect("valid package fixture");

        let scopes = collect_hidden_dir_scopes(&config, Some(&pkg), &[]);

        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].root(), dir.path());
        let mut dirs = scopes[0].dirs().to_vec();
        dirs.sort();
        assert_eq!(dirs, [".config", ".scripts"]);
    }

    #[test]
    fn hidden_segment_extraction_rejects_escape_paths() {
        assert_eq!(
            extract_hidden_segments(".foo/.bar/x.js"),
            vec![".foo".to_string(), ".bar".to_string()]
        );
        assert!(extract_hidden_segments("../../.config/eslint.config.js").is_empty());
        assert!(extract_hidden_segments(".env").is_empty());
    }

    #[test]
    fn categorized_entry_points_dedups_each_bucket() {
        let entry = EntryPoint {
            path: PathBuf::from("/repo/src/index.ts"),
            source: EntryPointSource::DefaultIndex,
        };
        let engine = CategorizedEntryPoints {
            all: vec![entry.clone(), entry.clone()],
            runtime: vec![entry.clone(), entry.clone()],
            test: Vec::new(),
        }
        .dedup();

        assert_eq!(engine.all.len(), 1);
        assert_eq!(engine.runtime.len(), 1);
        assert_eq!(engine.test.len(), 0);
        assert_eq!(engine.all[0].path, entry.path);
    }
}
