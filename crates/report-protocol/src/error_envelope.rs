//! The structured error envelope emitted on stdout for `--format json`.

use serde::Serialize;

/// Structured JSON error that a tool writes to stdout when `--format json` is
/// active and a command fails. It has no `kind` discriminator. The required
/// `error: true` member separates it from a success envelope, which carries a
/// `kind`. The error document is one of the root branches of the output
/// schema. A consumer that passes `--format json` and sees a non-zero exit
/// code parses this shape from stdout.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ErrorOutput {
    /// Always `true`. The discriminator that separates an error document from a
    /// success envelope (which instead carries a `kind`).
    pub error: bool,
    /// Human-readable error message.
    pub message: String,
    /// The process exit code the CLI returns alongside this document.
    pub exit_code: u8,
    /// Stable machine-readable code such as `FALLOW_INVALID_COVERAGE_PATH`,
    /// when the failure has one. Present so an agent can branch on the reason
    /// without pattern-matching the human message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Remediation hint for the caller, when the failure has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl ErrorOutput {
    /// Build an error envelope for the given message and exit code.
    #[must_use]
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            error: true,
            message: message.into(),
            exit_code,
            code: None,
            help: None,
        }
    }

    /// Attach a stable machine-readable code.
    #[must_use]
    pub fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }

    /// Attach a remediation hint.
    #[must_use]
    pub fn with_help(mut self, help: Option<String>) -> Self {
        self.help = help;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorOutput;

    /// `code` and `help` are additive-optional: an envelope without them must
    /// serialize exactly as it did before the fields existed, so a consumer
    /// pinned to the old shape sees no new keys.
    #[test]
    fn absent_code_and_help_add_no_keys() {
        let json = serde_json::to_value(ErrorOutput::new("boom", 2)).expect("serializes");
        let object = json.as_object().expect("object");
        assert_eq!(object.len(), 3, "{json}");
        assert!(!object.contains_key("code"));
        assert!(!object.contains_key("help"));
    }

    #[test]
    fn code_and_help_reach_the_wire_when_present() {
        let json = serde_json::to_value(
            ErrorOutput::new("boom", 2)
                .with_code(Some("FALLOW_BOOM".to_owned()))
                .with_help(Some("try harder".to_owned())),
        )
        .expect("serializes");
        assert_eq!(json["code"], "FALLOW_BOOM");
        assert_eq!(json["help"], "try harder");
    }
}
