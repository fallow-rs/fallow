//! Step inputs of well-known GitHub Actions that hold a shell command.
//!
//! An action runs such an input as a command, as a `run:` step does. Only the
//! inputs that the action documents as a command count, because other inputs
//! hold free text.

use super::ci::CiCommand;

/// An action and its inputs that hold a shell command.
struct CommandInputAction {
    /// The `owner/repo` part of the `uses:` value.
    name: &'static str,
    inputs: &'static [&'static str],
    /// The input that sets the working directory of the commands.
    cwd_input: Option<&'static str>,
}

const COMMAND_INPUT_ACTIONS: &[CommandInputAction] = &[CommandInputAction {
    // https://github.com/changesets/action#inputs
    name: "changesets/action",
    inputs: &["version", "publish"],
    cwd_input: Some("cwd"),
}];

/// The `uses:` and `with:` keys of one step, read line by line.
#[derive(Default)]
pub(super) struct StepActionInputs {
    action: Option<&'static CommandInputAction>,
    /// The key column of `with:` while its entries are read.
    with_column: Option<usize>,
    /// The column of the first `with:` entry. Deeper lines are values.
    entry_column: Option<usize>,
    /// Single-line `with:` entries as key, value and line.
    entries: Vec<(String, String, usize)>,
}

impl StepActionInputs {
    /// Read one line of the step. `column` is the key column, `indent` the
    /// indentation of the line, and `key` the line without a list marker.
    pub(super) fn observe(
        &mut self,
        step_column: usize,
        column: usize,
        indent: usize,
        key: &str,
        line: usize,
    ) {
        if self.with_column.is_some_and(|with| indent <= with) {
            self.with_column = None;
            self.entry_column = None;
        }
        if column == step_column {
            if let Some(uses) = strip_key(key, "uses") {
                self.action = known_action(uses);
            } else if strip_key(key, "with")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('#'))
            {
                self.with_column = Some(column);
            }
            return;
        }
        if self.with_column.is_none() {
            return;
        }
        let entry_column = *self.entry_column.get_or_insert(column);
        if column != entry_column {
            return;
        }
        if let Some((name, value)) = key.split_once(':') {
            let value = super::ci::unquote(super::ci::strip_yaml_comment(value.trim()));
            if !value.is_empty() && !value.starts_with(['|', '>']) {
                self.entries
                    .push((name.trim().to_string(), value.to_string(), line));
            }
        }
    }

    /// The commands that the action of the step runs.
    pub(super) fn into_commands(self) -> Vec<CiCommand> {
        let Some(action) = self.action else {
            return Vec::new();
        };
        let working_directory = action.cwd_input.and_then(|cwd| {
            self.entries
                .iter()
                .find(|(name, _, _)| name == cwd)
                .map(|(_, value, _)| value.clone())
        });
        self.entries
            .into_iter()
            .filter(|(name, _, _)| action.inputs.contains(&name.as_str()))
            .map(|(_, text, line)| CiCommand {
                text,
                working_directory: working_directory.clone(),
                line,
            })
            .collect()
    }
}

fn known_action(uses: &str) -> Option<&'static CommandInputAction> {
    let uses = super::ci::unquote(super::ci::strip_yaml_comment(uses.trim()));
    let name = uses.split_once('@').map_or(uses, |(name, _)| name);
    COMMAND_INPUT_ACTIONS
        .iter()
        .find(|action| action.name.eq_ignore_ascii_case(name))
}

fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.strip_prefix(key)?.strip_prefix(':').map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::super::ci::extract_ci_commands;

    #[test]
    fn command_inputs_of_a_known_action_are_commands() {
        let workflow = r#"
jobs:
  release:
    steps:
      - uses: actions/checkout@v4
      - name: Create release pull request
        uses: changesets/action@v1 # pinned
        with: # inputs
          commit: "chore(release): version packages"
          version: node scripts/version.js
          publish: 'pnpm release'
        env:
          version: node scripts/env-only.js
      - run: pnpm test
"#;
        assert_eq!(
            extract_ci_commands(workflow),
            vec!["node scripts/version.js", "pnpm release", "pnpm test"]
        );
    }

    #[test]
    fn inputs_of_an_unknown_action_and_block_values_are_not_commands() {
        let workflow = r"
steps:
  - uses: some/other-action@v2
    with:
      version: node scripts/a.js
  - with:
      publish: |
        node scripts/b.js
    uses: changesets/action@v1
";
        assert!(extract_ci_commands(workflow).is_empty());
    }

    #[test]
    fn the_cwd_input_sets_the_working_directory() {
        let workflow = r"
steps:
  - uses: changesets/action@v1
    with:
      cwd: packages/web
      publish: node scripts/publish.js
";
        let steps = super::super::ci::extract_ci_steps(workflow);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].working_directory.as_deref(), Some("packages/web"));
    }
}
