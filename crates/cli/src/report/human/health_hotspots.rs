use std::path::Path;

use colored::Colorize;

use super::{plural, relative_path, split_dir_filename};

const DOCS_HEALTH: &str = "https://docs.fallow.tools/explanations/health";

fn render_ownership_summary(report: &fallow_output::HealthReport) -> Option<String> {
    if report.hotspots.len() < 2 {
        return None;
    }
    let with_ownership: Vec<&fallow_output::OwnershipMetrics> = report
        .hotspots
        .iter()
        .filter_map(|h| h.ownership.as_ref())
        .collect();
    if with_ownership.is_empty() {
        return None;
    }

    let total = with_ownership.len();
    let bus1_count = with_ownership.iter().filter(|o| o.bus_factor == 1).count();

    let mut tally: rustc_hash::FxHashMap<String, u32> = rustc_hash::FxHashMap::default();
    for o in &with_ownership {
        *tally
            .entry(o.top_contributor.identifier.clone())
            .or_insert(0) += 1;
    }
    let mut ranked: Vec<(String, u32)> = tally.into_iter().collect();
    ranked.sort_by_key(|b| std::cmp::Reverse(b.1));
    let top_authors: Vec<String> = ranked
        .iter()
        .take(3)
        .map(|(id, n)| format!("{id} ({n})"))
        .collect();

    let mut segments: Vec<String> = Vec::new();
    if bus1_count > 0 {
        let label = if bus1_count == total {
            format!("all {total} hotspots depend on a single recent contributor")
        } else {
            format!("{bus1_count}/{total} hotspots depend on a single recent contributor")
        };
        segments.push(label.red().bold().to_string());
    }
    if !top_authors.is_empty() {
        segments.push(
            format!("top authors: {}", top_authors.join(", "))
                .dimmed()
                .to_string(),
        );
    }

    if segments.is_empty() {
        None
    } else {
        Some(segments.join("  ·  "))
    }
}

fn handle_matches_owner(identifier: &str, declared_owner: &str) -> bool {
    let owner_handle = declared_owner.trim_start_matches('@');
    if owner_handle.is_empty() || identifier.is_empty() {
        return false;
    }
    let id_handle = identifier.split('@').next().unwrap_or(identifier);
    let id_handle = id_handle.split('+').next_back().unwrap_or(id_handle);
    id_handle.eq_ignore_ascii_case(owner_handle)
}

fn render_ownership_line(
    ownership: &fallow_output::OwnershipMetrics,
    trend: fallow_engine::ChurnTrend,
) -> String {
    let mut parts: Vec<String> = Vec::new();

    let top_share = ownership.top_contributor.share;
    let is_accelerating = matches!(trend, fallow_engine::ChurnTrend::Accelerating);
    let is_extreme = top_share >= 0.9 || (ownership.bus_factor == 1 && is_accelerating);
    let bus_str = if top_share >= 0.9999 {
        format!("bus={} (sole author)", ownership.bus_factor)
    } else if ownership.bus_factor <= 1 && is_extreme {
        format!("bus={} (at risk)", ownership.bus_factor)
    } else {
        format!("bus={}", ownership.bus_factor)
    };
    let bus_colored = if is_extreme {
        bus_str.red().bold().to_string()
    } else if ownership.bus_factor <= 1 {
        bus_str.yellow().to_string()
    } else {
        bus_str.dimmed().to_string()
    };
    parts.push(bus_colored);

    let top = &ownership.top_contributor;
    let collapsed = ownership
        .declared_owner
        .as_deref()
        .filter(|owner| handle_matches_owner(&top.identifier, owner));
    if let Some(owner) = collapsed {
        parts.push(
            format!(
                "owned by {} ({:.0}%, declared {})",
                top.identifier,
                top.share * 100.0,
                owner,
            )
            .dimmed()
            .to_string(),
        );
    } else {
        parts.push(
            format!("top={} ({:.0}%)", top.identifier, top.share * 100.0)
                .dimmed()
                .to_string(),
        );
        if let Some(owner) = &ownership.declared_owner {
            parts.push(format!("owner={owner}").dimmed().to_string());
        }
    }

    if ownership.unowned == Some(true) {
        parts.push("unowned".red().to_string());
    }

    if ownership.ownership_state == fallow_output::OwnershipState::DeclaredInactive {
        parts.push("declared owner inactive".yellow().to_string());
    }

    if ownership.drift {
        parts.push("drift".yellow().to_string());
    }

    parts.join("  ")
}

pub(super) fn render_hotspots(
    lines: &mut Vec<String>,
    report: &fallow_output::HealthReport,
    root: &Path,
) {
    if report.hotspots.is_empty() {
        return;
    }

    push_hotspots_header(lines, report);

    if let Some(summary_line) = render_ownership_summary(report) {
        lines.push(format!("  {summary_line}"));
        lines.push(String::new());
    }

    for entry in &report.hotspots {
        push_hotspot_row(lines, entry, root);
    }

    push_hotspots_footer(lines, report);
}

fn push_hotspots_header(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let header = report.hotspot_summary.as_ref().map_or_else(
        || format!("Hotspots ({} files)", report.hotspots.len()),
        |summary| {
            format!(
                "Hotspots ({} files, since {})",
                report.hotspots.len(),
                summary.since,
            )
        },
    );
    lines.push(format!("{} {}", "\u{25cf}".red(), header.red().bold()));
    lines.push(String::new());
}

fn push_hotspot_row(lines: &mut Vec<String>, entry: &fallow_output::HotspotEntry, root: &Path) {
    let file_str = relative_path(&entry.path, root).display().to_string();
    let (dir, filename) = split_dir_filename(&file_str);
    lines.push(format!(
        "  {} {}  {}{}{}",
        hotspot_score_colored(entry.score),
        hotspot_trend_symbol(entry.trend),
        dir.dimmed(),
        filename,
        hotspot_test_tag(entry.is_test_path),
    ));
    lines.push(format!(
        "         {} commits  {} churn  {} density  {} fan-in  {}",
        format!("{:>3}", entry.commits).dimmed(),
        format!("{:>5}", entry.lines_added + entry.lines_deleted).dimmed(),
        format!("{:.2}", entry.complexity_density).dimmed(),
        format!("{:>2}", entry.fan_in).dimmed(),
        hotspot_trend_label(entry.trend),
    ));
    if let Some(ownership) = &entry.ownership {
        lines.push(format!(
            "         {}",
            render_ownership_line(ownership, entry.trend)
        ));
    }
    lines.push(String::new());
}

fn hotspot_score_colored(score: f64) -> String {
    let score_str = format!("{score:>5.1}");
    if score >= 70.0 {
        score_str.red().bold().to_string()
    } else if score >= 30.0 {
        score_str.yellow().to_string()
    } else {
        score_str.green().to_string()
    }
}

fn hotspot_trend_symbol(trend: fallow_engine::ChurnTrend) -> String {
    match trend {
        fallow_engine::ChurnTrend::Accelerating => "\u{25b2}".red().to_string(),
        fallow_engine::ChurnTrend::Cooling => "\u{25bc}".green().to_string(),
        fallow_engine::ChurnTrend::Stable => "\u{2500}".dimmed().to_string(),
    }
}

fn hotspot_trend_label(trend: fallow_engine::ChurnTrend) -> String {
    match trend {
        fallow_engine::ChurnTrend::Accelerating => "\u{25b2} accelerating".red().to_string(),
        fallow_engine::ChurnTrend::Cooling => "\u{25bc} cooling".green().to_string(),
        fallow_engine::ChurnTrend::Stable => "\u{2500} stable".dimmed().to_string(),
    }
}

fn hotspot_test_tag(is_test_path: bool) -> String {
    if is_test_path {
        format!(" {}", "[test]".dimmed())
    } else {
        String::new()
    }
}

fn push_hotspots_footer(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    push_hotspots_excluded_line(lines, report);
    if hotspots_have_history_only_ownership(report) {
        lines.push(format!(
            "  {}",
            "No CODEOWNERS file discovered, ownership signals limited to change history.".dimmed()
        ));
    }
    lines.push(format!(
        "  {}",
        format!("Files with high churn and high complexity: {DOCS_HEALTH}#hotspot-metrics")
            .dimmed()
    ));
    lines.push(String::new());
}

fn push_hotspots_excluded_line(lines: &mut Vec<String>, report: &fallow_output::HealthReport) {
    let Some(summary) = report.hotspot_summary.as_ref() else {
        return;
    };
    if summary.files_excluded == 0 {
        return;
    }
    lines.push(format!(
        "  {}",
        format!(
            "{} file{} excluded (< {} commits)",
            summary.files_excluded,
            plural(summary.files_excluded),
            summary.min_commits,
        )
        .dimmed()
    ));
    lines.push(String::new());
}

fn hotspots_have_history_only_ownership(report: &fallow_output::HealthReport) -> bool {
    let any_ownership = report.hotspots.iter().any(|h| h.ownership.is_some());
    let no_codeowners_anywhere = report
        .hotspots
        .iter()
        .filter_map(|h| h.ownership.as_ref())
        .all(|o| o.unowned.is_none());
    any_ownership && no_codeowners_anywhere
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_engine::ChurnTrend;

    use super::super::plain;
    use super::*;
    use fallow_output::{
        ContributorEntry, ContributorIdentifierFormat, HealthReport, HotspotEntry, HotspotFinding,
        HotspotSummary, OwnershipMetrics, OwnershipState,
    };

    fn contributor(identifier: &str, share: f64) -> ContributorEntry {
        ContributorEntry {
            identifier: identifier.to_owned(),
            format: ContributorIdentifierFormat::Handle,
            share,
            stale_days: 0,
            commits: 10,
        }
    }

    fn ownership(
        identifier: &str,
        share: f64,
        bus_factor: u32,
        owner: Option<&str>,
        state: OwnershipState,
    ) -> OwnershipMetrics {
        OwnershipMetrics {
            bus_factor,
            contributor_count: 2,
            top_contributor: contributor(identifier, share),
            recent_contributors: vec![contributor(identifier, share)],
            suggested_reviewers: Vec::new(),
            declared_owner: owner.map(str::to_owned),
            unowned: None,
            ownership_state: state,
            drift: matches!(state, OwnershipState::Drifting),
            drift_reason: None,
        }
    }

    fn hotspot(path: PathBuf, score: f64, trend: ChurnTrend) -> HotspotEntry {
        HotspotEntry {
            path,
            score,
            commits: 5,
            weighted_commits: 5.0,
            lines_added: 80,
            lines_deleted: 20,
            complexity_density: 0.42,
            fan_in: 7,
            trend,
            ownership: None,
            is_test_path: false,
        }
    }

    #[test]
    fn owner_matching_handles_email_plus_suffixes_and_empty_values() {
        assert!(handle_matches_owner("alice@example.com", "@alice"));
        assert!(handle_matches_owner("team+alice@example.com", "alice"));
        assert!(handle_matches_owner("Alice", "@alice"));
        assert!(!handle_matches_owner("", "@alice"));
        assert!(!handle_matches_owner("alice@example.com", "@"));
        assert!(!handle_matches_owner("bob@example.com", "@alice"));
    }

    #[test]
    fn ownership_line_collapses_matching_owner_and_flags_risk() {
        let matching = ownership(
            "team+alice@example.com",
            1.0,
            1,
            Some("@alice"),
            OwnershipState::Active,
        );
        let line = plain(&[render_ownership_line(&matching, ChurnTrend::Accelerating)]);

        assert!(line.contains("bus=1 (sole author)"));
        assert!(line.contains("owned by team+alice@example.com (100%, declared @alice)"));

        let mut drifting = ownership(
            "bob@example.com",
            0.75,
            1,
            Some("@alice"),
            OwnershipState::Drifting,
        );
        drifting.unowned = Some(true);
        let line = plain(&[render_ownership_line(&drifting, ChurnTrend::Stable)]);

        assert!(line.contains("top=bob@example.com (75%)"));
        assert!(line.contains("owner=@alice"));
        assert!(line.contains("unowned"));
        assert!(line.contains("drift"));
    }

    #[test]
    fn hotspots_render_summary_ownership_and_exclusions() {
        let root = PathBuf::from("/repo");
        let mut first = hotspot(root.join("src/api.ts"), 75.0, ChurnTrend::Accelerating);
        first.ownership = Some(ownership(
            "alice@example.com",
            0.91,
            1,
            None,
            OwnershipState::Unowned,
        ));
        let mut second = hotspot(root.join("tests/api.test.ts"), 20.0, ChurnTrend::Cooling);
        second.is_test_path = true;
        second.ownership = Some(ownership(
            "alice@example.com",
            0.70,
            1,
            None,
            OwnershipState::DeclaredInactive,
        ));

        let report = HealthReport {
            hotspots: vec![HotspotFinding::from(first), HotspotFinding::from(second)],
            hotspot_summary: Some(HotspotSummary {
                since: "90d".to_owned(),
                min_commits: 3,
                files_analyzed: 10,
                files_excluded: 1,
                shallow_clone: false,
            }),
            ..HealthReport::default()
        };
        let mut lines = Vec::new();

        render_hotspots(&mut lines, &report, &root);
        let text = plain(&lines);

        assert!(text.contains("Hotspots (2 files, since 90d)"));
        assert!(text.contains("all 2 hotspots depend on a single recent contributor"));
        assert!(text.contains("top authors: alice@example.com (2)"));
        assert!(text.contains("src/api.ts"));
        assert!(text.contains("tests/api.test.ts [test]"));
        assert!(text.contains("1 file excluded (< 3 commits)"));
        assert!(text.contains("No CODEOWNERS file discovered"));
    }
}
