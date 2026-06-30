use std::process::ExitCode;

use fallow_types::output_format::OutputFormat;

/// Emit an error as structured JSON on stdout when `--format json` is active,
/// then return the given exit code. For non-JSON formats, emit to stderr as usual.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "structured error emission for CLI surfaces, preserved verbatim from the CLI error module"
)]
pub fn emit_error(message: &str, exit_code: u8, output: OutputFormat) -> ExitCode {
    if matches!(output, OutputFormat::Json) {
        let error_obj = serde_json::json!({
            "error": true,
            "message": message,
            "exit_code": exit_code,
        });
        if let Ok(json) = serde_json::to_string_pretty(&error_obj) {
            println!("{json}");
        }
    } else {
        eprintln!("Error: {message}");
    }
    ExitCode::from(exit_code)
}
