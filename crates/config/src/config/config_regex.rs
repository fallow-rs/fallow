//! Compile regexes that come from project config or plugin config.

/// The largest compiled program of one config regex, in bytes. The search
/// time of a regex grows with its program, and a short pattern such as
/// `(?:\B|a){40000}` can fill the 10 MiB default of the `regex` crate.
const CONFIG_REGEX_SIZE_LIMIT: usize = 1024 * 1024;

/// The largest lazy DFA cache of one config regex, in bytes.
const CONFIG_REGEX_DFA_SIZE_LIMIT: usize = 1024 * 1024;

/// Compile a regex from project or plugin config with
/// [`CONFIG_REGEX_SIZE_LIMIT`] on its compiled program.
///
/// # Errors
///
/// Returns the `regex` error for a pattern that is not valid or whose
/// compiled program is larger than the limit.
pub fn compile_config_regex(pattern: &str) -> Result<regex::Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .size_limit(CONFIG_REGEX_SIZE_LIMIT)
        .dfa_size_limit(CONFIG_REGEX_DFA_SIZE_LIMIT)
        .build()
}

/// The most regexes that one config list, such as
/// `health.tailwindIgnorePatterns`, can hold. Each regex keeps its compiled
/// program in memory for the full run.
pub(super) const MAX_CONFIG_REGEX_LIST: usize = 128;

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::FallowConfig;

    /// The wall-clock bound of one config load in a debug build.
    const LOAD_BOUND: Duration = Duration::from_secs(3);

    /// A short pattern whose compiled program fills the default limit of the
    /// `regex` crate. The search time grows with the program size.
    const LARGE_PROGRAM_REGEX: &str = r"(?:\B|\B|\B|\B|a){40000}$";

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn config_regex_with_a_large_program_is_a_config_error() {
        for config in [
            serde_json::json!({ "unusedComponentProps": { "ignorePattern": LARGE_PROGRAM_REGEX } }),
            serde_json::json!({ "health": { "tailwindIgnorePatterns": [LARGE_PROGRAM_REGEX] } }),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(".fallowrc.json");
            std::fs::write(&path, config.to_string()).unwrap();

            let start = Instant::now();
            let error = FallowConfig::load(&path)
                .expect_err("large regex refused")
                .to_string();

            assert!(error.contains("size limit"), "{error}");
            assert!(start.elapsed() < LOAD_BOUND);
        }
    }

    #[test]
    fn config_regex_helper_keeps_ordinary_patterns() {
        for pattern in [
            r"^on[A-Z]",
            r"^bg-(red|blue)-\d{3}$",
            r"\w{1,16}",
            r"[A-Za-z0-9_]{1,64}",
        ] {
            assert!(super::compile_config_regex(pattern).is_ok(), "{pattern}");
        }
        assert!(super::compile_config_regex(LARGE_PROGRAM_REGEX).is_err());
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn tailwind_ignore_pattern_list_above_the_limit_is_a_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".fallowrc.json");
        let patterns: Vec<String> = (0..=super::MAX_CONFIG_REGEX_LIST)
            .map(|index| format!("^p{index}-"))
            .collect();
        let config = serde_json::json!({ "health": { "tailwindIgnorePatterns": patterns } });
        std::fs::write(&path, config.to_string()).unwrap();

        let error = FallowConfig::load(&path)
            .expect_err("too many patterns")
            .to_string();

        assert!(error.contains("tailwindIgnorePatterns lists"), "{error}");
    }
}
