//! The telemetry kill switches `DO_NOT_TRACK` and `FALLOW_TELEMETRY_DISABLED`.
//!
//! A kill switch must fail safe: a user who sets `DO_NOT_TRACK=enabled` or
//! `DO_NOT_TRACK=y` means "do not track". So any value that is not empty and
//! not an explicit off value counts as an opt-out. The CLI telemetry, the
//! update check and the MCP server read the switches through this module, so
//! all of them agree on one value.

/// The `consoledonottrack.com` convention.
pub const DO_NOT_TRACK_ENV: &str = "DO_NOT_TRACK";
/// The Fallow admin and fleet kill switch.
pub const TELEMETRY_DISABLED_ENV: &str = "FALLOW_TELEMETRY_DISABLED";

/// True when a kill switch with this value disables telemetry.
///
/// An empty value and the explicit off values `0`, `false`, `no` and `off`
/// (case-insensitive) leave telemetry alone. Every other value disables it.
#[must_use]
pub fn is_opt_out_value(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    !matches!(value.as_str(), "" | "0" | "false" | "no" | "off")
}

/// True when `DO_NOT_TRACK` or `FALLOW_TELEMETRY_DISABLED` disables telemetry
/// in this process.
#[must_use]
pub fn telemetry_opted_out() -> bool {
    opted_out_with(|name| std::env::var(name).ok())
}

/// [`telemetry_opted_out`] over an injected lookup, so a test does not change
/// the environment of the process.
#[must_use]
pub fn opted_out_with(lookup: impl Fn(&str) -> Option<String>) -> bool {
    [DO_NOT_TRACK_ENV, TELEMETRY_DISABLED_ENV]
        .into_iter()
        .any(|name| lookup(name).is_some_and(|value| is_opt_out_value(&value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_set_value_except_an_off_value_opts_out() {
        for value in [
            "1", "true", "yes", "on", "enabled", "y", "TRUE", " 1 ", "anything",
        ] {
            assert!(is_opt_out_value(value), "{value:?} must opt out");
        }
    }

    #[test]
    fn empty_and_off_values_keep_telemetry_alone() {
        for value in ["", " ", "0", "false", "FALSE", "no", "off", " Off "] {
            assert!(!is_opt_out_value(value), "{value:?} must not opt out");
        }
    }

    #[test]
    fn either_switch_opts_out() {
        let dnt = |name: &str| (name == DO_NOT_TRACK_ENV).then(|| "enabled".to_owned());
        let admin = |name: &str| (name == TELEMETRY_DISABLED_ENV).then(|| "y".to_owned());
        let off = |_: &str| Some("0".to_owned());
        assert!(opted_out_with(dnt));
        assert!(opted_out_with(admin));
        assert!(!opted_out_with(off));
        assert!(!opted_out_with(|_| None));
    }
}
