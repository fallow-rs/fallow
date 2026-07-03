#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CliFallbackReason {
    Baseline,
    Regression,
    GroupedOutput,
    DuplicationExplainSkipped,
    HealthMinScoreGate,
    HealthMinSeverity,
    HealthChurnFile,
    HealthSnapshot,
    HealthTrend,
    HealthSummary,
    HealthRuntimeCoverage,
}

pub(super) fn baseline_fallback_reason(
    baseline: Option<&str>,
    save_baseline: Option<&str>,
) -> Option<CliFallbackReason> {
    (filled(baseline) || filled(save_baseline)).then_some(CliFallbackReason::Baseline)
}

pub(super) fn regression_fallback_reason(
    fail_on_regression: Option<bool>,
    tolerance: Option<&str>,
    regression_baseline: Option<&str>,
    save_regression_baseline: Option<&str>,
) -> Option<CliFallbackReason> {
    (fail_on_regression == Some(true)
        || filled(tolerance)
        || filled(regression_baseline)
        || filled(save_regression_baseline))
    .then_some(CliFallbackReason::Regression)
}

pub(super) fn grouped_fallback_reason(group_by: Option<&str>) -> Option<CliFallbackReason> {
    filled(group_by).then_some(CliFallbackReason::GroupedOutput)
}

pub(super) fn duplication_fallback_reason(
    group_by: Option<&str>,
    explain_skipped: Option<bool>,
) -> Option<CliFallbackReason> {
    grouped_fallback_reason(group_by).or_else(|| {
        (explain_skipped == Some(true)).then_some(CliFallbackReason::DuplicationExplainSkipped)
    })
}

pub(super) fn filled(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn baseline_reason_tracks_either_baseline_surface() {
        assert_eq!(
            baseline_fallback_reason(Some("base.json"), None),
            Some(CliFallbackReason::Baseline)
        );
        assert_eq!(
            baseline_fallback_reason(None, Some("next.json")),
            Some(CliFallbackReason::Baseline)
        );
        assert_eq!(baseline_fallback_reason(None, None), None);
    }

    #[test]
    fn duplication_reason_preserves_grouping_precedence() {
        assert_eq!(
            duplication_fallback_reason(Some("owner"), Some(true)),
            Some(CliFallbackReason::GroupedOutput)
        );
        assert_eq!(
            duplication_fallback_reason(None, Some(true)),
            Some(CliFallbackReason::DuplicationExplainSkipped)
        );
    }

    #[test]
    fn cli_fallback_surfaces_are_explicitly_owned() {
        let tools_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tools");
        let unconditional_cli_backed = [
            "check_runtime_coverage.rs",
            "code_mode.rs",
            "code_mode_subprocess.rs",
            "fix.rs",
            "guard.rs",
            "impact.rs",
            "inspect_target.rs",
            "mod.rs",
            "security.rs",
        ];
        let conditional_cli_backed = [
            "analyze.rs",
            "audit.rs",
            "check_changed.rs",
            "dupes.rs",
            "health.rs",
        ];

        for entry in std::fs::read_dir(&tools_dir).expect("read tools dir") {
            let entry = entry.expect("read tools entry");
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }

            let file_name = file_name(&path);
            if file_name == "fallback_policy.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read mcp tool source");
            let invokes_cli = invokes_cli_dispatch(&source);
            let is_unconditional = unconditional_cli_backed.contains(&file_name);
            let is_conditional = conditional_cli_backed.contains(&file_name);

            if invokes_cli {
                assert!(
                    is_unconditional || is_conditional,
                    "{file_name} must be API-backed or added to the explicit CLI fallback policy"
                );
            }

            if is_conditional {
                assert!(
                    source.contains("requires_cli_fallback"),
                    "{file_name} must gate subprocess execution behind requires_cli_fallback"
                );
                continue;
            }

            if !is_unconditional {
                assert!(
                    !invokes_cli,
                    "{file_name} is a pure API-backed MCP tool and must not spawn the CLI"
                );
            }
        }

        for file_name in unconditional_cli_backed {
            let path = tools_dir.join(file_name);
            let source = std::fs::read_to_string(&path).expect("read mcp tool source");
            assert!(
                invokes_cli_dispatch(&source),
                "{file_name} is listed as unconditional CLI-backed but production code no longer spawns the CLI"
            );
        }
    }

    fn file_name(path: &Path) -> &str {
        path.file_name()
            .and_then(|name| name.to_str())
            .expect("utf-8 filename")
    }

    fn invokes_cli_dispatch(source: &str) -> bool {
        source.contains("run_tool(binary")
            || source.contains("run_fallow(binary")
            || source.contains("run_fallow_sync(")
            || source.contains("Command::new(binary)")
    }
}
