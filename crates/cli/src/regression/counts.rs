use fallow_core::results::AnalysisResults;

/// Regression baseline: stores issue counts per type for comparison.
///
/// Unlike `BaselineData` which stores individual issue identities for suppression,
/// this stores counts for "did the total go up?" regression detection.
///
/// `schema_version` is the forward-compatibility gate; unknown fields are tolerated
/// intentionally (see `CheckCounts` `#[serde(default)]`) so adding a new issue type
/// stays backwards-compatible with existing baselines. Bumping `schema_version`
/// signals "this baseline cannot be safely loaded by older fallow builds" and
/// triggers a hard-fail with a regenerate hint in `load_regression_baseline`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct RegressionBaseline {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Fallow version that produced this baseline.
    pub fallow_version: String,
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Git SHA at baseline time, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_sha: Option<String>,
    /// Dead code issue counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckCounts>,
    /// Duplication counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dupes: Option<DupesCounts>,
}

pub const REGRESSION_SCHEMA_VERSION: u32 = 1;

/// Per-type issue counts for dead code analysis.
///
/// All fields use `#[serde(default)]` for forward compatibility: when fallow adds a new
/// issue type, old baselines will deserialize with the new field defaulting to zero
/// instead of failing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckCounts {
    #[serde(default)]
    pub total_issues: usize,
    #[serde(default)]
    pub unused_files: usize,
    #[serde(default)]
    pub unused_exports: usize,
    #[serde(default)]
    pub unused_types: usize,
    #[serde(default)]
    pub unused_dependencies: usize,
    #[serde(default)]
    pub unused_dev_dependencies: usize,
    #[serde(default)]
    pub unused_optional_dependencies: usize,
    #[serde(default)]
    pub unused_enum_members: usize,
    #[serde(default)]
    pub unused_class_members: usize,
    #[serde(default)]
    pub unresolved_imports: usize,
    #[serde(default)]
    pub unlisted_dependencies: usize,
    #[serde(default)]
    pub duplicate_exports: usize,
    #[serde(default)]
    pub circular_dependencies: usize,
    #[serde(default)]
    pub re_export_cycles: usize,
    #[serde(default)]
    pub type_only_dependencies: usize,
    #[serde(default)]
    pub test_only_dependencies: usize,
    #[serde(default)]
    pub boundary_violations: usize,
    #[serde(default)]
    pub boundary_coverage_violations: usize,
    #[serde(default)]
    pub boundary_call_violations: usize,
}

impl CheckCounts {
    #[must_use]
    pub const fn from_results(results: &AnalysisResults) -> Self {
        Self {
            total_issues: results.total_issues(),
            unused_files: results.unused_files.len(),
            unused_exports: results.unused_exports.len(),
            unused_types: results.unused_types.len(),
            unused_dependencies: results.unused_dependencies.len(),
            unused_dev_dependencies: results.unused_dev_dependencies.len(),
            unused_optional_dependencies: results.unused_optional_dependencies.len(),
            unused_enum_members: results.unused_enum_members.len(),
            unused_class_members: results.unused_class_members.len(),
            unresolved_imports: results.unresolved_imports.len(),
            unlisted_dependencies: results.unlisted_dependencies.len(),
            duplicate_exports: results.duplicate_exports.len(),
            circular_dependencies: results.circular_dependencies.len(),
            re_export_cycles: results.re_export_cycles.len(),
            type_only_dependencies: results.type_only_dependencies.len(),
            test_only_dependencies: results.test_only_dependencies.len(),
            boundary_violations: results.boundary_violations.len(),
            boundary_coverage_violations: results.boundary_coverage_violations.len(),
            boundary_call_violations: results.boundary_call_violations.len(),
        }
    }

    /// Convert from config-embedded baseline.
    #[must_use]
    pub const fn from_config_baseline(b: &fallow_config::RegressionBaseline) -> Self {
        Self {
            total_issues: b.total_issues,
            unused_files: b.unused_files,
            unused_exports: b.unused_exports,
            unused_types: b.unused_types,
            unused_dependencies: b.unused_dependencies,
            unused_dev_dependencies: b.unused_dev_dependencies,
            unused_optional_dependencies: b.unused_optional_dependencies,
            unused_enum_members: b.unused_enum_members,
            unused_class_members: b.unused_class_members,
            unresolved_imports: b.unresolved_imports,
            unlisted_dependencies: b.unlisted_dependencies,
            duplicate_exports: b.duplicate_exports,
            circular_dependencies: b.circular_dependencies,
            re_export_cycles: b.re_export_cycles,
            type_only_dependencies: b.type_only_dependencies,
            test_only_dependencies: b.test_only_dependencies,
            boundary_violations: b.boundary_violations,
            boundary_coverage_violations: b.boundary_coverage_violations,
            boundary_call_violations: b.boundary_call_violations,
        }
    }

    /// Convert to config-embeddable baseline.
    #[must_use]
    pub const fn to_config_baseline(&self) -> fallow_config::RegressionBaseline {
        fallow_config::RegressionBaseline {
            total_issues: self.total_issues,
            unused_files: self.unused_files,
            unused_exports: self.unused_exports,
            unused_types: self.unused_types,
            unused_dependencies: self.unused_dependencies,
            unused_dev_dependencies: self.unused_dev_dependencies,
            unused_optional_dependencies: self.unused_optional_dependencies,
            unused_enum_members: self.unused_enum_members,
            unused_class_members: self.unused_class_members,
            unresolved_imports: self.unresolved_imports,
            unlisted_dependencies: self.unlisted_dependencies,
            duplicate_exports: self.duplicate_exports,
            circular_dependencies: self.circular_dependencies,
            re_export_cycles: self.re_export_cycles,
            type_only_dependencies: self.type_only_dependencies,
            test_only_dependencies: self.test_only_dependencies,
            boundary_violations: self.boundary_violations,
            boundary_coverage_violations: self.boundary_coverage_violations,
            boundary_call_violations: self.boundary_call_violations,
        }
    }

    /// Per-type deltas (current - baseline) for display. Only includes types with changes.
    pub fn deltas(&self, current: &Self) -> Vec<(&'static str, isize)> {
        let pairs: Vec<(&str, usize, usize)> = vec![
            ("unused_files", self.unused_files, current.unused_files),
            (
                "unused_exports",
                self.unused_exports,
                current.unused_exports,
            ),
            ("unused_types", self.unused_types, current.unused_types),
            (
                "unused_dependencies",
                self.unused_dependencies,
                current.unused_dependencies,
            ),
            (
                "unused_dev_dependencies",
                self.unused_dev_dependencies,
                current.unused_dev_dependencies,
            ),
            (
                "unused_optional_dependencies",
                self.unused_optional_dependencies,
                current.unused_optional_dependencies,
            ),
            (
                "unused_enum_members",
                self.unused_enum_members,
                current.unused_enum_members,
            ),
            (
                "unused_class_members",
                self.unused_class_members,
                current.unused_class_members,
            ),
            (
                "unresolved_imports",
                self.unresolved_imports,
                current.unresolved_imports,
            ),
            (
                "unlisted_dependencies",
                self.unlisted_dependencies,
                current.unlisted_dependencies,
            ),
            (
                "duplicate_exports",
                self.duplicate_exports,
                current.duplicate_exports,
            ),
            (
                "circular_dependencies",
                self.circular_dependencies,
                current.circular_dependencies,
            ),
            (
                "re_export_cycles",
                self.re_export_cycles,
                current.re_export_cycles,
            ),
            (
                "type_only_dependencies",
                self.type_only_dependencies,
                current.type_only_dependencies,
            ),
            (
                "test_only_dependencies",
                self.test_only_dependencies,
                current.test_only_dependencies,
            ),
            (
                "boundary_violations",
                self.boundary_violations,
                current.boundary_violations,
            ),
            (
                "boundary_coverage_violations",
                self.boundary_coverage_violations,
                current.boundary_coverage_violations,
            ),
            (
                "boundary_call_violations",
                self.boundary_call_violations,
                current.boundary_call_violations,
            ),
        ];
        pairs
            .into_iter()
            .filter_map(|(name, baseline, current)| {
                let delta = current as isize - baseline as isize;
                if delta != 0 {
                    Some((name, delta))
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Duplication counts for regression baseline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DupesCounts {
    #[serde(default)]
    pub clone_groups: usize,
    #[serde(default)]
    pub duplication_percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::results::*;
    use std::path::PathBuf;

    #[test]
    fn check_counts_from_results() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("b.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let counts = CheckCounts::from_results(&results);
        assert_eq!(counts.total_issues, 2);
        assert_eq!(counts.unused_files, 1);
        assert_eq!(counts.unused_exports, 1);
        assert_eq!(counts.unused_types, 0);
    }

    #[test]
    fn deltas_reports_changes_only() {
        let baseline = CheckCounts {
            total_issues: 10,
            unused_files: 5,
            unused_exports: 3,
            unused_types: 2,
            unused_dependencies: 0,
            unused_dev_dependencies: 0,
            unused_optional_dependencies: 0,
            unused_enum_members: 0,
            unused_class_members: 0,
            unresolved_imports: 0,
            unlisted_dependencies: 0,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let current = CheckCounts {
            unused_files: 7,   // +2
            unused_exports: 1, // -2
            unused_types: 2,   // 0 (no change)
            ..baseline
        };
        let deltas = baseline.deltas(&current);
        assert_eq!(deltas.len(), 2);
        assert!(deltas.contains(&("unused_files", 2)));
        assert!(deltas.contains(&("unused_exports", -2)));
    }

    #[test]
    fn regression_baseline_roundtrip() {
        let baseline = RegressionBaseline {
            schema_version: 1,
            fallow_version: "2.4.0".into(),
            timestamp: "2026-03-27T10:00:00Z".into(),
            git_sha: Some("abc123".into()),
            check: Some(CheckCounts {
                total_issues: 42,
                unused_files: 5,
                unused_exports: 20,
                unused_types: 8,
                unused_dependencies: 3,
                unused_dev_dependencies: 2,
                unused_optional_dependencies: 0,
                unused_enum_members: 1,
                unused_class_members: 1,
                unresolved_imports: 0,
                unlisted_dependencies: 1,
                duplicate_exports: 0,
                circular_dependencies: 1,
                re_export_cycles: 0,
                type_only_dependencies: 0,
                test_only_dependencies: 0,
                boundary_violations: 0,
                boundary_coverage_violations: 0,
                boundary_call_violations: 0,
            }),
            dupes: Some(DupesCounts {
                clone_groups: 12,
                duplication_percentage: 4.2,
            }),
        };
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let loaded: RegressionBaseline = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.check.as_ref().unwrap().total_issues, 42);
        assert_eq!(loaded.dupes.as_ref().unwrap().clone_groups, 12);
    }

    #[test]
    fn check_counts_config_roundtrip() {
        let counts = CheckCounts {
            total_issues: 42,
            unused_files: 5,
            unused_exports: 20,
            unused_types: 8,
            unused_dependencies: 3,
            unused_dev_dependencies: 2,
            unused_optional_dependencies: 1,
            unused_enum_members: 1,
            unused_class_members: 1,
            unresolved_imports: 0,
            unlisted_dependencies: 1,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let config_baseline = counts.to_config_baseline();
        let roundtripped = CheckCounts::from_config_baseline(&config_baseline);
        assert_eq!(roundtripped.total_issues, 42);
        assert_eq!(roundtripped.unused_files, 5);
        assert_eq!(roundtripped.unused_exports, 20);
        assert_eq!(roundtripped.unused_types, 8);
        assert_eq!(roundtripped.unused_dependencies, 3);
        assert_eq!(roundtripped.unused_dev_dependencies, 2);
        assert_eq!(roundtripped.unused_optional_dependencies, 1);
        assert_eq!(roundtripped.unused_enum_members, 1);
        assert_eq!(roundtripped.unused_class_members, 1);
        assert_eq!(roundtripped.unresolved_imports, 0);
        assert_eq!(roundtripped.unlisted_dependencies, 1);
        assert_eq!(roundtripped.duplicate_exports, 0);
        assert_eq!(roundtripped.circular_dependencies, 0);
        assert_eq!(roundtripped.type_only_dependencies, 0);
        assert_eq!(roundtripped.test_only_dependencies, 0);
    }

    #[test]
    fn check_counts_zero_config_roundtrip() {
        let counts = CheckCounts {
            total_issues: 0,
            unused_files: 0,
            unused_exports: 0,
            unused_types: 0,
            unused_dependencies: 0,
            unused_dev_dependencies: 0,
            unused_optional_dependencies: 0,
            unused_enum_members: 0,
            unused_class_members: 0,
            unresolved_imports: 0,
            unlisted_dependencies: 0,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let config_baseline = counts.to_config_baseline();
        let roundtripped = CheckCounts::from_config_baseline(&config_baseline);
        assert_eq!(roundtripped.total_issues, 0);
        assert_eq!(roundtripped.unused_files, 0);
    }

    #[test]
    fn deltas_empty_when_identical() {
        let counts = CheckCounts {
            total_issues: 10,
            unused_files: 5,
            unused_exports: 3,
            unused_types: 2,
            unused_dependencies: 0,
            unused_dev_dependencies: 0,
            unused_optional_dependencies: 0,
            unused_enum_members: 0,
            unused_class_members: 0,
            unresolved_imports: 0,
            unlisted_dependencies: 0,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let deltas = counts.deltas(&counts);
        assert!(deltas.is_empty());
    }

    #[test]
    fn deltas_all_categories_changed() {
        let baseline = CheckCounts {
            total_issues: 0,
            unused_files: 0,
            unused_exports: 0,
            unused_types: 0,
            unused_dependencies: 0,
            unused_dev_dependencies: 0,
            unused_optional_dependencies: 0,
            unused_enum_members: 0,
            unused_class_members: 0,
            unresolved_imports: 0,
            unlisted_dependencies: 0,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let current = CheckCounts {
            total_issues: 14,
            unused_files: 1,
            unused_exports: 1,
            unused_types: 1,
            unused_dependencies: 1,
            unused_dev_dependencies: 1,
            unused_optional_dependencies: 1,
            unused_enum_members: 1,
            unused_class_members: 1,
            unresolved_imports: 1,
            unlisted_dependencies: 1,
            duplicate_exports: 1,
            circular_dependencies: 1,
            re_export_cycles: 0,
            type_only_dependencies: 1,
            test_only_dependencies: 1,
            boundary_violations: 1,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let deltas = baseline.deltas(&current);
        assert_eq!(deltas.len(), 15);
        for (_, d) in &deltas {
            assert_eq!(*d, 1);
        }
    }

    #[test]
    fn deltas_mixed_increase_decrease() {
        let baseline = CheckCounts {
            total_issues: 10,
            unused_files: 5,
            unused_exports: 3,
            unused_types: 2,
            unused_dependencies: 0,
            unused_dev_dependencies: 0,
            unused_optional_dependencies: 0,
            unused_enum_members: 0,
            unused_class_members: 0,
            unresolved_imports: 0,
            unlisted_dependencies: 0,
            duplicate_exports: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
        };
        let current = CheckCounts {
            unused_files: 3,
            unused_exports: 5,
            unused_types: 0,
            unresolved_imports: 1,
            ..baseline
        };
        let deltas = baseline.deltas(&current);
        assert_eq!(deltas.len(), 4);
        assert!(deltas.contains(&("unused_files", -2)));
        assert!(deltas.contains(&("unused_exports", 2)));
        assert!(deltas.contains(&("unused_types", -2)));
        assert!(deltas.contains(&("unresolved_imports", 1)));
    }

    #[test]
    fn dupes_counts_roundtrip() {
        let dupes = DupesCounts {
            clone_groups: 8,
            duplication_percentage: 3.17,
        };
        let json = serde_json::to_string(&dupes).unwrap();
        let loaded: DupesCounts = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.clone_groups, 8);
        assert!((loaded.duplication_percentage - 3.17).abs() < f64::EPSILON);
    }

    #[test]
    fn dupes_counts_default_fields() {
        let json = "{}";
        let loaded: DupesCounts = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.clone_groups, 0);
        assert!((loaded.duplication_percentage).abs() < f64::EPSILON);
    }

    #[test]
    fn baseline_without_check_section() {
        let baseline = RegressionBaseline {
            schema_version: 1,
            fallow_version: "2.4.0".into(),
            timestamp: "2026-03-27T10:00:00Z".into(),
            git_sha: None,
            check: None,
            dupes: Some(DupesCounts {
                clone_groups: 3,
                duplication_percentage: 1.0,
            }),
        };
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let loaded: RegressionBaseline = serde_json::from_str(&json).unwrap();
        assert!(loaded.check.is_none());
        assert!(loaded.dupes.is_some());
    }

    #[test]
    fn baseline_without_dupes_section() {
        let baseline = RegressionBaseline {
            schema_version: 1,
            fallow_version: "2.4.0".into(),
            timestamp: "2026-03-27T10:00:00Z".into(),
            git_sha: Some("deadbeef".into()),
            check: Some(CheckCounts {
                total_issues: 1,
                unused_files: 1,
                ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
            }),
            dupes: None,
        };
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        let loaded: RegressionBaseline = serde_json::from_str(&json).unwrap();
        assert!(loaded.check.is_some());
        assert!(loaded.dupes.is_none());
        assert_eq!(loaded.git_sha.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn baseline_without_git_sha() {
        let baseline = RegressionBaseline {
            schema_version: 1,
            fallow_version: "2.4.0".into(),
            timestamp: "2026-03-27T10:00:00Z".into(),
            git_sha: None,
            check: None,
            dupes: None,
        };
        let json = serde_json::to_string_pretty(&baseline).unwrap();
        assert!(!json.contains("git_sha"));
        let loaded: RegressionBaseline = serde_json::from_str(&json).unwrap();
        assert!(loaded.git_sha.is_none());
    }

    #[test]
    fn baseline_json_with_unknown_check_fields_deserializes() {
        let json = r#"{
            "schema_version": 1,
            "fallow_version": "3.0.0",
            "timestamp": "2026-03-27T10:00:00Z",
            "check": {
                "total_issues": 10,
                "unused_files": 2,
                "some_future_field": 99
            }
        }"#;
        let loaded: Result<RegressionBaseline, _> = serde_json::from_str(json);
        assert!(loaded.is_ok());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.check.as_ref().unwrap().total_issues, 10);
    }
}
