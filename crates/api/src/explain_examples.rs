//! The example project of each rule: a small project that makes the rule
//! report a finding.
//!
//! The files are in `crates/api/rule-examples/<rule>/`, where `<rule>` is the
//! rule id without its namespace prefix:
//!
//! - `files/` holds the project. For a rule of the `audit` command, it holds
//!   the project after the change.
//! - `base/` holds the project before the change. Only an `audit` rule has it.
//!   A file of `base/` that `files/` does not have stays as it is.
//! - `example.json` is optional. `args` are the arguments after the command
//!   that the finding needs. `hidden` lists the files that a reader does not
//!   need to understand the finding, such as a plain `package.json`.
//!   `anchor` is `"file-level"` for a rule whose finding is on a whole file.
//!   Without it, the finding is on a line of the example code.
//!
//! `explain_examples_table.rs` embeds the files. The CLI integration test `rule_example_tests`
//! runs fallow on each project and checks that the rule reports it, so an
//! example cannot go stale.

use crate::explain::{RuleDef, bare_rule_id};

#[path = "explain_examples_table.rs"]
mod table;

/// One file of an example project.
pub struct ExampleFile {
    /// The path from the project root, with `/` separators.
    pub path: &'static str,
    /// The text of the file.
    pub code: &'static str,
    /// Whether the explain output leaves the file out. The file is still part
    /// of the project that reports the finding.
    pub hidden: bool,
}

/// Where the finding of an example is in its file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExampleAnchor {
    /// The finding is on the line of the code that causes it. The example
    /// keeps that code away from line 1, column 0, so a location that a
    /// format fills in for a missing span cannot pass as a real one.
    Line,
    /// The finding is on the whole file, such as an unused file or a route
    /// file. A format reports no line, or line 1.
    FileLevel,
}

/// The example project of one rule.
pub struct RuleExample {
    /// The rule id without its namespace prefix, such as `unused-export`.
    pub rule: &'static str,
    /// The arguments after the command that the finding needs.
    pub args: &'static [&'static str],
    /// Where the finding is in its file.
    pub anchor: ExampleAnchor,
    /// The project before the change. Empty unless the rule reports on a change.
    pub base: &'static [ExampleFile],
    /// The project. For a rule that reports on a change, the files after it.
    pub files: &'static [ExampleFile],
}

/// Every example project, in order of rule id.
#[must_use]
pub fn rule_examples() -> &'static [RuleExample] {
    table::RULE_EXAMPLES
}

/// The example project of `rule`. `None` when the rule needs data that a
/// small project does not have, such as production runtime data.
#[must_use]
pub fn rule_example(rule: &RuleDef) -> Option<&'static RuleExample> {
    let bare = bare_rule_id(rule);
    rule_examples().iter().find(|example| example.rule == bare)
}

fn visible(files: &[ExampleFile]) -> Vec<fallow_output::ExplainExampleFile> {
    files
        .iter()
        .filter(|file| !file.hidden)
        .map(|file| fallow_output::ExplainExampleFile {
            path: file.path.to_string(),
            code: file.code.to_string(),
        })
        .collect()
}

/// The visible files of the example of `rule`: the project, then the project
/// before the change. Both are empty when the rule has no example.
pub(crate) fn visible_example_files(
    rule: &RuleDef,
) -> (
    Vec<fallow_output::ExplainExampleFile>,
    Vec<fallow_output::ExplainExampleFile>,
) {
    rule_example(rule).map_or_else(
        || (Vec::new(), Vec::new()),
        |example| (visible(example.files), visible(example.base)),
    )
}

#[cfg(test)]
#[path = "explain_examples_tests.rs"]
mod tests;
