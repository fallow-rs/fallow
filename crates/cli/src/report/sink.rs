//! Ambient output sink for the report layer.
//!
//! By default the `outln!` macro writes report CONTENT to stdout, so the CLI
//! behaves exactly as it always has. When the user passes
//! `--output-file <PATH>`, `main` opens the file and calls [`set_file_sink`]
//! once before dispatch; from then on every `outln!` lands in the file instead
//! of stdout. The sink is process-global and ambient, so no command `Options`
//! struct needs to thread the path through, and the programmatic / NAPI
//! consumers (which call the `build_*` helpers and never the `print_*`
//! dispatch) are unaffected because they never set the sink.
//!
//! Progress, errors, and the "Report written to `<path>`" confirmation stay on
//! stderr (plain `eprintln!`); interactive terminal chrome (the `--explain`
//! tip, the combined orientation header) is gated on [`is_redirected`] so it
//! never pollutes the file.
//!
//! Stdout writes go through [`write_all_retrying`]. A parent process can make
//! the stdout pipe non-blocking (Bun does this, issue #3276), so a large write
//! can fail with `WouldBlock`. The sink then waits until stdout is writable and
//! writes the remaining bytes. A closed reader (`BrokenPipe`) stops further
//! stdout output without an error. The sink keeps any other stdout write error,
//! and [`finish_stdout`] changes it into exit code 2 at the end of the run.

use std::fmt;
use std::io::{self, BufWriter, Write};
use std::sync::Mutex;

struct SinkInner {
    /// `Some` once `--output-file` redirected output. `None` means stdout.
    file: Option<BufWriter<std::fs::File>>,
    /// First write error seen against the file sink, surfaced by [`flush`] so a
    /// truncated / failed write does not masquerade as a successful report.
    error: Option<io::Error>,
    /// Whether any report content was written to the file sink. Lets the caller
    /// suppress the "Report written" confirmation when a command errored out
    /// before rendering anything (the error went to stdout, the file is empty).
    wrote: bool,
    /// State of stdout after the last report write.
    stdout: StdoutState,
}

/// Result of the stdout writes so far.
enum StdoutState {
    /// All writes succeeded.
    Open,
    /// The reader closed the pipe. The sink drops further output without an
    /// error, so `fallow ... | head` stays clean.
    Closed,
    /// A write failed with an error other than `BrokenPipe`. The sink drops
    /// further output, and [`finish_stdout`] reports the error.
    Failed(io::Error),
}

static SINK: Mutex<SinkInner> = Mutex::new(SinkInner {
    file: None,
    error: None,
    wrote: false,
    stdout: StdoutState::Open,
});

fn lock() -> std::sync::MutexGuard<'static, SinkInner> {
    SINK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Redirect all subsequent report content to `file` (truncating it). Call once,
/// before any rendering. Also resets any prior sticky write error.
pub fn set_file_sink(file: std::fs::File) {
    let mut inner = lock();
    inner.file = Some(BufWriter::new(file));
    inner.error = None;
    inner.wrote = false;
}

/// Whether report content is currently being redirected to a file. Used to gate
/// interactive terminal chrome that must not land in the file.
pub fn is_redirected() -> bool {
    lock().file.is_some()
}

/// Whether any report content was written to the file sink. False when stdout
/// was the target, or when a command errored before rendering anything.
pub fn wrote() -> bool {
    lock().wrote
}

/// Flush the file sink and surface the first write error, if any. No-op (Ok)
/// when writing to stdout. Call after rendering, before the confirmation.
pub fn flush() -> io::Result<()> {
    let mut inner = lock();
    if let Some(error) = inner.error.take() {
        return Err(error);
    }
    match inner.file.as_mut() {
        Some(writer) => writer.flush(),
        None => Ok(()),
    }
}

/// Write a line of report content (a trailing newline is added). Routed to the
/// file sink when redirected, else stdout. Backs the `outln!` macro.
pub fn write_fmt_line(args: fmt::Arguments<'_>) {
    let mut inner = lock();
    if inner.error.is_some() {
        return;
    }
    let Some(writer) = inner.file.as_mut() else {
        write_stdout_line(&mut inner, args);
        return;
    };
    let result = writeln!(writer, "{args}");
    inner.wrote = true;
    if let Err(error) = result {
        inner.error = Some(error);
    }
}

/// Write a line to stdout, also when `--output-file` is set. Backs the
/// `stdoutln!` macro for command output that does not use the file sink.
pub fn write_stdout_fmt_line(args: fmt::Arguments<'_>) {
    let mut inner = lock();
    write_stdout_line(&mut inner, args);
}

fn write_stdout_line(inner: &mut SinkInner, args: fmt::Arguments<'_>) {
    if !matches!(inner.stdout, StdoutState::Open) {
        return;
    }
    let mut line = fmt::format(args);
    line.push('\n');
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    match write_all_retrying(&mut handle, line.as_bytes(), wait_until_stdout_writable) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
            inner.stdout = StdoutState::Closed;
        }
        Err(error) => inner.stdout = StdoutState::Failed(error),
    }
}

/// Write all of `buf` and flush `writer`. On `WouldBlock`, call `wait` and try
/// again with the remaining bytes. Retry on `Interrupted`. Return any other
/// error.
fn write_all_retrying<W: Write>(
    writer: &mut W,
    mut buf: &[u8],
    wait: impl Fn() -> io::Result<()>,
) -> io::Result<()> {
    while !buf.is_empty() {
        match writer.write(buf) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => buf = &buf[written..],
            Err(error) => retry_or_fail(error, &wait)?,
        }
    }
    loop {
        match writer.flush() {
            Ok(()) => return Ok(()),
            Err(error) => retry_or_fail(error, &wait)?,
        }
    }
}

fn retry_or_fail(error: io::Error, wait: &impl Fn() -> io::Result<()>) -> io::Result<()> {
    match error.kind() {
        io::ErrorKind::Interrupted => Ok(()),
        io::ErrorKind::WouldBlock => wait(),
        _ => Err(error),
    }
}

/// Block until stdout accepts more bytes. A hang-up or an error on the fd also
/// ends the wait, so that the next write returns the real error.
#[cfg(unix)]
fn wait_until_stdout_writable() -> io::Result<()> {
    use rustix::event::{PollFd, PollFlags, poll};
    use std::os::fd::AsFd;

    let stdout = io::stdout();
    let fd = stdout.as_fd();
    let mut fds = [PollFd::new(&fd, PollFlags::OUT)];
    match poll(&mut fds, None) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(errno) => Err(errno.into()),
    }
}

/// A parent process on Windows does not give a non-blocking stdout pipe. If a
/// write still returns `WouldBlock`, wait a short time before the next try.
#[cfg(not(unix))]
fn wait_until_stdout_writable() -> io::Result<()> {
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(1);
    std::thread::sleep(RETRY_DELAY);
    Ok(())
}

/// Report a stdout write failure at the end of the run. A failed write gives
/// exit code 2 and a message on stderr, so that a cut-off report never looks
/// like a successful run. A closed reader keeps `code`.
pub fn finish_stdout(code: std::process::ExitCode) -> std::process::ExitCode {
    let mut inner = lock();
    if !matches!(inner.stdout, StdoutState::Failed(_)) {
        return code;
    }
    let StdoutState::Failed(error) = std::mem::replace(&mut inner.stdout, StdoutState::Closed)
    else {
        return code;
    };
    drop(inner);
    eprintln!("Error: failed to write the report to stdout: {error}");
    std::process::ExitCode::from(2)
}

/// Write a line of report content to the sink. Drop-in replacement for
/// `println!` on report CONTENT (not progress / errors / interactive chrome).
macro_rules! outln {
    () => {
        $crate::report::sink::write_fmt_line(::std::format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::report::sink::write_fmt_line(::std::format_args!($($arg)*))
    };
}

pub(crate) use outln;

/// Write a line to stdout through the sink, also when `--output-file` is set.
/// Drop-in replacement for `println!` on large command output that does not
/// use the file sink. Unlike `println!`, it writes the full line to a
/// non-blocking stdout pipe and does not panic on a closed reader.
macro_rules! stdoutln {
    ($($arg:tt)*) => {
        $crate::report::sink::write_stdout_fmt_line(::std::format_args!($($arg)*))
    };
}

pub(crate) use stdoutln;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    // The sink is process-global; these tests mutate it and must not run
    // concurrently with each other. They run serially within this module via a
    // shared guard.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    fn reset() {
        let mut inner = lock();
        inner.file = None;
        inner.error = None;
    }

    /// A writer that accepts at most `chunk` bytes per call and returns
    /// `error_kind` before each accepted chunk.
    struct ChokedWriter {
        data: Vec<u8>,
        chunk: usize,
        calls: usize,
        error_kind: io::ErrorKind,
        flushed: bool,
    }

    impl Write for ChokedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls % 2 == 1 {
                return Err(self.error_kind.into());
            }
            let len = buf.len().min(self.chunk);
            self.data.extend_from_slice(&buf[..len]);
            Ok(len)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed = true;
            Ok(())
        }
    }

    fn choked(error_kind: io::ErrorKind) -> ChokedWriter {
        ChokedWriter {
            data: Vec::new(),
            chunk: 7,
            calls: 0,
            error_kind,
            flushed: false,
        }
    }

    #[test]
    fn retries_would_block_until_all_bytes_are_written() {
        let payload: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let mut writer = choked(io::ErrorKind::WouldBlock);
        let waits = std::cell::Cell::new(0);
        write_all_retrying(&mut writer, &payload, || {
            waits.set(waits.get() + 1);
            Ok(())
        })
        .expect("write succeeds after retries");
        assert_eq!(writer.data, payload);
        assert!(writer.flushed);
        assert!(waits.get() > 0, "WouldBlock must call the wait function");
    }

    #[test]
    fn retries_interrupted_without_waiting() {
        let mut writer = choked(io::ErrorKind::Interrupted);
        write_all_retrying(&mut writer, b"hello world", || {
            panic!("Interrupted must not call the wait function")
        })
        .expect("write succeeds after retries");
        assert_eq!(writer.data, b"hello world");
    }

    #[test]
    fn returns_other_write_errors() {
        let mut writer = choked(io::ErrorKind::BrokenPipe);
        let error = write_all_retrying(&mut writer, b"x", || Ok(())).expect_err("error");
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn returns_the_wait_error() {
        let mut writer = choked(io::ErrorKind::WouldBlock);
        let error = write_all_retrying(&mut writer, b"x", || Err(io::Error::other("poll failed")))
            .expect_err("error");
        assert_eq!(error.to_string(), "poll failed");
    }

    #[test]
    fn redirects_content_to_file_and_reports_flush_state() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        assert!(!is_redirected());

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("out.txt");
        let file = std::fs::File::create(&path).expect("create");
        set_file_sink(file);
        assert!(is_redirected());

        outln!("line one");
        outln!("end");
        flush().expect("flush ok");

        let mut contents = String::new();
        std::fs::File::open(&path)
            .expect("open")
            .read_to_string(&mut contents)
            .expect("read");
        assert_eq!(contents, "line one\nend\n");
        assert!(!contents.contains('\u{1b}'), "no ANSI escapes in file");

        reset();
        assert!(!is_redirected());
    }

    #[test]
    fn flush_is_ok_when_writing_to_stdout() {
        let _g = TEST_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        assert!(flush().is_ok());
    }
}
