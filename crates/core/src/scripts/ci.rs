//! CI config file scanner for dependency usage detection.
//!
//! Extracts shell commands from `.gitlab-ci.yml` and `.github/workflows/*.yml`
//! files, parses them for binary invocations (especially `npx`), and maps
//! binaries to npm package names. This prevents false "unused dependency"
//! reports for packages only used in CI pipelines.

use std::borrow::Cow;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    IgnoredCommandEntries, ScriptCatalog, analyze_commands_with_context, could_be_file_path,
};

/// Result of scanning CI config files: package names used by CI tooling AND
/// project-relative file paths referenced as command-line arguments.
#[derive(Debug, Default)]
pub struct CiAnalysis {
    /// npm package names used as binaries in CI shell commands.
    pub used_packages: FxHashSet<String>,
    /// File paths extracted as positional arguments or `--config` values
    /// (e.g., `node scripts/deploy.ts` in a GitHub Actions `run:` block).
    /// Paths are project-root-relative; CI files always live at the root.
    pub entry_files: Vec<String>,
    /// The subset of `entry_files` that a built-in test runner runs as tests
    /// (`node --test scripts/*.test.mjs`). These are test entry points.
    pub test_entry_files: Vec<String>,
}

/// Inputs shared by every CI file of one analysis.
struct CiContext<'a> {
    root: &'a Path,
    bin_map: &'a FxHashMap<String, String>,
    declared_packages: &'a FxHashSet<String>,
    scripts: &'a ScriptCatalog,
    ignored: IgnoredCommandEntries<'a>,
}

/// Analyze CI config files for package binary invocations and file references.
///
/// Scans GitLab CI and GitHub Actions workflow files for shell commands,
/// extracts binary names AND positional file path arguments, and returns both
/// the set of npm package names used and the file paths referenced as command
/// arguments. The file paths are seeded as entry points so scripts invoked from
/// CI (`node scripts/deploy.ts`) do not get reported as `unused-files`.
///
/// CI files always live at `.gitlab-ci.yml` or `.github/workflows/*.yml`
/// relative to the project root, so no workspace-prefix transformation applies.
/// The catalog spans the whole project, so a CI step that resolves to a script
/// declared only by a workspace package credits that body's dependencies but
/// contributes none of its file arguments, which are relative to that package.
pub fn analyze_ci_files(
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
    scripts: &ScriptCatalog,
    ignored: IgnoredCommandEntries<'_>,
) -> CiAnalysis {
    let _span = tracing::info_span!("analyze_ci_files").entered();
    let mut analysis = CiAnalysis::default();
    let context = CiContext {
        root,
        bin_map,
        declared_packages,
        scripts,
        ignored,
    };

    let gitlab_ci = root.join(".gitlab-ci.yml");
    if let Ok(content) = std::fs::read_to_string(&gitlab_ci) {
        extract_ci_signals(&content, &context, &mut analysis);
    }

    let workflows_dir = root.join(".github/workflows");
    if let Ok(entries) = std::fs::read_dir(&workflows_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if (name_str.ends_with(".yml") || name_str.ends_with(".yaml"))
                && let Ok(content) = std::fs::read_to_string(entry.path())
            {
                extract_ci_signals(&content, &context, &mut analysis);
            }
        }
    }

    for (action_dir, content) in read_composite_actions(root) {
        extract_ci_signals_in(&content, &context, Some(&action_dir), &mut analysis);
    }

    analysis.entry_files.sort();
    analysis.entry_files.dedup();
    analysis.test_entry_files.sort();
    analysis.test_entry_files.dedup();
    analysis
}

/// Extract package names AND file path references from shell commands found in
/// a CI config file.
///
/// Uses line-based heuristics to find shell command lines in YAML CI configs.
/// This intentionally avoids a full YAML parser to keep dependencies minimal.
/// Known limitations (line-based parsing): variable interpolation
/// (`${{ matrix.env }}/deploy.ts`), `\` line-continuations, YAML anchors
/// (`<<: *defaults`) are silently skipped.
fn extract_ci_signals(content: &str, context: &CiContext<'_>, analysis: &mut CiAnalysis) {
    extract_ci_signals_in(content, context, None, analysis);
}

/// [`extract_ci_signals`] for a file whose `${{ github.action_path }}` is
/// `action_dir`, relative to the project root. `None` for a workflow file.
///
/// Each command runs in its `working-directory`. The commands of one
/// directory are analyzed together, and their file arguments resolve in that
/// directory. Package credit does not depend on the directory.
fn extract_ci_signals_in(
    content: &str,
    context: &CiContext<'_>,
    action_dir: Option<&str>,
    analysis: &mut CiAnalysis,
) {
    let mut groups: Vec<(Vec<Segment>, Vec<String>)> = Vec::new();
    for step in extract_ci_steps(content) {
        let dir = step
            .working_directory
            .as_deref()
            .map(parse_working_directory)
            .unwrap_or_default();
        let text = rewrite_anchors(&step.text, dir.len(), action_dir).into_owned();
        match groups.iter_mut().find(|(group_dir, _)| *group_dir == dir) {
            Some((_, commands)) => commands.push(text),
            None => groups.push((dir, vec![text])),
        }
    }

    for (dir, commands) in groups {
        let catalog = working_directory_catalog(context.scripts, &dir);
        let parsed = analyze_commands_with_context(
            &commands,
            context.root,
            context.bin_map,
            context.declared_packages,
            &catalog,
            context.ignored,
        );
        let resolve = |entry: String| {
            if dir.is_empty() {
                Some(entry)
            } else {
                resolve_in_working_dir(&dir, &entry)
            }
        };
        analysis.used_packages.extend(parsed.used_packages);
        analysis.entry_files.extend(
            parsed
                .config_files
                .into_iter()
                .filter(|s| could_be_file_path(s))
                .filter_map(resolve),
        );
        analysis
            .entry_files
            .extend(parsed.entry_files.into_iter().filter_map(resolve));
        analysis
            .test_entry_files
            .extend(parsed.test_entry_files.into_iter().filter_map(resolve));
    }
}

/// The run steps of the composite actions in `.github/actions/<name>/` and
/// `.github/actions/<group>/<name>/`, and of the root `action.yml`, with the
/// directory of each action relative to the project root.
fn read_composite_actions(root: &Path) -> Vec<(String, String)> {
    let mut actions = Vec::new();
    push_action_file(root, "", &mut actions);
    let Ok(entries) = std::fs::read_dir(root.join(".github/actions")) else {
        return actions;
    };
    let mut dirs: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| format!(".github/actions/{}", entry.file_name().to_string_lossy()))
        .collect();
    dirs.sort();
    for dir in dirs {
        if push_action_file(root, &dir, &mut actions) {
            continue;
        }
        let Ok(children) = std::fs::read_dir(root.join(&dir)) else {
            continue;
        };
        let mut nested: Vec<String> = children
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| format!("{dir}/{}", entry.file_name().to_string_lossy()))
            .collect();
        nested.sort();
        for child in nested {
            push_action_file(root, &child, &mut actions);
        }
    }
    actions
}

/// Read the `action.yml` (or `action.yaml`) in `dir`. Returns whether the
/// directory holds one.
fn push_action_file(root: &Path, dir: &str, actions: &mut Vec<(String, String)>) -> bool {
    ["action.yml", "action.yaml"].iter().any(|name| {
        let Ok(content) = std::fs::read_to_string(root.join(dir).join(name)) else {
            return false;
        };
        actions.push((dir.to_string(), content));
        true
    })
}

/// One segment of a `working-directory`, relative to the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Name(String),
    /// A segment whose value is known only at run time (`${{ matrix.name }}`),
    /// or the unknown location of a directory outside the project.
    Expression,
}

/// Parse a `working-directory` value into segments relative to the project
/// root. A workspace anchor (`$GITHUB_WORKSPACE`) resets to the root. A value
/// outside the project (`/tmp`, `~`, `${{ runner.temp }}`) starts with an
/// [`Segment::Expression`].
fn parse_working_directory(raw: &str) -> Vec<Segment> {
    let value = raw.split_once(" #").map_or(raw, |(value, _)| value);
    let value = unquote(value.trim());
    let mut dir = Vec::new();
    if value.starts_with('/') {
        dir.push(Segment::Expression);
    }
    for part in split_path(value) {
        match part {
            "" | "." => {}
            ".." => {
                if dir.pop().is_none() {
                    dir.push(Segment::Expression);
                }
            }
            part if anchor_target(part) == Some(AnchorTarget::Root) => dir.clear(),
            part if part.contains('$') || part.starts_with('~') => dir.push(Segment::Expression),
            part => dir.push(Segment::Name(part.to_string())),
        }
    }
    dir
}

fn unquote(value: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|quote| {
            value
                .strip_prefix(*quote)
                .and_then(|rest| rest.strip_suffix(*quote))
        })
        .unwrap_or(value)
}

/// Split a path on `/`, but keep each `${{ ... }}` expression in one part.
fn split_path(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < value.len() {
        let rest = &value[index..];
        if rest.starts_with("${{")
            && let Some(close) = rest.find("}}")
        {
            index += close + 2;
            continue;
        }
        if rest.starts_with('/') {
            parts.push(&value[start..index]);
            start = index + 1;
        }
        index += rest.chars().next().map_or(1, char::len_utf8);
    }
    parts.push(&value[start..]);
    parts
}

/// The directory that a workspace anchor stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorTarget {
    /// The project root: `$GITHUB_WORKSPACE`, `${{ github.workspace }}`,
    /// GitLab `$CI_PROJECT_DIR`.
    Root,
    /// The directory of the composite action: `${{ github.action_path }}`.
    Action,
}

const ROOT_ANCHOR_VARIABLES: &[&str] = &["GITHUB_WORKSPACE", "CI_PROJECT_DIR"];
const ROOT_ANCHOR_EXPRESSIONS: &[&str] = &["github.workspace", "env.GITHUB_WORKSPACE"];
const ACTION_ANCHOR_EXPRESSION: &str = "github.action_path";

/// The target of `anchor` when the whole token is a workspace anchor.
fn anchor_target(anchor: &str) -> Option<AnchorTarget> {
    match anchor_at(anchor) {
        Some((target, len)) if len == anchor.len() => Some(target),
        _ => None,
    }
}

/// The anchor at the start of `text`, with its byte length.
fn anchor_at(text: &str) -> Option<(AnchorTarget, usize)> {
    if let Some(rest) = text.strip_prefix("${{") {
        let close = rest.find("}}")?;
        let expression = rest[..close].trim();
        let len = 3 + close + 2;
        if ROOT_ANCHOR_EXPRESSIONS.contains(&expression) {
            return Some((AnchorTarget::Root, len));
        }
        return (expression == ACTION_ANCHOR_EXPRESSION).then_some((AnchorTarget::Action, len));
    }
    if let Some(rest) = text.strip_prefix("${") {
        let close = rest.find('}')?;
        return ROOT_ANCHOR_VARIABLES
            .contains(&&rest[..close])
            .then_some((AnchorTarget::Root, 2 + close + 1));
    }
    let rest = text.strip_prefix('$')?;
    let name_len = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(rest.len());
    ROOT_ANCHOR_VARIABLES
        .contains(&&rest[..name_len])
        .then_some((AnchorTarget::Root, 1 + name_len))
}

/// Replace each workspace anchor that starts a path (`$GITHUB_WORKSPACE/x`,
/// `"$GITHUB_WORKSPACE"/x`) with the relative path from a working directory
/// `depth` segments below the root. `action_dir` is the target of
/// `${{ github.action_path }}`; without it, that anchor stays.
fn rewrite_anchors<'a>(command: &'a str, depth: usize, action_dir: Option<&str>) -> Cow<'a, str> {
    if !command.contains('$') {
        return Cow::Borrowed(command);
    }
    let up = if depth == 0 {
        ".".to_string()
    } else {
        vec![".."; depth].join("/")
    };
    let mut out = String::with_capacity(command.len());
    let mut changed = false;
    let mut index = 0;
    while index < command.len() {
        let rest = &command[index..];
        if let Some((target, len)) = anchor_at(rest)
            && starts_path(&rest[len..])
        {
            let replacement = match (target, action_dir) {
                (AnchorTarget::Root, _) | (AnchorTarget::Action, Some("")) => Some(up.clone()),
                (AnchorTarget::Action, Some(dir)) if depth == 0 => Some(dir.to_string()),
                (AnchorTarget::Action, Some(dir)) => Some(format!("{up}/{dir}")),
                (AnchorTarget::Action, None) => None,
            };
            if let Some(replacement) = replacement {
                out.push_str(&replacement);
                index += len;
                changed = true;
                continue;
            }
        }
        let Some(ch) = rest.chars().next() else {
            break;
        };
        out.push(ch);
        index += ch.len_utf8();
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(command)
    }
}

/// Whether the text after an anchor continues a path: a `/`, directly or
/// after the closing quote of the anchor.
fn starts_path(after: &str) -> bool {
    after.starts_with('/') || after.starts_with("\"/") || after.starts_with("'/")
}

/// The catalog that resolves the script calls of commands in `dir`. A
/// workspace package directory uses the scripts of that package. Another
/// directory keeps the script names and bodies, but no body resolves file
/// arguments, because those are relative to the package root.
fn working_directory_catalog<'a>(
    catalog: &'a ScriptCatalog,
    dir: &[Segment],
) -> Cow<'a, ScriptCatalog> {
    if dir.is_empty() {
        return Cow::Borrowed(catalog);
    }
    let names: Option<Vec<&str>> = dir
        .iter()
        .map(|segment| match segment {
            Segment::Name(name) => Some(name.as_str()),
            Segment::Expression => None,
        })
        .collect();
    if let Some(names) = names
        && let Some(package) = catalog
            .workspaces
            .as_deref()
            .and_then(|workspaces| workspaces.find_dir(&names.join("/")))
    {
        return Cow::Owned(catalog.package_catalog(package));
    }
    let mut foreign = catalog.clone();
    for body in foreign.bodies.values_mut() {
        body.local = false;
    }
    Cow::Owned(foreign)
}

/// Resolve a file argument of a command in `dir` to a path relative to the
/// project root. `None` when the path leaves the project or depends on a
/// run-time value.
fn resolve_in_working_dir(dir: &[Segment], entry: &str) -> Option<String> {
    if entry.starts_with('/') {
        return None;
    }
    let mut stack = dir.to_vec();
    let mut parts = entry.split('/').peekable();
    while let Some(part) = parts.next() {
        match part {
            "" if parts.peek().is_some() => return None,
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            name => stack.push(Segment::Name(name.to_string())),
        }
    }
    let names: Option<Vec<String>> = stack
        .into_iter()
        .map(|segment| match segment {
            Segment::Name(name) => Some(name),
            Segment::Expression => None,
        })
        .collect();
    names
        .filter(|names| !names.is_empty())
        .map(|names| names.join("/"))
}

/// Extract shell command strings from a CI config file.
///
/// Recognizes:
/// - YAML list items in script blocks: `  - npx tool --flag`
/// - GitHub Actions run fields: `  run: command`
/// - Block scalar run blocks: `  run: |` or `  run: >` followed by indented lines
/// - Plain multi-line scalars: `  run: command` whose continuation lines are
///   indented past the `run` key column and fold into the same command
pub(super) fn extract_ci_commands(content: &str) -> Vec<String> {
    extract_ci_steps(content)
        .into_iter()
        .map(|command| command.text)
        .collect()
}

/// A shell command of a CI file, with the `working-directory` of its step.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CiCommand {
    pub(super) text: String,
    /// The raw `working-directory` value of the step, or of the innermost
    /// `defaults.run` that applies. `None` for the project root.
    pub(super) working_directory: Option<String>,
}

/// Extract the shell commands of a CI file with their working directories.
/// See [`extract_ci_commands`] for the recognized command forms.
pub(super) fn extract_ci_steps(content: &str) -> Vec<CiCommand> {
    let mut commands = Vec::new();
    let mut multiline_run = MultilineRunState::default();
    let mut scope = WorkingDirectoryScope::default();

    for line in content.lines() {
        let trimmed = line.trim();

        if should_skip_ci_line(trimmed) {
            continue;
        }

        let default_dir = scope.default_dir();
        if push_multiline_run_command(
            line,
            trimmed,
            &mut multiline_run,
            &mut commands,
            default_dir,
        ) {
            continue;
        }

        scope.observe(line, trimmed, &mut commands);
        let default_dir = scope.default_dir();

        if let Some(rest) = yaml_run_value(trimmed) {
            if is_multiline_run_marker(rest) {
                multiline_run.start(run_key_column(line, trimmed), false);
            } else if !rest.is_empty() {
                commands.push(CiCommand {
                    text: rest.to_string(),
                    working_directory: default_dir,
                });
                multiline_run.start(run_key_column(line, trimmed), true);
            }
            continue;
        }

        push_yaml_list_command(trimmed, &mut commands, default_dir);
    }

    scope.close_step(&mut commands);
    commands
}

/// Tracks the GitHub Actions step and `defaults.run` scopes that set a
/// `working-directory`, with line-based column heuristics.
#[derive(Default)]
struct WorkingDirectoryScope {
    step: Option<OpenStep>,
    /// `defaults.run.working-directory` values, outermost first, with the
    /// column of their `defaults` key.
    defaults: Vec<(usize, String)>,
    pending_defaults: Option<PendingDefaults>,
}

struct OpenStep {
    /// The key column of the list item that opened the step.
    column: usize,
    /// The index of the first command of the step.
    first_command: usize,
    working_directory: Option<String>,
}

struct PendingDefaults {
    column: usize,
    run_column: Option<usize>,
}

impl WorkingDirectoryScope {
    fn default_dir(&self) -> Option<String> {
        self.defaults.last().map(|(_, dir)| dir.clone())
    }

    fn observe(&mut self, line: &str, trimmed: &str, commands: &mut [CiCommand]) {
        let indent = line.len() - line.trim_start().len();
        let column = run_key_column(line, trimmed);
        let item = trimmed.strip_prefix("- ").map(str::trim_start);

        while self
            .defaults
            .last()
            .is_some_and(|(defaults_column, _)| *defaults_column > indent)
        {
            self.defaults.pop();
        }

        if let Some(step) = &self.step {
            let closes = match item {
                Some(_) => column <= step.column,
                None => indent < step.column,
            };
            if closes {
                self.close_step(commands);
            }
        }
        if item.is_some() && self.step.is_none() {
            self.step = Some(OpenStep {
                column,
                first_command: commands.len(),
                working_directory: None,
            });
        }

        let key = item.unwrap_or(trimmed);
        if let Some(value) = strip_yaml_key(key, "working-directory") {
            self.set_working_directory(column, indent, value.trim());
        }
        self.track_defaults(column, key);
    }

    fn set_working_directory(&mut self, column: usize, indent: usize, value: &str) {
        if value.is_empty() {
            return;
        }
        if let Some(step) = &mut self.step
            && step.column == column
        {
            step.working_directory = Some(value.to_string());
            return;
        }
        if let Some(pending) = &self.pending_defaults
            && pending
                .run_column
                .is_some_and(|run_column| indent > run_column)
        {
            self.defaults.push((pending.column, value.to_string()));
            self.pending_defaults = None;
        }
    }

    fn track_defaults(&mut self, column: usize, key: &str) {
        if key == "defaults:" {
            self.pending_defaults = Some(PendingDefaults {
                column,
                run_column: None,
            });
            return;
        }
        let Some(pending) = &mut self.pending_defaults else {
            return;
        };
        if column <= pending.column {
            self.pending_defaults = None;
        } else if key == "run:" {
            pending.run_column = Some(column);
        }
    }

    fn close_step(&mut self, commands: &mut [CiCommand]) {
        let Some(step) = self.step.take() else {
            return;
        };
        if let Some(dir) = step.working_directory {
            for command in commands.iter_mut().skip(step.first_command) {
                command.working_directory = Some(dir.clone());
            }
        }
    }
}

#[derive(Default)]
struct MultilineRunState {
    active: bool,
    indent: usize,
    folding: bool,
}

impl MultilineRunState {
    /// Anchor at the `run` key column so sibling step keys (`env:`, `with:`),
    /// which sit at the same column, terminate the scalar instead of being
    /// swallowed as continuation lines. `folding` marks a plain scalar whose
    /// continuations fold into the already-pushed command.
    fn start(&mut self, key_column: usize, folding: bool) {
        self.active = true;
        self.indent = key_column;
        self.folding = folding;
    }

    fn stop(&mut self) {
        self.active = false;
        self.folding = false;
    }
}

/// Column of the `run` key itself, past any leading `- ` list indicator.
fn run_key_column(line: &str, trimmed: &str) -> usize {
    let base = line.len() - line.trim_start().len();
    match trimmed.strip_prefix("- ") {
        Some(rest) => base + 2 + (rest.len() - rest.trim_start().len()),
        None => base,
    }
}

fn should_skip_ci_line(trimmed: &str) -> bool {
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn push_multiline_run_command(
    line: &str,
    trimmed: &str,
    state: &mut MultilineRunState,
    commands: &mut Vec<CiCommand>,
    working_directory: Option<String>,
) -> bool {
    if !state.active {
        return false;
    }

    let indent = line.len() - line.trim_start().len();
    if indent > state.indent && !trimmed.is_empty() {
        if state.folding {
            if let Some(last) = commands.last_mut() {
                last.text.push(' ');
                last.text.push_str(trimmed);
            }
        } else {
            commands.push(CiCommand {
                text: trimmed.to_string(),
                working_directory,
            });
        }
        return true;
    }

    state.stop();
    false
}

fn yaml_run_value(trimmed: &str) -> Option<&str> {
    strip_yaml_key(trimmed, "run")
        .or_else(|| {
            trimmed
                .strip_prefix("- ")
                .and_then(|rest| strip_yaml_key(rest.trim(), "run"))
        })
        .map(str::trim)
}

/// Recognize a YAML block scalar header on a `run:` value.
///
/// Accepts the literal (`|`) and folded (`>`) styles with any combination of a
/// chomping indicator (`-`, `+`) and an explicit indentation indicator (`1`-`9`),
/// in either order, matching the YAML block header grammar. A header carrying a
/// trailing comment is still not recognized.
fn is_multiline_run_marker(value: &str) -> bool {
    let mut chars = value.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return false;
    }

    let mut chomping = false;
    let mut indentation = false;
    for ch in chars {
        match ch {
            '-' | '+' if !chomping => chomping = true,
            '1'..='9' if !indentation => indentation = true,
            _ => return false,
        }
    }
    true
}

fn push_yaml_list_command(
    trimmed: &str,
    commands: &mut Vec<CiCommand>,
    working_directory: Option<String>,
) {
    let Some(rest) = trimmed.strip_prefix("- ") else {
        return;
    };
    let rest = rest.trim();
    if !rest.is_empty()
        && !rest.starts_with('{')
        && !rest.starts_with('[')
        && !is_yaml_mapping(rest)
    {
        commands.push(CiCommand {
            text: rest.to_string(),
            working_directory,
        });
    }
}

/// Strip a YAML key prefix from a line, returning the value part.
/// Handles `key: value` and `key:` (empty value).
fn strip_yaml_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.strip_prefix(':')?;
    Some(rest)
}

/// Check if a string looks like a YAML mapping (key: value) rather than a shell command.
fn is_yaml_mapping(s: &str) -> bool {
    if let Some(first_word) = s.split_whitespace().next()
        && first_word.ends_with(':')
        && !first_word.starts_with("http")
        && !first_word.starts_with("ftp")
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_set() -> FxHashSet<String> {
        FxHashSet::default()
    }

    fn set(values: &[&str]) -> FxHashSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn catalog(scripts: &[(&str, &str)]) -> ScriptCatalog {
        #[expect(
            clippy::disallowed_types,
            reason = "ScriptCatalog is built from serde-deserialized HashMap"
        )]
        let scripts: std::collections::HashMap<String, String> = scripts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect();
        ScriptCatalog::from_scripts(&scripts)
    }

    fn analyze_content(content: &str) -> CiAnalysis {
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &empty_set(),
                scripts: &catalog(&[]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        analysis
    }

    #[test]
    fn gitlab_ci_script_items() {
        let content = r"
stages:
  - build
  - test

build:
  stage: build
  script:
    - npm ci
    - npx @cyclonedx/cyclonedx-npm --output-file sbom.json
    - npm run build
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(
            commands.contains(&"npx @cyclonedx/cyclonedx-npm --output-file sbom.json".to_string())
        );
        assert!(commands.contains(&"npm run build".to_string()));
    }

    #[test]
    fn github_actions_inline_run() {
        let content = r"
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm ci
      - run: npx eslint src
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(commands.contains(&"npx eslint src".to_string()));
    }

    #[test]
    fn github_actions_multiline_run() {
        let content = r"
jobs:
  build:
    steps:
      - run: |
          npm ci
          npx @cyclonedx/cyclonedx-npm --output sbom.json
          npm run build
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(commands.contains(&"npx @cyclonedx/cyclonedx-npm --output sbom.json".to_string()));
        assert!(commands.contains(&"npm run build".to_string()));
    }

    /// The shape of GitHub's own code-scanning/eslint.yml starter workflow: a
    /// plain (unquoted) scalar whose continuation lines are indented past the
    /// `run` key column and fold into one command (issue #2016).
    #[test]
    fn github_actions_plain_multiline_run_folds_continuations() {
        let content = r"
jobs:
  eslint:
    steps:
      - name: Run ESLint
        run: npx eslint .
          --config .eslintrc.js
          --ext .js,.jsx,.ts,.tsx
        continue-on-error: true
";
        let commands = extract_ci_commands(content);
        assert!(
            commands.contains(
                &"npx eslint . --config .eslintrc.js --ext .js,.jsx,.ts,.tsx".to_string()
            ),
            "continuation lines must fold into the run command, got: {commands:?}"
        );
        assert!(
            !commands.iter().any(|c| c.contains("continue-on-error")),
            "sibling step keys must not be swallowed as continuations, got: {commands:?}"
        );
    }

    /// Sibling keys sit at the `run` key column, so key-column anchoring must
    /// terminate a plain scalar there; their path-looking values must not flow
    /// into entry_files where they could hide genuinely unused files.
    #[test]
    fn plain_run_sibling_key_values_do_not_leak_into_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: npx eslint .
          --max-warnings 0
        env:
          CONFIG_PATH: scripts/config.ts
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npx eslint . --max-warnings 0".to_string()));
        assert!(!commands.iter().any(|c| c.contains("CONFIG_PATH")));

        let analysis = analyze_content(content);
        assert!(
            !analysis.entry_files.iter().any(|f| f.contains("config.ts")),
            "env values must not seed entry files, got: {:?}",
            analysis.entry_files
        );
    }

    /// End-to-end coverage for folded block scalars, beyond the marker
    /// predicate test: commands inside `run: >-` must reach package analysis.
    #[test]
    fn folded_run_block_commands_analyzed() {
        let content = r"
jobs:
  sbom:
    steps:
      - run: >-
          npx @cyclonedx/cyclonedx-npm
          --output-file sbom.json
";
        let analysis = analyze_content(content);
        assert!(analysis.used_packages.contains("@cyclonedx/cyclonedx-npm"));
    }

    #[test]
    fn yaml_mappings_filtered() {
        let content = r"
image: node:18
stages:
  - build
variables:
  NODE_ENV: production
build:
  script:
    - npm ci
";
        let commands = extract_ci_commands(content);
        assert!(!commands.iter().any(|c| c.contains("node:18")));
        assert!(!commands.iter().any(|c| c.contains("NODE_ENV")));
        assert!(commands.contains(&"npm ci".to_string()));
    }

    #[test]
    fn comments_and_empty_lines_skipped() {
        let content = r"
# This is a comment
  # Indented comment

build:
  script:
    - npm ci
";
        let commands = extract_ci_commands(content);
        assert_eq!(commands, vec!["npm ci"]);
    }

    #[test]
    fn npx_package_extracted() {
        let content = r"
build:
  script:
    - npx @cyclonedx/cyclonedx-npm --output-file sbom.json
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(
            packages.contains("@cyclonedx/cyclonedx-npm"),
            "packages: {packages:?}"
        );
    }

    #[test]
    fn multiple_binaries_extracted() {
        let content = r"
build:
  script:
    - npx eslint src
    - npx prettier --check .
    - tsc --noEmit
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(packages.contains("eslint"));
        assert!(packages.contains("prettier"));
        assert!(packages.contains("typescript")); // tsc → typescript via resolve
    }

    #[test]
    fn builtin_commands_not_extracted() {
        let content = r"
build:
  script:
    - echo 'hello'
    - mkdir -p dist
    - cp -r build/* dist/
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(
            packages.is_empty(),
            "should not extract built-in commands: {packages:?}"
        );
    }

    #[test]
    fn github_actions_npx_extracted() {
        let content = r"
jobs:
  sbom:
    steps:
      - run: npx @cyclonedx/cyclonedx-npm --output-file sbom.json
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(packages.contains("@cyclonedx/cyclonedx-npm"));
    }

    #[test]
    fn github_actions_pnpm_bare_declared_binary_extracted() {
        let content = r"
jobs:
  info:
    steps:
      - run: pnpm envinfo --system
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["envinfo"]),
                scripts: &catalog(&[]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("envinfo"));
    }

    #[test]
    fn github_actions_pnpm_script_name_shorthand_skipped() {
        let content = r"
jobs:
  build:
    steps:
      - run: pnpm build
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["build"]),
                scripts: &catalog(&[("build", "vite build")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(!analysis.used_packages.contains("build"));
    }

    /// The dominant real-world shape: CI never names the linter, it runs the
    /// package.json script and adds the CI formatter flag (issue #2016).
    #[test]
    fn github_actions_npm_run_script_forwards_formatter_flag() {
        let content = r"
jobs:
  lint:
    steps:
      - run: npm run lint -- --format gha
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["eslint", "eslint-formatter-gha"]),
                scripts: &catalog(&[("lint", "eslint .")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("eslint"));
        assert!(analysis.used_packages.contains("eslint-formatter-gha"));
    }

    /// A root CI step resolving to a workspace-only script must not turn that
    /// body's file arguments into root-relative entry patterns (issue #2016).
    #[test]
    fn workspace_only_script_body_does_not_seed_root_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: npm run build -- --mode ci
";
        #[expect(
            clippy::disallowed_types,
            reason = "ScriptCatalog is built from serde-deserialized HashMap"
        )]
        let ws_scripts: std::collections::HashMap<String, String> =
            std::iter::once(("build".to_string(), "esbuild scripts/bundle.js".to_string()))
                .collect();
        let mut scripts = ScriptCatalog::default();
        scripts.merge_workspace_scripts(&ws_scripts);

        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["esbuild"]),
                scripts: &scripts,
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("esbuild"));
        assert!(analysis.entry_files.is_empty());
    }

    /// Bun runs a declared script first. When no script has the name and the
    /// argument is a script file, Bun runs that file.
    #[test]
    fn github_actions_bun_file_runner_seeds_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: bun scripts/a.ts
      - run: bun run scripts/b.ts
      - run: bun --watch scripts/c.ts
      - run: bun run build --minify
      - run: bun run scripts/d.ts
      - run: bun run dev
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["esbuild"]),
                scripts: &catalog(&[("build", "esbuild --bundle"), ("scripts/d.ts", "tsc")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert_eq!(
            analysis.entry_files,
            vec!["scripts/a.ts", "scripts/b.ts", "scripts/c.ts"]
        );
        assert!(analysis.used_packages.contains("esbuild"));
        assert!(!analysis.used_packages.contains("bun"));
    }

    #[test]
    fn node_test_run_lines_record_test_entry_files() {
        let content = "
jobs:
  test:
    steps:
      - run: node --test .github/scripts/*.test.mjs
      - run: node --test --experimental-test-coverage scripts/a.test.mjs
      - run: node scripts/release.mjs
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &FxHashSet::default(),
                scripts: &ScriptCatalog::default(),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert_eq!(
            analysis.test_entry_files,
            vec![".github/scripts/*.test.mjs", "scripts/a.test.mjs"]
        );
        assert!(
            analysis
                .entry_files
                .contains(&"scripts/release.mjs".to_string())
        );
    }

    #[test]
    fn github_actions_expression_fragments_not_entry_files() {
        let content = r#"
jobs:
  health-check:
    steps:
      - run: |
          RESPONSE_CODE=$(curl -s -o "$TMPFILE" -w "%{http_code}" -m 15 "${{ env.ENVIRONMENT_URL }}/api/health/ready")
          echo "$RESPONSE_CODE"
"#;
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains("${{") && !path.contains("}}"),
                "entry_files must not contain GitHub Actions expression fragments, got: {path:?}"
            );
        }
    }

    #[test]
    fn jq_array_iterator_not_entry_file() {
        let content = r#"
jobs:
  process:
    steps:
      - run: |
          jq -c '.[]' /tmp/x.json | while read item; do echo "$item"; done
          result=$(jq -r '.[]' data.json)
"#;
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains(".[]"),
                "entry_files must not contain jq array-iterator fragments, got: {path:?}"
            );
        }
    }

    #[test]
    fn heredoc_js_array_literal_not_entry_file() {
        let content = r"
jobs:
  discover:
    steps:
      - name: Build matrix
        run: |
          node <<'NODE'
          const roots = process.env.SCOPE === 'public'
            ? ['packages/apps/public']
            : ['packages/apps/internal', 'packages/apps/public'];
          NODE
";
        let analysis = analyze_content(content);
        assert!(
            !analysis.entry_files.iter().any(|f| f.contains('[')),
            "a JS array literal must not become an entry pattern, got: {:?}",
            analysis.entry_files
        );
    }

    #[test]
    fn grep_perl_regex_fragment_not_entry_file() {
        let content = r"
jobs:
  deploy:
    steps:
      - run: |
          grep -oP '(?<=Module )\./[^ ]+(?= has finished with an error)' deploy.log
";
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains(r"\./"),
                "entry_files must not contain regex escape fragments, got: {path:?}"
            );
            assert!(
                !path.contains("[^"),
                "entry_files must not contain unclosed character class fragments, got: {path:?}"
            );
        }
    }

    /// Regression test for issue #2592: a quoted jq filter passed as a shell
    /// argument to `jq -r` (the alternative operator `//` gives it a `/`)
    /// must not be harvested as an entry pattern. Reported against the exact
    /// `mapfile < <(jq ...)` shape from a real workflow.
    #[test]
    fn jq_alternative_operator_filter_not_entry_file() {
        let content = r#"
jobs:
  process:
    steps:
      - name: List proposal PRs
        run: |
          state=".github/state.json"
          mapfile -t proposal_prs < <(jq -r '[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]' "$state")
"#;
        let analysis = analyze_content(content);
        assert!(
            analysis.entry_files.is_empty(),
            "jq filter must not become an entry pattern, got: {:?}",
            analysis.entry_files
        );
    }

    /// A `run:` block invoking a real script by path is genuine dependency
    /// evidence and must still be harvested, even next to a `run:` block
    /// whose quoted jq filter must be dropped (issue #2592).
    #[test]
    fn genuine_script_path_alongside_jq_filter_still_harvested() {
        let content = r#"
jobs:
  process:
    steps:
      - name: List proposal PRs
        run: |
          state=".github/state.json"
          mapfile -t proposal_prs < <(jq -r '[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]' "$state")
      - name: Run real script
        run: node scripts/deploy.js --env production
"#;
        let analysis = analyze_content(content);
        assert!(
            analysis
                .entry_files
                .contains(&"scripts/deploy.js".to_string()),
            "genuine script path must still be harvested, got: {:?}",
            analysis.entry_files
        );
        assert!(
            !analysis.entry_files.iter().any(|f| f.contains("proposals")),
            "jq filter must not leak into entry_files, got: {:?}",
            analysis.entry_files
        );
    }

    #[test]
    fn strip_yaml_key_basic() {
        assert_eq!(strip_yaml_key("run: npm test", "run"), Some(" npm test"));
        assert_eq!(strip_yaml_key("run:", "run"), Some(""));
        assert_eq!(strip_yaml_key("other: value", "run"), None);
    }

    #[test]
    fn is_yaml_mapping_basic() {
        assert!(is_yaml_mapping("NODE_ENV: production"));
        assert!(is_yaml_mapping("image: node:18"));
        assert!(!is_yaml_mapping("npm ci"));
        assert!(!is_yaml_mapping("npx eslint src"));
        assert!(!is_yaml_mapping("https://example.com"));
    }

    /// GitHub Actions accepts the folded style as readily as the literal one, and
    /// fallow's own release-validation workflow uses it.
    #[test]
    fn folded_run_scalar_is_a_block_marker() {
        for marker in ["|", "|-", "|+", ">", ">-", ">+", "|2", ">2-", ">-2"] {
            assert!(
                is_multiline_run_marker(marker),
                "{marker} is a block header"
            );
        }
        for value in [
            "", "npm ci", "|foo", ">out.txt", "||", ">>", "-", "|0", "|--",
        ] {
            assert!(
                !is_multiline_run_marker(value),
                "{value} is a command, not a block header"
            );
        }
    }

    fn analyze_with_catalog(content: &str, scripts: &ScriptCatalog) -> CiAnalysis {
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &empty_set(),
                scripts,
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        analysis
    }

    /// The entries as the entry-pattern step normalizes them.
    fn sorted(entries: &[String]) -> Vec<String> {
        let mut entries: Vec<String> = entries
            .iter()
            .filter_map(|entry| super::super::normalize_script_entry_pattern("", entry))
            .collect();
        entries.sort();
        entries
    }

    #[test]
    fn step_working_directory_prefixes_file_args() {
        let content = r"
jobs:
  build:
    steps:
      - name: Generate
        working-directory: packages/web
        run: node scripts/direct.js
      - run: node scripts/root.js
";
        assert_eq!(
            sorted(&analyze_content(content).entry_files),
            ["packages/web/scripts/direct.js", "scripts/root.js"]
        );
    }

    #[test]
    fn working_directory_after_run_key_applies() {
        let content = r"
jobs:
  build:
    steps:
      - run: |
          node scripts/a.js
          node scripts/b.js
        working-directory: tools
";
        assert_eq!(
            sorted(&analyze_content(content).entry_files),
            ["tools/scripts/a.js", "tools/scripts/b.js"]
        );
    }

    #[test]
    fn with_input_working_directory_is_ignored() {
        let content = r"
jobs:
  build:
    steps:
      - uses: some/action@v1
        with:
          working-directory: packages/web
      - run: node scripts/a.js
";
        assert_eq!(analyze_content(content).entry_files, ["scripts/a.js"]);
    }

    #[test]
    fn job_defaults_working_directory_applies_until_next_job() {
        let content = r"
jobs:
  app:
    defaults:
      run:
        working-directory: apps/desktop
    steps:
      - run: node scripts/a.js
      - run: node scripts/b.js
        working-directory: tools
  other:
    steps:
      - run: node scripts/c.js
";
        assert_eq!(
            sorted(&analyze_content(content).entry_files),
            [
                "apps/desktop/scripts/a.js",
                "scripts/c.js",
                "tools/scripts/b.js"
            ]
        );
    }

    #[test]
    fn workflow_defaults_working_directory_applies() {
        let content = r"
defaults:
  run:
    working-directory: site
jobs:
  build:
    steps:
      - run: node scripts/a.js
";
        assert_eq!(analyze_content(content).entry_files, ["site/scripts/a.js"]);
    }

    #[test]
    fn github_workspace_anchor_forms_resolve_to_root() {
        let content = r#"
jobs:
  build:
    steps:
      - run: node $GITHUB_WORKSPACE/scripts/a.ts
      - run: node ${GITHUB_WORKSPACE}/scripts/b.ts
      - run: node "$GITHUB_WORKSPACE"/scripts/c.ts
      - run: node "${{ github.workspace }}/scripts/d.ts"
      - run: node ${{github.workspace}}/scripts/e.ts
      - run: node ${{ env.GITHUB_WORKSPACE }}/scripts/f.ts
"#;
        assert_eq!(
            sorted(&analyze_content(content).entry_files),
            [
                "scripts/a.ts",
                "scripts/b.ts",
                "scripts/c.ts",
                "scripts/d.ts",
                "scripts/e.ts",
                "scripts/f.ts"
            ]
        );
    }

    #[test]
    fn anchor_inside_working_directory_ignores_the_directory() {
        let content = r"
jobs:
  build:
    steps:
      - working-directory: packages/web
        run: node $GITHUB_WORKSPACE/scripts/a.ts
";
        assert_eq!(analyze_content(content).entry_files, ["scripts/a.ts"]);
    }

    #[test]
    fn expression_working_directory_resolves_parent_relative_args_only() {
        let content = r"
jobs:
  build:
    steps:
      - working-directory: ecosystem-ci/${{ matrix.project.name }}
        run: |
          node ../patch-project.ts
          node ./inner.ts
";
        assert_eq!(
            analyze_content(content).entry_files,
            ["ecosystem-ci/patch-project.ts"]
        );
    }

    #[test]
    fn outside_working_directory_drops_relative_args_keeps_anchored() {
        let content = r"
jobs:
  build:
    steps:
      - working-directory: ${{ runner.temp }}/project
        run: |
          node ./verify.ts
          node $GITHUB_WORKSPACE/ecosystem-ci/verify-install.ts
";
        assert_eq!(
            analyze_content(content).entry_files,
            ["ecosystem-ci/verify-install.ts"]
        );
    }

    #[test]
    fn npm_run_in_workspace_working_directory_uses_package_script() {
        let content = r"
jobs:
  build:
    steps:
      - working-directory: packages/web
        run: npm run gen -- --watch
";
        #[expect(
            clippy::disallowed_types,
            reason = "package.json scripts are deserialized as std HashMap"
        )]
        let web_scripts: std::collections::HashMap<String, String> =
            std::iter::once(("gen".to_string(), "node scripts/gen.js".to_string())).collect();
        let mut workspaces = super::super::WorkspacePackages::default();
        workspaces.add_root("root", None);
        workspaces.add("web", "packages/web", Some(&web_scripts));
        let mut scripts = ScriptCatalog::default();
        scripts.merge_workspace_scripts(&web_scripts);
        let scripts = scripts.with_workspaces(std::sync::Arc::new(workspaces), "");
        assert_eq!(
            analyze_with_catalog(content, &scripts).entry_files,
            ["packages/web/scripts/gen.js"]
        );
    }

    #[test]
    fn root_script_file_args_do_not_resolve_in_another_directory() {
        let content = r"
jobs:
  build:
    steps:
      - working-directory: docs
        run: npm run gen -- --watch
";
        let scripts = catalog(&[("gen", "node scripts/gen.js")]);
        assert!(
            analyze_with_catalog(content, &scripts)
                .entry_files
                .is_empty()
        );
    }

    #[test]
    fn gitlab_ci_project_dir_anchor() {
        let content = r"
build:
  script:
    - node $CI_PROJECT_DIR/scripts/a.ts
    - node ${CI_PROJECT_DIR}/scripts/b.ts
";
        assert_eq!(
            sorted(&analyze_content(content).entry_files),
            ["scripts/a.ts", "scripts/b.ts"]
        );
    }

    #[test]
    fn bare_workspace_anchor_without_slash_is_untouched() {
        assert_eq!(
            rewrite_anchors("cd $GITHUB_WORKSPACE && ls $GITHUB_WORKSPACE_X/a", 2, None),
            "cd $GITHUB_WORKSPACE && ls $GITHUB_WORKSPACE_X/a"
        );
        assert_eq!(
            rewrite_anchors("node $GITHUB_WORKSPACE/a.ts", 2, None),
            "node ../../a.ts"
        );
    }

    #[test]
    fn composite_action_steps_scanned_with_action_path_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let action = dir.path().join(".github/actions/build");
        std::fs::create_dir_all(&action).unwrap();
        std::fs::write(
            action.join("action.yml"),
            r#"
name: Build
runs:
  using: composite
  steps:
    - run: pnpm exec napi build
      shell: bash
    - run: node ${{ github.action_path }}/run.ts
      shell: bash
    - run: node "${{ github.action_path }}/../shared/util.ts"
      shell: bash
"#,
        )
        .unwrap();
        let nested = dir.path().join(".github/actions/group/deploy");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("action.yaml"),
            "runs:\n  steps:\n    - run: node scripts/deploy.ts\n",
        )
        .unwrap();
        let analysis = analyze_ci_files(
            dir.path(),
            &FxHashMap::default(),
            &set(&["@napi-rs/cli"]),
            &catalog(&[]),
            IgnoredCommandEntries::NONE,
        );
        assert!(analysis.used_packages.contains("@napi-rs/cli"));
        assert_eq!(
            sorted(&analysis.entry_files),
            [
                ".github/actions/build/run.ts",
                ".github/actions/shared/util.ts",
                "scripts/deploy.ts"
            ]
        );
    }

    #[test]
    fn working_directory_parsing() {
        assert_eq!(
            parse_working_directory("'./packages/web/'"),
            [
                Segment::Name("packages".into()),
                Segment::Name("web".into())
            ]
        );
        assert_eq!(
            parse_working_directory("web # the site"),
            [Segment::Name("web".into())]
        );
        assert_eq!(
            parse_working_directory("${{ github.workspace }}/site"),
            [Segment::Name("site".into())]
        );
        assert_eq!(
            parse_working_directory("/tmp/x"),
            [
                Segment::Expression,
                Segment::Name("tmp".into()),
                Segment::Name("x".into())
            ]
        );
        assert_eq!(
            parse_working_directory("${{ format('{0}/x', inputs.dir) }}/y"),
            [Segment::Expression, Segment::Name("y".into())]
        );
    }
}
