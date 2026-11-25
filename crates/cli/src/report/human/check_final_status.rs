//! The final status lines of the grouped human check output.

use super::*;

pub(super) fn emit_grouped_final_status(
    groups: &[crate::report::grouping::ResultGroup],
    grand_total: usize,
    elapsed: Duration,
    run_fails: bool,
    failed_parse_files: usize,
) {
    if grand_total == 0 {
        eprintln!("{}", clean_status_line(elapsed, failed_parse_files));
    } else {
        let non_empty_groups = groups
            .iter()
            .filter(|g| g.results.total_issues() > 0)
            .count();
        super::super::print_check_status_line(
            &format!(
                "{grand_total} issue{} across {non_empty_groups} group{} ({:.2}s)",
                plural(grand_total),
                plural(non_empty_groups),
                elapsed.as_secs_f64()
            ),
            run_fails,
        );
    }
}
