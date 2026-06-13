use crate::params::HealthParams;

use super::{push_baseline, push_global, push_scope, push_str_flag};

/// Build CLI arguments for the `check_health` tool.
pub fn build_health_args(params: &HealthParams) -> Vec<String> {
    HealthArgsBuilder {
        args: vec![
            "health".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--quiet".to_string(),
            "--explain".to_string(),
        ],
        params,
    }
    .build()
}

struct HealthArgsBuilder<'a> {
    args: Vec<String>,
    params: &'a HealthParams,
}

impl HealthArgsBuilder<'_> {
    fn build(mut self) -> Vec<String> {
        self.push_global_scope();
        self.push_thresholds();
        self.push_sort_and_diff();
        self.push_analysis_sections();
        self.push_ownership();
        self.push_target_and_coverage_gates();
        self.push_score_and_severity();
        self.push_history();
        self.push_snapshot_and_baseline();
        self.push_coverage();
        self.push_runtime_coverage();
        push_str_flag(
            &mut self.args,
            "--group-by",
            self.params.group_by.as_deref(),
        );
        self.args
    }

    fn push_global_scope(&mut self) {
        push_global(
            &mut self.args,
            self.params.root.as_deref(),
            self.params.config.as_deref(),
            self.params.no_cache,
            self.params.threads,
        );
        push_scope(
            &mut self.args,
            self.params.production,
            self.params.workspace.as_deref(),
        );
    }

    fn push_thresholds(&mut self) {
        if let Some(max_cyclomatic) = self.params.max_cyclomatic {
            self.args
                .extend(["--max-cyclomatic".to_string(), max_cyclomatic.to_string()]);
        }
        if let Some(max_cognitive) = self.params.max_cognitive {
            self.args
                .extend(["--max-cognitive".to_string(), max_cognitive.to_string()]);
        }
        if let Some(max_crap) = self.params.max_crap {
            self.args
                .extend(["--max-crap".to_string(), format!("{max_crap}")]);
        }
        if let Some(top) = self.params.top {
            self.args.extend(["--top".to_string(), top.to_string()]);
        }
    }

    fn push_sort_and_diff(&mut self) {
        push_str_flag(&mut self.args, "--sort", self.params.sort.as_deref());
        push_str_flag(
            &mut self.args,
            "--changed-since",
            self.params.changed_since.as_deref(),
        );
    }

    fn push_analysis_sections(&mut self) {
        if self.params.complexity == Some(true) {
            self.args.push("--complexity".to_string());
        }
        if self.params.complexity_breakdown == Some(true) {
            self.args.push("--complexity-breakdown".to_string());
        }
        if self.params.file_scores == Some(true) {
            self.args.push("--file-scores".to_string());
        }
        if self.params.css == Some(true) {
            self.args.push("--css".to_string());
        }
    }

    fn push_ownership(&mut self) {
        let ownership_active =
            self.params.ownership == Some(true) || self.params.ownership_email_mode.is_some();
        if self.params.hotspots == Some(true) || ownership_active {
            self.args.push("--hotspots".to_string());
        }
        if ownership_active {
            self.args.push("--ownership".to_string());
        }
        if let Some(mode) = self.params.ownership_email_mode {
            self.args
                .extend(["--ownership-emails".to_string(), mode.as_cli().to_string()]);
        }
    }

    fn push_target_and_coverage_gates(&mut self) {
        if self.params.targets == Some(true) {
            self.args.push("--targets".to_string());
        }
        if self.params.coverage_gaps == Some(true) {
            self.args.push("--coverage-gaps".to_string());
        }
    }

    fn push_score_and_severity(&mut self) {
        if self.params.score == Some(true) {
            self.args.push("--score".to_string());
        }
        if let Some(min_score) = self.params.min_score {
            self.args
                .extend(["--min-score".to_string(), min_score.to_string()]);
        }
        push_str_flag(
            &mut self.args,
            "--min-severity",
            self.params.min_severity.as_deref(),
        );
    }

    fn push_history(&mut self) {
        push_str_flag(&mut self.args, "--since", self.params.since.as_deref());
        if let Some(min_commits) = self.params.min_commits {
            self.args
                .extend(["--min-commits".to_string(), min_commits.to_string()]);
        }
        push_str_flag(
            &mut self.args,
            "--churn-file",
            self.params.churn_file.as_deref(),
        );
    }

    fn push_snapshot_and_baseline(&mut self) {
        if let Some(ref path) = self.params.save_snapshot {
            if path.is_empty() {
                self.args.push("--save-snapshot".to_string());
            } else {
                self.args
                    .extend(["--save-snapshot".to_string(), path.clone()]);
            }
        }
        push_baseline(
            &mut self.args,
            self.params.baseline.as_deref(),
            self.params.save_baseline.as_deref(),
        );
        if self.params.trend == Some(true) {
            self.args.push("--trend".to_string());
        }
        push_str_flag(&mut self.args, "--effort", self.params.effort.as_deref());
        if self.params.summary == Some(true) {
            self.args.push("--summary".to_string());
        }
    }

    fn push_coverage(&mut self) {
        push_str_flag(
            &mut self.args,
            "--coverage",
            self.params.coverage.as_deref(),
        );
        push_str_flag(
            &mut self.args,
            "--coverage-root",
            self.params.coverage_root.as_deref(),
        );
        push_str_flag(
            &mut self.args,
            "--runtime-coverage",
            self.params.runtime_coverage.as_deref(),
        );
    }

    fn push_runtime_coverage(&mut self) {
        if let Some(min_invocations_hot) = self.params.min_invocations_hot {
            self.args.extend([
                "--min-invocations-hot".to_string(),
                min_invocations_hot.to_string(),
            ]);
        }
        if let Some(min_observation_volume) = self.params.min_observation_volume {
            self.args.extend([
                "--min-observation-volume".to_string(),
                min_observation_volume.to_string(),
            ]);
        }
        if let Some(low_traffic_threshold) = self.params.low_traffic_threshold {
            self.args.extend([
                "--low-traffic-threshold".to_string(),
                format!("{low_traffic_threshold}"),
            ]);
        }
    }
}
