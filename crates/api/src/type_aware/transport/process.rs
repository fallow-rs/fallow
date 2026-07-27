//! Bounded one-shot process IO shared with persistent sessions.

use super::*;

pub(super) fn run_sidecar_json<Request, Response>(
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

pub(super) fn sidecar_command(sidecar: &Path, root: &Path) -> Result<Command, String> {
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

pub(super) fn sanitize_search_path(root: &Path, value: &OsStr) -> Option<OsString> {
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

pub(super) fn read_bounded_stream(
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

pub(super) fn join_sidecar_worker<T>(
    name: &str,
    worker: std::thread::JoinHandle<Result<T, String>>,
) -> Result<T, String> {
    worker
        .join()
        .map_err(|_| format!("type-aware sidecar {name} panicked"))?
}

pub(super) struct SidecarTimeout {
    done: mpsc::Sender<()>,
    timed_out: Arc<AtomicBool>,
    watcher: std::thread::JoinHandle<()>,
    duration: Duration,
}

impl SidecarTimeout {
    pub(super) fn start(
        terminator: fallow_process::ProcessTreeTerminator,
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

    pub(super) fn finish(self) -> Result<(), String> {
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

pub(super) fn bounded_text(bytes: &[u8], max_chars: usize) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}
