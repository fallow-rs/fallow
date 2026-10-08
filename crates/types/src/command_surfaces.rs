//! One row per analysis subcommand: the JSON kinds it writes and the surfaces
//! that must know about it.
//!
//! A new analysis command must reach the root `FallowOutput` schema, `fallow
//! report --from`, the MCP server and the drift harness. Each of those
//! surfaces has a test that reads [`COMMAND_ENVELOPES`], so a command that is
//! added to the CLI without a row, or a row without a surface, fails a test
//! instead of shipping a silent gap. Every other visible subcommand is listed
//! in [`COMMANDS_WITHOUT_ANALYSIS_ENVELOPE`] with the reason it has no row.

/// How `fallow report --from` treats a saved envelope of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFrom {
    /// `fallow report --from` renders the saved envelope, flat and grouped.
    Renders,
    /// `fallow report --from` refuses the saved envelope, for this reason.
    Refused(&'static str),
}

/// The machine contract of one analysis subcommand.
#[derive(Debug, Clone, Copy)]
pub struct CommandEnvelope {
    /// The subcommand, as `fallow <command>` spells it.
    pub command: &'static str,
    /// The root `kind` of `fallow <command> --format json`.
    pub kind: &'static str,
    /// The root `kind` with `--group-by`, when grouping writes another kind.
    pub grouped_kind: Option<&'static str>,
    /// How `fallow report --from` treats a saved envelope.
    pub report_from: ReportFrom,
    /// The MCP tool whose CLI analogue is this command.
    pub mcp_tool: Option<&'static str>,
    /// Why no MCP tool exists. `Some` exactly when `mcp_tool` is `None`.
    pub mcp_omission: Option<&'static str>,
    /// Whether the drift harness compares the verdict of the JSON and human
    /// runs (invariant I7).
    pub verdict: bool,
}

/// Every analysis subcommand with its JSON kinds and surfaces.
pub const COMMAND_ENVELOPES: &[CommandEnvelope] = &[
    CommandEnvelope {
        command: "dead-code",
        kind: "dead-code",
        grouped_kind: Some("dead-code-grouped"),
        report_from: ReportFrom::Renders,
        mcp_tool: Some("analyze"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "architecture",
        kind: "architecture",
        grouped_kind: Some("architecture-grouped"),
        report_from: ReportFrom::Renders,
        mcp_tool: Some("check_architecture"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "dupes",
        kind: "dupes",
        grouped_kind: None,
        report_from: ReportFrom::Renders,
        mcp_tool: Some("find_dupes"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "health",
        kind: "health",
        grouped_kind: None,
        report_from: ReportFrom::Renders,
        mcp_tool: Some("check_health"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "security",
        kind: "security",
        grouped_kind: None,
        report_from: ReportFrom::Renders,
        mcp_tool: Some("security_candidates"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "audit",
        kind: "audit",
        grouped_kind: None,
        report_from: ReportFrom::Renders,
        mcp_tool: Some("audit"),
        mcp_omission: None,
        verdict: true,
    },
    CommandEnvelope {
        command: "flags",
        kind: "feature-flags",
        grouped_kind: None,
        report_from: ReportFrom::Refused(
            "feature flags are an inventory with no CI annotation or review surface",
        ),
        mcp_tool: Some("feature_flags"),
        mcp_omission: None,
        verdict: false,
    },
    CommandEnvelope {
        command: "similar-code",
        kind: "similar-code",
        grouped_kind: None,
        report_from: ReportFrom::Refused(
            "similar-code candidates are unverified and never feed a CI surface",
        ),
        mcp_tool: Some("find_similar_code"),
        mcp_omission: None,
        verdict: false,
    },
];

/// Visible subcommands that write no analysis report, with the reason.
pub const COMMANDS_WITHOUT_ANALYSIS_ENVELOPE: &[(&str, &str)] = &[
    (
        "guard",
        "per-file rule lookup before an edit; it reports rules, not findings",
    ),
    ("watch", "interactive loop that prints human output only"),
    (
        "fix",
        "applies fixes; `report --from` detects its kind-less envelope by its fields",
    ),
    (
        "list",
        "project inspection; writes `list-boundaries` or `list-workspaces`",
    ),
    ("inspect", "evidence bundle for one target"),
    ("trace", "call-chain and import-path trace for one symbol"),
    ("trace-error", "stack-trace frame resolution"),
    (
        "decision-surface",
        "advisory review signals of a change, not findings",
    ),
    ("workspaces", "workspace discovery diagnostics"),
    ("explain", "static issue-type documentation"),
    ("suppressions", "inventory of suppression markers"),
    ("impact", "local impact digest of fallow itself"),
    ("viz", "writes an HTML map"),
    ("doctor", "project readiness checks"),
    ("init", "writes a config file"),
    ("agent", "wires fallow into agent tools"),
    ("audit-cache", "maintains audit base-snapshot caches"),
    ("recommend", "config recommendation for an agent"),
    ("migrate", "config migration"),
    ("config", "resolved config"),
    ("config-schema", "JSON Schema of the config"),
    ("plugin-schema", "JSON Schema of external plugins"),
    ("plugin-check", "external plugin dry run"),
    ("rule-pack", "rule-pack management"),
    ("rule-pack-schema", "JSON Schema of rule packs"),
    ("type-aware", "semantic companion status"),
    ("ci", "builds PR and MR feedback from a saved envelope"),
    ("ci-template", "CI template output"),
    ("report", "renders a saved envelope"),
    ("hooks", "Git and agent hook management"),
    ("setup-hooks", "deprecated alias of hook installation"),
    ("coverage", "runtime coverage setup and analysis"),
    ("license", "license management"),
    ("telemetry", "telemetry settings"),
    ("schema", "capability manifest"),
];

/// The row of `command`, when it is an analysis subcommand.
#[must_use]
pub fn command_envelope(command: &str) -> Option<&'static CommandEnvelope> {
    COMMAND_ENVELOPES.iter().find(|row| row.command == command)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn rows_name_each_command_and_kind_once() {
        let commands: BTreeSet<&str> = COMMAND_ENVELOPES.iter().map(|row| row.command).collect();
        assert_eq!(commands.len(), COMMAND_ENVELOPES.len());
        let kinds: Vec<&str> = COMMAND_ENVELOPES
            .iter()
            .flat_map(|row| std::iter::once(row.kind).chain(row.grouped_kind))
            .collect();
        let unique: BTreeSet<&str> = kinds.iter().copied().collect();
        assert_eq!(unique.len(), kinds.len(), "a kind is in two rows");
        for (command, _) in COMMANDS_WITHOUT_ANALYSIS_ENVELOPE {
            assert!(
                command_envelope(command).is_none(),
                "{command} is in both lists"
            );
        }
    }

    #[test]
    fn every_row_has_an_mcp_tool_or_a_reason() {
        for row in COMMAND_ENVELOPES {
            assert_eq!(
                row.mcp_tool.is_some(),
                row.mcp_omission.is_none(),
                "{}: set exactly one of mcp_tool and mcp_omission",
                row.command
            );
            if let Some(tool) = row.mcp_tool {
                let info = crate::mcp_manifest::MCP_TOOLS
                    .iter()
                    .find(|info| info.name == tool)
                    .unwrap_or_else(|| panic!("{}: no MCP tool `{tool}`", row.command));
                let expected = format!("fallow {} ", row.command);
                assert!(
                    info.cli_command
                        .is_some_and(|cli| cli.starts_with(expected.as_str())),
                    "{}: MCP tool `{tool}` names another CLI analogue: {:?}",
                    row.command,
                    info.cli_command
                );
            }
        }
    }
}
