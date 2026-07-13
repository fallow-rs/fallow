use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitCode};

use fallow_config::OutputFormat;
use serde_json::{Value, json};

use crate::error::emit_error;
use crate::report;
use crate::report::sink::outln;
use fallow_output::{
    InspectEvidence, InspectEvidenceScope, InspectEvidenceSection, InspectFileIdentity,
    InspectIdentity, InspectOutput, InspectSectionStatus, InspectSymbolIdentity,
    InspectTargetDescriptor,
};

#[derive(Clone)]
pub enum InspectTarget {
    File { file: String },
    Symbol { file: String, export_name: String },
}

pub struct InspectOptions<'a> {
    pub root: &'a Path,
    pub config_path: Option<&'a PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub no_production: bool,
    pub max_file_size: Option<u32>,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub workspace: Option<&'a Vec<String>>,
    pub target: InspectTarget,
    /// OPT-IN cache location for in-process target churn evidence. `None`
    /// keeps git-history analysis entirely off the default path.
    pub churn_cache_dir: Option<&'a Path>,
    /// OPT-IN: also run the best-effort symbol-level call chain
    /// (`fallow trace`) and attach it as the `symbol_chain` evidence section.
    /// Only meaningful for a SYMBOL target. Default off (best-effort, off the
    /// ranked path).
    pub symbol_chain: bool,
}

#[derive(Debug)]
struct NormalizedTarget {
    file: String,
    export_name: Option<String>,
}

impl NormalizedTarget {
    fn new(root: &Path, target: &InspectTarget) -> Result<Self, String> {
        match target {
            InspectTarget::File { file } => {
                require_non_empty("file", file)?;
                let file = normalize_target_file(root, file)?;
                Ok(Self {
                    file,
                    export_name: None,
                })
            }
            InspectTarget::Symbol { file, export_name } => {
                require_non_empty("symbol file", file)?;
                require_non_empty("symbol export", export_name)?;
                let file = normalize_target_file(root, file)?;
                Ok(Self {
                    file,
                    export_name: Some(export_name.clone()),
                })
            }
        }
    }

    fn target_descriptor(&self) -> InspectTargetDescriptor {
        match self.export_name.as_deref() {
            Some(export_name) => InspectTargetDescriptor::Symbol {
                file: self.file.clone(),
                export_name: export_name.to_string(),
            },
            None => InspectTargetDescriptor::File {
                file: self.file.clone(),
            },
        }
    }
}

pub fn run_inspect(opts: &InspectOptions<'_>) -> ExitCode {
    let target = match NormalizedTarget::new(opts.root, &opts.target) {
        Ok(target) => target,
        Err(message) => return emit_error(&message, 2, opts.output),
    };

    let target_file = target.file.as_str();
    let trace_file = match run_required_json(opts, trace_file_args(target_file)) {
        Ok(value) => value,
        Err(message) => return emit_error(&message, 2, opts.output),
    };
    let trace_export = match collect_trace_export(opts, &target) {
        Ok(value) => value,
        Err(message) => return emit_error(&message, 2, opts.output),
    };

    let mut warnings = Vec::new();
    if target.export_name.is_some() {
        warnings.push(
            "dead_code, duplication, complexity, and security evidence is file-scoped in v1; file:line symbol narrowing is a follow-up"
                .to_string(),
        );
    }

    let evidence = build_inspect_evidence(opts, &target, &trace_file, trace_export.clone());
    push_inspect_warnings(&mut warnings, &evidence);

    let identity = build_inspect_identity(&target, &trace_file, trace_export.as_ref());

    let bundle = InspectOutput {
        target: target.target_descriptor(),
        identity,
        evidence,
        warnings,
    };

    emit_inspect_bundle(bundle, opts)
}

/// Run the `trace_export` child when the target is a symbol, else `Ok(None)`.
fn collect_trace_export(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
) -> Result<Option<Value>, String> {
    let Some(export_name) = target.export_name.as_deref() else {
        return Ok(None);
    };
    run_required_json(opts, trace_export_args(&target.file, export_name)).map(Some)
}

/// Compose the evidence sections (trace, dead-code, duplication, complexity,
/// security, impact-closure, plus the OPT-IN symbol chain) for the inspect
/// bundle.
fn build_inspect_evidence(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    trace_file: &Value,
    trace_export: Option<Value>,
) -> InspectEvidence {
    let optional_threads = parallel_child_threads(opts.threads);
    let child_evidence = collect_inspect_child_evidence(opts, target, optional_threads);

    InspectEvidence {
        trace_file: InspectEvidenceSection::ok(InspectEvidenceScope::File, trace_file.clone()),
        trace_export: trace_export
            .map(|value| InspectEvidenceSection::ok(InspectEvidenceScope::Symbol, value)),
        dead_code: child_evidence.dead_code,
        duplication: child_evidence.duplication,
        complexity: child_evidence.complexity,
        security: child_evidence.security,
        impact_closure: child_evidence.impact_closure,
        churn: child_evidence.churn,
        symbol_chain: build_symbol_chain_section(opts, target, optional_threads),
    }
}

struct InspectChildEvidence {
    dead_code: InspectEvidenceSection,
    duplication: InspectEvidenceSection,
    complexity: InspectEvidenceSection,
    security: InspectEvidenceSection,
    impact_closure: InspectEvidenceSection,
    churn: Option<InspectEvidenceSection>,
}

fn collect_inspect_child_evidence(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    optional_threads: usize,
) -> InspectChildEvidence {
    let target_file = target.file.as_str();

    std::thread::scope(|scope| {
        let dead_code = scope.spawn(|| {
            optional_section(
                opts,
                dead_code_args(target_file),
                InspectEvidenceScope::File,
                optional_threads,
                |value| value,
            )
        });
        let duplication = scope.spawn(|| {
            optional_section(
                opts,
                dupes_args(),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                |value| filter_path_array(&value, target_file, "clone_groups"),
            )
        });
        let complexity = scope.spawn(|| {
            optional_section(
                opts,
                health_args(),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                |value| filter_path_array(&value, target_file, "findings"),
            )
        });
        let security = scope.spawn(|| {
            optional_section(
                opts,
                security_args(target_file),
                InspectEvidenceScope::File,
                optional_threads,
                |value| value,
            )
        });
        let impact_closure = scope.spawn(|| {
            optional_section(
                opts,
                impact_closure_args(target_file),
                InspectEvidenceScope::ProjectFilteredToFile,
                optional_threads,
                |value| value,
            )
        });
        let churn = opts.churn_cache_dir.map(|cache_dir| {
            scope.spawn(|| collect_target_churn_section(opts, target_file, cache_dir))
        });

        join_inspect_child_evidence(
            dead_code,
            duplication,
            complexity,
            security,
            impact_closure,
            churn,
        )
    })
}

const fn parallel_child_threads(parent_threads: usize) -> usize {
    let threads = parent_threads / 4;
    if threads == 0 { 1 } else { threads }
}

fn join_inspect_child_evidence(
    dead_code: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    duplication: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    complexity: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    security: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    impact_closure: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    churn: Option<std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>>,
) -> InspectChildEvidence {
    InspectChildEvidence {
        dead_code: join_inspect_section(dead_code, InspectEvidenceScope::File),
        duplication: join_inspect_section(duplication, InspectEvidenceScope::ProjectFilteredToFile),
        complexity: join_inspect_section(complexity, InspectEvidenceScope::ProjectFilteredToFile),
        security: join_inspect_section(security, InspectEvidenceScope::File),
        impact_closure: join_inspect_section(
            impact_closure,
            InspectEvidenceScope::ProjectFilteredToFile,
        ),
        churn: join_optional_inspect_section(churn, InspectEvidenceScope::ProjectFilteredToFile),
    }
}

fn join_optional_inspect_section(
    handle: Option<std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>>,
    scope: InspectEvidenceScope,
) -> Option<InspectEvidenceSection> {
    handle.map(|handle| join_inspect_section(handle, scope))
}

fn join_inspect_section(
    handle: std::thread::ScopedJoinHandle<'_, InspectEvidenceSection>,
    scope: InspectEvidenceScope,
) -> InspectEvidenceSection {
    match handle.join() {
        Ok(section) => section,
        Err(_) => {
            InspectEvidenceSection::error(scope, "inspect evidence worker panicked".to_string())
        }
    }
}

fn collect_target_churn_section(
    opts: &InspectOptions<'_>,
    target_file: &str,
    cache_dir: &Path,
) -> InspectEvidenceSection {
    let options = fallow_engine::health::TargetChurnOptions {
        root: opts.root,
        target: Path::new(target_file),
        cache_dir: cache_dir.to_path_buf(),
        no_cache: opts.no_cache,
        since: None,
        min_commits: None,
    };
    churn_section_from_result(
        target_file,
        fallow_engine::health::analyze_target_churn(&options),
    )
}

fn churn_section_from_result(
    target_file: &str,
    result: Result<fallow_engine::health::TargetChurnOutcome, String>,
) -> InspectEvidenceSection {
    let scope = InspectEvidenceScope::ProjectFilteredToFile;
    match result {
        Ok(fallow_engine::health::TargetChurnOutcome::Found(evidence)) => {
            InspectEvidenceSection::ok(
                scope,
                json!({
                    "file": target_file,
                    "matched_count": 1,
                    "commits": evidence.file.commits,
                    "weighted_commits": evidence.file.weighted_commits,
                    "lines_added": evidence.file.lines_added,
                    "lines_deleted": evidence.file.lines_deleted,
                    "trend": evidence.file.trend,
                    "window": evidence.since.display,
                    "minimum_commits": evidence.min_commits,
                    "shallow_clone": evidence.shallow_clone,
                }),
            )
        }
        Ok(fallow_engine::health::TargetChurnOutcome::NoQualifyingChurn {
            observed_commits,
            since,
            min_commits,
            shallow_clone,
        }) => InspectEvidenceSection::ok(
            scope,
            json!({
                "file": target_file,
                "matched_count": 0,
                "observed_commits": observed_commits,
                "window": since.display,
                "minimum_commits": min_commits,
                "shallow_clone": shallow_clone,
            }),
        ),
        Ok(fallow_engine::health::TargetChurnOutcome::Unavailable { message }) => {
            InspectEvidenceSection::unavailable(scope, message)
        }
        Err(message) => InspectEvidenceSection::error(scope, message),
    }
}

/// Build the OPT-IN symbol-level call-chain section. Returns `None` (the
/// section is omitted) unless `--symbol-chain` was requested AND the target is a
/// SYMBOL. Best-effort, syntactic, OFF the ranked path: it is attached as
/// separate evidence, never folded into the trusted sections.
fn build_symbol_chain_section(
    opts: &InspectOptions<'_>,
    target: &NormalizedTarget,
    threads: usize,
) -> Option<InspectEvidenceSection> {
    if !opts.symbol_chain {
        return None;
    }
    let export_name = target.export_name.as_deref()?;
    Some(optional_section(
        opts,
        symbol_chain_args(&target.file, export_name),
        InspectEvidenceScope::Symbol,
        threads,
        |value| value,
    ))
}

/// Derive the identity summary from the trace evidence (symbol when an export
/// trace is present, file otherwise).
fn build_inspect_identity(
    target: &NormalizedTarget,
    trace_file: &Value,
    trace_export: Option<&Value>,
) -> InspectIdentity {
    match trace_export {
        Some(export) => InspectIdentity::Symbol(InspectSymbolIdentity {
            file: target.file.clone(),
            export_name: target.export_name.clone().unwrap_or_default(),
            file_reachable: export.get("file_reachable").cloned(),
            is_entry_point: export.get("is_entry_point").cloned(),
            is_used: export.get("is_used").cloned(),
            reason: export.get("reason").cloned(),
        }),
        None => InspectIdentity::File(InspectFileIdentity {
            file: target.file.clone(),
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
    }
}

/// Serialize and emit the inspect bundle in the requested output format.
fn emit_inspect_bundle(bundle: InspectOutput, opts: &InspectOptions<'_>) -> ExitCode {
    match opts.output {
        OutputFormat::Json => {
            let value = match fallow_output::serialize_inspect_json_output(
                bundle,
                crate::output_runtime::current_root_envelope_mode(),
                crate::output_runtime::telemetry_analysis_run_id().as_deref(),
            ) {
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
    outln!();
    outln!("Evidence");
    print_evidence_summary("trace_file", &bundle.evidence.trace_file);
    if let Some(section) = bundle.evidence.trace_export.as_ref() {
        print_evidence_summary("trace_export", section);
    }
    print_evidence_summary("dead_code", &bundle.evidence.dead_code);
    print_evidence_summary("duplication", &bundle.evidence.duplication);
    print_evidence_summary("complexity", &bundle.evidence.complexity);
    print_evidence_summary("security", &bundle.evidence.security);
    print_evidence_summary("impact_closure", &bundle.evidence.impact_closure);
    if let Some(section) = bundle.evidence.churn.as_ref() {
        print_evidence_summary("churn", section);
    }
    if let Some(section) = bundle.evidence.symbol_chain.as_ref() {
        print_evidence_summary("symbol_chain", section);
    }
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

fn print_evidence_summary(name: &str, section: &InspectEvidenceSection) {
    let status = match section.status {
        InspectSectionStatus::Ok => "ok",
        InspectSectionStatus::Unavailable => "unavailable",
        InspectSectionStatus::Error => "error",
    };
    let detail = evidence_detail(section)
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    outln!(
        "  {name}: {status} [{}]{detail}",
        evidence_scope_label(section.scope)
    );
}

fn evidence_scope_label(scope: InspectEvidenceScope) -> &'static str {
    match scope {
        InspectEvidenceScope::Symbol => "symbol",
        InspectEvidenceScope::File => "file",
        InspectEvidenceScope::ProjectFilteredToFile => "project filtered to file",
    }
}

fn evidence_detail(section: &InspectEvidenceSection) -> Option<String> {
    if let Some(message) = section.message.as_deref() {
        return Some(message.to_string());
    }
    let data = section.data.as_ref()?;
    if let Some(count) = data.get("matched_count").and_then(Value::as_u64) {
        return Some(format!("matches: {count}"));
    }
    if let Some(exports) = data.get("exports").and_then(Value::as_array) {
        return Some(format!("exports: {}", exports.len()));
    }
    None
}

fn run_required_json(opts: &InspectOptions<'_>, args: Vec<String>) -> Result<Value, String> {
    run_child_json(opts, args, opts.threads).and_then(|output| output.value)
}

fn optional_section<F>(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    scope: InspectEvidenceScope,
    threads: usize,
    filter: F,
) -> InspectEvidenceSection
where
    F: FnOnce(Value) -> Value,
{
    match run_child_json(opts, args, threads) {
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

fn run_child_json(
    opts: &InspectOptions<'_>,
    args: Vec<String>,
    threads: usize,
) -> Result<ChildJson, String> {
    let binary = std::env::current_exe()
        .map_err(|err| format!("failed to locate current fallow binary: {err}"))?;
    let mut command = Command::new(binary);
    command.args(build_child_args(opts, args, threads));
    let output = command
        .output()
        .map_err(|err| format!("failed to run child analysis: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(2);
    if code > 1 {
        let message = child_error_message(code, &stdout, &stderr);
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

fn build_child_args(
    opts: &InspectOptions<'_>,
    command_args: Vec<String>,
    threads: usize,
) -> Vec<String> {
    let command_name = command_args.first().map(String::as_str);
    let mut args = vec![
        "--root".to_string(),
        opts.root.to_string_lossy().to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    if let Some(config) = opts.config_path {
        args.extend(["--config".to_string(), config.to_string_lossy().to_string()]);
    }
    if opts.no_cache {
        args.push("--no-cache".to_string());
    }
    if opts.no_production && command_name != Some("security") {
        args.push("--no-production".to_string());
    }
    if let Some(max_file_size) = opts.max_file_size {
        args.extend(["--max-file-size".to_string(), max_file_size.to_string()]);
    }
    args.extend(["--threads".to_string(), threads.to_string()]);
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

fn impact_closure_args(file: &str) -> Vec<String> {
    vec![
        "dead-code".to_string(),
        "--impact-closure".to_string(),
        file.to_string(),
    ]
}

fn symbol_chain_args(file: &str, export_name: &str) -> Vec<String> {
    vec![
        "trace".to_string(),
        format!("{file}:{export_name}"),
        "--callers".to_string(),
        "--callees".to_string(),
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

fn normalize_target_file(root: &Path, file: &str) -> Result<String, String> {
    let raw = file.trim();
    let normalized_raw = raw.replace('\\', "/");
    let path = Path::new(&normalized_raw);
    let relative = if path.is_absolute() {
        let absolute = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        absolute
            .strip_prefix(root)
            .map_err(|_| {
                format!(
                    "inspect target must be inside the project root: {}",
                    absolute.display()
                )
            })?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "inspect target must be a root-relative path inside the project: {raw}"
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err("inspect target file must not be empty".to_string());
    }
    Ok(parts.join("/"))
}

fn child_error_message(code: i32, stdout: &str, stderr: &str) -> String {
    structured_child_message(stdout)
        .or_else(|| {
            let trimmed = strip_ansi(stderr.trim());
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_else(|| format!("child analysis exited with code {code}"))
}

fn structured_child_message(stdout: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(stdout.trim()).ok()?;
    value
        .get("message")
        .or_else(|| value.get("error_message"))
        .and_then(Value::as_str)
        .map(strip_ansi)
        .filter(|message| !message.is_empty())
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_control() && ch != '\n' && ch != '\t' {
            continue;
        }
        output.push(ch);
    }
    output.trim().to_string()
}

fn push_inspect_warnings(warnings: &mut Vec<String>, evidence: &InspectEvidence) {
    push_warning(warnings, "dead_code", &evidence.dead_code);
    push_warning(warnings, "duplication", &evidence.duplication);
    push_warning(warnings, "complexity", &evidence.complexity);
    push_warning(warnings, "security", &evidence.security);
    push_warning(warnings, "impact_closure", &evidence.impact_closure);
    if let Some(churn) = evidence.churn.as_ref() {
        push_warning(warnings, "churn", churn);
    }
}

fn push_warning(warnings: &mut Vec<String>, section: &str, evidence: &InspectEvidenceSection) {
    if !matches!(evidence.status, InspectSectionStatus::Ok)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inspect_options<'a>(
        root: &'a Path,
        config_path: Option<&'a PathBuf>,
        target: InspectTarget,
    ) -> InspectOptions<'a> {
        InspectOptions {
            root,
            config_path,
            output: OutputFormat::Json,
            no_cache: true,
            no_production: true,
            max_file_size: Some(2),
            threads: 3,
            quiet: true,
            production: false,
            workspace: None,
            target,
            churn_cache_dir: None,
            symbol_chain: false,
        }
    }

    #[test]
    fn normalized_target_uses_root_relative_posix_path() {
        let root = std::env::current_dir().unwrap();
        let file = root
            .join("src")
            .join("api.ts")
            .to_string_lossy()
            .to_string();

        let target = NormalizedTarget::new(&root, &InspectTarget::File { file }).unwrap();

        assert_eq!(target.file, "src/api.ts");
    }

    #[test]
    fn normalized_target_rejects_parent_paths() {
        let root = PathBuf::from("/repo");
        let file = "../other.ts".to_string();

        let err = NormalizedTarget::new(&root, &InspectTarget::File { file }).unwrap_err();

        assert!(err.contains("inside the project"));
    }

    #[test]
    fn child_args_forward_global_inspect_overrides() {
        let root = PathBuf::from("/repo");
        let config_path = Some(PathBuf::from("/repo/.fallowrc.json"));
        let opts = inspect_options(
            &root,
            config_path.as_ref(),
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );

        let args = build_child_args(&opts, dead_code_args("src/api.ts"), opts.threads);

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--config", "/repo/.fallowrc.json"])
        );
        assert!(args.contains(&"--no-cache".to_string()));
        assert!(args.contains(&"--no-production".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--max-file-size", "2"]));
        assert!(args.windows(2).any(|pair| pair == ["--threads", "3"]));
    }

    #[test]
    fn child_args_do_not_forward_production_overrides_to_security() {
        let root = PathBuf::from("/repo");
        let config_path = None;
        let opts = inspect_options(
            &root,
            config_path.as_ref(),
            InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
        );

        let args = build_child_args(&opts, security_args("src/api.ts"), opts.threads);

        assert!(!args.contains(&"--no-production".to_string()));
        assert!(!args.contains(&"--production".to_string()));
    }

    #[test]
    fn child_error_prefers_structured_stdout_message() {
        let stdout = r#"{"message":"\u001b[31mconfig failed\u001b[0m","exit_code":2}"#;
        let stderr = "warning before JSON\n";

        assert_eq!(child_error_message(2, stdout, stderr), "config failed");
    }

    #[test]
    fn parallel_child_threads_caps_optional_evidence_workers() {
        assert_eq!(parallel_child_threads(1), 1);
        assert_eq!(parallel_child_threads(4), 1);
        assert_eq!(parallel_child_threads(8), 2);
    }

    #[test]
    fn churn_section_distinguishes_no_history_unavailable_and_failure() {
        let since = fallow_engine::churn::parse_since("6m").unwrap();
        let no_history = churn_section_from_result(
            "src/api.ts",
            Ok(
                fallow_engine::health::TargetChurnOutcome::NoQualifyingChurn {
                    observed_commits: Some(2),
                    since,
                    min_commits: 3,
                    shallow_clone: false,
                },
            ),
        );
        let unavailable = churn_section_from_result(
            "src/api.ts",
            Ok(fallow_engine::health::TargetChurnOutcome::Unavailable {
                message: "git repository unavailable".to_string(),
            }),
        );
        let failed =
            churn_section_from_result("src/api.ts", Err("git churn analysis failed".to_string()));

        assert_eq!(no_history.status, InspectSectionStatus::Ok);
        assert_eq!(no_history.data.unwrap()["matched_count"], 0);
        assert_eq!(unavailable.status, InspectSectionStatus::Unavailable);
        assert_eq!(failed.status, InspectSectionStatus::Error);
    }

    #[test]
    fn churn_worker_failure_remains_a_partial_evidence_section() {
        std::thread::scope(|scope| {
            let churn = scope
                .spawn(|| -> InspectEvidenceSection { panic!("simulated churn worker failure") });
            let section = join_optional_inspect_section(
                Some(churn),
                InspectEvidenceScope::ProjectFilteredToFile,
            )
            .expect("requested churn must retain a section");

            assert_eq!(section.status, InspectSectionStatus::Error);
            assert_eq!(
                section.message.as_deref(),
                Some("inspect evidence worker panicked")
            );
        });
    }
}
