//! Footer of the human `check` report.

use super::*;

pub(super) fn print_human_footer(input: &PrintHumanInput<'_>, total: usize) {
    if total == 0 {
        eprintln!(
            "{}",
            clean_status_line(input.elapsed, input.failed_parse_files)
        );
        return;
    }

    let summary = build_summary_footer(input.results);
    eprintln!(
        "{}",
        super::super::findings_status_line(
            &format!("{summary} ({:.2}s)", input.elapsed.as_secs_f64()),
            input.run_fails,
        )
    );
    print_suppression_footer(input.results);
    if let Some(line) = cascade_footer(input.results) {
        eprintln!("  {}", line.dimmed());
    }
}
