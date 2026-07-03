//! Single source of truth for the agent-discoverability task-to-command matrix
//! (R2/R3). One const slice drives four render surfaces: the `fallow schema`
//! manifest (`task_matrix`), the `init --agents` AGENTS.md template, the
//! `hooks install --target agent` managed block, and the root `--help` cheat
//! sheet. The `scripts/generate-agent-docs.mjs` generator renders the same
//! table into SKILL.md from the schema-serialized form, so the Markdown
//! surfaces stay consistent without duplicating the rows.
//!
//! Read-only-evidence principle (R1): the matrix carries NO mutating commands
//! (`fix`, `init`, `hooks`, `migrate`, `setup-hooks`, `watch`). A unit test
//! pins that contract, mirroring the `next_steps[]` builder in
//! `report/suggestions.rs`.

/// One task-to-command row for the agent-discoverability cheat sheet (R2/R3).
///
/// `command` MAY contain `<placeholder>` or glob tokens because it renders
/// into docs and help text, unlike the runnable-only `next_steps[]` contract.
/// `probe` is the runnable clap token sequence (placeholders and values
/// replaced with concrete dummies) that the schema drift test parses through
/// `Cli::try_parse_from`, so a row can never name a flag or subcommand that
/// does not exist. A row whose command is a bare flag fragment (no leading
/// subcommand) carries an empty `probe`; the drift test skips it and a
/// dedicated test asserts the flags exist on the live global arg set instead.
pub struct TaskRow {
    /// The agent intent, phrased as "when the agent is about to ...".
    pub task: &'static str,
    /// The command to run, render-ready (may contain `<placeholder>` tokens).
    pub command: &'static str,
    /// Optional clarifying note appended in parentheses in the rendered table.
    pub note: Option<&'static str>,
    /// Runnable clap token sequence the drift test parses, or empty for a
    /// flag-fragment row that is covered by the global-flag existence test.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "read only by the schema drift tests via try_parse_from"
        )
    )]
    pub probe: &'static [&'static str],
}

/// The canonical task-to-command matrix. Verified against the live clap
/// command tree; the schema drift test re-checks every non-empty `probe`.
pub const TASK_MATRIX: &[TaskRow] = &[
    TaskRow {
        task: "delete an \"unused\" export or file",
        command: "fallow dead-code --trace <file>:<export>",
        note: None,
        probe: &["dead-code", "--trace", "src/index.ts:foo"],
    },
    TaskRow {
        task: "delete an \"unused\" dependency",
        command: "fallow dead-code --trace-dependency <name>",
        note: None,
        probe: &["dead-code", "--trace-dependency", "lodash"],
    },
    TaskRow {
        task: "commit or open a PR",
        command: "fallow audit --base <ref>",
        note: None,
        probe: &["audit", "--base", "main"],
    },
    TaskRow {
        task: "prioritize refactoring",
        command: "fallow health --hotspots --targets",
        note: None,
        probe: &["health", "--hotspots", "--targets"],
    },
    TaskRow {
        task: "ask who owns code",
        command: "fallow health --ownership",
        note: None,
        probe: &["health", "--ownership"],
    },
    TaskRow {
        task: "check untested-but-reachable code",
        command: "fallow health --coverage-gaps",
        note: None,
        probe: &["health", "--coverage-gaps"],
    },
    TaskRow {
        task: "consolidate duplication",
        command: "fallow dupes --trace dup:<fingerprint>",
        note: None,
        probe: &["dupes", "--trace", "dup:abc123"],
    },
    TaskRow {
        task: "find feature flags",
        command: "fallow flags",
        note: None,
        probe: &["flags"],
    },
    TaskRow {
        task: "check which architecture rules apply to a file before changing it",
        command: "fallow guard <files>",
        note: None,
        probe: &["guard", "src/index.ts"],
    },
    TaskRow {
        task: "surface security candidates",
        command: "fallow security",
        note: None,
        probe: &["security"],
    },
    TaskRow {
        task: "understand a finding",
        command: "fallow explain <issue-type>",
        note: None,
        probe: &["explain", "unused-export"],
    },
    TaskRow {
        task: "scope a monorepo",
        command: "--workspace <glob> / --changed-workspaces <ref>",
        note: Some("global flags, prefix any command"),
        // Flag-fragment row: no leading subcommand. Covered by
        // `task_matrix_workspace_flags_are_global` in schema.rs instead.
        probe: &[],
    },
];

/// Mutating command tokens the matrix must never reference (R1 read-only
/// principle). Shared with the schema exclusion test.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "read only by the matrix exclusion tests in this crate"
    )
)]
pub const MUTATING_COMMANDS: &[&str] = &["fix", "init", "hooks", "migrate", "setup-hooks", "watch"];

/// Render the task-to-command matrix as a Markdown table. Used by the
/// `init --agents` template and the `hooks install --target agent` managed
/// block so the two surfaces never drift; the `.mjs` generator emits the same
/// shape into SKILL.md from the schema JSON.
#[must_use]
pub fn render_task_matrix_markdown() -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(1024);
    out.push_str("| When the agent is about to... | Run |\n");
    out.push_str("|---|---|\n");
    for row in TASK_MATRIX {
        let suffix = match row.note {
            Some(note) => format!(" ({note})"),
            None => String::new(),
        };
        // Writing to a String is infallible.
        let _ = writeln!(out, "| {} | `{}`{suffix} |", row.task, row.command);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_non_empty() {
        assert!(!TASK_MATRIX.is_empty());
    }

    #[test]
    fn render_contains_every_command() {
        let table = render_task_matrix_markdown();
        assert!(table.contains("When the agent is about to..."));
        for row in TASK_MATRIX {
            assert!(
                table.contains(row.command),
                "rendered table missing command {}",
                row.command
            );
        }
    }

    /// Read-only-evidence contract (R1): no row may name a mutating command.
    #[test]
    fn matrix_excludes_mutating_commands() {
        for row in TASK_MATRIX {
            let after_fallow = row.command.strip_prefix("fallow ").unwrap_or(row.command);
            let first_token = after_fallow.split_whitespace().next().unwrap_or("");
            assert!(
                !MUTATING_COMMANDS.contains(&first_token),
                "task matrix row '{}' names mutating command '{first_token}'",
                row.task
            );
        }
    }
}
