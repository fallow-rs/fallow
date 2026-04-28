use std::process::ExitCode;

use clap::CommandFactory;

use crate::Cli;

pub fn run_schema() -> ExitCode {
    let cmd = Cli::command();
    let schema = build_cli_schema(&cmd);
    match serde_json::to_string_pretty(&schema) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize schema: {e}");
            ExitCode::from(2)
        }
    }
}

pub fn build_cli_schema(cmd: &clap::Command) -> serde_json::Value {
    let mut global_flags = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.get_id() == "help" || arg.get_id() == "version" {
            continue;
        }
        global_flags.push(build_arg_schema(arg));
    }

    let mut commands = Vec::new();
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let mut flags = Vec::new();
        for arg in sub.get_arguments() {
            if arg.get_id() == "help" || arg.get_id() == "version" {
                continue;
            }
            flags.push(build_arg_schema(arg));
        }
        commands.push(serde_json::json!({
            "name": sub.get_name(),
            "description": sub.get_about().map(std::string::ToString::to_string),
            "flags": flags,
        }));
    }

    serde_json::json!({
        "name": cmd.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "description": cmd.get_about().map(std::string::ToString::to_string),
        "global_flags": global_flags,
        "commands": commands,
        "default_command": null,
        "default_behavior": "Runs all analyses (check + dupes + health). Use --only/--skip to select.",
        "issue_types": [
            {
                "id": "unused-file",
                "description": "File is not reachable from any entry point",
                "filter_flag": "--unused-files",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-file unused-file"
            },
            {
                "id": "unused-export",
                "description": "Export is never imported by other modules",
                "filter_flag": "--unused-exports",
                "fixable": true,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-next-line unused-export"
            },
            {
                "id": "unused-type",
                "description": "Type export is never imported by other modules",
                "filter_flag": "--unused-types",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-next-line unused-type"
            },
            {
                "id": "unused-dependency",
                "description": "Package in dependencies is never imported",
                "filter_flag": "--unused-deps",
                "fixable": true,
                "suppressible": false,
                "note": "--unused-deps controls both unused-dependency and unused-dev-dependency"
            },
            {
                "id": "unused-dev-dependency",
                "description": "Package in devDependencies is never imported",
                "filter_flag": "--unused-deps",
                "fixable": true,
                "suppressible": false,
                "note": "--unused-deps controls both unused-dependency and unused-dev-dependency"
            },
            {
                "id": "unused-enum-member",
                "description": "Enum member is never referenced",
                "filter_flag": "--unused-enum-members",
                "fixable": true,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-next-line unused-enum-member"
            },
            {
                "id": "unused-class-member",
                "description": "Class member is never referenced",
                "filter_flag": "--unused-class-members",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-next-line unused-class-member"
            },
            {
                "id": "unresolved-import",
                "description": "Import specifier could not be resolved to a file",
                "filter_flag": "--unresolved-imports",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-next-line unresolved-import"
            },
            {
                "id": "unlisted-dependency",
                "description": "Package is imported but not in package.json",
                "filter_flag": "--unlisted-deps",
                "fixable": false,
                "suppressible": false
            },
            {
                "id": "duplicate-export",
                "description": "Same export name appears in multiple modules",
                "filter_flag": "--duplicate-exports",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-file duplicate-export"
            },
            {
                "id": "type-only-dependency",
                "description": "Production dependency only used via import type (should be devDependency)",
                "filter_flag": null,
                "fixable": false,
                "suppressible": false,
                "note": "Only reported in --production mode"
            },
            {
                "id": "circular-dependency",
                "description": "Files form a circular import chain",
                "filter_flag": "--circular-deps",
                "fixable": false,
                "suppressible": true,
                "suppress_comment": "// fallow-ignore-file circular-dependency",
                "note": "Only file-level suppression is supported (fallow-ignore-file), not next-line suppression"
            }
        ],
        "suppression_comments": {
            "next_line": "// fallow-ignore-next-line [issue-type]",
            "file": "// fallow-ignore-file [issue-type]",
            "note": "Omit [issue-type] to suppress all issue types. Unknown tokens are silently ignored."
        },
        "output_formats": ["human", "json", "sarif", "compact", "markdown", "codeclimate", "gitlab-codequality", "badge"],
        "exit_codes": {
            "0": "Success (no error-severity issues found)",
            "1": "Error-severity issues found (per rules config, or --fail-on-issues promotes warn→error)",
            "2": "Error (invalid config, invalid input, etc.). When --format json is active, errors are emitted as structured JSON on stdout: {\"error\": true, \"message\": \"...\", \"exit_code\": 2}"
        },
        "environment_variables": environment_variables_schema(),
        "severity_levels": ["error", "warn", "off"]
    })
}

fn environment_variables_schema() -> serde_json::Value {
    serde_json::json!({
        "FALLOW_FORMAT": "Default output format (json/human/sarif/compact/markdown/codeclimate/gitlab-codequality/badge). CLI --format flag overrides this.",
        "FALLOW_QUIET": "Set to \"1\" or \"true\" to suppress progress output. CLI --quiet flag overrides this.",
        "FALLOW_PRODUCTION": "Set to true/false to override production mode for all analyses.",
        "FALLOW_PRODUCTION_DEAD_CODE": "Set to true/false to override production mode for dead-code analysis.",
        "FALLOW_PRODUCTION_HEALTH": "Set to true/false to override production mode for health analysis.",
        "FALLOW_PRODUCTION_DUPES": "Set to true/false to override production mode for duplication analysis.",
        "FALLOW_BIN": "Path to fallow binary (used by fallow-mcp server)."
    })
}

fn build_arg_schema(arg: &clap::Arg) -> serde_json::Value {
    let name = arg
        .get_long()
        .map_or_else(|| arg.get_id().to_string(), |l| format!("--{l}"));

    let arg_type = match arg.get_action() {
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse => "bool",
        clap::ArgAction::Count => "count",
        _ => "string",
    };

    let possible: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();

    let mut schema = serde_json::json!({
        "name": name,
        "type": arg_type,
        "required": arg.is_required_set(),
        "description": arg.get_help().map(std::string::ToString::to_string),
    });

    if let Some(short) = arg.get_short() {
        schema["short"] = serde_json::json!(format!("-{short}"));
    }

    if let Some(default) = arg.get_default_values().first() {
        schema["default"] = serde_json::json!(default.to_str());
    }

    if !possible.is_empty() {
        schema["possible_values"] = serde_json::json!(possible);
    }

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_includes_environment_variables() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let env_vars = &schema["environment_variables"];
        assert!(env_vars["FALLOW_FORMAT"].is_string());
        assert!(env_vars["FALLOW_QUIET"].is_string());
        assert!(env_vars["FALLOW_BIN"].is_string());
    }

    #[test]
    fn schema_exit_code_2_mentions_json_errors() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let exit_2 = schema["exit_codes"]["2"].as_str().unwrap();
        assert!(exit_2.contains("JSON"));
    }

    #[test]
    fn schema_has_name_and_version() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        assert_eq!(schema["name"], "fallow");
        assert!(schema["version"].is_string());
    }

    #[test]
    fn schema_has_commands_array() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let commands = schema["commands"].as_array().unwrap();
        assert!(!commands.is_empty());
        // Should not include the "help" subcommand
        assert!(
            !commands
                .iter()
                .any(|c| c["name"].as_str().unwrap() == "help")
        );
    }

    #[test]
    fn schema_has_global_flags() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let flags = schema["global_flags"].as_array().unwrap();
        // Should not include "help" or "version" flags
        assert!(!flags.iter().any(|f| f["name"].as_str().unwrap() == "help"));
        assert!(
            !flags
                .iter()
                .any(|f| f["name"].as_str().unwrap() == "version")
        );
    }

    #[test]
    fn schema_has_issue_types() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let issue_types = schema["issue_types"].as_array().unwrap();
        assert!(!issue_types.is_empty());
        // Verify each issue type has required fields
        for issue_type in issue_types {
            assert!(issue_type["id"].is_string());
            assert!(issue_type["description"].is_string());
        }
    }

    #[test]
    fn schema_output_formats_include_all_formats() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let formats = schema["output_formats"].as_array().unwrap();
        for expected in [
            "human",
            "json",
            "sarif",
            "compact",
            "markdown",
            "codeclimate",
            "gitlab-codequality",
            "badge",
        ] {
            assert!(
                formats.iter().any(|f| f.as_str().unwrap() == expected),
                "missing format: {expected}"
            );
        }
    }

    #[test]
    fn schema_severity_levels() {
        let cmd = Cli::command();
        let schema = build_cli_schema(&cmd);
        let levels = schema["severity_levels"].as_array().unwrap();
        for expected in ["error", "warn", "off"] {
            assert!(
                levels.iter().any(|l| l.as_str().unwrap() == expected),
                "missing severity level: {expected}"
            );
        }
    }

    #[test]
    fn build_arg_schema_bool_type() {
        let cmd = Cli::command();
        // Find a boolean arg like --quiet
        let quiet_arg = cmd.get_arguments().find(|a| a.get_id() == "quiet").unwrap();
        let schema = build_arg_schema(quiet_arg);
        assert_eq!(schema["type"], "bool");
    }

    #[test]
    fn build_arg_schema_includes_short_flag() {
        let cmd = Cli::command();
        // Find an arg with a short flag
        let quiet_arg = cmd.get_arguments().find(|a| a.get_id() == "quiet").unwrap();
        let schema = build_arg_schema(quiet_arg);
        if quiet_arg.get_short().is_some() {
            assert!(schema["short"].is_string());
        }
    }
}
