use crate::params::TypeAwareRequireParam;

pub(super) fn baseline_requested(baseline: Option<&str>, save_baseline: Option<&str>) -> bool {
    filled(baseline) || filled(save_baseline)
}

pub(super) fn regression_requested(
    fail_on_regression: Option<bool>,
    tolerance: Option<&str>,
    regression_baseline: Option<&str>,
    save_regression_baseline: Option<&str>,
) -> bool {
    fail_on_regression == Some(true)
        || filled(tolerance)
        || filled(regression_baseline)
        || filled(save_regression_baseline)
}

pub(super) fn grouped_requested(group_by: Option<&str>) -> bool {
    filled(group_by)
}

/// The programmatic duplication route has no threshold gate. A typed call with
/// a `threshold` compares nothing, publishes no `gate_outcomes`, and returns a
/// result that reads as a pass. The CLI owns that comparison, so a call that
/// sets a threshold takes the CLI.
pub(super) fn duplication_needs_cli(
    group_by: Option<&str>,
    explain_skipped: Option<bool>,
    threshold: Option<f64>,
) -> bool {
    threshold.is_some() || grouped_requested(group_by) || explain_skipped == Some(true)
}

/// Whether the call asks for type-aware analysis, which only the CLI runs.
pub(super) fn type_aware_requested(
    type_aware: Option<bool>,
    projects: Option<&[String]>,
    require: Option<&TypeAwareRequireParam>,
) -> bool {
    type_aware == Some(true)
        || projects.is_some_and(|projects| !projects.is_empty())
        || require.is_some()
}

pub(super) fn filled(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn cli_fallback_surfaces_are_explicitly_owned() {
        let tools_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tools");
        let unconditional_cli_backed = [
            "check_runtime_coverage.rs",
            "cloud_runtime_context.rs",
            "code_mode.rs",
            "code_mode_subprocess.rs",
            "fix.rs",
            "guard.rs",
            "impact.rs",
            "inspect_target.rs",
            "mod.rs",
            "recommend.rs",
            "security.rs",
            "semantic.rs",
            "similar_code.rs",
            "suppressions.rs",
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

    /// Whether a tool module dispatches to the CLI. Matched on the call name
    /// alone, not on `(binary` adjacency: rustfmt wraps a long call across
    /// lines and a formatting change must not read as a routing change.
    fn invokes_cli_dispatch(source: &str) -> bool {
        [
            "run_tool(",
            "run_tool_with_limit(",
            "run_tool_with_timeout(",
            "run_tool_with_stdin_timeout(",
            "run_tool_with_top_level_warnings(",
            "run_fallow_sync(",
            "Command::new(binary)",
        ]
        .iter()
        .any(|call| source.contains(call))
    }
}
