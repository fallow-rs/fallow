use std::path::Path;
use std::process::ExitCode;

use fallow_api::{DEFAULT_TYPE_AWARE_BACKEND_FAMILY, TypeAwareStatus};

use super::DispatchContext;
use crate::cli_args::TypeAwareCli;
use crate::error::emit_input_error;

#[path = "type_aware_setup/mod.rs"]
mod setup;

/// The word before the backend version. The default backend is TypeScript, and
/// each other backend has a version scheme of its own, so it keeps its name.
fn backend_label(family: Option<&str>) -> &str {
    match family {
        None | Some(DEFAULT_TYPE_AWARE_BACKEND_FAMILY) => "TypeScript",
        Some(family) => family,
    }
}

/// The message of the human `type-aware status` line for an available companion.
pub(super) fn available_message(status: &TypeAwareStatus) -> String {
    format!(
        "Type-aware companion: available ({}, protocol {}, {} {})",
        status.package_version.as_deref().unwrap_or("unknown"),
        status.protocol_version,
        backend_label(status.backend_family.as_deref()),
        status.backend_version.as_deref().unwrap_or("unknown"),
    )
}

pub(super) fn dispatch_type_aware_command(
    dispatch: &DispatchContext<'_>,
    subcommand: TypeAwareCli,
) -> ExitCode {
    match subcommand {
        TypeAwareCli::Setup { yes, from } => setup::run_setup(&setup::SetupInput {
            root: dispatch.root,
            yes,
            from: from.as_deref(),
            output: dispatch.output,
            json_style: dispatch.json_style,
        }),
        TypeAwareCli::Cleanup { yes } => setup::run_cleanup(&setup::CleanupInput {
            yes,
            output: dispatch.output,
            json_style: dispatch.json_style,
        }),
        TypeAwareCli::Status => {
            let status = fallow_api::type_aware_status(dispatch.root);
            match dispatch.output {
                fallow_config::OutputFormat::Json => emit_type_aware_status_json(
                    dispatch.root,
                    status,
                    dispatch.output,
                    dispatch.json_style,
                ),
                fallow_config::OutputFormat::Human => {
                    if status.available {
                        crate::report::sink::outln!(
                            "{}",
                            crate::report::human_status_line(
                                crate::report::HumanStatus::Ok,
                                available_message(&status),
                            )
                        );
                    } else {
                        crate::report::sink::outln!(
                            "{}",
                            crate::report::human_status_line(
                                crate::report::HumanStatus::Inactive,
                                "Type-aware companion: unavailable"
                            )
                        );
                        if let Some(remediation) = status.remediation {
                            crate::report::sink::outln!(
                                "{}",
                                crate::report::human_status_line(
                                    crate::report::HumanStatus::Warning,
                                    format_args!("Action: {remediation}")
                                )
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                _ => emit_input_error(
                    "type-aware status supports human and json output",
                    dispatch.output,
                ),
            }
        }
    }
}

/// Print the `type-aware-status` JSON envelope.
pub fn emit_type_aware_status_json(
    root: &Path,
    status: fallow_api::TypeAwareStatus,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let value = match fallow_output::serialize_type_aware_status_json_output(
        super::type_aware_status_output(root, status),
    ) {
        Ok(value) => value,
        Err(error) => {
            return emit_input_error(
                &format!("failed to build type-aware status: {error}"),
                output,
            );
        }
    };
    match json_style.serialize(&value) {
        Ok(json) => {
            crate::report::sink::outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => emit_input_error(
            &format!("failed to serialize type-aware status: {error}"),
            output,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(family: Option<&str>, version: Option<&str>) -> TypeAwareStatus {
        TypeAwareStatus {
            available: true,
            discovery_source: None,
            companion_path: None,
            package_version: Some("3.38.1".to_owned()),
            protocol_version: 7,
            backend_family: family.map(str::to_owned),
            backend_version: version.map(str::to_owned),
            remediation: None,
        }
    }

    #[test]
    fn default_backend_line_stays_byte_identical() {
        assert_eq!(
            available_message(&status(Some("typescript-go"), Some("7.0.2"))),
            "Type-aware companion: available (3.38.1, protocol 7, TypeScript 7.0.2)"
        );
    }

    #[test]
    fn tsrs_version_carries_the_tsrs_label() {
        assert_eq!(
            available_message(&status(Some("tsrs"), Some("0.20.1-ts7.1.0-beta"))),
            "Type-aware companion: available (3.38.1, protocol 7, tsrs 0.20.1-ts7.1.0-beta)"
        );
    }

    #[test]
    fn missing_family_keeps_the_typescript_label() {
        assert_eq!(
            available_message(&status(None, None)),
            "Type-aware companion: available (3.38.1, protocol 7, TypeScript unknown)"
        );
    }
}
