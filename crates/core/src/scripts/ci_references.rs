//! Script references in a CI file outside a plain `run:` value.
//!
//! - A step can run a matrix value: `run: ${{ matrix.project.command }}`.
//!   The values of that matrix key become the commands of the step.
//! - An `actions/github-script` step runs JavaScript from its `script:` input.
//!   The script can load a project file through `process.env.GITHUB_WORKSPACE`,
//!   `${{ github.workspace }}`, or a `require('./path')` that resolves from the
//!   workspace.
//!
//! Like the rest of the CI scanner, this is a line-based reading of YAML.

use super::{
    CiCommand, is_multiline_run_marker, is_yaml_mapping, run_key_column, should_skip_ci_line,
    strip_yaml_key, unquote,
};

/// The action that runs the JavaScript of a `script:` input.
const GITHUB_SCRIPT_ACTION: &str = "actions/github-script";

/// The anchors in a github-script body that stand for the project root. The
/// Actions runner replaces `${{ github.workspace }}` before the script runs.
const SCRIPT_ROOT_ANCHORS: &[&str] = &[
    "${process.env.GITHUB_WORKSPACE}/",
    "${{ github.workspace }}/",
];

/// The starts of a JavaScript comment line, so a commented-out `require`
/// names no file.
const JS_COMMENT_STARTS: &[&str] = &["//", "/*", "*"];

/// The anchor in a github-script body of a composite action that stands for
/// the action directory.
const SCRIPT_ACTION_ANCHOR: &str = "${{ github.action_path }}/";

/// The `require` calls whose relative path resolves from the working
/// directory of github-script, the project root. A relative `import()`
/// resolves from the action bundle instead, so it is not read.
const SCRIPT_REQUIRE_CALLS: &[&str] = &["require('./", "require(\"./", "require(`./"];

/// Replace each command that is a matrix value reference with the values of
/// that matrix key in the job of the step. Each value keeps the working
/// directory of the step.
pub(super) fn expand_matrix_runs(content: &str, commands: Vec<CiCommand>) -> Vec<CiCommand> {
    if !commands
        .iter()
        .any(|command| matrix_path(&command.text).is_some())
    {
        return commands;
    }
    let lines: Vec<&str> = content.lines().collect();
    let mut expanded = Vec::with_capacity(commands.len());
    for command in commands {
        let Some(path) = matrix_path(&command.text) else {
            expanded.push(command);
            continue;
        };
        let job = job_lines(&lines, command.line);
        expanded.extend(
            matrix_values(&lines[job], &path)
                .into_iter()
                .map(|text| CiCommand {
                    text,
                    working_directory: command.working_directory.clone(),
                    line: command.line,
                }),
        );
    }
    expanded
}

/// The keys of a `${{ matrix.<path> }}` command, when the whole command is
/// that expression.
fn matrix_path(command: &str) -> Option<Vec<&str>> {
    let path = command
        .trim()
        .strip_prefix("${{")?
        .strip_suffix("}}")?
        .trim()
        .strip_prefix("matrix.")?;
    let keys: Vec<&str> = path.split('.').collect();
    keys.iter()
        .all(|key| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        })
        .then_some(keys)
}

/// The lines of the workflow job that holds `line`. A file without a
/// top-level `jobs:` key is one job.
fn job_lines(lines: &[&str], line: usize) -> std::ops::Range<usize> {
    let significant = |index: &usize| !should_skip_ci_line(lines[*index].trim());
    let indent = |index: usize| lines[index].len() - lines[index].trim_start().len();
    let Some(jobs) = lines[..line.min(lines.len())]
        .iter()
        .position(|text| text.trim_end() == "jobs:")
    else {
        return 0..lines.len();
    };
    let Some(job_column) = (jobs + 1..lines.len()).find(significant).map(indent) else {
        return 0..lines.len();
    };
    let start = (jobs + 1..=line)
        .rev()
        .filter(significant)
        .find(|index| indent(*index) == job_column)
        .unwrap_or(jobs + 1);
    let end = (line + 1..lines.len())
        .filter(significant)
        .find(|index| indent(*index) <= job_column)
        .unwrap_or(lines.len());
    start..end
}

/// The command lines of the `path` key in the `matrix:` blocks of `lines`: a
/// plain scalar, each line of a block scalar, or each item of a block
/// sequence. The parent keys must match the path, directly or under
/// `include:`, so a key under `exclude:` or another group gives nothing.
fn matrix_values(lines: &[&str], path: &[&str]) -> Vec<String> {
    let Some((key, parents)) = path.split_last() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    let mut matrix_column: Option<usize> = None;
    let mut ancestors: Vec<(usize, &str)> = Vec::new();
    let mut value: Option<MatrixValue> = None;

    for line in lines {
        let trimmed = line.trim();
        if should_skip_ci_line(trimmed) {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(open) = &value {
            match open.kind {
                MatrixValueKind::Lines if indent > open.column => {
                    values.push(trimmed.to_string());
                    continue;
                }
                MatrixValueKind::Items
                    if indent > open.column
                        || (indent == open.column && trimmed.starts_with("- ")) =>
                {
                    if let Some(item) = trimmed.strip_prefix("- ").map(str::trim)
                        && !item.starts_with(['[', '{'])
                        && !is_yaml_mapping(item)
                    {
                        values.push(unquote(item).to_string());
                    }
                    continue;
                }
                _ => value = None,
            }
        }
        if matrix_column.is_some_and(|column| indent <= column) {
            matrix_column = None;
        }
        let column = run_key_column(line, trimmed);
        let entry = trimmed.strip_prefix("- ").map_or(trimmed, str::trim_start);
        let Some(matrix) = matrix_column else {
            if entry == "matrix:" {
                matrix_column = Some(column);
                ancestors.clear();
            }
            continue;
        };
        ancestors.retain(|(ancestor_column, _)| *ancestor_column < column);
        let Some((entry_key, rest)) = entry.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        if entry_key == *key && column > matrix && parents_match(&ancestors, parents) {
            if is_multiline_run_marker(rest) {
                value = Some(MatrixValue {
                    column,
                    kind: MatrixValueKind::Lines,
                });
            } else if rest.is_empty() {
                value = Some(MatrixValue {
                    column,
                    kind: MatrixValueKind::Items,
                });
            } else if !rest.starts_with(['[', '{']) {
                values.push(unquote(rest).to_string());
            }
            continue;
        }
        ancestors.push((column, entry_key));
    }
    values
}

/// Whether the keys above a matrix key are `parents`, or `include` and then
/// `parents`.
fn parents_match(ancestors: &[(usize, &str)], parents: &[&str]) -> bool {
    let keys: Vec<&str> = ancestors.iter().map(|(_, key)| *key).collect();
    keys == parents
        || keys
            .split_first()
            .is_some_and(|(first, rest)| *first == "include" && rest == parents)
}

struct MatrixValue {
    column: usize,
    kind: MatrixValueKind,
}

enum MatrixValueKind {
    /// A block scalar: each line is a command.
    Lines,
    /// A block sequence: each item is a command.
    Items,
}

/// The project-root-relative files that the `actions/github-script` steps of
/// a CI file load. `action_dir` is the directory of a composite action, or
/// `None` for a workflow file.
pub(super) fn github_script_files(content: &str, action_dir: Option<&str>) -> Vec<String> {
    if !content.contains(GITHUB_SCRIPT_ACTION) {
        return Vec::new();
    }
    let mut files = Vec::new();
    for body in github_script_bodies(content) {
        for line in &body {
            script_line_files(line, action_dir, &mut files);
        }
    }
    files
}

/// The `script:` lines of each step that uses `actions/github-script`.
fn github_script_bodies(content: &str) -> Vec<Vec<String>> {
    let mut bodies = Vec::new();
    let mut step: Option<ScriptStep> = None;
    let mut block_column: Option<usize> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(column) = block_column {
            if indent > column {
                if let Some(open) = &mut step
                    && !JS_COMMENT_STARTS
                        .iter()
                        .any(|start| trimmed.starts_with(start))
                {
                    open.script.push(trimmed.to_string());
                }
                continue;
            }
            block_column = None;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let column = run_key_column(line, trimmed);
        let item = trimmed.strip_prefix("- ").map(str::trim_start);
        if let Some(open) = &step {
            let closes = match item {
                Some(_) => column <= open.column,
                None => indent < open.column,
            };
            if closes && let Some(body) = step.take().and_then(ScriptStep::into_body) {
                bodies.push(body);
            }
        }
        if item.is_some() && step.is_none() {
            step = Some(ScriptStep {
                column,
                uses_github_script: false,
                script: Vec::new(),
            });
        }
        let Some(open) = &mut step else {
            continue;
        };
        let entry = item.unwrap_or(trimmed);
        if let Some(uses) = strip_yaml_key(entry, "uses") {
            open.uses_github_script = unquote(uses.trim()).starts_with(GITHUB_SCRIPT_ACTION);
        } else if let Some(script) = strip_yaml_key(entry, "script").map(str::trim) {
            if is_multiline_run_marker(script) {
                block_column = Some(column);
            } else if !script.is_empty() {
                open.script.push(script.to_string());
            }
        }
    }
    if let Some(body) = step.and_then(ScriptStep::into_body) {
        bodies.push(body);
    }
    bodies
}

struct ScriptStep {
    /// The key column of the list item that opened the step.
    column: usize,
    uses_github_script: bool,
    script: Vec<String>,
}

impl ScriptStep {
    fn into_body(self) -> Option<Vec<String>> {
        (self.uses_github_script && !self.script.is_empty()).then_some(self.script)
    }
}

/// Push the files that one line of a github-script body loads.
fn script_line_files(line: &str, action_dir: Option<&str>, files: &mut Vec<String>) {
    let mut starts: Vec<(usize, Option<&str>)> = Vec::new();
    for anchor in SCRIPT_ROOT_ANCHORS {
        starts.extend(
            line.match_indices(anchor)
                .map(|(at, _)| (at + anchor.len(), None)),
        );
    }
    if let Some(dir) = action_dir {
        starts.extend(
            line.match_indices(SCRIPT_ACTION_ANCHOR)
                .map(|(at, _)| (at + SCRIPT_ACTION_ANCHOR.len(), Some(dir))),
        );
    }
    for call in SCRIPT_REQUIRE_CALLS {
        starts.extend(
            line.match_indices(call)
                .map(|(at, _)| (at + call.len(), None)),
        );
    }
    for (start, base) in starts {
        let Some(path) = script_path(&line[start..]) else {
            continue;
        };
        files.push(match base {
            Some(dir) if !dir.is_empty() => format!("{dir}/{path}"),
            _ => path,
        });
    }
}

/// The static path at the start of `rest`, up to the closing quote. `None`
/// for a path with an interpolation or a parent segment.
fn script_path(rest: &str) -> Option<String> {
    let end = rest.find(['`', '\'', '"'])?;
    let path = &rest[..end];
    if path.is_empty() || path.ends_with('/') || path.contains(['$', '{', ' ']) {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(text: &str, line: usize) -> CiCommand {
        CiCommand {
            text: text.to_string(),
            working_directory: Some("ecosystem".to_string()),
            line,
        }
    }

    #[test]
    fn matrix_path_reads_whole_matrix_expressions_only() {
        assert_eq!(
            matrix_path("${{ matrix.project.command }}"),
            Some(vec!["project", "command"])
        );
        assert_eq!(matrix_path(" ${{matrix.check}} "), Some(vec!["check"]));
        assert_eq!(matrix_path("npm run ${{ matrix.script }}"), None);
        assert_eq!(matrix_path("${{ matrix.project.command || 'x' }}"), None);
        assert_eq!(matrix_path("${{ inputs.command }}"), None);
    }

    fn expand(content: &str, runs: &[&str]) -> Vec<String> {
        let commands = runs
            .iter()
            .map(|run| {
                let line = content
                    .lines()
                    .position(|text| text.trim_start() == format!("- run: {run}"))
                    .expect("run line");
                command(run, line)
            })
            .collect();
        expand_matrix_runs(content, commands)
            .into_iter()
            .inspect(|c| assert_eq!(c.working_directory.as_deref(), Some("ecosystem")))
            .map(|c| c.text)
            .collect()
    }

    #[test]
    fn matrix_runs_expand_to_the_values_of_the_key() {
        let content = "\
jobs:
  e2e:
    strategy:
      matrix:
        project:
          - name: one
            other: node unrelated.ts
            command: |
              # node commented.ts
              node verify.ts
              vp test
          - name: two
            command: 'node two.ts'
        check:
        - node list.ts
        - [skipped]
        group:
          - command: node group.ts
        exclude:
          - check: node excluded.ts
        include:
          - check: node included.ts
    steps:
      - run: ${{ matrix.project.command }}
      - run: ${{ matrix.check }}
      - run: npm test
";
        assert_eq!(
            expand(
                content,
                &[
                    "${{ matrix.project.command }}",
                    "${{ matrix.check }}",
                    "npm test"
                ]
            ),
            [
                "node verify.ts",
                "vp test",
                "node two.ts",
                "node list.ts",
                "node included.ts",
                "npm test"
            ]
        );
    }

    #[test]
    fn matrix_runs_read_only_the_matrix_of_their_job() {
        let content = "\
jobs:
  first:
    strategy:
      matrix:
        command: [a]
        include:
          - command: node first.ts
    steps:
      - run: echo first
  second:
    steps:
      - run: ${{ matrix.command }}
";
        assert!(expand(content, &["${{ matrix.command }}"]).is_empty());
    }

    #[test]
    fn github_script_bodies_name_workspace_files() {
        let content = "\
jobs:
  comment:
    steps:
      - uses: actions/github-script@v7
        with:
          script: |
            const a = await import(`${process.env.GITHUB_WORKSPACE}/.github/scripts/a.mjs`);
            const b = require('./scripts/b.js');
            const c = await import('./scripts/relative.mjs');
            const d = require(`${process.env.GITHUB_WORKSPACE}/${name}.js`);
            const e = require('${{ github.workspace }}/../outside.js');
            // const old = require('./scripts/old.js');
            const dir = '${{ github.workspace }}/scripts/' + name;
      - name: not github-script
        uses: actions/other@v1
        with:
          script: require('./scripts/other.js')
      - uses: actions/github-script@v7
        with:
          script: require(\"${{ github.workspace }}/scripts/inline.js\")
";
        assert_eq!(
            github_script_files(content, None),
            [".github/scripts/a.mjs", "scripts/b.js", "scripts/inline.js"]
        );
    }

    #[test]
    fn github_script_in_a_composite_action_reads_the_action_path() {
        let content = "\
runs:
  using: composite
  steps:
    - uses: actions/github-script@v7
      with:
        script: |
          require('${{ github.action_path }}/local.js')
";
        assert_eq!(
            github_script_files(content, Some(".github/actions/report")),
            [".github/actions/report/local.js"]
        );
        assert_eq!(github_script_files(content, Some("")), ["local.js"]);
    }
}
