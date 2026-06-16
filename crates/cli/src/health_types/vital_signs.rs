//! Vital signs: project-wide metrics for trend tracking and snapshots.

/// Current snapshot schema version. Independent of the report's SCHEMA_VERSION.
/// v2: Added `score` and `grade` fields.
/// v3: Added `coverage_model` field.
/// v4: Added risk profiles (`unit_size_profile`, `unit_interfacing_profile`) and
///     coupling concentration (`p95_fan_in`, `coupling_high_pct`).
/// v5: Added duplication penalty to health score formula.
/// v6: Added `total_loc` to vital signs (always computed from parsed modules).
/// v7: MI formula dampening for small files (values change for files < 50 lines).
/// v8: Added scale-invariant tail/density metrics for health score calibration.
/// v9: Added render fan-in concentration (`p95_render_fan_in`,
///     `render_fan_in_high_pct`, `max_render_fan_in`), the component-graph
///     analogue of module fan-in / coupling concentration. Additive optional
///     fields (matches the v4 precedent that added coupling concentration).
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 9;

/// Project-wide vital signs — a fixed set of metrics for trend tracking.
///
/// Metrics are `Option` when the data source was not available in the current run
/// (e.g., `duplication_pct` is `None` unless the duplication pipeline was run,
/// `hotspot_count` is `None` without git history).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VitalSigns {
    /// Percentage of files not reachable from any entry point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_file_pct: Option<f64>,
    /// Percentage of exports never imported by other modules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_export_pct: Option<f64>,
    /// Average cyclomatic complexity across all functions.
    pub avg_cyclomatic: f64,
    /// Percentage of functions at or above the critical cyclomatic threshold.
    /// Used by the scale-invariant health score.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub critical_complexity_pct: Option<f64>,
    /// 90th percentile cyclomatic complexity.
    pub p90_cyclomatic: u32,
    /// Code duplication percentage (None if duplication pipeline was not run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication_pct: Option<f64>,
    /// Number of hotspot files (score >= 50). None if git history unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotspot_count: Option<u32>,
    /// Number of files in the top 1% of the within-project hotspot ranking.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hotspot_top_pct_count: Option<u32>,
    /// Average maintainability index across all scored files (0–100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintainability_avg: Option<f64>,
    /// Percentage of scored files with maintainability index below 70. Null if
    /// file scores were not computed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub maintainability_low_pct: Option<f64>,
    /// Number of unused dependencies (dependencies + devDependencies + optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_dep_count: Option<u32>,
    /// Unused dependencies per 1,000 files. Null if dead code analysis did not
    /// run.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub unused_deps_per_k_files: Option<f64>,
    /// Number of circular dependency chains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circular_dep_count: Option<u32>,
    /// Circular dependency chains per 1,000 files. Null if dead code analysis
    /// did not run.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub circular_deps_per_k_files: Option<f64>,
    /// Raw counts backing the percentages (for orientation header display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<VitalSignsCounts>,
    /// Function size risk profile: percentage of functions in each size bin.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub unit_size_profile: Option<RiskProfile>,
    /// Functions above 60 LOC per 1,000 functions. Null if no functions
    /// analyzed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub functions_over_60_loc_per_k: Option<f64>,
    /// Parameter count risk profile: percentage of functions in each param bin.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub unit_interfacing_profile: Option<RiskProfile>,
    /// 95th percentile fan-in across all files. Null if file scores not
    /// computed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub p95_fan_in: Option<u32>,
    /// Percentage of files with fan-in above the project's p95 threshold.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub coupling_high_pct: Option<f64>,
    /// Number of located prop-drilling chains (React/Preact props forwarded
    /// unchanged through 3+ pass-through components). `None` unless the opt-in
    /// `prop-drilling` rule is enabled (it defaults to off), so the small capped
    /// penalty and the hotspot surface are dormant by default.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prop_drilling_chain_count: Option<u32>,
    /// The deepest located prop-drilling chain's depth (forwarding hops). `None`
    /// when no chains were found or the rule is off. Descriptive context only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prop_drilling_max_depth: Option<u32>,
    /// 95th-percentile DISTINCT-PARENTS render fan-in across React/Preact
    /// components (the component-graph analogue of `p95_fan_in`, which percentiles
    /// per-FILE module fan-in). `None` on non-React runs. Descriptive
    /// blast-radius context, NOT a gate. Mirrors `compute_coupling_concentration`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub p95_render_fan_in: Option<u32>,
    /// Percentage of components whose render fan-in exceeds the project's
    /// `max(p95, 10)` threshold (reuses the coupling-concentration floor; NO new
    /// tunable constant). `None` on non-React runs. Mirrors `coupling_high_pct`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub render_fan_in_high_pct: Option<f64>,
    /// The single highest DISTINCT-PARENTS count across all components (the
    /// headline blast-radius number: the most distinct render LOCATIONS any one
    /// component is rendered from, the honest edit-ripple count). `render_sites`
    /// (incl. repeats) is secondary per-component context, never the headline.
    /// `None` on non-React runs. Descriptive context, no threshold.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_render_fan_in: Option<u32>,
    /// The highest-fan-in React/Preact components, located (component name +
    /// project-relative path + render-site / distinct-parent counts), sorted by
    /// distinct parents (the honest headline axis) descending, tie-broken on
    /// render sites descending, and capped at a small N. Lets a consumer see
    /// WHICH component carries the headline `max_render_fan_in`, not just the
    /// number. Empty (and omitted from JSON) on non-React runs, so the contract
    /// stays byte-identical there. Descriptive blast-radius context, NOT a gate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_render_fan_in: Vec<RenderFanInTopComponent>,
    /// Total lines of code across all parsed modules.
    #[serde(default)]
    pub total_loc: u64,
}

/// One located high-fan-in React/Preact component for the descriptive
/// `top_render_fan_in` blast-radius list on [`VitalSigns`].
///
/// The component-graph analogue of a high-fan-in module: `distinct_parents` is
/// the HEADLINE axis (the honest count of distinct parent components / render
/// LOCATIONS that render this component), `render_sites` is secondary "incl.
/// repeats" context (every JSX render SITE, so a single parent rendering one
/// child five times is five sites but one parent). Undercount-safe like the
/// underlying metric: a child rendered via a JSX spread / dynamic /
/// member-expression tag resolves to no component, so a true high-fan-in
/// component can only be undersold.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RenderFanInTopComponent {
    /// The component name.
    pub component: String,
    /// Project-relative path of the file declaring the component. Serialized with
    /// forward slashes (same serializer the other relativized health paths use).
    #[serde(serialize_with = "fallow_types::serde_path::serialize")]
    pub path: std::path::PathBuf,
    /// Total JSX render SITES that resolve to this component across the project.
    /// SECONDARY "incl. repeats" context, not the headline (see `distinct_parents`).
    pub render_sites: u32,
    /// Distinct `(parent_file, parent_component)` keys that render this component.
    /// The HEADLINE blast-radius axis: distinct render LOCATIONS.
    pub distinct_parents: u32,
}

/// Risk profile: percentage of functions in each risk bin.
///
/// Bins are defined by thresholds that depend on the measured property:
/// - **Unit size**: low risk (1-15 LOC), medium risk (16-30), high risk (31-60), very high risk (>60)
/// - **Unit interfacing**: low risk (0-2 params), medium risk (3-4), high risk (5-6), very high risk (>=7)
///
/// Percentages sum to approximately 100.0 (subject to rounding).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[allow(
    clippy::struct_field_names,
    reason = "risk suffix conveys that higher values are worse"
)]
pub struct RiskProfile {
    /// Percentage of functions in the low-risk bin.
    pub low_risk: f64,
    /// Percentage of functions in the medium-risk bin.
    pub medium_risk: f64,
    /// Percentage of functions in the high-risk bin.
    pub high_risk: f64,
    /// Percentage of functions in the very-high-risk bin.
    pub very_high_risk: f64,
}

/// Raw counts backing the vital signs percentages.
///
/// Stored alongside `VitalSigns` in snapshots so that Phase 2b trend reporting
/// can decompose percentage changes into numerator vs denominator shifts.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VitalSignsCounts {
    /// Total number of discovered source files.
    pub total_files: usize,
    /// Total number of exports across all files.
    pub total_exports: usize,
    pub dead_files: usize,
    pub dead_exports: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicated_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_scored: Option<usize>,
    pub total_deps: usize,
}

/// A point-in-time snapshot of project vital signs, persisted to disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VitalSignsSnapshot {
    /// Schema version for snapshot format (independent of report schema_version).
    pub snapshot_schema_version: u32,
    /// Fallow version that produced this snapshot.
    pub version: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Git commit SHA at time of snapshot (None if not in a git repo).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// Git branch name (None if not in a git repo or detached HEAD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Whether the repository is a shallow clone.
    #[serde(default)]
    pub shallow_clone: bool,
    /// The vital signs metrics.
    pub vital_signs: VitalSigns,
    /// Raw counts for trend decomposition.
    pub counts: VitalSignsCounts,
    /// Project health score (0–100). Added in schema v2.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub score: Option<f64>,
    /// Letter grade (A/B/C/D/F). Added in schema v2.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub grade: Option<String>,
    /// Coverage model used for CRAP computation. Added in schema v3.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub coverage_model: Option<super::CoverageModel>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vital_signs_serialization_roundtrip() {
        let vs = VitalSigns {
            dead_file_pct: Some(3.2),
            dead_export_pct: Some(8.1),
            avg_cyclomatic: 4.7,
            critical_complexity_pct: Some(1.2),
            p90_cyclomatic: 12,
            duplication_pct: None,
            hotspot_count: Some(5),
            hotspot_top_pct_count: Some(12),
            maintainability_avg: Some(72.4),
            maintainability_low_pct: Some(4.1),
            unused_dep_count: Some(4),
            unused_deps_per_k_files: Some(3.3),
            circular_dep_count: Some(2),
            circular_deps_per_k_files: Some(1.7),
            counts: None,
            unit_size_profile: None,
            functions_over_60_loc_per_k: Some(8.2),
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            prop_drilling_chain_count: None,
            prop_drilling_max_depth: None,
            p95_render_fan_in: Some(3),
            render_fan_in_high_pct: Some(4.5),
            max_render_fan_in: Some(6),
            top_render_fan_in: Vec::new(),
            total_loc: 42_000,
        };
        let json = serde_json::to_string(&vs).unwrap();
        let deserialized: VitalSigns = serde_json::from_str(&json).unwrap();
        assert!((deserialized.avg_cyclomatic - 4.7).abs() < f64::EPSILON);
        assert_eq!(deserialized.p95_render_fan_in, Some(3));
        assert_eq!(deserialized.max_render_fan_in, Some(6));
        assert_eq!(deserialized.p90_cyclomatic, 12);
        assert_eq!(deserialized.hotspot_count, Some(5));
        assert!(!json.contains("duplication_pct"));
        assert!(deserialized.duplication_pct.is_none());
    }

    #[test]
    fn vital_signs_snapshot_roundtrip() {
        let snapshot = VitalSignsSnapshot {
            snapshot_schema_version: SNAPSHOT_SCHEMA_VERSION,
            version: "1.8.1".into(),
            timestamp: "2026-03-25T14:30:00Z".into(),
            git_sha: Some("abc1234".into()),
            git_branch: Some("main".into()),
            shallow_clone: false,
            vital_signs: VitalSigns {
                dead_file_pct: Some(3.2),
                dead_export_pct: Some(8.1),
                avg_cyclomatic: 4.7,
                critical_complexity_pct: None,
                p90_cyclomatic: 12,
                duplication_pct: None,
                hotspot_count: None,
                hotspot_top_pct_count: None,
                maintainability_avg: Some(72.4),
                maintainability_low_pct: None,
                unused_dep_count: Some(4),
                unused_deps_per_k_files: None,
                circular_dep_count: Some(2),
                circular_deps_per_k_files: None,
                counts: None,
                unit_size_profile: None,
                functions_over_60_loc_per_k: None,
                unit_interfacing_profile: None,
                p95_fan_in: None,
                coupling_high_pct: None,
                prop_drilling_chain_count: None,
                prop_drilling_max_depth: None,
                p95_render_fan_in: None,
                render_fan_in_high_pct: None,
                max_render_fan_in: None,
                top_render_fan_in: Vec::new(),
                total_loc: 42_000,
            },
            counts: VitalSignsCounts {
                total_files: 1200,
                total_exports: 5400,
                dead_files: 38,
                dead_exports: 437,
                duplicated_lines: None,
                total_lines: None,
                files_scored: Some(1150),
                total_deps: 42,
            },
            score: Some(78.5),
            grade: Some("B".into()),
            coverage_model: Some(crate::health_types::CoverageModel::StaticEstimated),
        };
        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        let rt: VitalSignsSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(rt.snapshot_schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(rt.git_sha.as_deref(), Some("abc1234"));
        assert_eq!(rt.counts.total_files, 1200);
        assert_eq!(rt.counts.dead_exports, 437);
        assert_eq!(rt.score, Some(78.5));
        assert_eq!(rt.grade.as_deref(), Some("B"));
    }

    #[test]
    fn vital_signs_all_none_optional_fields_omitted() {
        let vs = VitalSigns {
            dead_file_pct: None,
            dead_export_pct: None,
            avg_cyclomatic: 5.0,
            critical_complexity_pct: None,
            p90_cyclomatic: 10,
            duplication_pct: None,
            hotspot_count: None,
            hotspot_top_pct_count: None,
            maintainability_avg: None,
            maintainability_low_pct: None,
            unused_dep_count: None,
            unused_deps_per_k_files: None,
            circular_dep_count: None,
            circular_deps_per_k_files: None,
            counts: None,
            unit_size_profile: None,
            functions_over_60_loc_per_k: None,
            unit_interfacing_profile: None,
            p95_fan_in: None,
            coupling_high_pct: None,
            prop_drilling_chain_count: None,
            prop_drilling_max_depth: None,
            p95_render_fan_in: None,
            render_fan_in_high_pct: None,
            max_render_fan_in: None,
            top_render_fan_in: Vec::new(),
            total_loc: 0,
        };
        let json = serde_json::to_string(&vs).unwrap();
        assert!(!json.contains("dead_file_pct"));
        assert!(!json.contains("p95_render_fan_in"));
        assert!(!json.contains("top_render_fan_in"));
        assert!(!json.contains("render_fan_in_high_pct"));
        assert!(!json.contains("max_render_fan_in"));
        assert!(!json.contains("dead_export_pct"));
        assert!(!json.contains("duplication_pct"));
        assert!(!json.contains("hotspot_count"));
        assert!(!json.contains("maintainability_avg"));
        assert!(!json.contains("unused_dep_count"));
        assert!(!json.contains("circular_dep_count"));
        assert!(json.contains("avg_cyclomatic"));
        assert!(json.contains("p90_cyclomatic"));
    }

    #[test]
    fn snapshot_schema_version_is_nine() {
        assert_eq!(SNAPSHOT_SCHEMA_VERSION, 9);
    }

    #[test]
    fn snapshot_v1_deserializes_with_default_score_and_grade() {
        let json = r#"{
            "snapshot_schema_version": 1,
            "version": "1.5.0",
            "timestamp": "2025-01-01T00:00:00Z",
            "shallow_clone": false,
            "vital_signs": {
                "avg_cyclomatic": 2.0,
                "p90_cyclomatic": 5
            },
            "counts": {
                "total_files": 100,
                "total_exports": 500,
                "dead_files": 0,
                "dead_exports": 0,
                "total_deps": 20
            }
        }"#;
        let snap: VitalSignsSnapshot = serde_json::from_str(json).unwrap();
        assert!(snap.score.is_none());
        assert!(snap.grade.is_none());
        assert_eq!(snap.snapshot_schema_version, 1);
    }
}
