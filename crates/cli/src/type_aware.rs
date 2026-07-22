//! Backend-neutral protocol for the experimental type-aware refinement pass.

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use fallow_types::envelope::TypeAwareMeta;
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::UnusedClassMemberFinding;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

const PROTOCOL_VERSION: u32 = 1;
const OPERATION: &str = "class-member-uses";
const BACKEND: &str = "typescript-go";
const BACKEND_VERSION: &str = "7.0.2";
const SIDECAR_BINARY: &str = "fallow-type-aware";
const SIDECAR_TIMEOUT: Duration = Duration::from_mins(2);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_CHARS: usize = 4_096;
const MAX_WARNINGS: usize = 20;
const MAX_WARNING_CHARS: usize = 512;
const MAX_SELECTED_TSCONFIGS: usize = 256;

#[derive(Debug)]
pub struct TypeAwareOutcome {
    pub meta: TypeAwareMeta,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct TypeAwareError(String);

impl std::fmt::Display for TypeAwareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for TypeAwareError {}

impl From<String> for TypeAwareError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeAwareRequest {
    protocol_version: u32,
    operation: &'static str,
    root: String,
    candidates: Vec<ClassMemberCandidate>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ClassMemberCandidate {
    id: usize,
    path: String,
    parent_name: String,
    member_name: String,
    kind: MemberKind,
    line: u32,
    col: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeAwareResponse {
    protocol_version: u32,
    backend: String,
    backend_version: String,
    selected_tsconfigs: Vec<String>,
    confirmed_used_candidate_ids: Vec<usize>,
    unresolved_candidate_ids: Vec<usize>,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

/// Run the semantic sidecar and remove only candidates it positively confirms
/// are used. Every unconfirmed candidate remains in the result.
pub fn refine_unused_class_members(
    root: &Path,
    findings: &mut Vec<UnusedClassMemberFinding>,
) -> Result<TypeAwareOutcome, TypeAwareError> {
    let root = root.canonicalize().map_err(|err| {
        TypeAwareError(format!(
            "failed to resolve project root {}: {err}",
            root.display()
        ))
    })?;
    let sidecar = discover_type_aware_sidecar(&root)?;
    let request = build_request(&root, findings)?;
    let response = run_sidecar(&sidecar, &root, &request, SIDECAR_TIMEOUT)?;
    let validated = validate_response(&request, response)?;

    let confirmed_indices = validated
        .confirmed_used_candidate_ids
        .iter()
        .copied()
        .collect::<FxHashSet<_>>();
    let mut index = 0_usize;
    findings.retain(|_| {
        let retain = !confirmed_indices.contains(&index);
        index += 1;
        retain
    });

    let candidate_count = request.candidates.len();
    let confirmed_used_count = validated.confirmed_used_candidate_ids.len();
    let unresolved_count = validated.unresolved_candidate_ids.len();
    let warning_count = validated.warnings.len();
    Ok(TypeAwareOutcome {
        meta: TypeAwareMeta {
            protocol_version: validated.protocol_version,
            backend: validated.backend,
            backend_version: validated.backend_version,
            selected_tsconfigs: validated.selected_tsconfigs,
            candidate_count,
            confirmed_used_count,
            unresolved_count,
            warning_count,
            elapsed_ms: validated.elapsed_ms,
        },
        warnings: validated.warnings,
    })
}

fn build_request(
    root: &Path,
    findings: &[UnusedClassMemberFinding],
) -> Result<TypeAwareRequest, String> {
    let candidates = findings
        .iter()
        .enumerate()
        .map(|(index, finding)| {
            let member = &finding.member;
            Ok(ClassMemberCandidate {
                id: index,
                path: relative_protocol_path(root, &member.path)?,
                parent_name: member.parent_name.clone(),
                member_name: member.member_name.clone(),
                kind: member.kind,
                line: member.line,
                col: member.col,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(TypeAwareRequest {
        protocol_version: PROTOCOL_VERSION,
        operation: OPERATION,
        root: path_to_protocol_string(root),
        candidates,
    })
}

fn relative_protocol_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).map_err(|_| {
            format!(
                "type-aware candidate path {} is outside project root {}",
                path.display(),
                root.display()
            )
        })?
    } else {
        path
    };
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "type-aware candidate path {} is not project-relative",
            path.display()
        ));
    }
    Ok(path_to_protocol_string(relative))
}

fn path_to_protocol_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn discover_type_aware_sidecar(root: &Path) -> Result<PathBuf, String> {
    if let Some(value) = non_empty_env("FALLOW_TYPE_AWARE_BIN") {
        let path = PathBuf::from(&value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "FALLOW_TYPE_AWARE_BIN is set to {value}, but no file exists there. Unset it to use automatic discovery or point it at the {SIDECAR_BINARY} executable."
        ));
    }

    if let Some(path) = find_project_local_sidecar(root) {
        return Ok(path);
    }
    if let Some(path) = find_on_path(SIDECAR_BINARY) {
        return Ok(path);
    }

    Err(format!(
        "Type-aware sidecar `{SIDECAR_BINARY}` was not found. Build the repository reference sidecar and set FALLOW_TYPE_AWARE_BIN to its executable. The normal command still works without --type-aware."
    ))
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn find_project_local_sidecar(root: &Path) -> Option<PathBuf> {
    root.ancestors().find_map(|ancestor| {
        let bin_dir = ancestor.join("node_modules").join(".bin");
        binary_names(SIDECAR_BINARY)
            .into_iter()
            .map(|name| bin_dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        binary_names(binary)
            .into_iter()
            .map(|name| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn binary_names(binary: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            binary.to_owned(),
            format!("{binary}.exe"),
            format!("{binary}.cmd"),
            format!("{binary}.bat"),
        ]
    } else {
        vec![binary.to_owned()]
    }
}

fn run_sidecar(
    sidecar: &Path,
    root: &Path,
    request: &TypeAwareRequest,
    timeout: Duration,
) -> Result<TypeAwareResponse, String> {
    let mut command = Command::new(sidecar);
    command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::signal::ScopedChild::spawn(&mut command)
        .map_err(|err| format!("failed to spawn {}: {err}", sidecar.display()))?;

    let write_result = child
        .take_stdin()
        .ok_or_else(|| "type-aware sidecar stdin was not available".to_owned())
        .and_then(|mut stdin| {
            serde_json::to_writer(&mut stdin, request)
                .map_err(|err| format!("failed to serialize type-aware request: {err}"))?;
            stdin
                .write_all(b"\n")
                .and_then(|()| stdin.flush())
                .map_err(|err| format!("failed to write type-aware request: {err}"))
        });
    if let Err(error) = write_result {
        terminate_process(child.id());
        let _ = child.wait();
        return Err(error);
    }

    let output = wait_with_timeout(child, timeout)?;
    validate_process_output(&output)?;
    if output.stdout.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "type-aware sidecar response exceeded the {MAX_RESPONSE_BYTES}-byte limit"
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse type-aware sidecar response: {err}"))
}

fn wait_with_timeout(
    child: crate::signal::ScopedChild,
    timeout: Duration,
) -> Result<Output, String> {
    let pid = child.id();
    let timed_out = Arc::new(AtomicBool::new(false));
    let watcher_timed_out = Arc::clone(&timed_out);
    let (done_tx, done_rx) = mpsc::channel();
    let watcher = std::thread::spawn(move || match done_rx.recv_timeout(timeout) {
        Err(mpsc::RecvTimeoutError::Timeout) => {
            watcher_timed_out.store(true, Ordering::Release);
            terminate_process(pid);
        }
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
    });
    let output = child.wait_with_output();
    let _ = done_tx.send(());
    let _ = watcher.join();

    if timed_out.load(Ordering::Acquire) {
        return Err(format!(
            "type-aware sidecar timed out after {} seconds",
            timeout.as_secs_f64()
        ));
    }
    output.map_err(|err| format!("failed to wait for type-aware sidecar: {err}"))
}

#[cfg(unix)]
fn terminate_process(pid: u32) {
    let _ = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(windows)]
fn terminate_process(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(unix, windows)))]
fn terminate_process(_pid: u32) {}

fn validate_process_output(output: &Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = bounded_text(&output.stderr, MAX_STDERR_CHARS);
    let suffix = if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    };
    Err(format!(
        "type-aware sidecar exited with status {}{suffix}",
        output.status
    ))
}

fn bounded_text(bytes: &[u8], max_chars: usize) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn validate_response(
    request: &TypeAwareRequest,
    response: TypeAwareResponse,
) -> Result<TypeAwareResponse, String> {
    if response.protocol_version != PROTOCOL_VERSION {
        return Err(format!(
            "unsupported type-aware protocol version {}; expected {PROTOCOL_VERSION}",
            response.protocol_version
        ));
    }
    if response.backend != BACKEND {
        return Err(format!(
            "unsupported type-aware backend `{}`; expected `{BACKEND}`",
            response.backend
        ));
    }
    validate_backend_version(&response.backend_version)?;
    validate_selected_tsconfigs(&response.selected_tsconfigs, request.candidates.is_empty())?;
    validate_warnings(&response.warnings)?;

    let known = request
        .candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<FxHashSet<_>>();
    let confirmed = validate_id_list(
        "confirmed_used_candidate_ids",
        &response.confirmed_used_candidate_ids,
        &known,
    )?;
    let unresolved = validate_id_list(
        "unresolved_candidate_ids",
        &response.unresolved_candidate_ids,
        &known,
    )?;

    if let Some(id) = confirmed.intersection(&unresolved).next() {
        return Err(format!(
            "type-aware response candidate ID `{id}` is both confirmed and unresolved"
        ));
    }
    let classified = confirmed
        .union(&unresolved)
        .copied()
        .collect::<FxHashSet<_>>();
    if classified.len() != known.len() {
        let mut missing = known.difference(&classified).copied().collect::<Vec<_>>();
        missing.sort_unstable();
        return Err(format!(
            "type-aware response omitted candidate IDs: {}",
            missing
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(response)
}

fn validate_backend_version(version: &str) -> Result<(), String> {
    if version != BACKEND_VERSION {
        return Err(format!(
            "unsupported type-aware backend version `{version}`; expected TypeScript {BACKEND_VERSION}"
        ));
    }
    Ok(())
}

fn validate_selected_tsconfigs(configs: &[String], no_candidates: bool) -> Result<(), String> {
    if configs.len() > MAX_SELECTED_TSCONFIGS {
        return Err(format!(
            "type-aware response selected more than {MAX_SELECTED_TSCONFIGS} tsconfig files"
        ));
    }
    if configs.is_empty() && !no_candidates {
        return Err("type-aware response did not select a tsconfig for any candidate".to_owned());
    }
    let mut previous: Option<&str> = None;
    for config in configs {
        if config.trim().is_empty() || Path::new(config).is_absolute() {
            return Err(format!(
                "type-aware response contains invalid tsconfig path `{config}`"
            ));
        }
        if Path::new(config).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!(
                "type-aware response tsconfig path `{config}` is not project-relative"
            ));
        }
        if previous.is_some_and(|value| value >= config.as_str()) {
            return Err(
                "type-aware response selected_tsconfigs must be sorted and unique".to_owned(),
            );
        }
        previous = Some(config);
    }
    Ok(())
}

fn validate_warnings(warnings: &[String]) -> Result<(), String> {
    if warnings.len() > MAX_WARNINGS {
        return Err(format!(
            "type-aware response exceeded the {MAX_WARNINGS}-warning limit"
        ));
    }
    if let Some(warning) = warnings
        .iter()
        .find(|warning| warning.trim().is_empty() || warning.chars().count() > MAX_WARNING_CHARS)
    {
        return Err(format!(
            "type-aware response contains an empty or oversized warning: `{}`",
            warning.chars().take(80).collect::<String>()
        ));
    }
    Ok(())
}

fn validate_id_list(
    field: &str,
    ids: &[usize],
    known: &FxHashSet<usize>,
) -> Result<FxHashSet<usize>, String> {
    let mut seen = FxHashSet::default();
    seen.reserve(ids.len());
    for id in ids {
        if !known.contains(id) {
            return Err(format!(
                "type-aware response {field} contains unknown candidate ID `{id}`"
            ));
        }
        if !seen.insert(*id) {
            return Err(format!(
                "type-aware response {field} contains duplicate candidate ID `{id}`"
            ));
        }
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::UnusedMember;

    fn findings() -> Vec<UnusedClassMemberFinding> {
        let mut findings = Vec::new();
        for (line, member_name) in [(4, "used"), (8, "dead")] {
            findings.push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("src/service.ts"),
                parent_name: "Service".to_owned(),
                member_name: member_name.to_owned(),
                kind: MemberKind::ClassMethod,
                line,
                col: 2,
            }));
        }
        findings
    }

    fn request_with_candidates() -> TypeAwareRequest {
        build_request(Path::new("/project"), &findings()).expect("request")
    }

    fn valid_response() -> TypeAwareResponse {
        TypeAwareResponse {
            protocol_version: PROTOCOL_VERSION,
            backend: BACKEND.to_owned(),
            backend_version: "7.0.2".to_owned(),
            selected_tsconfigs: vec!["tsconfig.json".to_owned()],
            confirmed_used_candidate_ids: vec![0],
            unresolved_candidate_ids: vec![1],
            warnings: vec![],
            elapsed_ms: 12,
        }
    }

    #[test]
    fn request_contains_only_class_member_candidates() {
        let request = request_with_candidates();
        assert_eq!(request.protocol_version, 1);
        assert_eq!(request.operation, "class-member-uses");
        assert_eq!(request.candidates.len(), 2);
        assert_eq!(request.candidates[0].path, "src/service.ts");
        assert_eq!(request.candidates[0].id, 0);
    }

    #[test]
    fn accepts_complete_conservative_response() {
        let request = request_with_candidates();
        let response = validate_response(&request, valid_response()).expect("valid response");
        assert_eq!(response.confirmed_used_candidate_ids, [0]);
        assert_eq!(response.unresolved_candidate_ids, [1]);
    }

    #[test]
    fn rejects_unknown_duplicate_overlapping_and_missing_ids() {
        let request = request_with_candidates();

        let mut unknown = valid_response();
        unknown.confirmed_used_candidate_ids = vec![99_999_999];
        assert!(validate_response(&request, unknown).is_err());

        let mut duplicate = valid_response();
        duplicate.confirmed_used_candidate_ids = vec![0, 0];
        assert!(validate_response(&request, duplicate).is_err());

        let mut overlapping = valid_response();
        overlapping.unresolved_candidate_ids = vec![0, 1];
        assert!(validate_response(&request, overlapping).is_err());

        let mut missing = valid_response();
        missing.unresolved_candidate_ids.clear();
        assert!(validate_response(&request, missing).is_err());
    }

    #[test]
    fn rejects_protocol_backend_version_and_nondeterministic_configs() {
        let request = request_with_candidates();

        let mut protocol = valid_response();
        protocol.protocol_version = 2;
        assert!(validate_response(&request, protocol).is_err());

        let mut backend = valid_response();
        backend.backend = "other".to_owned();
        assert!(validate_response(&request, backend).is_err());

        let mut version = valid_response();
        version.backend_version = "6.9.0".to_owned();
        assert!(validate_response(&request, version).is_err());

        let mut newer_version = valid_response();
        newer_version.backend_version = "7.1.0".to_owned();
        assert!(validate_response(&request, newer_version).is_err());

        let mut configs = valid_response();
        configs.selected_tsconfigs = vec!["z.json".to_owned(), "a.json".to_owned()];
        assert!(validate_response(&request, configs).is_err());
    }

    #[test]
    fn malformed_or_extended_response_is_rejected_by_serde() {
        let malformed = br#"{"protocol_version":1}"#;
        assert!(serde_json::from_slice::<TypeAwareResponse>(malformed).is_err());

        let extended = br#"{
            "protocol_version":1,
            "backend":"typescript-go",
            "backend_version":"7.0.2",
            "selected_tsconfigs":[],
            "confirmed_used_candidate_ids":[],
            "unresolved_candidate_ids":[],
            "warnings":[],
            "elapsed_ms":0,
            "unexpected":true
        }"#;
        assert!(serde_json::from_slice::<TypeAwareResponse>(extended).is_err());
    }

    #[test]
    fn warning_and_tsconfig_bounds_are_enforced() {
        let request = request_with_candidates();
        let mut warning = valid_response();
        warning.warnings = vec!["warning".to_owned(); MAX_WARNINGS + 1];
        assert!(validate_response(&request, warning).is_err());

        let mut absolute = valid_response();
        absolute.selected_tsconfigs = vec!["/project/tsconfig.json".to_owned()];
        assert!(validate_response(&request, absolute).is_err());
    }

    #[test]
    fn process_error_output_is_bounded() {
        let text = bounded_text(&vec![b'x'; MAX_STDERR_CHARS + 100], MAX_STDERR_CHARS);
        assert_eq!(text.chars().count(), MAX_STDERR_CHARS);
    }
}
