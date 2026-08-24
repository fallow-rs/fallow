//! Backend-neutral protocol for the opt-in type-aware analysis pass.

mod discovery;
mod process;
mod session;

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use fallow_types::envelope::TypeAwareMeta;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::manifest::{
    BACKEND_FAMILY, BACKEND_VERSION, SESSION_ENVELOPE_TYPES, SIDECAR_PACKAGE as SIDECAR_BINARY,
    STATUS_OPERATION, WIRE_PROTOCOL_VERSION as TYPE_AWARE_PROTOCOL_VERSION,
};
#[cfg(windows)]
use discovery::canonical_sidecar_path;
#[cfg(test)]
use discovery::discover_type_aware_sidecar_from;
#[cfg(all(test, unix))]
use discovery::find_installed_sidecar;
use discovery::{canonicalize_root, discover_type_aware_sidecar, non_empty_env};
#[cfg(all(test, windows))]
use process::sidecar_command_with_script;
use process::{
    SidecarTimeout, join_sidecar_worker, read_bounded_stream, run_sidecar_json, sidecar_command,
};
#[cfg(test)]
use process::{bounded_text, sanitize_search_path};
#[cfg(all(test, unix))]
use session::TypeAwareSessionProcess;
pub use session::{TypeAwareFileChanges, TypeAwareSession};

#[cfg(windows)]
const SIDECAR_SCRIPT_ENV: &str = "FALLOW_TYPE_AWARE_SCRIPT";
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

/// Semantic-companion availability report produced by [`status`].
#[derive(Debug, Clone, Serialize)]
pub struct TypeAwareStatus {
    /// True only when the companion was found and its protocol, package,
    /// and backend versions all match this fallow build exactly.
    pub available: bool,
    /// How the companion binary was located: `installed-sibling`,
    /// `environment-override`, or a wrapper marker such as `npm-wrapper`;
    /// `None` when discovery failed.
    pub discovery_source: Option<&'static str>,
    /// Discovered companion binary path; `None` when discovery failed.
    #[serde(serialize_with = "fallow_types::serde_path::serialize_option")]
    pub companion_path: Option<PathBuf>,
    /// `fallow-type-aware` package version the companion reported.
    pub package_version: Option<String>,
    /// Protocol version: the companion's when it answered, otherwise the
    /// version this fallow build speaks.
    pub protocol_version: u32,
    /// Type-checker backend family the companion reported.
    pub backend_family: Option<String>,
    /// Type-checker backend version the companion reported.
    pub backend_version: Option<String>,
    /// Remediation instruction when the companion is unavailable or
    /// mismatched; `None` when `available` is true.
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

/// Marker set alongside `FALLOW_TYPE_AWARE_BIN` by fallow's own launchers
/// (npm CLI launcher, Node-API loader, GitHub Action) when they wire a
/// version-matched companion themselves. Distinguishes tooling-provided wiring
/// from a user-set override in status output.
const SIDECAR_SOURCE_ENV: &str = "FALLOW_TYPE_AWARE_BIN_SOURCE";
const WRAPPER_SOURCES: &[&str] = &["npm-wrapper", "github-action"];

fn discovery_source_label(override_set: bool, source_marker: Option<&str>) -> &'static str {
    if !override_set {
        return "installed-sibling";
    }
    source_marker
        .and_then(|marker| WRAPPER_SOURCES.iter().find(|known| **known == marker))
        .map_or("environment-override", |known| known)
}

/// Inspect the optional semantic companion without loading or analyzing a
/// TypeScript project.
pub fn status(root: &Path) -> TypeAwareStatus {
    let discovery_source = discovery_source_label(
        non_empty_env("FALLOW_TYPE_AWARE_BIN").is_some(),
        non_empty_env(SIDECAR_SOURCE_ENV).as_deref(),
    );
    let sidecar = match discover_type_aware_sidecar(root) {
        Ok(sidecar) => sidecar,
        Err(error) => {
            // Both the sibling and wrapper paths are installation problems the
            // npm package fixes; only a user-set override keeps the raw error.
            let remediation = if discovery_source != "environment-override" {
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
        operation: STATUS_OPERATION,
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

/// Result of a completed type-aware pass.
#[derive(Debug)]
pub struct TypeAwareOutcome {
    /// Pass metadata merged into output envelope `meta` sections.
    pub meta: TypeAwareMeta,
    /// Non-fatal warnings surfaced to the caller alongside the results.
    pub warnings: Vec<String>,
}

/// Opaque type-aware transport failure; the message is the full description.
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
    fn discovery_source_labels_each_resolution_path() {
        assert_eq!(discovery_source_label(false, None), "installed-sibling");
        // A stale wrapper marker without an override must not relabel siblings.
        assert_eq!(
            discovery_source_label(false, Some("npm-wrapper")),
            "installed-sibling"
        );
        assert_eq!(discovery_source_label(true, None), "environment-override");
        assert_eq!(
            discovery_source_label(true, Some("npm-wrapper")),
            "npm-wrapper"
        );
        assert_eq!(
            discovery_source_label(true, Some("github-action")),
            "github-action"
        );
        // Unrecognized marker values never leak into the reported taxonomy.
        assert_eq!(
            discovery_source_label(true, Some("custom-marker")),
            "environment-override"
        );
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
            dunce::canonicalize(override_sidecar).expect("canonical override sidecar")
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
            dunce::canonicalize(sibling).expect("canonical sibling sidecar")
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
        let root = dunce::canonicalize(project.path()).expect("canonical project root");
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
            [dunce::canonicalize(trusted.path()).expect("canonical trusted path")]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_sidecar_command_cannot_resolve_a_project_root_node_shim() {
        let _lock = lock_sidecar_lifecycle_tests();
        let project = tempfile::tempdir().expect("temporary project directory");
        let root = dunce::canonicalize(project.path()).expect("canonical project root");
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
        let root = dunce::canonicalize(project.path()).expect("canonical project root");
        let install = tempfile::tempdir().expect("temporary trusted install directory");
        let electron = install.path().join("Code.exe");
        let script = install.path().join("fallow-type-aware.mjs");
        std::fs::write(&electron, []).expect("write Electron fixture");
        std::fs::write(&script, []).expect("write sidecar script fixture");
        let canonical_script =
            canonical_sidecar_path(&script).expect("canonical sidecar script fixture");
        assert_eq!(
            canonical_script,
            dunce::simplified(&canonical_script),
            "Node must receive a non-verbatim Windows path"
        );

        let command = sidecar_command_with_script(&electron, &root, Some(&script))
            .expect("configure sidecar command");
        let electron_run_as_node = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("ELECTRON_RUN_AS_NODE"))
            .and_then(|(_, value)| value);

        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [canonical_script.as_os_str()]
        );
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
        // The transport writes the request only after its post-spawn
        // termination-epoch re-check passed, so signaling readiness after
        // consuming stdin guarantees termination lands past the start window
        // and surfaces the exit status instead of start cancellation (#2231).
        fs::write(
            &sidecar,
            "#!/bin/sh\nread request\nprintf ready > sidecar-ready\nexec sleep 30\n",
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

    #[cfg(unix)]
    #[test]
    fn persistent_session_reuses_one_process_and_preserves_revision_identity() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        #[derive(Serialize)]
        struct Request {
            value: u32,
        }

        #[derive(Debug, Deserialize)]
        struct Response {
            value: u32,
            revision: u64,
            invalidate_all: bool,
            process_id: u32,
        }

        let _lifecycle_lock = lock_sidecar_lifecycle_tests();
        let root = tempfile::tempdir().expect("temporary sidecar root");
        let install = tempfile::tempdir().expect("temporary sidecar install");
        let sidecar = install.path().join("session-sidecar.mjs");
        fs::write(
            &sidecar,
            r#"#!/usr/bin/env node
import readline from "node:readline";
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const envelope = JSON.parse(line);
  if (envelope.type === "shutdown") process.exit(0);
  process.stdout.write(`${JSON.stringify({
    request_id: envelope.request_id,
    revision: envelope.revision,
    response: {
      value: envelope.request.value,
      revision: envelope.revision,
      invalidate_all: envelope.file_changes?.invalidate_all === true,
      process_id: process.pid,
    },
  })}\n`);
}
"#,
        )
        .expect("write persistent sidecar fixture");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let canonical_root = root.path().canonicalize().expect("canonical root");
        let canonical_sidecar = sidecar.canonicalize().expect("canonical sidecar");
        let process = TypeAwareSessionProcess::spawn(&canonical_sidecar, &canonical_root)
            .expect("spawn persistent sidecar");
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut session = TypeAwareSession {
            root: canonical_root.clone(),
            sidecar: canonical_sidecar,
            process: Some(process),
            request_id: 0,
            revision: 0,
            cancellation: Some(Arc::clone(&cancellation)),
        };

        let first: Response = session
            .run_semantic_request(&canonical_root, &Request { value: 1 }, None)
            .expect("first request");
        let second: Response = session
            .run_semantic_request(
                &canonical_root,
                &Request { value: 2 },
                Some(&TypeAwareFileChanges {
                    invalidate_all: true,
                    ..TypeAwareFileChanges::default()
                }),
            )
            .expect("second request");

        assert_eq!(first.value, 1);
        assert_eq!(first.revision, 1);
        assert!(!first.invalidate_all);
        assert_eq!(second.value, 2);
        assert_eq!(second.revision, 2);
        assert!(second.invalidate_all);
        assert_eq!(first.process_id, second.process_id);
        assert_eq!(session.request_id, 2);
        assert_eq!(session.revision, 2);

        cancellation.store(true, Ordering::SeqCst);
        let error = session
            .run_semantic_request::<_, Response>(&canonical_root, &Request { value: 3 }, None)
            .expect_err("a cancelled owner must reject new requests");
        assert!(
            error.to_string().contains("owner is closing"),
            "unexpected error: {error}"
        );
        assert_eq!(session.request_id, 2);
    }

    #[cfg(unix)]
    #[test]
    fn persistent_session_restarts_once_with_full_invalidation() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        #[derive(Serialize)]
        struct Request {
            value: u32,
        }

        #[derive(Debug, Deserialize)]
        struct Response {
            value: u32,
            request_id: u64,
            revision: u64,
            invalidate_all: bool,
            launches: u32,
            first_process_id: u32,
            process_id: u32,
        }

        let _lifecycle_lock = lock_sidecar_lifecycle_tests();
        let root = tempfile::tempdir().expect("temporary sidecar root");
        let install = tempfile::tempdir().expect("temporary sidecar install");
        let sidecar = install.path().join("restart-sidecar.mjs");
        fs::write(
            &sidecar,
            r#"#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
const statePath = path.join(process.cwd(), ".session-restart-state.json");
const state = fs.existsSync(statePath)
  ? JSON.parse(fs.readFileSync(statePath, "utf8"))
  : { launches: 0, first_process_id: process.pid };
state.launches += 1;
fs.writeFileSync(statePath, JSON.stringify(state));
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const envelope = JSON.parse(line);
  if (envelope.type === "shutdown") process.exit(0);
  if (state.launches === 1) {
    process.stdout.write(`${JSON.stringify({
      request_id: envelope.request_id + 1,
      revision: envelope.revision,
      response: {},
    })}\n`);
    process.exit(0);
  }
  process.stdout.write(`${JSON.stringify({
    request_id: envelope.request_id,
    revision: envelope.revision,
    response: {
      value: envelope.request.value,
      request_id: envelope.request_id,
      revision: envelope.revision,
      invalidate_all: envelope.file_changes?.invalidate_all === true,
      launches: state.launches,
      first_process_id: state.first_process_id,
      process_id: process.pid,
    },
  })}\n`);
}
"#,
        )
        .expect("write restart sidecar fixture");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let canonical_root = root.path().canonicalize().expect("canonical root");
        let canonical_sidecar = sidecar.canonicalize().expect("canonical sidecar");
        let process = TypeAwareSessionProcess::spawn(&canonical_sidecar, &canonical_root)
            .expect("spawn persistent sidecar");
        let mut session = TypeAwareSession {
            root: canonical_root.clone(),
            sidecar: canonical_sidecar,
            process: Some(process),
            request_id: 0,
            revision: 0,
            cancellation: None,
        };

        let response: Response = session
            .run_semantic_request(&canonical_root, &Request { value: 7 }, None)
            .expect("request succeeds after one restart");

        assert_eq!(response.value, 7);
        assert_eq!(response.request_id, 2);
        assert_eq!(response.revision, 2);
        assert!(response.invalidate_all);
        assert_eq!(response.launches, 2);
        assert_ne!(response.first_process_id, response.process_id);
        assert_eq!(session.request_id, 2);
        assert_eq!(session.revision, 2);
    }

    #[cfg(unix)]
    #[test]
    fn cancelled_session_does_not_restart_after_request_failure() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        #[derive(Serialize)]
        struct Request {
            value: u32,
        }

        let _lifecycle_lock = lock_sidecar_lifecycle_tests();
        let root = tempfile::tempdir().expect("temporary sidecar root");
        let install = tempfile::tempdir().expect("temporary sidecar install");
        let sidecar = install.path().join("cancelled-restart-sidecar.mjs");
        fs::write(
            &sidecar,
            r#"#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
const launchesPath = path.join(process.cwd(), ".session-launches");
const launches = fs.existsSync(launchesPath)
  ? Number(fs.readFileSync(launchesPath, "utf8")) + 1
  : 1;
fs.writeFileSync(launchesPath, String(launches));
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const envelope = JSON.parse(line);
  if (envelope.type === "shutdown") process.exit(0);
  fs.writeFileSync(path.join(process.cwd(), ".session-request-seen"), "1");
  const cancellationAcknowledgement = path.join(process.cwd(), ".session-cancelled");
  for (let attempt = 0; attempt < 2000 && !fs.existsSync(cancellationAcknowledgement); attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  process.exit(1);
}
"#,
        )
        .expect("write cancelled restart sidecar fixture");
        let mut permissions = fs::metadata(&sidecar)
            .expect("sidecar metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&sidecar, permissions).expect("make sidecar executable");

        let canonical_root = root.path().canonicalize().expect("canonical root");
        let canonical_sidecar = sidecar.canonicalize().expect("canonical sidecar");
        let sidecar_dir = canonical_sidecar
            .parent()
            .expect("sidecar install directory")
            .to_path_buf();
        let process = TypeAwareSessionProcess::spawn(&canonical_sidecar, &canonical_root)
            .expect("spawn persistent sidecar");
        let cancellation = Arc::new(AtomicBool::new(false));
        let request_marker = sidecar_dir.join(".session-request-seen");
        let cancellation_marker = sidecar_dir.join(".session-cancelled");
        let mut session = TypeAwareSession {
            root: canonical_root.clone(),
            sidecar: canonical_sidecar,
            process: Some(process),
            request_id: 0,
            revision: 0,
            cancellation: Some(Arc::clone(&cancellation)),
        };
        let request_root = canonical_root;
        let request = std::thread::spawn(move || {
            let error = session
                .run_semantic_request::<_, serde_json::Value>(
                    &request_root,
                    &Request { value: 9 },
                    None,
                )
                .expect_err("cancelled owner prevents restart");
            (session, error)
        });

        let request_deadline = Instant::now() + Duration::from_secs(5);
        while !request_marker.exists() && Instant::now() < request_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let request_seen = request_marker.exists();
        cancellation.store(true, Ordering::SeqCst);
        fs::write(cancellation_marker, "1").expect("acknowledge cancellation");
        let (session, error) = request.join().expect("semantic request");

        assert!(request_seen, "sidecar did not observe the semantic request");

        assert!(
            error.to_string().contains("owner is closing"),
            "unexpected error: {error}"
        );
        assert_eq!(
            fs::read_to_string(sidecar_dir.join(".session-launches")).expect("launch counter"),
            "1"
        );
        assert_eq!(session.request_id, 1);
        assert_eq!(session.revision, 1);
    }
}
