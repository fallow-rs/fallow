//! The `run:` value of a CI step: a plain scalar that can fold over
//! lines, a block scalar (`run: |`), or a quoted scalar.

use super::{CiCommand, strip_yaml_key};

/// Decode a YAML single-quoted or double-quoted scalar: `'a ''b'''` is
/// `a 'b'`, and `"a \"b\""` is `a "b"`. Only a comment can follow the
/// closing quote. `None` when `text` is not one complete quoted scalar.
pub(super) fn quoted_scalar_value(text: &str) -> Option<String> {
    let mut chars = text.char_indices();
    let (_, quote) = chars.next().filter(|(_, ch)| matches!(ch, '\'' | '"'))?;
    let mut value = String::with_capacity(text.len());
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\'' if quote == '\'' => {
                if text[index + 1..].starts_with('\'') {
                    chars.next();
                    value.push('\'');
                    continue;
                }
                return closes_scalar(&text[index + 1..]).then_some(value);
            }
            '"' if quote == '"' => return closes_scalar(&text[index + 1..]).then_some(value),
            '\\' if quote == '"' => {
                let (_, escaped) = chars.next()?;
                value.push(match escaped {
                    'n' => '\n',
                    't' => '\t',
                    other => other,
                });
            }
            other => value.push(other),
        }
    }
    None
}

/// Whether `rest`, the text after a closing quote, holds only a comment.
fn closes_scalar(rest: &str) -> bool {
    let rest = rest.trim_start();
    rest.is_empty() || rest.starts_with('#')
}

#[derive(Default)]
pub(super) struct MultilineRunState {
    active: bool,
    indent: usize,
    folding: bool,
}

impl MultilineRunState {
    /// Anchor at the `run` key column so sibling step keys (`env:`, `with:`),
    /// which sit at the same column, terminate the scalar instead of being
    /// swallowed as continuation lines. `folding` marks a plain scalar whose
    /// continuations fold into the already-pushed command.
    pub(super) fn start(&mut self, key_column: usize, folding: bool) {
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
pub(super) fn run_key_column(line: &str, trimmed: &str) -> usize {
    let base = line.len() - line.trim_start().len();
    match trimmed.strip_prefix("- ") {
        Some(rest) => base + 2 + (rest.len() - rest.trim_start().len()),
        None => base,
    }
}

pub(super) fn push_multiline_run_command(
    line: &str,
    line_index: usize,
    state: &mut MultilineRunState,
    commands: &mut Vec<CiCommand>,
    working_directory: Option<String>,
) -> bool {
    if !state.active {
        return false;
    }

    let trimmed = line.trim();
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
                line: line_index,
            });
        }
        return true;
    }

    state.stop();
    false
}

pub(super) fn yaml_run_value(trimmed: &str) -> Option<&str> {
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
pub(super) fn is_multiline_run_marker(value: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::super::extract_ci_commands;

    #[test]
    fn github_actions_quoted_run_values() {
        let content = r#"
jobs:
  build:
    steps:
      - name: single
        run: 'node scripts/single.js artifacts >> "$GITHUB_STEP_SUMMARY"'
      - name: double
        run: "node scripts/double.js"
      - run: 'echo ''quoted'' && node scripts/escaped.js' # trailing comment
      - run: "node scripts/escaped-double.js \"$ARG\""
"#;
        let commands = extract_ci_commands(content);
        assert_eq!(
            commands,
            [
                r#"node scripts/single.js artifacts >> "$GITHUB_STEP_SUMMARY""#,
                "node scripts/double.js",
                "echo 'quoted' && node scripts/escaped.js",
                r#"node scripts/escaped-double.js "$ARG""#,
            ]
        );
    }

    #[test]
    fn github_actions_quoted_run_value_over_lines() {
        let content = r"
jobs:
  build:
    steps:
      - run: 'node scripts/long.js
          --flag value'
      - run: npm test
";
        let commands = extract_ci_commands(content);
        assert_eq!(commands, ["node scripts/long.js --flag value", "npm test"]);
    }

    #[test]
    fn gitlab_ci_quoted_script_items() {
        let content = r#"
build:
  script:
    - 'node scripts/single.js'
    - "node scripts/double.js"
"#;
        let commands = extract_ci_commands(content);
        assert_eq!(
            commands,
            ["node scripts/single.js", "node scripts/double.js"]
        );
    }
}
