//! Backend-neutral protocol for the opt-in type-aware analysis pass.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use fallow_types::envelope::TypeAwareMeta;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const SIDECAR_BINARY: &str = "fallow-type-aware";
#[cfg(windows)]
const SIDECAR_SCRIPT_ENV: &str = "FALLOW_TYPE_AWARE_SCRIPT";
const BACKEND_FAMILY: &str = "typescript-go";
const BACKEND_VERSION: &str = "7.0.2";
pub(super) const TYPE_AWARE_PROTOCOL_VERSION: u32 = 5;
const SIDECAR_TIMEOUT: Duration = Duration::from_mins(2);
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_SEMANTIC_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = MAX_STDERR_CHARS * 4;
const MAX_STDERR_CHARS: usize = 4_096;
static NEXT_ACTIVE_SIDECAR_ID: AtomicU64 = AtomicU64::new(1);
static SIDECAR_TERMINATION_EPOCH: AtomicU64 = AtomicU64::new(0);
static SIDECAR_SHUTDOWN: AtomicBool = AtomicBool::new(false);
static ACTIVE_SIDECARS: OnceLock<Mutex<BTreeMap<u64, fallow_process::ProcessTreeTerminator>>> =
    OnceLock::new();

fn active_sidecars() -> &'static Mutex<BTreeMap<u64, fallow_process::ProcessTreeTerminator>> {
    ACTIVE_SIDECARS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Terminate every active type-aware sidecar started by this process.
///
/// Editor shutdown uses this targeted registry so an in-flight semantic query
/// stops promptly without affecting unrelated subprocesses.
pub fn terminate_active_type_aware_sidecars() {
    SIDECAR_TERMINATION_EPOCH.fetch_add(1, Ordering::SeqCst);
    let terminators = active_sidecars()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for terminator in terminators {
        let _ = terminator.terminate();
    }
}

/// Permanently close semantic sidecar spawning and terminate active children.
///
/// LSP shutdown uses this terminal lifecycle transition. The persistent gate
/// closes the race where analysis finishes query preparation after the final
/// active-process drain and would otherwise start a new sidecar.
pub fn shutdown_type_aware_sidecars() {
    SIDECAR_SHUTDOWN.store(true, Ordering::SeqCst);
    terminate_active_type_aware_sidecars();
}

struct ActiveSidecarGuard {
    id: u64,
}

impl ActiveSidecarGuard {
    fn register(terminator: fallow_process::ProcessTreeTerminator) -> Self {
        let id = NEXT_ACTIVE_SIDECAR_ID.fetch_add(1, Ordering::SeqCst);
        active_sidecars()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, terminator);
        Self { id }
    }
}

impl Drop for ActiveSidecarGuard {
    fn drop(&mut self) {
        active_sidecars()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&self.id);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TypeAwareStatus {
    pub available: bool,
    pub discovery_source: Option<&'static str>,
    #[serde(serialize_with = "fallow_types::serde_path::serialize_option")]
    pub companion_path: Option<PathBuf>,
    pub package_version: Option<String>,
    pub protocol_version: u32,
    pub backend_family: Option<String>,
    pub backend_version: Option<String>,
    pub remediation: Option<String>,
}

#[derive(Serialize)]
struct StatusRequest {
    protocol_version: u32,
    operation: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusResponse {
    package_version: String,
    protocol_version: u32,
    backend_family: String,
    backend_version: String,
}

/// Inspect the optional semantic companion without loading or analyzing a
/// TypeScript project.
pub fn status(root: &Path) -> TypeAwareStatus {
    let discovery_source = if non_empty_env("FALLOW_TYPE_AWARE_BIN").is_some() {
        "environment-override"
    } else {
        "installed-sibling"
    };
    let sidecar = match discover_type_aware_sidecar(root) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            let remediation = if discovery_source == "installed-sibling" {
                format!(
                    "Install the matching companion with: npm install --save-dev fallow-type-aware@{}",
                    env!("CARGO_PKG_VERSION")
                )
            } else {
                error
            };
            return TypeAwareStatus {
                available: false,
                discovery_source: None,
                companion_path: None,
                package_version: None,
                protocol_version: TYPE_AWARE_PROTOCOL_VERSION,
                backend_family: None,
                backend_version: None,
                remediation: Some(remediation),
            };
        }
    };
    let request = StatusRequest {
        protocol_version: TYPE_AWARE_PROTOCOL_VERSION,
        operation: "status",
    };
    match run_sidecar_json::<_, StatusResponse>(
        &sidecar,
        root,
        &request,
        SIDECAR_TIMEOUT,
        MAX_RESPONSE_BYTES,
    ) {
        Ok(response)
            if response.protocol_version == TYPE_AWARE_PROTOCOL_VERSION
                && response.package_version == env!("CARGO_PKG_VERSION")
                && response.backend_family == BACKEND_FAMILY
                && response.backend_version == BACKEND_VERSION =>
        {
            TypeAwareStatus {
                available: true,
                discovery_source: Some(discovery_source),
                companion_path: Some(sidecar),
                package_version: Some(response.package_version),
                protocol_version: response.protocol_version,
                backend_family: Some(response.backend_family),
                backend_version: Some(response.backend_version),
                remediation: None,
            }
        }
        Ok(response) => TypeAwareStatus {
            available: false,
            discovery_source: Some(discovery_source),
            companion_path: Some(sidecar),
            package_version: Some(response.package_version),
            protocol_version: response.protocol_version,
            backend_family: Some(response.backend_family),
            backend_version: Some(response.backend_version),
            remediation: Some(
                "Install the exact fallow-type-aware version that matches Fallow".to_string(),
            ),
        },
        Err(error) => TypeAwareStatus {
            available: false,
            discovery_source: Some(discovery_source),
            companion_path: Some(sidecar),
            package_version: None,
            protocol_version: TYPE_AWARE_PROTOCOL_VERSION,
            backend_family: None,
            backend_version: None,
            remediation: Some(error),
        },
    }
}

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

fn canonicalize_root(root: &Path) -> Result<PathBuf, TypeAwareError> {
    root.canonicalize().map_err(|err| {
        TypeAwareError(format!(
            "failed to resolve project root {}: {err}",
            root.display()
        ))
    })
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

pub fn run_semantic_request<Request, Response>(
    root: &Path,
    request: &Request,
) -> Result<Response, TypeAwareError>
where
    Request: Serialize + ?Sized,
    Response: DeserializeOwned,
{
    let root = canonicalize_root(root)?;
    let sidecar = discover_type_aware_sidecar(&root)?;
    run_sidecar_json(
        &sidecar,
        &root,
        request,
        SIDECAR_TIMEOUT,
        MAX_SEMANTIC_RESPONSE_BYTES,
    )
    .map_err(TypeAwareError)
}

fn run_sidecar_json<Request, Response>(
    sidecar: &Path,
    root: &Path,
    request: &Request,
    timeout: Duration,
    max_response_bytes: usize,
) -> Result<Response, String>
where
    Request: Serialize + ?Sized,
    Response: DeserializeOwned,
{
    if SIDECAR_SHUTDOWN.load(Ordering::SeqCst) {
        return Err("type-aware sidecar spawning is closed during shutdown".to_owned());
    }
    let mut request_bytes = serde_json::to_vec(request)
        .map_err(|err| format!("failed to serialize type-aware request: {err}"))?;
    if request_bytes.len() > MAX_REQUEST_BYTES {
        return Err(format!(
            "type-aware request exceeded the {MAX_REQUEST_BYTES} byte limit"
        ));
    }
    request_bytes.push(b'\n');

    let termination_epoch = SIDECAR_TERMINATION_EPOCH.load(Ordering::SeqCst);
    let mut command = sidecar_command(sidecar, root)?;
    let mut child = fallow_process::ScopedChild::spawn_process_tree(&mut command)
        .map_err(|err| format!("failed to spawn {}: {err}", sidecar.display()))?;
    let Some(terminator) = child.process_tree_terminator() else {
        return Err("type-aware sidecar process tree was not available".to_owned());
    };
    let _active_sidecar = ActiveSidecarGuard::register(terminator.clone());
    if SIDECAR_SHUTDOWN.load(Ordering::SeqCst)
        || SIDECAR_TERMINATION_EPOCH.load(Ordering::SeqCst) != termination_epoch
    {
        let _ = terminator.terminate();
        return Err("type-aware sidecar start was cancelled during shutdown".to_owned());
    }
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
            max_response_bytes,
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
    #[cfg(windows)]
    let launch_via_script = if let Some(script) = non_empty_env(SIDECAR_SCRIPT_ENV) {
        let script = canonical_sidecar_path(Path::new(&script))?;
        command.arg(script);
        true
    } else {
        false
    };
    command
        .current_dir(install_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    restrict_sidecar_environment(&mut command, root);
    #[cfg(windows)]
    if launch_via_script {
        // VS Code supplies its Electron executable as `process.execPath`.
        // Node ignores this variable, while Electron needs it to run the
        // bundled module as a plain Node process.
        command.env("ELECTRON_RUN_AS_NODE", "1");
    }
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
    terminator: Option<fallow_process::ProcessTreeTerminator>,
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
    fn start(terminator: fallow_process::ProcessTreeTerminator, duration: Duration) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    static SIDECAR_LIFECYCLE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn lock_sidecar_lifecycle_tests() -> std::sync::MutexGuard<'static, ()> {
        SIDECAR_LIFECYCLE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
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
        let electron_run_as_node = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("ELECTRON_RUN_AS_NODE"))
            .and_then(|(_, value)| value);

        assert_eq!(command.get_current_dir(), Some(install.path()));
        assert_eq!(no_current_dir, Some(OsStr::new("1")));
        assert_eq!(electron_run_as_node, None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_script_sidecar_runs_electron_as_node() {
        let _lock = lock_sidecar_lifecycle_tests();
        let project = tempfile::tempdir().expect("temporary project directory");
        let root = project
            .path()
            .canonicalize()
            .expect("canonical project root");
        let install = tempfile::tempdir().expect("temporary trusted install directory");
        let electron = install.path().join("Code.exe");
        let script = install.path().join("fallow-type-aware.mjs");
        std::fs::write(&electron, []).expect("write Electron fixture");
        std::fs::write(&script, []).expect("write sidecar script fixture");
        let previous = std::env::var_os(SIDECAR_SCRIPT_ENV);
        unsafe {
            std::env::set_var(SIDECAR_SCRIPT_ENV, &script);
        }

        let command = sidecar_command(&electron, &root).expect("configure sidecar command");

        match previous {
            Some(value) => unsafe { std::env::set_var(SIDECAR_SCRIPT_ENV, value) },
            None => unsafe { std::env::remove_var(SIDECAR_SCRIPT_ENV) },
        }
        let electron_run_as_node = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("ELECTRON_RUN_AS_NODE"))
            .and_then(|(_, value)| value);

        assert_eq!(command.get_args().collect::<Vec<_>>(), [script.as_os_str()]);
        assert_eq!(electron_run_as_node, Some(OsStr::new("1")));
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

        let _lifecycle_lock = lock_sidecar_lifecycle_tests();
        let root = tempfile::tempdir().expect("temporary sidecar root");
        let sidecar = root.path().join("blocked-sidecar.sh");
        fs::write(&sidecar, "#!/bin/sh\nsleep 30\n").expect("write sidecar");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let request = StatusRequest {
            protocol_version: TYPE_AWARE_PROTOCOL_VERSION,
            operation: "status",
        };
        let started = Instant::now();
        let error = run_sidecar_json::<_, StatusResponse>(
            &sidecar,
            root.path(),
            &request,
            Duration::from_millis(100),
            MAX_RESPONSE_BYTES,
        )
        .expect_err("blocked sidecar should time out");

        assert!(error.contains("timed out"), "unexpected error: {error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn active_sidecar_termination_interrupts_blocking_request() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let _lifecycle_lock = lock_sidecar_lifecycle_tests();
        let root = tempfile::tempdir().expect("temporary sidecar root");
        let sidecar = root.path().join("blocking-sidecar.sh");
        let ready = root.path().join("sidecar-ready");
        fs::write(
            &sidecar,
            "#!/bin/sh\nprintf ready > sidecar-ready\nexec sleep 30\n",
        )
        .expect("write sidecar");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let root_path = root.path().to_path_buf();
        let request = std::thread::spawn(move || {
            run_sidecar_json::<_, StatusResponse>(
                &sidecar,
                &root_path,
                &StatusRequest {
                    protocol_version: TYPE_AWARE_PROTOCOL_VERSION,
                    operation: "status",
                },
                Duration::from_secs(30),
                MAX_RESPONSE_BYTES,
            )
        });

        let ready_deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < ready_deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "sidecar did not start");

        let started = Instant::now();
        terminate_active_type_aware_sidecars();
        let error = request
            .join()
            .expect("request thread")
            .expect_err("terminated sidecar should fail");

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "sidecar termination did not unblock the request"
        );
        assert!(
            error.contains("exited with status"),
            "unexpected termination error: {error}"
        );
    }
}
