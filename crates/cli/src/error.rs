use std::process::ExitCode;

use fallow_config::OutputFormat;

/// Emit an error as structured JSON on stdout when `--format json` is active,
/// then return the given exit code. For non-JSON formats, emit to stderr as usual.
pub fn emit_error(message: &str, exit_code: u8, output: OutputFormat) -> ExitCode {
    emit_error_with_style(message, exit_code, output, requested_json_style())
}

pub fn requested_json_style() -> crate::json_style::JsonStyle {
    if std::env::args_os().any(|arg| arg == "--pretty") {
        crate::json_style::JsonStyle::Pretty
    } else {
        crate::json_style::JsonStyle::Compact
    }
}

/// Emit a structured API failure without flattening it into a message string.
///
/// A [`fallow_api::ProgrammaticError`] already carries the stable `code` and
/// the remediation `help` an agent needs; routing it through [`emit_error`]
/// dropped both and left the caller parsing prose. JSON callers get all three
/// fields, human callers get the message with the hint appended. The style is
/// a parameter rather than sniffed from argv because every programmatic-error
/// call site already has it in hand.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "structured error emission for CLI surfaces"
)]
pub fn emit_programmatic_error(
    error: &fallow_api::ProgrammaticError,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    if matches!(output, OutputFormat::Json) {
        let error_obj = fallow_output::ErrorOutput::new(&error.message, error.exit_code)
            .with_code(error.code.clone())
            .with_help(error.help.clone());
        if let Ok(json) = json_style.serialize(&error_obj) {
            println!("{json}");
        }
    } else if let Some(help) = &error.help {
        eprintln!("Error: {}\n  hint: {help}", error.message);
    } else {
        eprintln!("Error: {}", error.message);
    }
    ExitCode::from(error.exit_code)
}

/// Emit a failure together with the remedy for it.
///
/// The same shape [`emit_programmatic_error`] uses, for the call sites that
/// have a hint in hand but no [`fallow_api::ProgrammaticError`]: JSON callers
/// read the remedy off `help`, human callers get it on its own `hint:` line
/// instead of one long sentence that soft-wraps.
#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "structured error emission for CLI surfaces"
)]
pub fn emit_error_with_hint(
    message: &str,
    hint: &str,
    exit_code: u8,
    output: OutputFormat,
) -> ExitCode {
    if matches!(output, OutputFormat::Json) {
        let error_obj =
            fallow_output::ErrorOutput::new(message, exit_code).with_help(Some(hint.to_string()));
        if let Ok(json) = requested_json_style().serialize(&error_obj) {
            println!("{json}");
        }
    } else {
        eprintln!("Error: {message}\n  hint: {hint}");
    }
    ExitCode::from(exit_code)
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
