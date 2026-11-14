//! The parameters that make the output of one tool smaller.
//!
//! An over-limit refusal carries this list as `narrow`, so an agent that got
//! no result can retry with a smaller request instead of guessing which
//! parameter to change. A drift test checks each name against the input
//! schema of its tool.

/// Narrowing parameters per tool, the most effective first. A tool that is
/// not listed has no parameter that reduces its output, and its refusal
/// omits `narrow`.
const NARROW_PARAMS: &[(&str, &[&str])] = &[
    ("analyze", &["issue_types", "file", "workspace"]),
    ("audit", &["only", "skip", "workspace"]),
    (
        "check_architecture",
        &["changed_since", "file", "workspace"],
    ),
    ("check_changed", &["workspace"]),
    ("check_health", &["top", "changed_since", "workspace"]),
    ("check_runtime_coverage", &["top", "workspace"]),
    ("feature_flags", &["top", "workspace"]),
    (
        "find_dupes",
        &[
            "top",
            "changed_since",
            "workspace",
            "min_tokens",
            "min_lines",
        ],
    ),
    (
        "find_similar_code",
        &["top", "changed_since", "paths", "workspace", "min_lines"],
    ),
    ("get_blast_radius", &["top", "workspace"]),
    ("get_cleanup_candidates", &["top", "workspace"]),
    ("get_cloud_deployment_changes", &["limit"]),
    ("get_cloud_runtime_context", &["top"]),
    ("get_hot_paths", &["top", "workspace"]),
    ("get_importance", &["top", "workspace"]),
    ("impact_all", &["limit"]),
    ("list_suppressions", &["changed_since", "file", "workspace"]),
    (
        "security_candidates",
        &["changed_since", "paths", "workspace"],
    ),
    ("trace_dependency", &["limit"]),
];

/// The narrowing parameters of `tool`, or `None` when it has none.
pub(super) fn narrow_params(tool: &str) -> Option<&'static [&'static str]> {
    NARROW_PARAMS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, params)| *params)
}

/// Every tool with narrowing parameters, for the schema drift test.
#[cfg(test)]
pub fn all_narrow_params() -> &'static [(&'static str, &'static [&'static str])] {
    NARROW_PARAMS
}
