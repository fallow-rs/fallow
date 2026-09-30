//! Cross-surface strings for the cloud runtime-coverage path.
//!
//! The cloud commands of `fallow coverage` and the matching MCP cloud tools
//! refuse the same call for the same reason, and an agent reads both.
//! Keeping the refusal text here makes the two surfaces say one sentence
//! instead of holding two copies that drift apart.

/// A cloud command that needs a Fallow Cloud API key. The refusal names the
/// command that ran, so the example line can be copied as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudCommand {
    /// `fallow coverage analyze --cloud`, and the MCP `get_cloud_runtime_context` tool.
    Analyze,
    /// `fallow coverage review-packet`, and the MCP `get_cloud_review_packet` tool.
    ReviewPacket,
    /// `fallow coverage deployment-changes`, and the MCP `get_cloud_deployment_changes` tool.
    DeploymentChanges,
}

impl CloudCommand {
    /// A minimal command line for this command, without the API key.
    #[must_use]
    pub const fn example(self) -> &'static str {
        match self {
            Self::Analyze => "fallow coverage analyze --cloud --repo owner/repo",
            Self::ReviewPacket => "fallow coverage review-packet --repo owner/repo",
            Self::DeploymentChanges => "fallow coverage deployment-changes --repo owner/repo",
        }
    }
}

/// Refusal emitted when no Fallow Cloud API key is available. Names the
/// environment variable first because that is the only route the MCP tools
/// have; the flag and the command line of `command` follow for the CLI surface.
#[must_use]
pub fn cloud_api_key_missing_message(command: CloudCommand) -> String {
    format!(
        "Cloud runtime coverage requires an API key.\n\nSet FALLOW_API_KEY or pass --api-key:\n\n  FALLOW_API_KEY=fallow_live_... {}",
        command.example()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_command_names_itself_in_the_example() {
        for (command, subcommand) in [
            (CloudCommand::Analyze, "coverage analyze --cloud"),
            (CloudCommand::ReviewPacket, "coverage review-packet"),
            (
                CloudCommand::DeploymentChanges,
                "coverage deployment-changes",
            ),
        ] {
            let message = cloud_api_key_missing_message(command);
            assert!(message.starts_with("Cloud runtime coverage requires an API key."));
            assert!(
                message.ends_with(&format!(
                    "fallow_live_... fallow {subcommand} --repo owner/repo"
                )),
                "{command:?}: {message}"
            );
        }
    }
}
