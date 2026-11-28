//! The stderr writer of the log output.
//!
//! A log event can hold a project string: a path, a config value or a name.
//! A line break in such a value starts a new stderr line, and a CI runner
//! reads some line prefixes (for example `::warning`) as commands. The writer
//! escapes each control character of an event (see
//! [`fallow_output::display_text`]), except the line end of the event.

use std::io::{self, Write};

/// A stderr writer for one log event at a time. With `ansi` on, the color
/// sequences of the log formatter stay (see [`fallow_output::display_line`]);
/// the formatter itself writes an escape character of the event text as
/// `\x1b`, so a value cannot add a sequence.
pub struct EscapedStderr {
    ansi: bool,
}

impl EscapedStderr {
    /// The writer for a log formatter that adds colors when `ansi` is on.
    pub const fn new(ansi: bool) -> Self {
        Self { ansi }
    }
}

/// One log event as stderr text: each control character escaped, and the
/// line end of the event kept.
fn escaped_event(event: &[u8], ansi: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(event);
    let (body, line_end) = text
        .strip_suffix('\n')
        .map_or((&*text, ""), |body| (body, "\n"));
    let body = if ansi {
        fallow_output::display_line(body)
    } else {
        fallow_output::display_text(body)
    };
    format!("{body}{line_end}").into_bytes()
}

impl Write for EscapedStderr {
    fn write(&mut self, event: &[u8]) -> io::Result<usize> {
        io::stderr().write_all(&escaped_event(event, self.ansi))?;
        Ok(event.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::escaped_event;

    #[test]
    fn an_event_keeps_its_line_end_and_escapes_other_line_breaks() {
        let event = b"WARN plugin path 'a\n::notice::x' skipped\n";
        assert_eq!(
            escaped_event(event, false),
            b"WARN plugin path 'a\\n::notice::x' skipped\n".to_vec()
        );
    }

    #[test]
    fn the_formatter_colors_stay_only_with_ansi() {
        let event = b"\x1b[33mWARN\x1b[0m a\rb\n";
        assert_eq!(
            escaped_event(event, true),
            b"\x1b[33mWARN\x1b[0m a\\rb\n".to_vec()
        );
        assert_eq!(
            escaped_event(event, false),
            b"\\u{001B}[33mWARN\\u{001B}[0m a\\rb\n".to_vec()
        );
    }
}
