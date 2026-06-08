//! Test-only stub `fallow-cov` sidecar used by
//! `crates/cli/tests/runtime_coverage_tests.rs` to exercise the full
//! spawn/marshalling pipeline without depending on the closed-source sidecar.
//!
//! Gated behind the `test-sidecar-key` cargo feature; the `compile_error!` in
//! `crates/cli/src/health/coverage.rs` prevents this binary from shipping in
//! release builds.
//!
//! Reads a `fallow_cov_protocol::Request` from stdin and emits a
//! `fallow_cov_protocol::Response` on stdout (or exits with a specific code)
//! based on the `FALLOW_STUB_MODE` env var:
//!
//! - unset / `"ok"`: clean response, exit 0
//! - `"protocol-mismatch"`: response with `protocol_version = "99.0.0"`, exit 0
//! - `"exit-4"` / `"exit-5"` / `"exit-6"`: prints the mode as stderr, exits
//!   with that code
//! - `"malformed-stdout"`: writes non-JSON bytes, exit 0
//! - `"empty-stdout"`: writes nothing, exit 0
//! - `"enforce-license-gate"`: mirrors the paid-shape sidecar gate for tests
//! - `"security-hot"`: response with `src/sink.ts::render` as a hot path
//! - `"capture-quality-short"`: clean response with a short-window
//!   `capture_quality` (`lazy_parse_warning = true`), exit 0
//! - `"capture-quality-long"`: clean response with a long-window
//!   `capture_quality` (`lazy_parse_warning = false`), exit 0

#![expect(
    clippy::print_stderr,
    reason = "stub sidecar emits diagnostic lines on stderr for failure modes; tests assert against them"
)]

use std::io::{Read, Write};
use std::process::ExitCode;

use fallow_cov_protocol::{
    CaptureQuality, HotPath, PROTOCOL_VERSION, ReportVerdict, Request, Response, Summary,
};

fn main() -> ExitCode {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    let parsed: Option<Request> = serde_json::from_slice(&buf).ok();

    let mode = std::env::var("FALLOW_STUB_MODE").unwrap_or_default();
    match mode.as_str() {
        "" | "ok" => emit_clean_response(PROTOCOL_VERSION, None),
        "protocol-mismatch" => emit_clean_response("99.0.0", None),
        "capture-quality-short" => emit_clean_response(
            PROTOCOL_VERSION,
            Some(CaptureQuality {
                window_seconds: 720,
                instances_observed: 1,
                lazy_parse_warning: true,
                untracked_ratio_percent: 42.5,
            }),
        ),
        "capture-quality-long" => emit_clean_response(
            PROTOCOL_VERSION,
            Some(CaptureQuality {
                window_seconds: 7 * 24 * 3600,
                instances_observed: 4,
                lazy_parse_warning: false,
                untracked_ratio_percent: 3.1,
            }),
        ),
        "security-hot" => emit_security_hot_response(),
        "malformed-stdout" => emit_bytes(b"definitely not JSON\n"),
        "empty-stdout" => ExitCode::SUCCESS,
        "enforce-license-gate" => enforce_license_gate(parsed),
        "exit-4" => {
            eprintln!("stub sidecar: simulated protocol mismatch");
            ExitCode::from(4)
        }
        "exit-5" => {
            eprintln!("stub sidecar: simulated input parse error");
            ExitCode::from(5)
        }
        "exit-6" => {
            eprintln!("stub sidecar: simulated internal error");
            ExitCode::from(6)
        }
        other => {
            eprintln!("stub sidecar: unknown FALLOW_STUB_MODE={other}");
            ExitCode::from(2)
        }
    }
}

fn enforce_license_gate(request: Option<Request>) -> ExitCode {
    let Some(request) = request else {
        eprintln!("stub sidecar: failed to parse request");
        return ExitCode::from(5);
    };
    if request.license.jwt.trim().is_empty() && request.coverage_sources.len() != 1 {
        eprintln!("stub sidecar: continuous runtime monitoring requires a valid license or trial");
        return ExitCode::from(3);
    }
    emit_clean_response(PROTOCOL_VERSION, None)
}

fn emit_security_hot_response() -> ExitCode {
    let hot_path: HotPath = match serde_json::from_value(serde_json::json!({
        "id": "fallow:hot:test",
        "file": "src/sink.ts",
        "function": "render",
        "line": 2,
        "end_line": 4,
        "invocations": 250,
        "percentile": 100,
        "identity": null,
    })) {
        Ok(hot_path) => hot_path,
        Err(err) => {
            eprintln!("stub sidecar: failed to build hot path: {err}");
            return ExitCode::from(6);
        }
    };
    let response = Response {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        verdict: ReportVerdict::HotPathTouched,
        summary: Summary {
            functions_tracked: 1,
            functions_hit: 1,
            functions_unhit: 0,
            functions_untracked: 0,
            coverage_percent: 100.0,
            trace_count: 250,
            period_days: 7,
            deployments_seen: 1,
            capture_quality: None,
        },
        findings: Vec::new(),
        hot_paths: vec![hot_path],
        blast_radius: Vec::new(),
        importance: Vec::new(),
        watermark: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    match serde_json::to_vec(&response) {
        Ok(bytes) => emit_bytes(&bytes),
        Err(err) => {
            eprintln!("stub sidecar: failed to serialize response: {err}");
            ExitCode::from(6)
        }
    }
}

fn emit_clean_response(
    protocol_version: &str,
    capture_quality: Option<CaptureQuality>,
) -> ExitCode {
    let response = Response {
        protocol_version: protocol_version.to_owned(),
        verdict: ReportVerdict::Clean,
        summary: Summary {
            functions_tracked: 0,
            functions_hit: 0,
            functions_unhit: 0,
            functions_untracked: 0,
            coverage_percent: 0.0,
            trace_count: 0,
            period_days: 0,
            deployments_seen: 0,
            capture_quality,
        },
        findings: Vec::new(),
        hot_paths: Vec::new(),
        blast_radius: Vec::new(),
        importance: Vec::new(),
        watermark: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    match serde_json::to_vec(&response) {
        Ok(bytes) => emit_bytes(&bytes),
        Err(err) => {
            eprintln!("stub sidecar: failed to serialize response: {err}");
            ExitCode::from(6)
        }
    }
}

fn emit_bytes(bytes: &[u8]) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    if stdout.write_all(bytes).is_err() || stdout.flush().is_err() {
        return ExitCode::from(6);
    }
    ExitCode::SUCCESS
}
