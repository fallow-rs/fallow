//! Single source of truth for the agent-discoverability task-to-command matrix
//! (R2/R3). One const slice drives every render surface: the `fallow schema`
//! manifest (`task_matrix`), the `init --agents` AGENTS.md template, the
//! `hooks install --target agent` managed block, the root `--help` cheat
//! sheet, and the `fallow://task-matrix` MCP resource. The
//! `scripts/generate-agent-docs.mjs` generator renders the same table into
//! SKILL.md from the schema-serialized form, so the Markdown surfaces stay
//! consistent without duplicating the rows.
//!
//! This module carries data only. The Markdown renderer and the clap probe
//! drift test live in `crates/cli`, which owns the command tree; the MCP
//! server projects the rows without `probe`.
//!
//! Read-only-evidence principle (R1): the matrix carries NO mutating commands
//! (`fix`, `init`, `hooks`, `migrate`, `setup-hooks`, `watch`). Unit tests in
//! this crate and in `crates/cli` pin that contract, mirroring the
//! `next_steps[]` builder in the CLI report layer.

/// One task-to-command row for the agent-discoverability cheat sheet (R2/R3).
///
/// `command` MAY contain `<placeholder>` or glob tokens because it renders
/// into docs and help text, unlike the runnable-only `next_steps[]` contract.
/// `probe` is the runnable clap token sequence (placeholders and values
/// replaced with concrete dummies) that the CLI schema drift test parses
/// through `Cli::try_parse_from`, so a row can never name a flag or subcommand
/// that does not exist. A row whose command is a bare flag fragment (no
/// leading subcommand) carries an empty `probe`; the drift test skips it and a
/// dedicated test asserts the flags exist on the live global arg set instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRow {
    /// The agent intent, phrased as "when the agent is about to ...".
    pub task: &'static str,
    /// The command to run, render-ready (may contain `<placeholder>` tokens).
    pub command: &'static str,
    /// Optional clarifying note appended in parentheses in the rendered table.
    pub note: Option<&'static str>,
    /// Runnable clap token sequence the CLI drift test parses, or empty for a
    /// flag-fragment row that is covered by the global-flag existence test.
    pub probe: &'static [&'static str],
}

/// The canonical task-to-command matrix. Verified against the live clap
/// command tree; the CLI schema drift test re-checks every non-empty `probe`.
pub const TASK_MATRIX: &[TaskRow] = &[
    TaskRow {
        task: "delete an \"unused\" export or file",
        command: "fallow dead-code --trace <file>:<export>",
        note: None,
        probe: &["dead-code", "--trace", "src/index.ts:foo"],
    },
    TaskRow {
        task: "prove a TypeScript symbol's exact consumers before refactoring",
        command: "fallow dead-code --type-aware --symbol-impact <file>:<export-or-class.method>",
        note: None,
        probe: &[
            "dead-code",
            "--type-aware",
            "--symbol-impact",
            "src/index.ts:foo",
        ],
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
        // `task_matrix_workspace_flags_are_global` in the CLI schema tests.
        probe: &[],
    },
];

impl TaskRow {
    /// The `fallow schema` `task_matrix` row: `task`, `command`, and `note`
    /// (`null` when absent, honoring the manifest's no-absent-key convention).
    /// `probe` is a test-only concern and never serializes.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task": self.task,
            "command": self.command,
            "note": self.note,
        })
    }
}

/// Mutating command tokens the matrix must never reference (R1 read-only
/// principle). Shared with the CLI schema exclusion test.
pub const MUTATING_COMMANDS: &[&str] =
    &["agent", "fix", "init", "hooks", "migrate", "setup-hooks", "watch"];

/// The first command token after the `fallow` prefix, or the empty string for
/// a bare flag-fragment row.
#[must_use]
pub fn leading_command_token(row: &TaskRow) -> &'static str {
    let after_fallow = row.command.strip_prefix("fallow ").unwrap_or(row.command);
    after_fallow.split_whitespace().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_non_empty() {
        assert!(!TASK_MATRIX.is_empty());
    }

    /// Read-only-evidence contract (R1): no row may name a mutating command.
    #[test]
    fn matrix_excludes_mutating_commands() {
        for row in TASK_MATRIX {
            let first_token = leading_command_token(row);
            assert!(
                !MUTATING_COMMANDS.contains(&first_token),
                "task matrix row '{}' names mutating command '{first_token}'",
                row.task
            );
        }
    }

    #[test]
    fn to_json_omits_probe_and_keeps_note_key() {
        let row = TaskRow {
            task: "t",
            command: "fallow flags",
            note: None,
            probe: &["flags"],
        };
        let value = row.to_json();
        assert_eq!(value["task"], "t");
        assert_eq!(value["command"], "fallow flags");
        assert!(value["note"].is_null());
        assert!(value.get("probe").is_none());
    }

    #[test]
    fn tasks_are_unique() {
        let mut tasks: Vec<&str> = TASK_MATRIX.iter().map(|row| row.task).collect();
        let total = tasks.len();
        tasks.sort_unstable();
        tasks.dedup();
        assert_eq!(tasks.len(), total, "duplicate task in TASK_MATRIX");
    }

    #[test]
    fn leading_token_skips_the_fallow_prefix() {
        let row = TaskRow {
            task: "t",
            command: "fallow audit --base main",
            note: None,
            probe: &[],
        };
        assert_eq!(leading_command_token(&row), "audit");
        let fragment = TaskRow {
            task: "t",
            command: "--workspace <glob>",
            note: None,
            probe: &[],
        };
        assert_eq!(leading_command_token(&fragment), "--workspace");
    }
}
