//! Shared security JSON payload contracts for programmatic consumers.

use serde::Serialize;

/// Gate mode for `fallow security --gate <mode>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SecurityGateMode {
    /// Fail when the change introduces a new security-sink candidate on a
    /// changed line, not merely a sink in a changed file.
    New,
    /// Fail when a candidate becomes runtime-reachable from an entry point in
    /// head but the matching candidate was not runtime-reachable in base.
    NewlyReachable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_gate_mode_uses_kebab_case_wire_names() {
        let value = serde_json::to_value(SecurityGateMode::NewlyReachable)
            .expect("serialize security gate mode");
        assert_eq!(value, serde_json::json!("newly-reachable"));
    }
}
