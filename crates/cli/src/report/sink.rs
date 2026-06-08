//! Ambient output sink for the report layer.
//!
//! By default the `outln!` / `out!` macros write report CONTENT to stdout, so
//! the CLI behaves exactly as it always has. When the user passes
//! `--output-file <PATH>`, `main` opens the file and calls [`set_file_sink`]
//! once before dispatch; from then on every `outln!` / `out!` lands in the file
//! instead of stdout. The sink is process-global and ambient, so no command
//! `Options` struct needs to thread the path through, and the programmatic /
//! NAPI consumers (which call the `build_*` helpers and never the `print_*`
//! dispatch) are unaffected because they never set the sink.
//!
//! Progress, errors, and the "Report written to `<path>`" confirmation stay on
//! stderr (plain `eprintln!`); interactive terminal chrome (the `--explain`
//! tip, the combined orientation header) is gated on [`is_redirected`] so it
//! never pollutes the file.

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
}

static SINK: Mutex<SinkInner> = Mutex::new(SinkInner {
    file: None,
    error: None,
    wrote: false,
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
    if inner.file.is_some() {
        inner.wrote = true;
    }
    let result = match inner.file.as_mut() {
        Some(writer) => writeln!(writer, "{args}"),
        None => {
            // Ignore stdout write errors (e.g. a closed pipe) rather than
            // panicking the way `println!` would.
            let _ = writeln!(io::stdout(), "{args}");
            Ok(())
        }
    };
    if let Err(error) = result {
        inner.error = Some(error);
    }
}

/// Write report content without a trailing newline. Backs the `out!` macro.
pub fn write_fmt_str(args: fmt::Arguments<'_>) {
    let mut inner = lock();
    if inner.error.is_some() {
        return;
    }
    if inner.file.is_some() {
        inner.wrote = true;
    }
    let result = match inner.file.as_mut() {
        Some(writer) => write!(writer, "{args}"),
        None => {
            let _ = write!(io::stdout(), "{args}");
            Ok(())
        }
    };
    if let Err(error) = result {
        inner.error = Some(error);
    }
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

/// Write report content without a trailing newline. Drop-in replacement for
/// `print!` on report content.
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::report::sink::write_fmt_str(::std::format_args!($($arg)*))
    };
}

pub(crate) use {out, outln};

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
        out!("partial ");
        outln!("end");
        flush().expect("flush ok");

        let mut contents = String::new();
        std::fs::File::open(&path)
            .expect("open")
            .read_to_string(&mut contents)
            .expect("read");
        assert_eq!(contents, "line one\npartial end\n");
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
