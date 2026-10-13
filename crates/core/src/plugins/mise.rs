//! mise plugin.
//!
//! mise reads tasks from `mise.toml` and related config files. A task `run`
//! command can start a package binary or a local script, so the plugin credits
//! those commands. mise installs its own tools on `PATH`, so a bare command
//! such as `eslint .` can be a global tool. The plugin credits a bare command
//! only when the config puts `node_modules/.bin` of the task directory on
//! `PATH`. Explicit forms always count: a package manager runner
//! (`npx eslint`, `pnpm exec vitest`), a `node_modules/.bin/` path, a relative
//! script path, and `node <file>`.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::config_parser::lexical_normalize;
use super::{Plugin, PluginResult};
use crate::scripts;

const CONFIG_PATTERNS: &[&str] = &[
    "{mise,.mise}{,.*}.toml",
    "{mise,.mise}/config{,.*}.toml",
    ".config/mise{,.*}.toml",
    ".config/mise/config{,.*}.toml",
    ".config/mise/mise{,.local}.toml",
    "{mise,.mise}/conf.d/[!.]*.toml",
    ".config/mise/conf.d/[!.]*.toml",
];

/// mise loads every config file that it finds, so each one stays used.
const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

/// A package script that calls `mise` also activates the plugin.
const SCRIPT_ENABLERS: &[&str] = &["mise"];

/// The fixed config locations that activate the plugin in a project
/// directory. The glob forms in `CONFIG_PATTERNS` only add files to a project
/// that one of these files, or a `mise` script, already activates.
const ACTIVATION_FILES: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];

/// Commands that run a package binary from the project.
const PACKAGE_MANAGERS: &[&str] = &["npm", "npx", "pnpm", "pnpx", "yarn", "bun", "bunx"];

/// Commands that run another command from their arguments.
const WRAPPERS: &[&str] = &[
    "c8",
    "concurrently",
    "cross-env",
    "dotenv",
    "dotenvx",
    "env",
    "env-cmd",
    "mise",
    "nice",
    "nohup",
    "nyc",
    "op",
    "retry-cli",
    "time",
];

/// Wrappers that run the command after a `--` separator.
const END_OF_OPTIONS_WRAPPERS: &[&str] = &["dotenvx", "env-cmd", "mise", "op"];

/// The limit for nested wrappers and substitutions in one command.
const MAX_DEPTH: usize = 8;

const LOCAL_BIN_DIR: &str = "node_modules/.bin";

pub struct MisePlugin;

impl Plugin for MisePlugin {
    fn name(&self) -> &'static str {
        "mise"
    }

    fn script_enablers(&self) -> &'static [&'static str] {
        SCRIPT_ENABLERS
    }

    fn is_enabled_with_deps(&self, _deps: &[String], root: &Path) -> bool {
        ACTIVATION_FILES
            .iter()
            .any(|file| root.join(file).is_file())
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn resolve_config(&self, config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        let Ok(config) = source.parse::<toml::Table>() else {
            return PluginResult::default();
        };
        let credits = collect_task_credits(&config, config_path);
        let mut packages: Vec<String> = credits.packages.into_iter().collect();
        packages.sort_unstable();
        PluginResult {
            referenced_dependencies: packages,
            setup_files: credits.files,
            ..PluginResult::default()
        }
    }
}

/// The packages and files that the tasks of one config file use.
#[derive(Debug, Default)]
struct TaskCredits {
    packages: FxHashSet<String>,
    files: Vec<PathBuf>,
}

fn collect_task_credits(config: &toml::Table, config_path: &Path) -> TaskCredits {
    let mut credits = TaskCredits::default();
    let Some(config_dir) = config_path.parent() else {
        return credits;
    };
    let Some(tasks) = config.get("tasks").and_then(toml::Value::as_table) else {
        return credits;
    };
    let file_name = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let config_root = config_root(config_dir, file_name);
    let default_dir = config
        .get("task_config")
        .and_then(|value| value.get("dir"))
        .and_then(toml::Value::as_str)
        .unwrap_or(".");
    let global_env = env_tables(config.get("env"));

    for task in tasks.values() {
        let (runs, task_dir, task_env) = match task {
            toml::Value::String(_) | toml::Value::Array(_) => (vec![task], None, Vec::new()),
            toml::Value::Table(table) => (
                ["run", "run_windows"]
                    .iter()
                    .filter_map(|key| table.get(*key))
                    .collect(),
                table.get("dir").and_then(toml::Value::as_str),
                env_tables(table.get("env")),
            ),
            _ => continue,
        };
        let Some(dir) = resolve_task_path(task_dir.unwrap_or(default_dir), &config_root) else {
            continue;
        };
        let local = has_local_bin_path(global_env.iter().chain(&task_env), &dir, &config_root);
        let mut collector = CommandCollector {
            dir: &dir,
            credits: &mut credits,
        };
        for script in runs.into_iter().flat_map(run_scripts) {
            if !has_template(script) {
                collector.collect_script(script, local, 0);
            }
        }
    }

    credits
}

/// Return the directory that mise uses for `{{ config_root }}` and for
/// relative task paths.
///
/// A file in a `mise/` or `.mise/` directory, and a fragment in `conf.d/`,
/// belongs to the parent of that directory. A file in `.config/` belongs to
/// the parent of `.config/`.
fn config_root(config_dir: &Path, file_name: &str) -> PathBuf {
    let dir_name = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_default()
    };
    let parent = |path: &Path| {
        path.parent()
            .map_or_else(|| path.to_path_buf(), Path::to_path_buf)
    };

    let is_fragment = dir_name(config_dir) == "conf.d";
    let grouped_dir = if is_fragment {
        parent(config_dir)
    } else {
        config_dir.to_path_buf()
    };
    let grouped_name = dir_name(&grouped_dir);
    let root = if (is_fragment || file_name.starts_with("config."))
        && (grouped_name == "mise" || grouped_name == ".mise")
    {
        parent(&grouped_dir)
    } else {
        config_dir.to_path_buf()
    };
    if dir_name(&root) == ".config" {
        parent(&root)
    } else {
        root
    }
}

fn has_template(value: &str) -> bool {
    value.contains("{{") || value.contains("{%")
}

/// Resolve a task path against the config root. A path with a template that
/// is not `config_root`, a home path, or a variable gives `None`.
fn resolve_task_path(value: &str, config_root: &Path) -> Option<PathBuf> {
    let expanded = expand_config_root(value, config_root);
    if has_template(&expanded) || expanded.starts_with('~') || expanded.contains('$') {
        return None;
    }
    Some(lexical_normalize(&config_root.join(expanded)))
}

/// Replace each `{{ config_root }}` placeholder with the config root.
fn expand_config_root(value: &str, config_root: &Path) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        expanded.push_str(&rest[..start]);
        if after[..end].trim() == "config_root" {
            expanded.push_str(&config_root.to_string_lossy());
        } else {
            expanded.push_str(&rest[start..start + 2 + end + 2]);
        }
        rest = &after[end + 2..];
    }
    expanded.push_str(rest);
    expanded
}

/// Return the env tables of an `env` value: one table or an array of tables.
fn env_tables(value: Option<&toml::Value>) -> Vec<&toml::Table> {
    match value {
        Some(toml::Value::Table(table)) => vec![table],
        Some(toml::Value::Array(items)) => items.iter().filter_map(toml::Value::as_table).collect(),
        _ => Vec::new(),
    }
}

/// Return true when the env tables put `node_modules/.bin` of the task
/// directory on `PATH`. A later `PATH` value resets an earlier entry.
fn has_local_bin_path<'a>(
    envs: impl Iterator<Item = &'a &'a toml::Table>,
    dir: &Path,
    config_root: &Path,
) -> bool {
    let local_bin = lexical_normalize(&dir.join(LOCAL_BIN_DIR));
    let mut local = false;
    for env in envs {
        if env.contains_key("PATH") {
            local = false;
        }
        let paths = env.get("_").and_then(|value| value.get("path"));
        let entries: Vec<&str> = match paths {
            Some(toml::Value::String(path)) => vec![path.as_str()],
            Some(toml::Value::Array(items)) => {
                items.iter().filter_map(toml::Value::as_str).collect()
            }
            _ => Vec::new(),
        };
        if entries
            .iter()
            .any(|path| resolve_task_path(path, config_root).as_deref() == Some(&local_bin))
        {
            local = true;
        }
    }
    local
}

/// Return the command strings of a `run` value. A task reference such as
/// `{ task = "lint" }` runs no command, so it gives nothing.
fn run_scripts(value: &toml::Value) -> Vec<&str> {
    match value {
        toml::Value::String(script) => vec![script.as_str()],
        toml::Value::Array(items) => items.iter().filter_map(toml::Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// One shell word, with its raw text and its value without quotes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    raw: String,
    value: String,
    quoted: bool,
}

struct CommandCollector<'a> {
    dir: &'a Path,
    credits: &'a mut TaskCredits,
}

impl CommandCollector<'_> {
    fn collect_script(&mut self, script: &str, local: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for body in scripts::command_substitutions(script) {
            self.collect_script(body, local, depth + 1);
        }
        for segment in split_segments(script) {
            self.collect_segment(&split_words(segment), local, depth);
        }
    }

    fn collect_segment(&mut self, words: &[Word], local: bool, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let mut index = 0;
        let mut path_override = false;
        while let Some(word) = words.get(index) {
            if is_noise(word) {
                index += 1;
            } else if is_redirect(word) {
                index += if is_bare_redirect(word) { 2 } else { 1 };
            } else if let Some(name) = assignment_name(word) {
                path_override |= name == "PATH";
                index += 1;
            } else {
                break;
            }
        }
        let Some(command) = words.get(index) else {
            return;
        };
        let local = local && !path_override;
        let command_name = command.value.as_str();
        let binary = command_name.rsplit('/').next().unwrap_or(command_name);

        if PACKAGE_MANAGERS.contains(&binary) && !command_name.contains('/') {
            self.analyze(&words[index..]);
        } else if command_name.contains("node_modules/.bin/") {
            let mut rest = words[index..].to_vec();
            rest[0] = Word {
                raw: binary.to_string(),
                value: binary.to_string(),
                quoted: false,
            };
            self.analyze(&rest);
        } else if command_name.starts_with("./") || command_name.starts_with("../") {
            self.credits
                .files
                .push(lexical_normalize(&self.dir.join(command_name)));
        } else if WRAPPERS.contains(&binary) {
            if local {
                self.analyze(&words[index..=index]);
            }
            self.collect_wrapped(binary, &words[index + 1..], local, depth);
        } else if binary == "find" {
            if let Some(child) = find_exec_command(&words[index + 1..]) {
                self.collect_segment(child, local, depth + 1);
            }
        } else if binary == "node" || local {
            self.analyze(&words[index..]);
        }
    }

    /// Collect the command that a wrapper runs from its arguments.
    fn collect_wrapped(&mut self, wrapper: &str, args: &[Word], local: bool, depth: usize) {
        if wrapper == "concurrently" {
            for arg in args.iter().filter(|arg| !arg.value.starts_with('-')) {
                self.collect_script(&arg.value, local, depth + 1);
            }
            return;
        }
        let child = if END_OF_OPTIONS_WRAPPERS.contains(&wrapper) {
            args.iter()
                .position(|arg| arg.raw == "--")
                .map(|separator| separator + 1)
        } else {
            args.iter()
                .position(|arg| !arg.quoted && !arg.value.starts_with('-'))
        };
        if let Some(child) = child {
            self.collect_segment(&args[child..], local, depth + 1);
        }
    }

    /// Run one command through the shared script parser and keep its
    /// packages and local file arguments.
    fn analyze(&mut self, words: &[Word]) {
        // The caller reads substitutions on their own, and a word with a
        // substitution has no static value.
        let command = words
            .iter()
            .filter(|word| !word.raw.contains("$(") && !word.raw.contains('`'))
            .map(|word| word.raw.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let analysis = scripts::analyze_command(&command, self.dir, &FxHashMap::default());
        self.credits.packages.extend(analysis.used_packages);
        for file in analysis.config_files.iter().chain(&analysis.entry_files) {
            self.credits
                .files
                .push(lexical_normalize(&self.dir.join(file)));
        }
    }
}

/// Return the command of a `find -exec` action, up to its terminator.
fn find_exec_command(args: &[Word]) -> Option<&[Word]> {
    let start = args
        .iter()
        .position(|arg| matches!(arg.value.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))?
        + 1;
    let rest = args.get(start..)?;
    let end = rest
        .iter()
        .position(|arg| matches!(arg.value.as_str(), ";" | "+"))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn is_noise(word: &Word) -> bool {
    !word.quoted && matches!(word.raw.as_str(), "!" | "{" | "}" | "(" | ")")
}

fn is_redirect(word: &Word) -> bool {
    if word.quoted {
        return false;
    }
    let rest = word.raw.trim_start_matches(|ch: char| ch.is_ascii_digit());
    rest.starts_with('>') || rest.starts_with('<') || rest.starts_with("&>")
}

/// Return true for a redirect operator whose target is the next word.
fn is_bare_redirect(word: &Word) -> bool {
    let rest = word.raw.trim_start_matches(|ch: char| ch.is_ascii_digit());
    matches!(rest, ">" | ">>" | "<" | "&>" | "&>>" | ">|")
}

/// Return the variable name of an environment assignment such as `KEY=value`.
fn assignment_name(word: &Word) -> Option<&str> {
    if word.quoted {
        return None;
    }
    let (name, _) = word.raw.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut chars = name.chars();
    let first = chars.next()?;
    let valid = (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    valid.then_some(name)
}

/// The quote and substitution state of a shell scanner.
#[derive(Default)]
struct ScanState {
    single: bool,
    double: bool,
    depth: usize,
}

impl ScanState {
    fn is_plain(&self) -> bool {
        !self.single && !self.double && self.depth == 0
    }

    /// Update the state for the byte at `index` and return the number of
    /// bytes that the scanner consumes.
    fn advance(&mut self, bytes: &[u8], index: usize) -> usize {
        let byte = bytes[index];
        if self.single {
            self.single = byte != b'\'';
            return 1;
        }
        match byte {
            b'\\' => return 2,
            b'\'' if !self.double => self.single = true,
            b'"' => self.double = !self.double,
            b'$' if bytes.get(index + 1) == Some(&b'(') => {
                self.depth += 1;
                return 2;
            }
            b'(' if self.depth > 0 => self.depth += 1,
            b')' if self.depth > 0 => self.depth -= 1,
            _ => {}
        }
        1
    }
}

/// Split a script on the unquoted shell operators `&&`, `||`, `;`, `|`, `&`,
/// and on line breaks.
fn split_segments(script: &str) -> Vec<&str> {
    let bytes = script.as_bytes();
    let mut segments = Vec::new();
    let mut state = ScanState::default();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if state.is_plain() {
            let operator_len = match bytes[index] {
                b'&' | b'|' if bytes.get(index + 1) == Some(&bytes[index]) => 2,
                b'&' if bytes.get(index + 1) == Some(&b'>') => 0,
                b'&' if index > 0 && matches!(bytes[index - 1], b'>' | b'<') => 0,
                b'|' if index > 0 && bytes[index - 1] == b'>' => 0,
                b';' | b'|' | b'&' | b'\n' => 1,
                _ => 0,
            };
            if operator_len > 0 {
                segments.push(&script[start..index]);
                index += operator_len;
                start = index;
                continue;
            }
        }
        index += state.advance(bytes, index);
    }
    segments.push(&script[start.min(script.len())..]);
    segments.retain(|segment| !segment.trim().is_empty());
    segments
}

/// Split one command segment into words. Quotes and `$(...)` keep their
/// spaces inside one word.
fn split_words(segment: &str) -> Vec<Word> {
    let bytes = segment.as_bytes();
    let mut words = Vec::new();
    let mut state = ScanState::default();
    let mut start: Option<usize> = None;
    let mut index = 0;
    while index < bytes.len() {
        if state.is_plain() && bytes[index].is_ascii_whitespace() {
            if let Some(word_start) = start.take() {
                words.push(make_word(&segment[word_start..index]));
            }
            index += 1;
            continue;
        }
        start.get_or_insert(index);
        index += state.advance(bytes, index);
    }
    if let Some(word_start) = start {
        words.push(make_word(&segment[word_start.min(segment.len())..]));
    }
    words
}

fn make_word(raw: &str) -> Word {
    let quoted = raw.starts_with('"') || raw.starts_with('\'');
    let mut value = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    let mut quote: Option<char> = None;
    while let Some(ch) = chars.next() {
        match (quote, ch) {
            (None, '\'' | '"') => quote = Some(ch),
            (Some(open), _) if ch == open => quote = None,
            (None | Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    value.push(next);
                }
            }
            _ => value.push(ch),
        }
    }
    Word {
        raw: raw.to_string(),
        value,
        quoted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a platform-absolute path from a logical `/`-rooted string.
    fn abs(logical: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{}", logical.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(logical)
        }
    }

    fn resolve(config_path: &str, source: &str) -> (Vec<String>, Vec<PathBuf>) {
        let result = MisePlugin.resolve_config(&abs(config_path), source, &abs("/project"));
        let mut packages = result.referenced_dependencies;
        packages.sort();
        (packages, result.setup_files)
    }

    #[test]
    fn credits_package_manager_runners_and_node_entries() {
        let source = r#"
[tasks]
lint = "time -p npx --no-install eslint ."
check = ["node scripts/check.ts"]

[tasks.test]
run = "! npm exec -- vitest run"

[tasks.build]
run = [
  { task = "lint" },
  { tasks = ["test", "format"] },
  "{ yarn exec assets build; } >build.log",
]
run_windows = "pnpm exec windows-checker"
"#;
        let (packages, files) = resolve("/project/mise.toml", source);
        assert_eq!(packages, ["assets", "eslint", "vitest", "windows-checker"]);
        assert_eq!(files, [abs("/project/scripts/check.ts")]);
    }

    #[test]
    fn skips_bare_commands_without_a_local_bin_path() {
        let source = r#"
[tasks.lint]
run = "eslint ."

[tasks.global]
run = [
  'concurrently "assets build" "terraform apply"',
  "mise exec npm:eslint -- eslint .",
  'echo "pnpm exec assets"',
  'find . -exec assets {} \;',
  "node --version",
]
"#;
        let (packages, files) = resolve("/project/mise.toml", source);
        assert!(packages.is_empty(), "unexpected credits: {packages:?}");
        assert!(files.is_empty());
    }

    #[test]
    fn credits_explicit_commands_inside_wrappers_and_substitutions() {
        let source = r#"
[tasks.local]
run = "cross-env GIT_SHA=$(git rev-parse HEAD) pnpm exec formatter . && assets build"

[tasks.substitution]
run = 'export RESULT="$(pnpm exec substitute)"'

[tasks.missing]
run = [
  "dotenvx run -- API_URL=https://example.test pnpm exec dotenv-cli-tool",
  'concurrently "c8 pnpm exec concurrent-tool $(git diff --name-only)" "terraform apply"',
  'find . -exec c8 pnpm exec -- find-tool {} \;',
]
"#;
        let (packages, _) = resolve("/project/mise.toml", source);
        assert_eq!(
            packages,
            [
                "concurrent-tool",
                "dotenv-cli-tool",
                "find-tool",
                "formatter",
                "substitute"
            ]
        );
    }

    #[test]
    fn skips_templates() {
        let source = r#"
[tasks.template]
run = "pnpm exec assets {{arg(name='target')}}"

[tasks.dynamic]
dir = "{{env.TASK_DIR}}"
run = "pnpm exec assets"
"#;
        let (packages, _) = resolve("/project/mise.toml", source);
        assert!(packages.is_empty(), "unexpected credits: {packages:?}");
    }

    #[test]
    fn credits_bare_commands_with_a_local_bin_path() {
        let source = r#"
[env]
_.path = ["{{ config_root }}/node_modules/.bin/"]

[tasks.lint]
run = "eslint ."

[tasks.build]
run = ['>build.log NODE_OPTIONS="--require ./preload.ts" cross-env c8 assets build']

[tasks.override]
env.PATH = "/usr/bin"
run = "overridden"

[tasks.inline]
run = 'cross-env PATH=/usr/bin inline-override'

[tasks.format]
run = "formatter ."
env.PATH = "/usr/bin"
env._.path = "node_modules/.bin"
"#;
        let (packages, _) = resolve("/project/.mise.toml", source);
        assert_eq!(
            packages,
            ["assets", "c8", "cross-env", "eslint", "formatter"]
        );
    }

    #[test]
    fn resolves_task_directories_and_paths() {
        let source = r#"
[task_config]
dir = "packages/web"

[tasks.check]
run = "node scripts/package-entry.ts"

[tasks.direct]
run = "./scripts/direct-entry.ts"

[tasks.lint]
run = "../../node_modules/.bin/eslint ."

[tasks.root]
dir = "."
run = "node scripts/root-entry.ts"
"#;
        let (packages, files) = resolve("/project/.mise.toml", source);
        assert_eq!(packages, ["eslint"]);
        assert_eq!(
            files,
            [
                abs("/project/packages/web/scripts/package-entry.ts"),
                abs("/project/packages/web/scripts/direct-entry.ts"),
                abs("/project/scripts/root-entry.ts"),
            ]
        );
    }

    #[test]
    fn config_root_follows_mise_directory_conventions() {
        let cases = [
            ("/project", "mise.toml", "/project"),
            ("/project/.config", "mise.toml", "/project"),
            ("/project/.config/mise", "config.toml", "/project"),
            ("/project/.config/mise/conf.d", "tasks.toml", "/project"),
            ("/project/.mise/conf.d", "release.toml", "/project"),
            ("/project/mise", "config.ci.toml", "/project"),
            (
                "/project/.config/mise",
                "mise.toml",
                "/project/.config/mise",
            ),
        ];
        for (dir, file, expected) in cases {
            assert_eq!(config_root(&abs(dir), file), abs(expected), "{dir}/{file}");
        }
    }

    #[test]
    fn conf_d_fragment_resolves_config_root_template() {
        let source = r#"
[task_config]
dir = "{{ config_root }}/scripts"

[tasks.release]
run = "node release.ts"
"#;
        let (_, files) = resolve("/project/.mise/conf.d/release.toml", source);
        assert_eq!(files, [abs("/project/scripts/release.ts")]);
    }

    #[test]
    fn splits_segments_outside_quotes_and_substitutions() {
        assert_eq!(
            split_segments(r#"a "b && c" && d $(e; f) | g 2>&1; h"#),
            [r#"a "b && c" "#, " d $(e; f) ", " g 2>&1", " h"]
        );
    }

    #[test]
    fn splits_words_with_quotes() {
        let words = split_words(r#"echo "pnpm exec x" 'y z' A=$(b c)"#);
        let values: Vec<&str> = words.iter().map(|word| word.value.as_str()).collect();
        assert_eq!(values, ["echo", "pnpm exec x", "y z", "A=$(b c)"]);
        assert!(words[1].quoted && words[2].quoted && !words[3].quoted);
    }

    #[test]
    fn activates_from_root_config_file() {
        let tmp = tempfile::tempdir().expect("temp dir");
        assert!(!MisePlugin.is_enabled_with_deps(&[], tmp.path()));
        std::fs::write(tmp.path().join(".mise.toml"), "").expect("write config");
        assert!(MisePlugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn activates_from_config_in_a_config_directory() {
        let tmp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(tmp.path().join(".config/mise")).expect("create dir");
        std::fs::write(tmp.path().join(".config/mise/config.toml"), "").expect("write config");
        assert!(MisePlugin.is_enabled_with_deps(&[], tmp.path()));
    }
}
