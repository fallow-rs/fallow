//! The parsed modules that typed tool calls share across calls.
//!
//! Each typed tool call builds a new analysis session. The server installs one
//! store of parsed modules for the process, so a call on an unchanged project
//! takes its modules from memory and does no parse work. The CLI subprocess
//! calls and the subprocess-backed Code Mode calls run in their own process
//! and do not use the store.

use std::ffi::OsString;
use std::sync::Arc;

use fallow_api::warm_parse::{self, WarmParseLimits, WarmParseStore};

/// Set this variable to `0`, `false`, `off` or `no` to turn the store off.
pub const WARM_SESSION_ENV: &str = "FALLOW_MCP_WARM_SESSION";

/// Install the store unless [`WARM_SESSION_ENV`] turns it off.
pub fn install_from_env() {
    install(enabled_from(|name| std::env::var_os(name)));
}

fn install(enabled: bool) {
    warm_parse::install(enabled.then(|| Arc::new(WarmParseStore::new(WarmParseLimits::default()))));
}

/// Whether the store is on, from a variable lookup. An unset or empty value
/// keeps it on.
fn enabled_from(lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    let Some(value) = lookup(WARM_SESSION_ENV) else {
        return true;
    };
    let value = value.to_string_lossy().trim().to_ascii_lowercase();
    !matches!(value.as_str(), "0" | "false" | "off" | "no")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_is_on_unless_the_variable_turns_it_off() {
        let with = |value: Option<&str>| {
            enabled_from(|name| {
                assert_eq!(name, WARM_SESSION_ENV);
                value.map(OsString::from)
            })
        };

        assert!(with(None));
        assert!(with(Some("")));
        assert!(with(Some("1")));
        assert!(with(Some("on")));
        for off in ["0", "false", "FALSE", " off ", "no"] {
            assert!(!with(Some(off)), "`{off}` turns the store off");
        }
    }
}
