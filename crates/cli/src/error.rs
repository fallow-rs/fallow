use std::process::ExitCode;

use fallow_config::OutputFormat;

/// Emit an error as structured JSON on stdout when `--format json` is active,
/// then return the given exit code. For non-JSON formats, emit to stderr as usual.
pub fn emit_error(message: &str, exit_code: u8, output: OutputFormat) -> ExitCode {
    emit_error_with_style(message, exit_code, output, requested_json_style())
}

fn requested_json_style() -> crate::json_style::JsonStyle {
    if std::env::args_os().any(|arg| arg == "--pretty") {
        crate::json_style::JsonStyle::Pretty
    } else {
        crate::json_style::JsonStyle::Compact
    }
}

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "structured error emission for CLI surfaces"
)]
pub fn emit_error_with_style(
    message: &str,
    exit_code: u8,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    if matches!(output, OutputFormat::Json) {
        let error_obj = fallow_output::ErrorOutput::new(message, exit_code);
        if let Ok(json) = json_style.serialize(&error_obj) {
            println!("{json}");
        }
    } else {
        eprintln!("Error: {message}");
    }
    ExitCode::from(exit_code)
}
