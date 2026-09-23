/// Error code for a configuration error.
const CONFIG_ERROR_CODE: &str = "E004";

/// Errors that can occur during analysis.
///
/// Analysis reports only configuration errors through this type. The
/// `Display` output is `error[E004]: Configuration error: <message>`.
#[derive(Debug)]
pub struct FallowError {
    /// The configuration error message.
    message: String,
}

#[expect(
    clippy::unused_self,
    clippy::unnecessary_wraps,
    reason = "the getters keep the method shape that callers of the public analyze functions use"
)]
impl FallowError {
    /// Create a config error with the `E004` error code.
    pub fn config(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the error code.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        Some(CONFIG_ERROR_CODE)
    }

    /// Returns the help text. Configuration errors have no help text.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        None
    }

    /// Returns the context string. Configuration errors have no context.
    #[must_use]
    pub fn context(&self) -> Option<&str> {
        None
    }
}

impl std::fmt::Display for FallowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[{CONFIG_ERROR_CODE}]: Configuration error: {}",
            self.message
        )
    }
}

impl std::error::Error for FallowError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_display_is_stable() {
        let err = FallowError::config("invalid TOML");
        assert_eq!(
            err.to_string(),
            "error[E004]: Configuration error: invalid TOML"
        );
        assert_eq!(err.code(), Some("E004"));
        assert_eq!(err.help(), None);
        assert_eq!(err.context(), None);
    }
}
