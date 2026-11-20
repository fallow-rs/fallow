//! The exit code tables of `fallow schema`: the shared table and the codes
//! whose meaning only one command gives.

use serde_json::{Value, json};

use crate::exit_codes::{
    COVERAGE_UPLOAD_AUTH_REJECTED_EXIT_CODE, COVERAGE_UPLOAD_PAYLOAD_TOO_LARGE_EXIT_CODE,
    COVERAGE_UPLOAD_SERVER_ERROR_EXIT_CODE, COVERAGE_UPLOAD_VALIDATION_EXIT_CODE,
    NETWORK_EXIT_CODE, RESOURCE_UNAVAILABLE_EXIT_CODE, RUNTIME_COVERAGE_INPUT_EXIT_CODE,
    RUNTIME_COVERAGE_INTERNAL_EXIT_CODE, RUNTIME_COVERAGE_SIDECAR_EXIT_CODE,
    SECURITY_GATE_EXIT_CODE,
};

/// One process exit code: the number, a stable name and what it means.
pub(super) struct ExitCodeDef {
    pub code: u8,
    pub name: &'static str,
    pub description: &'static str,
}

/// The exit codes that every command shares. A command that gives a code a
/// meaning of its own lists it in [`COMMAND_EXIT_CODES`].
pub(super) const EXIT_CODES: &[ExitCodeDef] = &[
    ExitCodeDef {
        code: 0,
        name: "success",
        description: "Success: no enforced gate failed. Bare fallow in a machine format (json, sarif, codeclimate, the GitHub formats, the comment and review formats) does not enforce findings without --fail-on-issues, so it exits 0 on findings. Read gate_outcomes: status \"fail\" with enforced false is a failing gate that did not set the exit code.",
    },
    ExitCodeDef {
        code: 1,
        name: "gate-failed",
        description: "An enforced gate failed, for example error-severity findings (per rules config, or --fail-on-issues promotes warn to error). This is an analysis result, not a tool failure.",
    },
    ExitCodeDef {
        code: 2,
        name: "invalid-input",
        description: "Invalid input or an execution error, for example an invalid config. With --format json, fallow writes the error to stdout as a JSON object with error true, a message and the exit_code.",
    },
    ExitCodeDef {
        code: RESOURCE_UNAVAILABLE_EXIT_CODE,
        name: "resource-unavailable",
        description: "Requested resource unavailable: config --path found no config, or a license is missing, invalid, or beyond its offline hard-fail window.",
    },
    ExitCodeDef {
        code: RUNTIME_COVERAGE_SIDECAR_EXIT_CODE,
        name: "runtime-coverage-sidecar",
        description: "The runtime coverage add-on is unavailable, unverifiable, protocol-incompatible, or stopped unexpectedly.",
    },
    ExitCodeDef {
        code: RUNTIME_COVERAGE_INPUT_EXIT_CODE,
        name: "runtime-coverage-input",
        description: "Runtime coverage input could not be prepared or parsed.",
    },
    ExitCodeDef {
        code: RUNTIME_COVERAGE_INTERNAL_EXIT_CODE,
        name: "runtime-coverage-internal",
        description: "The runtime coverage add-on reported an internal error.",
    },
    ExitCodeDef {
        code: NETWORK_EXIT_CODE,
        name: "network-failure",
        description: "Network or cloud request failed during a license, coverage, or CI operation.",
    },
    ExitCodeDef {
        code: SECURITY_GATE_EXIT_CODE,
        name: "security-gate",
        description: "Security gate matched a candidate selected by --gate.",
    },
    ExitCodeDef {
        code: COVERAGE_UPLOAD_VALIDATION_EXIT_CODE,
        name: "coverage-upload-validation",
        description: "Coverage inventory or static-findings upload input or project validation failed.",
    },
    ExitCodeDef {
        code: COVERAGE_UPLOAD_PAYLOAD_TOO_LARGE_EXIT_CODE,
        name: "coverage-upload-payload-too-large",
        description: "Coverage inventory or static-findings upload exceeded the server payload limit.",
    },
    ExitCodeDef {
        code: COVERAGE_UPLOAD_AUTH_REJECTED_EXIT_CODE,
        name: "coverage-upload-auth-rejected",
        description: "Coverage inventory or static-findings upload authentication or authorization was rejected.",
    },
    ExitCodeDef {
        code: COVERAGE_UPLOAD_SERVER_ERROR_EXIT_CODE,
        name: "coverage-upload-server-error",
        description: "Coverage inventory or static-findings upload failed after retries or returned another server error.",
    },
];

/// The exit codes that one command gives a meaning of its own.
///
/// A command table holds only a code whose meaning is specific to the
/// command, or a code that the shared [`EXIT_CODES`] table does not have. A
/// code that keeps its shared meaning stays out, so the shared text is never
/// repeated. Each `code` is the constant that the command returns.
pub(super) struct CommandExitCodes {
    pub path: &'static str,
    pub codes: &'static [ExitCodeDef],
}

pub(super) const COMMAND_EXIT_CODES: &[CommandExitCodes] = &[
    CommandExitCodes {
        path: "mutation",
        codes: &[
            ExitCodeDef {
                code: crate::mutation_cli::MIN_SCORE_EXIT_CODE,
                name: "min-score",
                description: "--min-score or mutation.minScore is set, and the mutation score is lower. Survivors alone do not fail a run.",
            },
            ExitCodeDef {
                code: crate::mutation_cli::INPUT_EXIT_CODE,
                name: "invalid-input",
                description: "Invalid input or an execution error, for example a dirty worktree without --allow-dirty, a missing runner, or a run in which baseline failures leave each mutant without a test result.",
            },
        ],
    },
    CommandExitCodes {
        path: "coverage setup",
        codes: &[ExitCodeDef {
            code: crate::coverage::setup_cloud::WAIT_TIMEOUT_EXIT_CODE,
            name: "wait-timeout",
            description: "--cloud --wait timed out before every setup step was done.",
        }],
    },
    CommandExitCodes {
        path: "coverage upload-source-maps",
        codes: &[ExitCodeDef {
            code: crate::coverage::UPLOAD_INCOMPLETE_EXIT_CODE,
            name: "upload-incomplete",
            description: "At least one source map was not uploaded. The summary on stderr names each failed map. With --fail-fast, the command stops at the first failed map.",
        }],
    },
];

pub(super) fn exit_code_json(def: &ExitCodeDef) -> Value {
    json!({ "code": def.code, "name": def.name, "description": def.description })
}

/// The command-specific exit codes of the command at `path`.
pub(super) fn command_exit_codes(path: &str) -> Value {
    COMMAND_EXIT_CODES
        .iter()
        .find(|entry| entry.path == path)
        .map_or_else(
            || json!([]),
            |entry| Value::Array(entry.codes.iter().map(exit_code_json).collect()),
        )
}
