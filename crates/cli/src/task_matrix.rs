//! CLI render surface for the agent-discoverability task-to-command matrix
//! (R2/R3). The row data lives in `fallow_types::task_matrix` so the MCP
//! server can project it as the `fallow://task-matrix` resource without a
//! dependency on this crate; this module keeps the Markdown renderer used by
//! the `init --agents` AGENTS.md template and the `hooks install --target
//! agent` managed block, plus the re-export the `fallow schema` manifest, the
//! root `--help` cheat sheet test, and the clap probe drift test read.

pub use fallow_types::task_matrix::TASK_MATRIX;

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
}
