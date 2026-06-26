//! Trace command output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use serde::Serialize;

/// Serialize the `fallow trace --format json` envelope.
///
/// # Errors
///
/// Returns a serde error when the trace output cannot be converted to JSON.
pub fn serialize_trace_json_output<T: Serialize>(
    output: T,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "trace", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trace_json_output_uses_output_owned_root_contract() {
        let value = serialize_trace_json_output(
            json!({"file": "src/app.ts", "symbol": "run"}),
            RootEnvelopeMode::Tagged,
            Some("run-trace"),
        )
        .expect("trace output should serialize");

        assert_eq!(value["kind"], "trace");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-trace");
    }
}
