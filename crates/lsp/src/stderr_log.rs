//! The filter for the tracing lines that the server writes to stderr.
//!
//! Editors show the stderr of a language server as error output. VS Code
//! labels each stderr line `[error]` in the Output channel. INFO progress
//! lines of the analysis therefore look like failures, and one run writes
//! one line per workspace for some of them. The server writes warnings and
//! errors only. `RUST_LOG` selects more detail when a user debugs a run.

use tracing_subscriber::filter::{LevelFilter, Targets};

/// Build the stderr filter from the value of `RUST_LOG`.
///
/// Without `RUST_LOG`, the filter keeps WARN and ERROR of the `fallow*`
/// targets, so dependency crates add no lines. A `RUST_LOG` value
/// replaces the default. The filter skips a directive that it cannot read,
/// such as a span or field filter, and keeps the others.
pub fn stderr_log_filter(rust_log: Option<&str>) -> Targets {
    let Some(value) = rust_log.map(str::trim) else {
        return Targets::new().with_target("fallow", LevelFilter::WARN);
    };
    let readable: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty() && directive.parse::<Targets>().is_ok())
        .collect();
    // An empty directive list parses to a filter that enables every target.
    if readable.is_empty() {
        return Targets::new();
    }
    readable.join(",").parse().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    const PLUGIN_TARGET: &str = "fallow_core::plugins::registry";

    #[test]
    fn default_filter_drops_info_progress_lines() {
        let filter = stderr_log_filter(None);
        assert!(!filter.would_enable(PLUGIN_TARGET, &Level::INFO));
        assert!(!filter.would_enable("fallow_engine::discover", &Level::INFO));
    }

    #[test]
    fn default_filter_keeps_warnings_and_errors() {
        let filter = stderr_log_filter(None);
        assert!(filter.would_enable(PLUGIN_TARGET, &Level::WARN));
        assert!(filter.would_enable("fallow_config", &Level::ERROR));
    }

    #[test]
    fn default_filter_drops_dependency_lines() {
        let filter = stderr_log_filter(None);
        assert!(!filter.would_enable("tower_lsp_server::service", &Level::WARN));
    }

    #[test]
    fn rust_log_selects_more_detail() {
        let filter = stderr_log_filter(Some("fallow_core=info"));
        assert!(filter.would_enable(PLUGIN_TARGET, &Level::INFO));
        assert!(!filter.would_enable("fallow_engine::discover", &Level::INFO));
    }

    #[test]
    fn empty_rust_log_disables_every_line() {
        let filter = stderr_log_filter(Some(""));
        assert!(!filter.would_enable(PLUGIN_TARGET, &Level::ERROR));
    }

    #[test]
    fn unreadable_directive_keeps_the_readable_ones() {
        let filter = stderr_log_filter(Some("fallow_core[span{a=1}]=debug,fallow_engine=info"));
        assert!(filter.would_enable("fallow_engine::discover", &Level::INFO));
    }
}
