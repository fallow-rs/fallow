use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fallow_config::OutputFormat;
use serde_json::{Value, json};

use crate::error::emit_error;
use crate::output_envelope::{
    FallowOutput, InspectEvidence, InspectEvidenceScope, InspectEvidenceSection,
    InspectFileIdentity, InspectIdentity, InspectOutput, InspectSectionStatus,
    InspectSymbolIdentity, InspectTargetDescriptor, serialize_root_output,
};
use crate::report;
use crate::report::sink::outln;

#[derive(Clone)]
pub enum InspectTarget {
    File { file: String },
    Symbol { file: String, export_name: String },
}

pub struct InspectOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub workspace: Option<&'a Vec<String>>,
    pub target: InspectTarget,
}

struct NormalizedTarget<'a> {
    file: &'a str,
    export_name: Option<&'a str>,
}

impl<'a> NormalizedTarget<'a> {
    fn new(target: &'a InspectTarget) -> Result<Self, String> {
        match target {
            InspectTarget::File { file } => {
                require_non_empty("file", file)?;
                Ok(Self {
                    file,
                    export_name: None,
                })
            }
            InspectTarget::Symbol { file, export_name } => {
                require_non_empty("symbol file", file)?;
                require_non_empty("symbol export", export_name)?;
                Ok(Self {
                    file,
                    export_name: Some(export_name),
                })
            }
        }
    }

    fn target_descriptor(&self) -> InspectTargetDescriptor {
        match self.export_name {
            Some(export_name) => InspectTargetDescriptor::Symbol {
                file: self.file.to_string(),
                export_name: export_name.to_string(),
            },
            None => InspectTargetDescriptor::File {
                file: self.file.to_string(),
            },
        }
    }
}

pub fn run_inspect(opts: &InspectOptions<'_>) -> ExitCode {
    let target = match NormalizedTarget::new(&opts.target) {
        Ok(target) => target,
        Err(message) => return emit_error(&message, 2, opts.output),
    };

    let trace_file = match run_required_json(opts, trace_file_args(target.file)) {
        Ok(value) => value,
        Err(message) => return emit_error(&message, 2, opts.output),
    };
    let trace_export = match target.export_name {
        Some(export_name) => {
            match run_required_json(opts, trace_export_args(target.file, export_name)) {
                Ok(value) => Some(value),
                Err(message) => return emit_error(&message, 2, opts.output),
            }
        }
        None => None,
    };

    let mut warnings = Vec::new();
    if target.export_name.is_some() {
        warnings.push(
            "dead_code, duplication, complexity, and security evidence is file-scoped in v1; file:line symbol narrowing is a follow-up"
                .to_string(),
        );
    }

    let evidence = InspectEvidence {
        trace_file: InspectEvidenceSection::ok(InspectEvidenceScope::File, trace_file.clone()),
        trace_export: trace_export
            .clone()
            .map(|value| InspectEvidenceSection::ok(InspectEvidenceScope::Symbol, value)),
        dead_code: optional_section(
            opts,
            dead_code_args(target.file),
            InspectEvidenceScope::File,
            |value| value,
        ),
        duplication: optional_section(
            opts,
            dupes_args(),
            InspectEvidenceScope::ProjectFilteredToFile,
            |value| filter_path_array(&value, target.file, "clone_groups"),
        ),
        complexity: optional_section(
            opts,
            health_args(),
            InspectEvidenceScope::ProjectFilteredToFile,
            |value| filter_path_array(&value, target.file, "findings"),
        ),
        security: optional_section(
            opts,
            security_args(target.file),
            InspectEvidenceScope::File,
            |value| value,
        ),
    };
    push_inspect_warnings(&mut warnings, &evidence);

    let identity = match trace_export.as_ref() {
        Some(export) => InspectIdentity::Symbol(InspectSymbolIdentity {
            file: target.file.to_string(),
            export_name: target.export_name.unwrap_or_default().to_string(),
            file_reachable: export.get("file_reachable").cloned(),
            is_entry_point: export.get("is_entry_point").cloned(),
            is_used: export.get("is_used").cloned(),
            reason: export.get("reason").cloned(),
        }),
        None => InspectIdentity::File(InspectFileIdentity {
            file: target.file.to_string(),
            is_reachable: trace_file.get("is_reachable").cloned(),
            is_entry_point: trace_file.get("is_entry_point").cloned(),
            export_count: trace_file
                .get("exports")
                .and_then(Value::as_array)
                .map(Vec::len),
            import_count: trace_file
                .get("imports_from")
                .and_then(Value::as_array)
                .map(Vec::len),
            imported_by_count: trace_file
                .get("imported_by")
                .and_then(Value::as_array)
                .map(Vec::len),
        }),
    };

    let bundle = InspectOutput {
        target: target.target_descriptor(),
        identity,
        evidence,
        warnings,
    };

    match opts.output {
        OutputFormat::Json => {
            let value = match serialize_root_output(FallowOutput::Inspect(bundle)) {
                Ok(value) => value,
                Err(err) => {
                    return emit_error(
                        &format!("failed to serialize inspect output: {err}"),
                        2,
                        opts.output,
                    );
                }
            };
            report::emit_json(&value, "inspect")
        }
        OutputFormat::Human => {
            print_human(&bundle, opts.quiet);
            ExitCode::SUCCESS
        }
        _ => emit_error("inspect supports --format json or human", 2, opts.output),
    }
}

fn print_human(bundle: &InspectOutput, quiet: bool) {
    outln!("Inspect target");
    outln!();
    outln!("  target: {}", json_display(&bundle.target));
    outln!("  identity: {}", json_display(&bundle.identity));
    if !bundle.warnings.is_empty() && !quiet {
        outln!();
        for warning in &bundle.warnings {
            outln!("  warning: {warning}");
        }
    }
}

fn json_display(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<unprintable>".to_string())
}

fn run_required_json(opts: &InspectOptions<'_>, args: Vec<String>) -> Result<Value, String> {
    run_child_json(opts, args).and_then(|output| output.value)
}

fn optional_section<F>(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    scope: InspectEvidenceScope,
    filter: F,
) -> InspectEvidenceSection
where
    F: FnOnce(Value) -> Value,
{
    match run_child_json(opts, args) {
        Ok(output) => match output.value {
            Ok(value) => InspectEvidenceSection::ok(scope, filter(value)),
            Err(message) => InspectEvidenceSection::error(scope, message),
        },
        Err(message) => InspectEvidenceSection::error(scope, message),
    }
}

struct ChildJson {
    value: Result<Value, String>,
}

fn run_child_json(opts: &InspectOptions<'_>, args: Vec<String>) -> Result<ChildJson, String> {
    let binary = std::env::current_exe()
        .map_err(|err| format!("failed to locate current fallow binary: {err}"))?;
    let mut command = Command::new(binary);
    command.args(build_child_args(opts, args));
    let output = command
        .output()
        .map_err(|err| format!("failed to run child analysis: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(2);
    if code > 1 {
        let message = if stderr.trim().is_empty() {
            format!("child analysis exited with code {code}")
        } else {
            stderr.trim().to_string()
        };
        return Err(message);
    }
    if stdout.trim().is_empty() {
        return Ok(ChildJson {
            value: Err("child analysis returned no JSON".to_string()),
        });
    }
    Ok(ChildJson {
        value: serde_json::from_str(&stdout)
            .map_err(|err| format!("child analysis returned invalid JSON: {err}")),
    })
}

fn build_child_args(opts: &InspectOptions<'_>, command_args: Vec<String>) -> Vec<String> {
    let command_name = command_args.first().map(String::as_str);
    let mut args = vec![
        "--root".to_string(),
        opts.root.to_string_lossy().to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    if let Some(config) = opts.config_path.as_ref() {
        args.extend(["--config".to_string(), config.to_string_lossy().to_string()]);
    }
    if opts.no_cache {
        args.push("--no-cache".to_string());
    }
    args.extend(["--threads".to_string(), opts.threads.to_string()]);
    if opts.production && command_name != Some("security") {
        args.push("--production".to_string());
    }
    if let Some(workspace) = opts.workspace {
        args.extend(["--workspace".to_string(), workspace.join(",")]);
    }
    args.extend(command_args);
    args
}

fn trace_file_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--trace-file".to_string(),
        file.to_string(),
    ]
}

fn trace_export_args(file: &str, export_name: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--trace".to_string(),
        format!("{file}:{export_name}"),
    ]
}

fn dead_code_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--file".to_string(),
        file.to_string(),
    ]
}

fn dupes_args() -> Vec<String> {
    vec!["dupes".to_string()]
}

fn health_args() -> Vec<String> {
    vec!["health".to_string(), "--complexity".to_string()]
}

fn security_args(file: &str) -> Vec<String> {
    vec![
        "security".to_string(),
        "--file".to_string(),
        file.to_string(),
    ]
}

fn filter_path_array(value: &Value, file: &str, key: &str) -> Value {
    let matched = value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| value_mentions_file(item, file))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let matched_count = matched.len();

    json!({
        key: matched,
        "matched_count": matched_count,
        "summary": value.get("summary").cloned(),
        "stats": value.get("stats").cloned(),
    })
}

fn value_mentions_file(value: &Value, file: &str) -> bool {
    match value {
        Value::String(s) => path_eq(s, file),
        Value::Array(items) => items.iter().any(|item| value_mentions_file(item, file)),
        Value::Object(map) => map.values().any(|item| value_mentions_file(item, file)),
        _ => false,
    }
}

fn path_eq(left: &str, right: &str) -> bool {
    left.replace('\\', "/") == right.replace('\\', "/")
}

fn push_inspect_warnings(warnings: &mut Vec<String>, evidence: &InspectEvidence) {
    push_warning(warnings, "dead_code", &evidence.dead_code);
    push_warning(warnings, "duplication", &evidence.duplication);
    push_warning(warnings, "complexity", &evidence.complexity);
    push_warning(warnings, "security", &evidence.security);
}

fn push_warning(warnings: &mut Vec<String>, section: &str, evidence: &InspectEvidenceSection) {
    if matches!(evidence.status, InspectSectionStatus::Error)
        && let Some(message) = evidence.message.as_ref()
    {
        warnings.push(format!("{section} evidence unavailable: {message}"));
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}
