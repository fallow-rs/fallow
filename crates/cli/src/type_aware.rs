//! Backend-neutral protocol for the experimental type-aware refinement pass.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use fallow_types::envelope::{
    TypeAwareAbstentionCounts, TypeAwareAbstentionReason, TypeAwareMeta, TypeAwarePhaseTimings,
    TypeAwareProjectMeta, TypeAwareProjectSource, TypeAwareProjectStatus,
};
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::UnusedClassMemberFinding;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

const PROTOCOL_VERSION: u32 = 2;
const OPERATION: &str = "class-member-uses";
const SIDECAR_VERSION_REQUIREMENT: &str = ">=0.1.0, <0.2.0";
const BACKEND: &str = "typescript-go";
const BACKEND_VERSION: &str = "7.0.2";
const SIDECAR_BINARY: &str = "fallow-type-aware";
const SIDECAR_TIMEOUT: Duration = Duration::from_mins(2);
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = MAX_STDERR_CHARS * 4;
const MAX_STDERR_CHARS: usize = 4_096;
const MAX_WARNINGS: usize = 20;
const MAX_WARNING_CHARS: usize = 512;
const MAX_SELECTED_TSCONFIGS: usize = 256;
const MAX_CANDIDATES: usize = 25_000;

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
    projects: Vec<String>,
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeAwareResponse {
    protocol_version: u32,
    sidecar_version: String,
    backend: String,
    backend_version: String,
    selected_tsconfigs: Vec<String>,
    confirmed_used_candidate_ids: Vec<usize>,
    unresolved_candidate_ids: Vec<usize>,
    abstentions: Vec<TypeAwareAbstention>,
    projects: Vec<TypeAwareProjectResponse>,
    phase_timings_ms: TypeAwarePhaseTimingsResponse,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeAwarePhaseTimingsResponse {
    project_setup: u64,
    diagnostics: u64,
    symbol_scan: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeAwareAbstention {
    candidate_id: usize,
    reason: TypeAwareAbstentionReason,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeAwareProjectResponse {
    config: String,
    source: TypeAwareProjectSource,
    status: TypeAwareProjectStatus,
    candidate_count: usize,
    confirmed_used_count: usize,
    unresolved_count: usize,
    abstained_count: usize,
    blocking_diagnostic_count: usize,
    source_file_count: usize,
    #[serde(default)]
    abstain_reason: Option<TypeAwareAbstentionReason>,
}

/// Run the semantic sidecar and remove only candidates it positively confirms
/// are used. Every unconfirmed candidate remains in the result.
pub fn refine_unused_class_members(
    root: &Path,
    findings: &mut Vec<UnusedClassMemberFinding>,
    projects: &[PathBuf],
) -> Result<Option<TypeAwareOutcome>, TypeAwareError> {
    if findings.is_empty() && projects.is_empty() {
        return Ok(None);
    }
    let root = canonicalize_root(root)?;
    if findings.is_empty() {
        resolve_explicit_projects(&root, projects)?;
        return Ok(None);
    }
    let request = build_request(&root, findings, projects)?;
    let sidecar = discover_type_aware_sidecar(&root)?;
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
    let abstained_count = validated.abstentions.len();
    let mut abstention_reasons = TypeAwareAbstentionCounts::default();
    for abstention in &validated.abstentions {
        match abstention.reason {
            TypeAwareAbstentionReason::NoProject => abstention_reasons.no_project += 1,
            TypeAwareAbstentionReason::AmbiguousProject => {
                abstention_reasons.ambiguous_project += 1;
            }
            TypeAwareAbstentionReason::BlockingDiagnostics => {
                abstention_reasons.blocking_diagnostics += 1;
            }
        }
    }
    let warning_count = validated.warnings.len();
    let warnings = validated.warnings.clone();
    Ok(Some(TypeAwareOutcome {
        meta: TypeAwareMeta {
            protocol_version: validated.protocol_version,
            sidecar_version: validated.sidecar_version,
            backend: validated.backend,
            backend_version: validated.backend_version,
            selected_tsconfigs: validated.selected_tsconfigs,
            candidate_count,
            confirmed_used_count,
            unresolved_count,
            abstained_count,
            abstention_reasons,
            projects: validated
                .projects
                .into_iter()
                .map(type_aware_project_meta)
                .collect(),
            warning_count,
            warnings: warnings.clone(),
            elapsed_ms: validated.elapsed_ms,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: validated.phase_timings_ms.project_setup,
                diagnostics: validated.phase_timings_ms.diagnostics,
                symbol_scan: validated.phase_timings_ms.symbol_scan,
            },
        },
        warnings,
    }))
}

fn canonicalize_root(root: &Path) -> Result<PathBuf, TypeAwareError> {
    root.canonicalize().map_err(|err| {
        TypeAwareError(format!(
            "failed to resolve project root {}: {err}",
            root.display()
        ))
    })
}

fn type_aware_project_meta(project: TypeAwareProjectResponse) -> TypeAwareProjectMeta {
    TypeAwareProjectMeta {
        config: project.config,
        source: project.source,
        status: project.status,
        candidate_count: project.candidate_count,
        confirmed_used_count: project.confirmed_used_count,
        unresolved_count: project.unresolved_count,
        abstained_count: project.abstained_count,
        blocking_diagnostic_count: project.blocking_diagnostic_count,
        source_file_count: project.source_file_count,
        abstain_reason: project.abstain_reason,
    }
}

fn build_request(
    root: &Path,
    findings: &[UnusedClassMemberFinding],
    projects: &[PathBuf],
) -> Result<TypeAwareRequest, String> {
    if findings.len() > MAX_CANDIDATES {
        return Err(format!(
            "type-aware refinement supports at most {MAX_CANDIDATES} candidates per run"
        ));
    }
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
        projects: resolve_explicit_projects(root, projects)?,
        candidates,
    })
}

fn resolve_explicit_projects(root: &Path, projects: &[PathBuf]) -> Result<Vec<String>, String> {
    if projects.len() > MAX_SELECTED_TSCONFIGS {
        return Err(format!(
            "type-aware refinement supports at most {MAX_SELECTED_TSCONFIGS} explicit projects"
        ));
    }
    let mut resolved = projects
        .iter()
        .map(|project| {
            let candidate = if project.is_absolute() {
                project.clone()
            } else {
                root.join(project)
            };
            let canonical = candidate.canonicalize().map_err(|err| {
                format!(
                    "failed to resolve type-aware project {}: {err}. Pass an existing tsconfig path with --type-aware-project",
                    project.display()
                )
            })?;
            if !canonical.is_file() {
                return Err(format!(
                    "type-aware project {} is not a file. Pass an existing tsconfig path with --type-aware-project",
                    project.display()
                ));
            }
            Ok(path_to_protocol_string(&canonical))
        })
        .collect::<Result<Vec<_>, String>>()?;
    resolved.sort();
    resolved.dedup();
    Ok(resolved)
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

fn discover_type_aware_sidecar(_root: &Path) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().ok();
    discover_type_aware_sidecar_from(
        non_empty_env("FALLOW_TYPE_AWARE_BIN").as_deref(),
        current_exe.as_deref(),
    )
}

fn discover_type_aware_sidecar_from(
    override_value: Option<&str>,
    current_exe: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(value) = override_value {
        let path = PathBuf::from(value);
        if path.is_file() {
            return canonical_sidecar_path(&path);
        }
        return Err(format!(
            "FALLOW_TYPE_AWARE_BIN is set to {value}, but no file exists there. Point it at a trusted {SIDECAR_BINARY} executable."
        ));
    }

    if let Some(path) = current_exe.and_then(find_installed_sidecar) {
        return Ok(path);
    }

    Err(format!(
        "Type-aware sidecar `{SIDECAR_BINARY}` was not found next to the active Fallow executable. Install it in the same directory as Fallow or set FALLOW_TYPE_AWARE_BIN to a trusted executable path. Project-local node_modules and PATH are intentionally not searched. The normal command still works without --type-aware."
    ))
}

fn canonical_sidecar_path(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|err| {
        format!(
            "failed to resolve type-aware sidecar {}: {err}",
            path.display()
        )
    })
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[expect(
    clippy::filetype_is_file,
    reason = "security-sensitive sidecar discovery accepts regular files only"
)]
fn find_installed_sidecar(current_exe: &Path) -> Option<PathBuf> {
    let current_exe = current_exe.canonicalize().ok()?;
    let install_dir = current_exe.parent()?;
    binary_names(SIDECAR_BINARY).into_iter().find_map(|name| {
        let candidate = install_dir.join(name);
        let metadata = candidate.symlink_metadata().ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        let canonical = candidate.canonicalize().ok()?;
        (canonical.parent() == Some(install_dir)).then_some(canonical)
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
    let mut request_bytes = serde_json::to_vec(request)
        .map_err(|err| format!("failed to serialize type-aware request: {err}"))?;
    if request_bytes.len() > MAX_REQUEST_BYTES {
        return Err(format!(
            "type-aware request exceeded the {MAX_REQUEST_BYTES} byte limit"
        ));
    }
    request_bytes.push(b'\n');

    let mut command = sidecar_command(sidecar, root)?;
    let mut child = crate::signal::ScopedChild::spawn_process_tree(&mut command)
        .map_err(|err| format!("failed to spawn {}: {err}", sidecar.display()))?;
    let Some(terminator) = child.process_tree_terminator() else {
        return Err("type-aware sidecar process tree was not available".to_owned());
    };
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.take_stdin(), child.take_stdout(), child.take_stderr())
    else {
        let _ = terminator.terminate();
        let _ = child.wait();
        return Err("type-aware sidecar pipes were not available".to_owned());
    };
    let timeout_guard = SidecarTimeout::start(terminator.clone(), timeout);

    let writer = std::thread::spawn(move || write_request(stdin, &request_bytes));
    let stdout_terminator = terminator.clone();
    let stdout_reader = std::thread::spawn(move || {
        read_bounded_stream(
            stdout,
            MAX_RESPONSE_BYTES,
            "response",
            Some(stdout_terminator),
        )
    });
    let stderr_reader = std::thread::spawn(move || {
        read_bounded_stream(stderr, MAX_STDERR_BYTES, "stderr", Some(terminator))
    });

    let status = child
        .wait()
        .map_err(|err| format!("failed to wait for type-aware sidecar: {err}"));
    let write_result = join_sidecar_worker("stdin writer", writer);
    let stdout_result = join_sidecar_worker("stdout reader", stdout_reader);
    let stderr_result = join_sidecar_worker("stderr reader", stderr_reader);
    let timeout_result = timeout_guard.finish();

    timeout_result?;
    let stdout = stdout_result?;
    let stderr = stderr_result?;
    let status = status?;
    write_result?;
    let output = Output {
        status,
        stdout,
        stderr,
    };
    validate_process_output(&output)?;
    serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse type-aware sidecar response: {err}"))
}

fn sidecar_command(sidecar: &Path, root: &Path) -> Result<Command, String> {
    let install_dir = sidecar.parent().ok_or_else(|| {
        format!(
            "type-aware sidecar {} has no trusted parent directory",
            sidecar.display()
        )
    })?;
    let mut command = Command::new(sidecar);
    command
        .current_dir(install_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    restrict_sidecar_environment(&mut command, root);
    Ok(command)
}

fn restrict_sidecar_environment(command: &mut Command, root: &Path) {
    const ALLOWED_ENV: &[&str] = &[
        "HOME",
        "PATHEXT",
        "COMSPEC",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "USERPROFILE",
        "WINDIR",
    ];
    let values = ALLOWED_ENV
        .iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    let path = std::env::var_os("PATH").and_then(|value| sanitize_search_path(root, &value));
    command.env_clear();
    command.envs(values);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    command.env("NoDefaultCurrentDirectoryInExePath", "1");
}

fn sanitize_search_path(root: &Path, value: &OsStr) -> Option<OsString> {
    let mut safe = Vec::new();
    for entry in std::env::split_paths(value) {
        if !entry.is_absolute() {
            continue;
        }
        let Ok(canonical) = entry.canonicalize() else {
            continue;
        };
        if canonical.starts_with(root) || safe.contains(&canonical) {
            continue;
        }
        safe.push(canonical);
    }
    std::env::join_paths(safe).ok()
}

fn write_request(mut stdin: std::process::ChildStdin, request: &[u8]) -> Result<(), String> {
    stdin
        .write_all(request)
        .and_then(|()| stdin.flush())
        .map_err(|err| format!("failed to write type-aware request: {err}"))
}

fn read_bounded_stream(
    reader: impl Read,
    limit: usize,
    stream: &str,
    terminator: Option<crate::signal::scoped_child::ProcessTreeTerminator>,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("failed to read type-aware sidecar {stream}: {err}"))?;
    if bytes.len() > limit {
        if let Some(terminator) = terminator {
            let _ = terminator.terminate();
        }
        return Err(format!(
            "type-aware sidecar {stream} exceeded the {limit}-byte limit"
        ));
    }
    Ok(bytes)
}

fn join_sidecar_worker<T>(
    name: &str,
    worker: std::thread::JoinHandle<Result<T, String>>,
) -> Result<T, String> {
    worker
        .join()
        .map_err(|_| format!("type-aware sidecar {name} panicked"))?
}

struct SidecarTimeout {
    done: mpsc::Sender<()>,
    timed_out: Arc<AtomicBool>,
    watcher: std::thread::JoinHandle<()>,
    duration: Duration,
}

impl SidecarTimeout {
    fn start(
        terminator: crate::signal::scoped_child::ProcessTreeTerminator,
        duration: Duration,
    ) -> Self {
        let timed_out = Arc::new(AtomicBool::new(false));
        let watcher_timed_out = Arc::clone(&timed_out);
        let (done, done_rx) = mpsc::channel();
        let watcher = std::thread::spawn(move || match done_rx.recv_timeout(duration) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                watcher_timed_out.store(true, Ordering::Release);
                let _ = terminator.terminate();
            }
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
        });
        Self {
            done,
            timed_out,
            watcher,
            duration,
        }
    }

    fn finish(self) -> Result<(), String> {
        let _ = self.done.send(());
        let _ = self.watcher.join();
        if self.timed_out.load(Ordering::Acquire) {
            return Err(format!(
                "type-aware sidecar timed out after {} seconds",
                self.duration.as_secs_f64()
            ));
        }
        Ok(())
    }
}

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
    validate_sidecar_version(&response.sidecar_version)?;
    if response.backend != BACKEND {
        return Err(format!(
            "unsupported type-aware backend `{}`; expected `{BACKEND}`",
            response.backend
        ));
    }
    validate_backend_version(&response.backend_version)?;
    validate_selected_tsconfigs(&response.selected_tsconfigs)?;
    validate_warnings(&response.warnings)?;
    validate_projects(&response.projects, &response.selected_tsconfigs)?;
    let phase_total = response
        .phase_timings_ms
        .project_setup
        .saturating_add(response.phase_timings_ms.diagnostics)
        .saturating_add(response.phase_timings_ms.symbol_scan);
    if phase_total > response.elapsed_ms.saturating_add(3) {
        return Err("type-aware response phase timings exceed total elapsed time".to_owned());
    }

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
    let abstained = validate_abstentions(&response.abstentions, &known)?;

    validate_disjoint_ids("confirmed", &confirmed, "unresolved", &unresolved)?;
    validate_disjoint_ids("confirmed", &confirmed, "abstained", &abstained)?;
    validate_disjoint_ids("unresolved", &unresolved, "abstained", &abstained)?;
    let classified = confirmed
        .union(&unresolved)
        .copied()
        .chain(abstained.iter().copied())
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
    validate_project_totals(&response, known.len())?;

    Ok(response)
}

fn validate_disjoint_ids(
    left_name: &str,
    left: &FxHashSet<usize>,
    right_name: &str,
    right: &FxHashSet<usize>,
) -> Result<(), String> {
    if let Some(id) = left.intersection(right).next() {
        return Err(format!(
            "type-aware response candidate ID `{id}` is both {left_name} and {right_name}"
        ));
    }
    Ok(())
}

fn validate_backend_version(version: &str) -> Result<(), String> {
    if version != BACKEND_VERSION {
        return Err(format!(
            "unsupported type-aware backend version `{version}`; expected TypeScript {BACKEND_VERSION}"
        ));
    }
    Ok(())
}

fn validate_sidecar_version(version: &str) -> Result<(), String> {
    let version = semver::Version::parse(version)
        .map_err(|_| format!("invalid type-aware sidecar version `{version}`"))?;
    let requirement = semver::VersionReq::parse(SIDECAR_VERSION_REQUIREMENT)
        .map_err(|err| format!("invalid built-in sidecar version requirement: {err}"))?;
    if !requirement.matches(&version) {
        return Err(format!(
            "unsupported type-aware sidecar version `{version}`; expected {SIDECAR_VERSION_REQUIREMENT}"
        ));
    }
    Ok(())
}

fn validate_selected_tsconfigs(configs: &[String]) -> Result<(), String> {
    if configs.len() > MAX_SELECTED_TSCONFIGS {
        return Err(format!(
            "type-aware response selected more than {MAX_SELECTED_TSCONFIGS} tsconfig files"
        ));
    }
    let mut previous: Option<&str> = None;
    for config in configs {
        if config.trim().is_empty() || Path::new(config).is_absolute() {
            return Err(format!(
                "type-aware response contains invalid tsconfig path `{config}`"
            ));
        }
        if Path::new(config)
            .components()
            .any(|component| matches!(component, Component::RootDir | Component::Prefix(_)))
        {
            return Err(format!(
                "type-aware response tsconfig path `{config}` is not relative"
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

fn validate_abstentions(
    abstentions: &[TypeAwareAbstention],
    known: &FxHashSet<usize>,
) -> Result<FxHashSet<usize>, String> {
    let mut ids = FxHashSet::default();
    ids.reserve(abstentions.len());
    let mut previous = None;
    for abstention in abstentions {
        if !known.contains(&abstention.candidate_id) {
            return Err(format!(
                "type-aware response abstentions contains unknown candidate ID `{}`",
                abstention.candidate_id
            ));
        }
        if !ids.insert(abstention.candidate_id) {
            return Err(format!(
                "type-aware response abstentions contains duplicate candidate ID `{}`",
                abstention.candidate_id
            ));
        }
        if previous.is_some_and(|id| id >= abstention.candidate_id) {
            return Err(
                "type-aware response abstentions must be sorted by candidate ID".to_owned(),
            );
        }
        previous = Some(abstention.candidate_id);
    }
    Ok(ids)
}

fn validate_projects(
    projects: &[TypeAwareProjectResponse],
    selected_tsconfigs: &[String],
) -> Result<(), String> {
    if projects.len() > MAX_SELECTED_TSCONFIGS {
        return Err(format!(
            "type-aware response contains more than {MAX_SELECTED_TSCONFIGS} project results"
        ));
    }
    let configs = projects
        .iter()
        .map(|project| project.config.as_str())
        .collect::<Vec<_>>();
    if configs != selected_tsconfigs {
        return Err("type-aware response project configs must match selected_tsconfigs".to_owned());
    }
    for project in projects {
        if project.candidate_count
            != project.confirmed_used_count + project.unresolved_count + project.abstained_count
        {
            return Err(format!(
                "type-aware response project `{}` has inconsistent candidate counts",
                project.config
            ));
        }
        if project.source_file_count == 0 {
            return Err(format!(
                "type-aware response project `{}` has no source files",
                project.config
            ));
        }
        match project.status {
            TypeAwareProjectStatus::Refined
                if project.abstained_count == 0
                    && project.blocking_diagnostic_count == 0
                    && project.abstain_reason.is_none() => {}
            TypeAwareProjectStatus::Abstained
                if project.confirmed_used_count == 0
                    && project.unresolved_count == 0
                    && project.abstained_count == project.candidate_count
                    && project.blocking_diagnostic_count > 0
                    && project.abstain_reason
                        == Some(TypeAwareAbstentionReason::BlockingDiagnostics) => {}
            _ => {
                return Err(format!(
                    "type-aware response project `{}` has inconsistent status metadata",
                    project.config
                ));
            }
        }
    }
    Ok(())
}

fn validate_project_totals(
    response: &TypeAwareResponse,
    candidate_count: usize,
) -> Result<(), String> {
    let project_candidate_count = response
        .projects
        .iter()
        .map(|project| project.candidate_count)
        .sum::<usize>();
    let unassigned_count = response
        .abstentions
        .iter()
        .filter(|abstention| abstention.reason != TypeAwareAbstentionReason::BlockingDiagnostics)
        .count();
    let project_confirmed_count = response
        .projects
        .iter()
        .map(|project| project.confirmed_used_count)
        .sum::<usize>();
    let project_unresolved_count = response
        .projects
        .iter()
        .map(|project| project.unresolved_count)
        .sum::<usize>();
    let project_abstained_count = response
        .projects
        .iter()
        .map(|project| project.abstained_count)
        .sum::<usize>();
    let diagnostic_abstention_count = response
        .abstentions
        .iter()
        .filter(|abstention| abstention.reason == TypeAwareAbstentionReason::BlockingDiagnostics)
        .count();
    if project_candidate_count + unassigned_count != candidate_count
        || project_confirmed_count != response.confirmed_used_candidate_ids.len()
        || project_unresolved_count != response.unresolved_candidate_ids.len()
        || project_abstained_count != diagnostic_abstention_count
    {
        return Err(
            "type-aware response project totals do not match candidate outcomes".to_owned(),
        );
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
    if warnings.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("type-aware response warnings must be sorted and unique".to_owned());
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
    let mut previous = None;
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
        if previous.is_some_and(|previous_id| previous_id >= *id) {
            return Err(format!(
                "type-aware response {field} must be sorted and unique"
            ));
        }
        previous = Some(*id);
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
        build_request(Path::new("/project"), &findings(), &[]).expect("request")
    }

    fn valid_response() -> TypeAwareResponse {
        TypeAwareResponse {
            protocol_version: PROTOCOL_VERSION,
            sidecar_version: "0.1.0".to_owned(),
            backend: BACKEND.to_owned(),
            backend_version: "7.0.2".to_owned(),
            selected_tsconfigs: vec!["tsconfig.json".to_owned()],
            confirmed_used_candidate_ids: vec![0],
            unresolved_candidate_ids: vec![1],
            abstentions: vec![],
            projects: vec![TypeAwareProjectResponse {
                config: "tsconfig.json".to_owned(),
                source: TypeAwareProjectSource::Auto,
                status: TypeAwareProjectStatus::Refined,
                candidate_count: 2,
                confirmed_used_count: 1,
                unresolved_count: 1,
                abstained_count: 0,
                blocking_diagnostic_count: 0,
                source_file_count: 12,
                abstain_reason: None,
            }],
            warnings: vec![],
            elapsed_ms: 12,
            phase_timings_ms: TypeAwarePhaseTimingsResponse {
                project_setup: 4,
                diagnostics: 5,
                symbol_scan: 2,
            },
        }
    }

    #[test]
    fn explicit_sidecar_override_wins_over_installed_sibling() {
        let install = tempfile::tempdir().expect("temporary install directory");
        let executable = install.path().join("fallow");
        let sibling = install.path().join(SIDECAR_BINARY);
        let override_dir = tempfile::tempdir().expect("temporary override directory");
        let override_sidecar = override_dir.path().join("custom-sidecar");
        std::fs::write(&executable, []).expect("write fallow executable fixture");
        std::fs::write(&sibling, []).expect("write sibling sidecar fixture");
        std::fs::write(&override_sidecar, []).expect("write override sidecar fixture");

        let discovered =
            discover_type_aware_sidecar_from(override_sidecar.to_str(), Some(executable.as_path()))
                .expect("explicit override should be discovered");

        assert_eq!(
            discovered,
            override_sidecar
                .canonicalize()
                .expect("canonical override sidecar")
        );
    }

    #[test]
    fn invalid_explicit_sidecar_override_does_not_fall_back_to_sibling() {
        let install = tempfile::tempdir().expect("temporary install directory");
        let executable = install.path().join("fallow");
        let sibling = install.path().join(SIDECAR_BINARY);
        std::fs::write(&executable, []).expect("write fallow executable fixture");
        std::fs::write(&sibling, []).expect("write sibling sidecar fixture");
        let missing = install.path().join("missing-sidecar");

        let error = discover_type_aware_sidecar_from(missing.to_str(), Some(executable.as_path()))
            .expect_err("invalid explicit override must not fall back");

        assert!(error.contains("FALLOW_TYPE_AWARE_BIN is set"));
        assert!(error.contains("Point it at a trusted fallow-type-aware executable"));
    }

    #[test]
    fn discovers_sidecar_next_to_active_fallow_executable() {
        let install = tempfile::tempdir().expect("temporary install directory");
        let executable = install.path().join("fallow");
        let sibling = install.path().join(SIDECAR_BINARY);
        std::fs::write(&executable, []).expect("write fallow executable fixture");
        std::fs::write(&sibling, []).expect("write sibling sidecar fixture");

        let discovered = discover_type_aware_sidecar_from(None, Some(executable.as_path()))
            .expect("installed sibling should be discovered");

        assert_eq!(
            discovered,
            sibling.canonicalize().expect("canonical sibling sidecar")
        );
    }

    #[cfg(unix)]
    #[test]
    fn installed_sibling_discovery_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let install = tempfile::tempdir().expect("temporary install directory");
        let executable = install.path().join("fallow");
        std::fs::write(&executable, []).expect("write fallow executable fixture");

        let outside = tempfile::tempdir().expect("temporary external directory");
        let external_sidecar = outside.path().join(SIDECAR_BINARY);
        std::fs::write(&external_sidecar, []).expect("write external sidecar fixture");
        symlink(&external_sidecar, install.path().join(SIDECAR_BINARY))
            .expect("create sibling symlink fixture");

        assert!(find_installed_sidecar(&executable).is_none());
    }

    #[test]
    fn does_not_discover_project_local_or_path_sidecars() {
        let install = tempfile::tempdir().expect("temporary install directory");
        let executable = install.path().join("fallow");
        std::fs::write(&executable, []).expect("write fallow executable fixture");

        let project = tempfile::tempdir().expect("temporary project directory");
        let project_sidecar = project
            .path()
            .join("node_modules")
            .join(".bin")
            .join(SIDECAR_BINARY);
        std::fs::create_dir_all(project_sidecar.parent().expect("project bin directory"))
            .expect("create project bin directory");
        std::fs::write(&project_sidecar, []).expect("write project-local sidecar fixture");

        let path_dir = tempfile::tempdir().expect("temporary PATH directory");
        std::fs::write(path_dir.path().join(SIDECAR_BINARY), [])
            .expect("write PATH sidecar fixture");

        let error = discover_type_aware_sidecar_from(None, Some(executable.as_path()))
            .expect_err("untrusted discovery locations must be ignored");

        assert!(error.contains("next to the active Fallow executable"));
        assert!(error.contains("node_modules and PATH are intentionally not searched"));
    }

    #[test]
    fn child_path_removes_project_local_node_shims() {
        let project = tempfile::tempdir().expect("temporary project directory");
        let root = project
            .path()
            .canonicalize()
            .expect("canonical project root");
        let project_bin = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&project_bin).expect("create project bin directory");
        std::fs::write(project_bin.join("node"), []).expect("write project-local node shim");

        let trusted = tempfile::tempdir().expect("temporary trusted directory");
        std::fs::write(trusted.path().join("node"), []).expect("write trusted node fixture");
        let value = std::env::join_paths([
            PathBuf::from("."),
            project_bin,
            trusted.path().to_path_buf(),
        ])
        .expect("join test PATH");

        let sanitized = sanitize_search_path(&root, &value).expect("sanitized PATH");
        let entries = std::env::split_paths(&sanitized).collect::<Vec<_>>();

        assert_eq!(
            entries,
            [trusted
                .path()
                .canonicalize()
                .expect("canonical trusted path")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_sidecar_command_cannot_resolve_a_project_root_node_shim() {
        let project = tempfile::tempdir().expect("temporary project directory");
        let root = project
            .path()
            .canonicalize()
            .expect("canonical project root");
        std::fs::write(root.join("node.cmd"), "@exit /b 99\r\n")
            .expect("write project-root node shim");

        let install = tempfile::tempdir().expect("temporary trusted install directory");
        let sidecar = install.path().join("fallow-type-aware.cmd");
        let command = sidecar_command(&sidecar, &root).expect("configure sidecar command");
        let no_current_dir = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("NoDefaultCurrentDirectoryInExePath"))
            .and_then(|(_, value)| value);

        assert_eq!(command.get_current_dir(), Some(install.path()));
        assert_eq!(no_current_dir, Some(OsStr::new("1")));
    }

    #[test]
    fn request_contains_only_class_member_candidates() {
        let request = request_with_candidates();
        assert_eq!(request.protocol_version, 2);
        assert_eq!(request.operation, "class-member-uses");
        assert!(request.projects.is_empty());
        assert_eq!(request.candidates.len(), 2);
        assert_eq!(request.candidates[0].path, "src/service.ts");
        assert_eq!(request.candidates[0].id, 0);
    }

    #[test]
    fn empty_candidates_do_not_require_a_root_or_sidecar() {
        let mut findings = Vec::new();
        let outcome = refine_unused_class_members(
            Path::new("/definitely/missing/type-aware-root"),
            &mut findings,
            &[],
        )
        .expect("empty refinement should be a no-op");

        assert!(outcome.is_none());
    }

    #[test]
    fn empty_candidates_still_validate_explicit_projects() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let mut findings = Vec::new();
        let error = refine_unused_class_members(
            workspace.path(),
            &mut findings,
            &[PathBuf::from("missing-tsconfig.json")],
        )
        .expect_err("invalid explicit project must be rejected");

        assert!(
            error
                .to_string()
                .contains("failed to resolve type-aware project")
        );
    }

    #[test]
    fn explicit_ancestor_projects_are_canonicalized_and_sorted() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let root = workspace.path().join("packages/app");
        std::fs::create_dir_all(&root).expect("create package root");
        let config = workspace.path().join("tsconfig.json");
        std::fs::write(&config, "{}").expect("write ancestor config");

        let request = build_request(
            &root,
            &findings(),
            &[PathBuf::from("../../tsconfig.json"), config.clone()],
        )
        .expect("ancestor project should be accepted");

        assert_eq!(
            request.projects,
            [path_to_protocol_string(
                &config.canonicalize().expect("canonical config")
            )]
        );
    }

    #[test]
    fn accepts_complete_conservative_response() {
        let request = request_with_candidates();
        let response = validate_response(&request, valid_response()).expect("valid response");
        assert_eq!(response.confirmed_used_candidate_ids, [0]);
        assert_eq!(response.unresolved_candidate_ids, [1]);
    }

    #[test]
    fn accepts_fail_closed_project_and_selection_abstentions() {
        let request = request_with_candidates();
        let mut diagnostics = valid_response();
        diagnostics.confirmed_used_candidate_ids.clear();
        diagnostics.unresolved_candidate_ids.clear();
        diagnostics.abstentions = vec![
            TypeAwareAbstention {
                candidate_id: 0,
                reason: TypeAwareAbstentionReason::BlockingDiagnostics,
            },
            TypeAwareAbstention {
                candidate_id: 1,
                reason: TypeAwareAbstentionReason::BlockingDiagnostics,
            },
        ];
        diagnostics.projects[0].status = TypeAwareProjectStatus::Abstained;
        diagnostics.projects[0].confirmed_used_count = 0;
        diagnostics.projects[0].unresolved_count = 0;
        diagnostics.projects[0].abstained_count = 2;
        diagnostics.projects[0].blocking_diagnostic_count = 1;
        diagnostics.projects[0].abstain_reason =
            Some(TypeAwareAbstentionReason::BlockingDiagnostics);
        assert!(validate_response(&request, diagnostics).is_ok());

        let mut no_project = valid_response();
        no_project.selected_tsconfigs.clear();
        no_project.confirmed_used_candidate_ids.clear();
        no_project.unresolved_candidate_ids.clear();
        no_project.projects.clear();
        no_project.abstentions = vec![
            TypeAwareAbstention {
                candidate_id: 0,
                reason: TypeAwareAbstentionReason::NoProject,
            },
            TypeAwareAbstention {
                candidate_id: 1,
                reason: TypeAwareAbstentionReason::NoProject,
            },
        ];
        assert!(validate_response(&request, no_project).is_ok());
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

        let mut inconsistent_project = valid_response();
        inconsistent_project.projects[0].candidate_count = 99;
        assert!(validate_response(&request, inconsistent_project).is_err());
    }

    #[test]
    fn rejects_protocol_backend_version_and_nondeterministic_configs() {
        let request = request_with_candidates();

        let mut protocol = valid_response();
        protocol.protocol_version = 99;
        assert!(validate_response(&request, protocol).is_err());

        let mut backend = valid_response();
        backend.backend = "other".to_owned();
        assert!(validate_response(&request, backend).is_err());

        let mut sidecar = valid_response();
        sidecar.sidecar_version = "0.2.0".to_owned();
        assert!(validate_response(&request, sidecar).is_err());

        let mut compatible_sidecar = valid_response();
        compatible_sidecar.sidecar_version = "0.1.1".to_owned();
        assert!(validate_response(&request, compatible_sidecar).is_ok());

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

        let mut invalid_reason = serde_json::to_value(valid_response()).expect("serialize fixture");
        invalid_reason["abstentions"] =
            serde_json::json!([{"candidate_id": 0, "reason": "best-effort"}]);
        assert!(serde_json::from_value::<TypeAwareResponse>(invalid_reason).is_err());
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

        let mut ancestor = valid_response();
        ancestor.selected_tsconfigs = vec!["../../tsconfig.json".to_owned()];
        ancestor.projects[0].config = "../../tsconfig.json".to_owned();
        assert!(validate_response(&request, ancestor).is_ok());
    }

    #[test]
    fn process_error_output_is_bounded() {
        let text = bounded_text(&vec![b'x'; MAX_STDERR_CHARS + 100], MAX_STDERR_CHARS);
        assert_eq!(text.chars().count(), MAX_STDERR_CHARS);
    }

    #[test]
    fn oversized_response_is_rejected_during_read() {
        let response = std::io::Cursor::new(vec![b'x'; 65]);
        let error = read_bounded_stream(response, 64, "response", None)
            .expect_err("oversized response should fail");

        assert!(error.contains("exceeded the 64-byte limit"));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_covers_blocked_stdin() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let root = tempfile::tempdir().expect("temporary sidecar root");
        let sidecar = root.path().join("blocked-sidecar.sh");
        fs::write(&sidecar, "#!/bin/sh\nsleep 30\n").expect("write sidecar");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let mut request = request_with_candidates();
        request.candidates[0].parent_name = "x".repeat(4 * 1024 * 1024);
        let started = Instant::now();
        let error = run_sidecar(&sidecar, root.path(), &request, Duration::from_millis(100))
            .expect_err("blocked sidecar should time out");

        assert!(error.contains("timed out"), "unexpected error: {error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
