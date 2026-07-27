//! Persistent semantic sessions, revisions, invalidation, and bounded restart.

use super::*;

/// Project-relative changes applied before the next persistent semantic query.
#[derive(Debug, Default, Clone, Serialize)]
pub struct TypeAwareFileChanges {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invalidate_all: bool,
}

#[derive(Serialize)]
struct SessionRequest<'a, Request: ?Sized> {
    r#type: &'static str,
    request_id: u64,
    revision: u64,
    request: &'a Request,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_changes: Option<&'a TypeAwareFileChanges>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionResponse<Response> {
    request_id: u64,
    revision: u64,
    response: Response,
}

pub(super) struct TypeAwareSessionProcess {
    _child: fallow_process::ScopedChild,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    terminator: fallow_process::ProcessTreeTerminator,
    _active: ActiveSidecarGuard,
    stderr_reader: Option<std::thread::JoinHandle<Result<Vec<u8>, String>>>,
}

impl TypeAwareSessionProcess {
    pub(super) fn spawn(sidecar: &Path, root: &Path) -> Result<Self, TypeAwareError> {
        if SIDECAR_SHUTDOWN.load(Ordering::SeqCst) {
            return Err(TypeAwareError::from(
                "type-aware sidecar spawning is closed during shutdown".to_string(),
            ));
        }
        let termination_epoch = SIDECAR_TERMINATION_EPOCH.load(Ordering::SeqCst);
        let mut command = sidecar_command(sidecar, root).map_err(TypeAwareError)?;
        command.arg("--session");
        let mut child =
            fallow_process::ScopedChild::spawn_process_tree(&mut command).map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to spawn persistent {}: {error}",
                    sidecar.display()
                ))
            })?;
        let terminator = child.process_tree_terminator().ok_or_else(|| {
            TypeAwareError::from(
                "persistent type-aware sidecar process tree was not available".to_string(),
            )
        })?;
        let active = ActiveSidecarGuard::register(terminator.clone());
        if SIDECAR_SHUTDOWN.load(Ordering::SeqCst)
            || SIDECAR_TERMINATION_EPOCH.load(Ordering::SeqCst) != termination_epoch
        {
            let _ = terminator.terminate();
            return Err(TypeAwareError::from(
                "persistent type-aware sidecar start was cancelled".to_string(),
            ));
        }
        let stdin = child.take_stdin().ok_or_else(|| {
            TypeAwareError::from("persistent type-aware stdin was unavailable".to_string())
        })?;
        let stdout = child.take_stdout().ok_or_else(|| {
            TypeAwareError::from("persistent type-aware stdout was unavailable".to_string())
        })?;
        let stderr = child.take_stderr().ok_or_else(|| {
            TypeAwareError::from("persistent type-aware stderr was unavailable".to_string())
        })?;
        let stderr_terminator = terminator.clone();
        let stderr_reader = std::thread::spawn(move || {
            read_bounded_stream(stderr, MAX_STDERR_BYTES, "stderr", Some(stderr_terminator))
        });
        Ok(Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            terminator,
            _active: active,
            stderr_reader: Some(stderr_reader),
        })
    }

    fn terminate(&mut self) {
        let _ = self.terminator.terminate();
        if let Some(reader) = self.stderr_reader.take() {
            let _ = join_sidecar_worker("stderr reader", reader);
        }
    }
}

/// Explicitly owned semantic sidecar session for one canonical project root.
pub struct TypeAwareSession {
    pub(super) root: PathBuf,
    pub(super) sidecar: PathBuf,
    pub(super) process: Option<TypeAwareSessionProcess>,
    pub(super) request_id: u64,
    pub(super) revision: u64,
    pub(super) cancellation: Option<Arc<AtomicBool>>,
}

impl TypeAwareSession {
    /// Start a root-bound persistent semantic session.
    pub fn new(root: &Path) -> Result<Self, TypeAwareError> {
        Self::new_inner(root, None)
    }

    /// Start a root-bound session that cannot restart after its owner cancels.
    pub fn new_cancellable(
        root: &Path,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Self, TypeAwareError> {
        Self::new_inner(root, Some(cancellation))
    }

    fn new_inner(
        root: &Path,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<Self, TypeAwareError> {
        if cancellation
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::SeqCst))
        {
            return Err(TypeAwareError::from(
                "type-aware session owner is closing".to_string(),
            ));
        }
        let root = canonicalize_root(root)?;
        let sidecar = discover_type_aware_sidecar(&root)?;
        let process = TypeAwareSessionProcess::spawn(&sidecar, &root)?;
        if cancellation
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::SeqCst))
        {
            drop(process);
            return Err(TypeAwareError::from(
                "type-aware session owner closed during startup".to_string(),
            ));
        }
        Ok(Self {
            root,
            sidecar,
            process: Some(process),
            request_id: 0,
            revision: 0,
            cancellation,
        })
    }

    pub(in crate::type_aware) fn run_semantic_request<Request, Response>(
        &mut self,
        root: &Path,
        request: &Request,
        changes: Option<&TypeAwareFileChanges>,
    ) -> Result<Response, TypeAwareError>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        self.ensure_owner_open()?;
        let root = canonicalize_root(root)?;
        if root != self.root {
            return Err(TypeAwareError::from(format!(
                "type-aware session root mismatch: expected {}, received {}",
                self.root.display(),
                root.display()
            )));
        }
        match self.request_once(request, changes) {
            Ok(response) => Ok(response),
            Err(first_error) => {
                self.restart()?;
                let invalidate_all = TypeAwareFileChanges {
                    invalidate_all: true,
                    ..TypeAwareFileChanges::default()
                };
                self.request_once(request, Some(&invalidate_all)).map_err(|error| {
                    TypeAwareError::from(format!(
                        "persistent type-aware request failed after one restart: {first_error}; {error}"
                    ))
                })
            }
        }
    }

    fn request_once<Request, Response>(
        &mut self,
        request: &Request,
        changes: Option<&TypeAwareFileChanges>,
    ) -> Result<Response, TypeAwareError>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        self.request_id += 1;
        self.revision += 1;
        let envelope = SessionRequest {
            r#type: SESSION_ENVELOPE_TYPES[0],
            request_id: self.request_id,
            revision: self.revision,
            request,
            file_changes: changes,
        };
        let mut bytes = serde_json::to_vec(&envelope).map_err(|error| {
            TypeAwareError::from(format!(
                "failed to serialize persistent type-aware request: {error}"
            ))
        })?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(TypeAwareError::from(format!(
                "persistent type-aware request exceeded the {MAX_REQUEST_BYTES} byte limit"
            )));
        }
        bytes.push(b'\n');
        let process = self.process.as_mut().ok_or_else(|| {
            TypeAwareError::from("persistent type-aware process is closed".to_string())
        })?;
        process
            .stdin
            .write_all(&bytes)
            .and_then(|()| process.stdin.flush())
            .map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to write persistent type-aware request: {error}"
                ))
            })?;
        let timeout = SidecarTimeout::start(process.terminator.clone(), SIDECAR_TIMEOUT);
        let mut response_bytes = Vec::new();
        process
            .stdout
            .by_ref()
            .take((MAX_SEMANTIC_RESPONSE_BYTES + 1) as u64)
            .read_until(b'\n', &mut response_bytes)
            .map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to read persistent type-aware response: {error}"
                ))
            })?;
        timeout.finish().map_err(TypeAwareError)?;
        if response_bytes.is_empty() {
            return Err(TypeAwareError::from(
                "persistent type-aware sidecar closed without a response".to_string(),
            ));
        }
        if response_bytes.len() > MAX_SEMANTIC_RESPONSE_BYTES {
            return Err(TypeAwareError::from(format!(
                "persistent type-aware response exceeded the {MAX_SEMANTIC_RESPONSE_BYTES} byte limit"
            )));
        }
        let response: SessionResponse<Response> =
            serde_json::from_slice(&response_bytes).map_err(|error| {
                TypeAwareError::from(format!(
                    "failed to parse persistent type-aware response: {error}"
                ))
            })?;
        if response.request_id != self.request_id || response.revision != self.revision {
            return Err(TypeAwareError::from(
                "persistent type-aware response identity mismatch".to_string(),
            ));
        }
        Ok(response.response)
    }

    fn restart(&mut self) -> Result<(), TypeAwareError> {
        self.ensure_owner_open()?;
        if let Some(mut process) = self.process.take() {
            process.terminate();
        }
        self.process = Some(TypeAwareSessionProcess::spawn(&self.sidecar, &self.root)?);
        self.ensure_owner_open()?;
        Ok(())
    }

    fn ensure_owner_open(&self) -> Result<(), TypeAwareError> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::SeqCst))
        {
            return Err(TypeAwareError::from(
                "type-aware session owner is closing".to_string(),
            ));
        }
        Ok(())
    }
}

impl Drop for TypeAwareSession {
    fn drop(&mut self) {
        if let Some(process) = self.process.as_mut() {
            let _ = process.stdin.write_all(b"{\"type\":\"shutdown\"}\n");
            let _ = process.stdin.flush();
            process.terminate();
        }
    }
}
