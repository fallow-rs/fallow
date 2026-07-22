//! Typed health-pipeline error.

/// A health-pipeline failure.
///
/// `Message` carries a CLI-facing message the CLI boundary renders; `Printed`
/// marks an error a lower layer (the runtime-coverage seam) already printed, so
/// the boundary must NOT re-print it, only honor its exit code.
#[derive(Debug, Clone)]
pub enum HealthError {
    /// A failure the CLI boundary still needs to render.
    Message {
        /// CLI-facing error message.
        message: String,
        /// Exit code this failure terminates with.
        exit_code: u8,
    },
    /// Already emitted by a lower layer; carry only the exit code.
    Printed(u8),
}

impl HealthError {
    /// Build a [`HealthError::Message`] from a message and exit code.
    #[must_use]
    pub(crate) fn message(message: impl Into<String>, exit_code: u8) -> Self {
        Self::Message {
            message: message.into(),
            exit_code,
        }
    }

    /// The exit code this failure terminates with.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Message { exit_code, .. } => *exit_code,
            Self::Printed(code) => *code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_carries_text_and_code() {
        let err = HealthError::message("boom", 2);
        assert_eq!(err.exit_code(), 2);
        match err {
            HealthError::Message { message, exit_code } => {
                assert_eq!(message, "boom");
                assert_eq!(exit_code, 2);
            }
            HealthError::Printed(_) => panic!("expected Message variant"),
        }
    }

    #[test]
    fn printed_carries_only_code() {
        let err = HealthError::Printed(5);
        assert_eq!(err.exit_code(), 5);
        assert!(matches!(err, HealthError::Printed(5)));
    }
}
