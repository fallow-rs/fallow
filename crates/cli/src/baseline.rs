use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::path::Path;

use fallow_core::duplicates::DuplicationReport;

/// Strip the project root from a path to produce a portable relative key.
///
/// Both `path` and `root` must be in the same form (both canonicalized or both
/// not) for `strip_prefix` to succeed. The analysis pipeline keeps all paths
/// non-canonicalized, so this invariant holds in practice.
fn relative_path(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => {
            tracing::debug!(
                path = %path.display(),
                root = %root.display(),
                "baseline key: path is not under project root, using absolute path as key"
            );
            path.to_string_lossy().replace('\\', "/")
        }
    }
}

fn package_json_dependency_key(package_name: &str, path: &Path, root: &Path) -> String {
    format!("{}:{package_name}", relative_path(path, root))
}

fn baseline_contains_dependency(
    baseline_keys: &FxHashSet<&str>,
    package_name: &str,
    path_key: &str,
) -> bool {
    baseline_keys.contains(path_key) || baseline_keys.contains(package_name)
}

/// Baseline data for comparison.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BaselineData {
    pub unused_files: Vec<String>,
    pub unused_exports: Vec<String>,
    pub unused_types: Vec<String>,
    #[serde(default)]
    pub private_type_leaks: Vec<String>,
    /// Unused dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    pub unused_dependencies: Vec<String>,
    /// Unused dev dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    pub unused_dev_dependencies: Vec<String>,
    /// Circular dependency chains, keyed by sorted file paths joined with `->`.
    #[serde(default)]
    pub circular_dependencies: Vec<String>,
    /// Unused optional dependencies, keyed by `package.json:package_name`.
    /// Legacy bare `package_name` keys are still matched for back-compat
    /// with baselines saved by older fallow versions.
    #[serde(default)]
    pub unused_optional_dependencies: Vec<String>,
    /// Unused enum members, keyed by `file:parent.member`.
    #[serde(default)]
    pub unused_enum_members: Vec<String>,
    /// Unused class members, keyed by `file:parent.member`.
    #[serde(default)]
    pub unused_class_members: Vec<String>,
    /// Unresolved imports, keyed by `file:specifier`.
    #[serde(default)]
    pub unresolved_imports: Vec<String>,
    /// Unlisted dependencies, keyed by package name.
    #[serde(default)]
    pub unlisted_dependencies: Vec<String>,
    /// Duplicate exports, keyed by export name.
    #[serde(default)]
    pub duplicate_exports: Vec<String>,
    /// Type-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    pub type_only_dependencies: Vec<String>,
    /// Test-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    pub test_only_dependencies: Vec<String>,
    /// Boundary violations, keyed by `from_path->to_path`.
    #[serde(default)]
    pub boundary_violations: Vec<String>,
    /// Stale suppressions, keyed by `file:line`.
    #[serde(default)]
    pub stale_suppressions: Vec<String>,
    /// Unused pnpm catalog entries, keyed by `catalog_name:entry_name`.
    #[serde(default)]
    pub unused_catalog_entries: Vec<String>,
}

impl BaselineData {
    pub fn from_results(results: &fallow_core::results::AnalysisResults, root: &Path) -> Self {
        Self {
            unused_files: results
                .unused_files
                .iter()
                .map(|f| relative_path(&f.path, root))
                .collect(),
            unused_exports: results
                .unused_exports
                .iter()
                .map(|e| format!("{}:{}", relative_path(&e.path, root), e.export_name))
                .collect(),
            unused_types: results
                .unused_types
                .iter()
                .map(|e| format!("{}:{}", relative_path(&e.path, root), e.export_name))
                .collect(),
            private_type_leaks: results
                .private_type_leaks
                .iter()
                .map(|e| {
                    format!(
                        "{}:{}->{}",
                        relative_path(&e.path, root),
                        e.export_name,
                        e.type_name
                    )
                })
                .collect(),
            unused_dependencies: results
                .unused_dependencies
                .iter()
                .map(|d| package_json_dependency_key(&d.package_name, &d.path, root))
                .collect(),
            unused_dev_dependencies: results
                .unused_dev_dependencies
                .iter()
                .map(|d| package_json_dependency_key(&d.package_name, &d.path, root))
                .collect(),
            circular_dependencies: results
                .circular_dependencies
                .iter()
                .map(|c| circular_dep_key(c, root))
                .collect(),
            unused_optional_dependencies: results
                .unused_optional_dependencies
                .iter()
                .map(|d| package_json_dependency_key(&d.package_name, &d.path, root))
                .collect(),
            unused_enum_members: results
                .unused_enum_members
                .iter()
                .map(|m| {
                    format!(
                        "{}:{}.{}",
                        relative_path(&m.path, root),
                        m.parent_name,
                        m.member_name
                    )
                })
                .collect(),
            unused_class_members: results
                .unused_class_members
                .iter()
                .map(|m| {
                    format!(
                        "{}:{}.{}",
                        relative_path(&m.path, root),
                        m.parent_name,
                        m.member_name
                    )
                })
                .collect(),
            unresolved_imports: results
                .unresolved_imports
                .iter()
                .map(|i| format!("{}:{}", relative_path(&i.path, root), i.specifier))
                .collect(),
            unlisted_dependencies: results
                .unlisted_dependencies
                .iter()
                .map(|d| d.package_name.clone())
                .collect(),
            duplicate_exports: results
                .duplicate_exports
                .iter()
                .map(|d| duplicate_export_key(d, root))
                .collect(),
            type_only_dependencies: results
                .type_only_dependencies
                .iter()
                .map(|d| package_json_dependency_key(&d.package_name, &d.path, root))
                .collect(),
            test_only_dependencies: results
                .test_only_dependencies
                .iter()
                .map(|d| package_json_dependency_key(&d.package_name, &d.path, root))
                .collect(),
            boundary_violations: results
                .boundary_violations
                .iter()
                .map(|v| boundary_violation_key(v, root))
                .collect(),
            stale_suppressions: results
                .stale_suppressions
                .iter()
                .map(|s| format!("{}:{}", relative_path(&s.path, root), s.line))
                .collect(),
            unused_catalog_entries: results
                .unused_catalog_entries
                .iter()
                .map(|e| format!("{}:{}", e.catalog_name, e.entry_name))
                .collect(),
        }
    }

    /// Total number of entries across all categories.
    pub fn total_entries(&self) -> usize {
        self.unused_files.len()
            + self.unused_exports.len()
            + self.unused_types.len()
            + self.private_type_leaks.len()
            + self.unused_dependencies.len()
            + self.unused_dev_dependencies.len()
            + self.circular_dependencies.len()
            + self.unused_optional_dependencies.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
            + self.unresolved_imports.len()
            + self.unlisted_dependencies.len()
            + self.duplicate_exports.len()
            + self.type_only_dependencies.len()
            + self.test_only_dependencies.len()
            + self.boundary_violations.len()
            + self.stale_suppressions.len()
            + self.unused_catalog_entries.len()
    }
}

/// Generate a stable key for a boundary violation: `from_path->to_path`.
fn boundary_violation_key(v: &fallow_core::results::BoundaryViolation, root: &Path) -> String {
    format!(
        "{}->{}",
        relative_path(&v.from_path, root),
        relative_path(&v.to_path, root),
    )
}

/// Generate a stable key for a duplicate export: `name|sorted_paths`.
fn duplicate_export_key(dup: &fallow_core::results::DuplicateExport, root: &Path) -> String {
    let mut locs: Vec<String> = dup
        .locations
        .iter()
        .map(|l| relative_path(&l.path, root))
        .collect();
    locs.sort();
    format!("{}|{}", dup.export_name, locs.join("|"))
}

/// Generate a stable key for a circular dependency based on sorted file paths.
fn circular_dep_key(dep: &fallow_core::results::CircularDependency, root: &Path) -> String {
    let mut paths: Vec<String> = dep.files.iter().map(|f| relative_path(f, root)).collect();
    paths.sort();
    paths.join("->")
}

fn private_type_leak_key(leak: &fallow_core::results::PrivateTypeLeak, root: &Path) -> String {
    format!(
        "{}:{}->{}",
        relative_path(&leak.path, root),
        leak.export_name,
        leak.type_name
    )
}

fn filter_private_type_leaks(
    leaks: &mut Vec<fallow_core::results::PrivateTypeLeak>,
    baseline_keys: &[String],
    root: &Path,
) {
    let baseline_private_type_leaks: FxHashSet<&str> =
        baseline_keys.iter().map(String::as_str).collect();
    leaks.retain(|leak| {
        let key = private_type_leak_key(leak, root);
        !baseline_private_type_leaks.contains(key.as_str())
    });
}

/// Filter results to only include issues not present in the baseline.
#[expect(
    clippy::too_many_lines,
    reason = "flat list of per-issue-type retain calls; one block per category keeps each filter local and easy to audit"
)]
pub fn filter_new_issues(
    mut results: fallow_core::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> fallow_core::results::AnalysisResults {
    let baseline_files: FxHashSet<&str> =
        baseline.unused_files.iter().map(String::as_str).collect();
    let baseline_exports: FxHashSet<&str> =
        baseline.unused_exports.iter().map(String::as_str).collect();
    let baseline_types: FxHashSet<&str> =
        baseline.unused_types.iter().map(String::as_str).collect();
    let baseline_deps: FxHashSet<&str> = baseline
        .unused_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_dev_deps: FxHashSet<&str> = baseline
        .unused_dev_dependencies
        .iter()
        .map(String::as_str)
        .collect();

    results
        .unused_files
        .retain(|f| !baseline_files.contains(relative_path(&f.path, root).as_str()));
    results.unused_exports.retain(|e| {
        let key = format!("{}:{}", relative_path(&e.path, root), e.export_name);
        !baseline_exports.contains(key.as_str())
    });
    results.unused_types.retain(|e| {
        let key = format!("{}:{}", relative_path(&e.path, root), e.export_name);
        !baseline_types.contains(key.as_str())
    });
    filter_private_type_leaks(
        &mut results.private_type_leaks,
        &baseline.private_type_leaks,
        root,
    );
    results.unused_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.package_name, &d.path, root);
        !baseline_contains_dependency(&baseline_deps, &d.package_name, key.as_str())
    });
    results.unused_dev_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.package_name, &d.path, root);
        !baseline_contains_dependency(&baseline_dev_deps, &d.package_name, key.as_str())
    });

    let baseline_circular: FxHashSet<&str> = baseline
        .circular_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    results.circular_dependencies.retain(|c| {
        let key = circular_dep_key(c, root);
        !baseline_circular.contains(key.as_str())
    });

    let baseline_optional_deps: FxHashSet<&str> = baseline
        .unused_optional_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    results.unused_optional_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.package_name, &d.path, root);
        !baseline_contains_dependency(&baseline_optional_deps, &d.package_name, key.as_str())
    });

    let baseline_enum_members: FxHashSet<&str> = baseline
        .unused_enum_members
        .iter()
        .map(String::as_str)
        .collect();
    results.unused_enum_members.retain(|m| {
        let key = format!(
            "{}:{}.{}",
            relative_path(&m.path, root),
            m.parent_name,
            m.member_name
        );
        !baseline_enum_members.contains(key.as_str())
    });

    let baseline_class_members: FxHashSet<&str> = baseline
        .unused_class_members
        .iter()
        .map(String::as_str)
        .collect();
    results.unused_class_members.retain(|m| {
        let key = format!(
            "{}:{}.{}",
            relative_path(&m.path, root),
            m.parent_name,
            m.member_name
        );
        !baseline_class_members.contains(key.as_str())
    });

    let baseline_unresolved: FxHashSet<&str> = baseline
        .unresolved_imports
        .iter()
        .map(String::as_str)
        .collect();
    results.unresolved_imports.retain(|i| {
        let key = format!("{}:{}", relative_path(&i.path, root), i.specifier);
        !baseline_unresolved.contains(key.as_str())
    });

    let baseline_unlisted: FxHashSet<&str> = baseline
        .unlisted_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    results
        .unlisted_dependencies
        .retain(|d| !baseline_unlisted.contains(d.package_name.as_str()));

    let baseline_dup_exports: FxHashSet<&str> = baseline
        .duplicate_exports
        .iter()
        .map(String::as_str)
        .collect();
    results.duplicate_exports.retain(|d| {
        let key = duplicate_export_key(d, root);
        !baseline_dup_exports.contains(key.as_str())
    });

    let baseline_type_only: FxHashSet<&str> = baseline
        .type_only_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    results.type_only_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.package_name, &d.path, root);
        !baseline_contains_dependency(&baseline_type_only, &d.package_name, key.as_str())
    });

    let baseline_test_only: FxHashSet<&str> = baseline
        .test_only_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    results.test_only_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.package_name, &d.path, root);
        !baseline_contains_dependency(&baseline_test_only, &d.package_name, key.as_str())
    });

    let baseline_boundary: FxHashSet<&str> = baseline
        .boundary_violations
        .iter()
        .map(String::as_str)
        .collect();
    results.boundary_violations.retain(|v| {
        let key = boundary_violation_key(v, root);
        !baseline_boundary.contains(key.as_str())
    });

    let baseline_stale: FxHashSet<&str> = baseline
        .stale_suppressions
        .iter()
        .map(String::as_str)
        .collect();
    results.stale_suppressions.retain(|s| {
        let key = format!("{}:{}", relative_path(&s.path, root), s.line);
        !baseline_stale.contains(key.as_str())
    });

    let baseline_catalog: FxHashSet<&str> = baseline
        .unused_catalog_entries
        .iter()
        .map(String::as_str)
        .collect();
    results.unused_catalog_entries.retain(|e| {
        let key = format!("{}:{}", e.catalog_name, e.entry_name);
        !baseline_catalog.contains(key.as_str())
    });

    results
}

/// Baseline data for duplication comparison.
///
/// Each clone group is keyed by a canonical string derived from its sorted
/// (`file:start_line-end_line`) instance locations. This allows stable comparison
/// across runs even if group ordering changes.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DuplicationBaselineData {
    /// Clone group keys: sorted list of `file:start-end` per group.
    pub clone_groups: Vec<String>,
}

impl DuplicationBaselineData {
    /// Build a duplication baseline from the current report.
    pub fn from_report(report: &DuplicationReport, root: &Path) -> Self {
        Self {
            clone_groups: report
                .clone_groups
                .iter()
                .map(|g| clone_group_key(g, root))
                .collect(),
        }
    }
}

/// Generate a stable key for a clone group based on its instance locations.
fn clone_group_key(group: &fallow_core::duplicates::CloneGroup, root: &Path) -> String {
    let mut parts: Vec<String> = group
        .instances
        .iter()
        .map(|i| {
            format!(
                "{}:{}-{}",
                relative_path(&i.file, root),
                i.start_line,
                i.end_line
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

/// Filter a duplication report to only include clone groups not present in the baseline.
pub fn filter_new_clone_groups(
    mut report: DuplicationReport,
    baseline: &DuplicationBaselineData,
    root: &Path,
) -> DuplicationReport {
    let baseline_keys: FxHashSet<&str> = baseline.clone_groups.iter().map(String::as_str).collect();

    report.clone_groups.retain(|g| {
        let key = clone_group_key(g, root);
        !baseline_keys.contains(key.as_str())
    });

    // Re-generate families from the filtered groups
    report.clone_families =
        fallow_core::duplicates::families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories = fallow_core::duplicates::families::detect_mirrored_directories(
        &report.clone_families,
        root,
    );

    // Re-compute stats for the filtered groups
    report.stats = recompute_stats(&report);

    report
}

/// Recompute duplication statistics after filtering (baseline or `--changed-since`).
///
/// Uses per-file line deduplication (matching `compute_stats` in `detect.rs`)
/// so overlapping clone instances don't inflate the duplicated line count.
pub fn recompute_stats(report: &DuplicationReport) -> fallow_core::duplicates::DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut file_dup_lines: FxHashMap<&Path, FxHashSet<usize>> = FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in &report.clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        duplicated_tokens += group.token_count * group.instances.len();
    }

    let duplicated_lines: usize = file_dup_lines.values().map(FxHashSet::len).sum();

    fallow_core::duplicates::DuplicationStats {
        total_files: report.stats.total_files,
        files_with_clones: files_with_clones.len(),
        total_lines: report.stats.total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens,
        clone_groups: report.clone_groups.len(),
        clone_instances,
        duplication_percentage: if report.stats.total_lines > 0 {
            (duplicated_lines as f64 / report.stats.total_lines as f64) * 100.0
        } else {
            0.0
        },
    }
}

// ── Health baseline ─────────────────────────────────────────────────

/// Baseline data for health (complexity) comparison.
///
/// New baselines store count-per-category-per-file data in `finding_counts` so
/// line shifts do not leak pre-existing findings. Legacy baselines with
/// `findings: ["path:name:line"]` still load so users can refresh them in
/// place with `--save-baseline`.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineData {
    /// Legacy health baseline keys: `relative_path:function_name:line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
    /// Count-per-category-per-file baseline buckets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub finding_counts: HealthFindingCountMap,
    /// Stable runtime-coverage finding IDs from the sidecar.
    #[serde(default)]
    pub runtime_coverage_findings: Vec<String>,
    /// Refactoring target keys: `relative_path:category`.
    #[serde(default)]
    pub target_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineCount {
    pub count: usize,
}

type HealthFindingCountMap = BTreeMap<String, BTreeMap<String, HealthBaselineCount>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthFindingDimension {
    Complexity,
    Crap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HealthFindingCategory {
    dimension: HealthFindingDimension,
    severity: crate::health_types::FindingSeverity,
}

impl HealthFindingCategory {
    const fn key(self) -> &'static str {
        match (self.dimension, self.severity) {
            (
                HealthFindingDimension::Complexity,
                crate::health_types::FindingSeverity::Moderate,
            ) => "complexity_moderate",
            (HealthFindingDimension::Complexity, crate::health_types::FindingSeverity::High) => {
                "complexity_high"
            }
            (
                HealthFindingDimension::Complexity,
                crate::health_types::FindingSeverity::Critical,
            ) => "complexity_critical",
            (HealthFindingDimension::Crap, crate::health_types::FindingSeverity::Moderate) => {
                "crap_moderate"
            }
            (HealthFindingDimension::Crap, crate::health_types::FindingSeverity::High) => {
                "crap_high"
            }
            (HealthFindingDimension::Crap, crate::health_types::FindingSeverity::Critical) => {
                "crap_critical"
            }
        }
    }
}

const HEALTH_FINDING_DIMENSIONS: [HealthFindingDimension; 2] = [
    HealthFindingDimension::Complexity,
    HealthFindingDimension::Crap,
];

impl HealthBaselineData {
    /// Build a health baseline from findings and targets.
    pub fn from_findings(
        findings: &[crate::health_types::HealthFinding],
        runtime_coverage_findings: &[crate::health_types::RuntimeCoverageFinding],
        targets: &[crate::health_types::RefactoringTarget],
        root: &Path,
    ) -> Self {
        Self {
            findings: Vec::new(),
            finding_counts: health_finding_counts(findings, root),
            runtime_coverage_findings: runtime_coverage_findings
                .iter()
                .map(|f| runtime_coverage_finding_key(f, root))
                .collect(),
            target_keys: targets
                .iter()
                .map(|t| target_baseline_key(t, root))
                .collect(),
        }
    }

    pub fn finding_entry_count(&self) -> usize {
        if !self.finding_counts.is_empty() {
            self.finding_counts
                .values()
                .flat_map(BTreeMap::values)
                .map(|entry| entry.count)
                .sum()
        } else {
            self.findings.len()
        }
    }

    pub fn overlap_entry_count(
        &self,
        findings: &[crate::health_types::HealthFinding],
        root: &Path,
    ) -> usize {
        if !self.finding_counts.is_empty() {
            let current_counts = health_finding_counts(findings, root);
            health_overlap_entry_count(&current_counts, &self.finding_counts)
        } else {
            let baseline_keys: FxHashSet<&str> = self.findings.iter().map(String::as_str).collect();
            findings
                .iter()
                .filter(|finding| {
                    baseline_keys.contains(health_finding_key(finding, root).as_str())
                })
                .count()
        }
    }
}

/// Generate a stable key for a refactoring target: `relative_path:category`.
fn target_baseline_key(target: &crate::health_types::RefactoringTarget, root: &Path) -> String {
    format!(
        "{}:{}",
        relative_path(&target.path, root),
        target.category.label()
    )
}

/// Generate a stable key for a health finding.
fn health_finding_key(finding: &crate::health_types::HealthFinding, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        relative_path(&finding.path, root),
        finding.name,
        finding.line
    )
}

fn health_finding_counts(
    findings: &[crate::health_types::HealthFinding],
    root: &Path,
) -> HealthFindingCountMap {
    let mut counts = BTreeMap::new();
    for finding in findings {
        let path = relative_path(&finding.path, root);
        let file_counts = counts.entry(path).or_insert_with(BTreeMap::new);
        for category in health_finding_categories(finding).into_iter().flatten() {
            file_counts
                .entry(category.key().to_string())
                .and_modify(|entry: &mut HealthBaselineCount| entry.count += 1)
                .or_insert(HealthBaselineCount { count: 1 });
        }
    }
    counts
}

fn health_finding_categories(
    finding: &crate::health_types::HealthFinding,
) -> [Option<HealthFindingCategory>; 2] {
    let complexity_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Complexity,
        severity: finding.severity,
    };
    let crap_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Crap,
        severity: finding.severity,
    };
    let has_complexity =
        finding.exceeded.includes_cyclomatic() || finding.exceeded.includes_cognitive();
    let has_crap = finding.exceeded.includes_crap();
    [
        has_complexity.then_some(complexity_category),
        has_crap.then_some(crap_category),
    ]
}

fn severity_index(severity: crate::health_types::FindingSeverity) -> usize {
    match severity {
        crate::health_types::FindingSeverity::Moderate => 0,
        crate::health_types::FindingSeverity::High => 1,
        crate::health_types::FindingSeverity::Critical => 2,
    }
}

fn severity_counts_for_dimension(
    file_counts: Option<&BTreeMap<String, HealthBaselineCount>>,
    dimension: HealthFindingDimension,
) -> [usize; 3] {
    let mut counts = [0; 3];
    for severity in [
        crate::health_types::FindingSeverity::Moderate,
        crate::health_types::FindingSeverity::High,
        crate::health_types::FindingSeverity::Critical,
    ] {
        let category = HealthFindingCategory {
            dimension,
            severity,
        };
        counts[severity_index(severity)] = file_counts
            .and_then(|entries| entries.get(category.key()))
            .map_or(0, |entry| entry.count);
    }
    counts
}

fn overflowing_severities(current: [usize; 3], baseline: [usize; 3]) -> [bool; 3] {
    let mut available = baseline;
    let mut overflow = [false; 3];

    // Match lower severities first with the least-flexible compatible baseline
    // slots so ambiguous cases still leave worse current severities visible.
    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];

        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }

    overflow
}

fn health_overflow_categories(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> FxHashMap<String, FxHashSet<&'static str>> {
    let mut overflow_by_path = FxHashMap::default();

    for (path, current_file_counts) in current_counts {
        let mut overflow_categories: FxHashSet<&'static str> = FxHashSet::default();
        let baseline_file_counts = baseline_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current = severity_counts_for_dimension(Some(current_file_counts), dimension);
            let baseline = severity_counts_for_dimension(baseline_file_counts, dimension);
            let overflow = overflowing_severities(current, baseline);

            for severity in [
                crate::health_types::FindingSeverity::Moderate,
                crate::health_types::FindingSeverity::High,
                crate::health_types::FindingSeverity::Critical,
            ] {
                if overflow[severity_index(severity)] {
                    overflow_categories.insert(
                        HealthFindingCategory {
                            dimension,
                            severity,
                        }
                        .key(),
                    );
                }
            }
        }

        if !overflow_categories.is_empty() {
            overflow_by_path.insert(path.clone(), overflow_categories);
        }
    }

    overflow_by_path
}

fn health_overlap_entry_count(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> usize {
    let mut overlap = 0;

    for (path, baseline_file_counts) in baseline_counts {
        let current_file_counts = current_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current_total: usize =
                severity_counts_for_dimension(current_file_counts, dimension)
                    .into_iter()
                    .sum();
            let baseline_total: usize =
                severity_counts_for_dimension(Some(baseline_file_counts), dimension)
                    .into_iter()
                    .sum();
            overlap += current_total.min(baseline_total);
        }
    }

    overlap
}

fn runtime_coverage_finding_key(
    finding: &crate::health_types::RuntimeCoverageFinding,
    _root: &Path,
) -> String {
    // Use the stable content-hash ID from the sidecar (e.g.
    // `fallow:prod:a7f3b2c1`). Line and path changes produce a new ID —
    // that's the correct behaviour for baseline dedup: a moved function
    // should appear as a fresh finding until re-baselined.
    finding.id.clone()
}

/// Filter health findings to only include those not present in the baseline.
pub fn filter_new_health_findings(
    mut findings: Vec<crate::health_types::HealthFinding>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<crate::health_types::HealthFinding> {
    if !baseline.finding_counts.is_empty() {
        let current_counts = health_finding_counts(&findings, root);
        let overflow_categories =
            health_overflow_categories(&current_counts, &baseline.finding_counts);
        findings.retain(|finding| {
            let path = relative_path(&finding.path, root);
            overflow_categories.get(&path).is_some_and(|categories| {
                health_finding_categories(finding)
                    .into_iter()
                    .flatten()
                    .any(|category| categories.contains(category.key()))
            })
        });
        return findings;
    }

    let baseline_keys: FxHashSet<&str> = baseline.findings.iter().map(String::as_str).collect();
    findings.retain(|f| {
        let key = health_finding_key(f, root);
        !baseline_keys.contains(key.as_str())
    });
    findings
}

pub fn filter_new_runtime_coverage_findings(
    mut findings: Vec<crate::health_types::RuntimeCoverageFinding>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<crate::health_types::RuntimeCoverageFinding> {
    let baseline_keys: FxHashSet<&str> = baseline
        .runtime_coverage_findings
        .iter()
        .map(String::as_str)
        .collect();
    findings.retain(|finding| {
        let key = runtime_coverage_finding_key(finding, root);
        !baseline_keys.contains(key.as_str())
    });
    findings
}

/// Filter refactoring targets to only include those not present in the baseline.
pub fn filter_new_health_targets(
    mut targets: Vec<crate::health_types::RefactoringTarget>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<crate::health_types::RefactoringTarget> {
    let baseline_keys: FxHashSet<&str> = baseline.target_keys.iter().map(String::as_str).collect();
    targets.retain(|t| {
        let key = target_baseline_key(t, root);
        !baseline_keys.contains(key.as_str())
    });
    targets
}

/// Per-category delta between current results and a baseline.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryDelta {
    pub current: usize,
    pub baseline: usize,
    pub delta: i64,
}

/// Deltas between current analysis results and a saved baseline.
///
/// Used in combined mode to show +/- counts in the failure summary and
/// to emit `baseline_deltas` in JSON output.
#[derive(Debug, Clone)]
pub struct BaselineDeltas {
    /// Net change in total issue count (positive = more issues).
    pub total_delta: i64,
    /// Per-category deltas keyed by category name.
    pub per_category: Vec<(String, CategoryDelta)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
    use fallow_core::results::{
        AnalysisResults, DependencyLocation, UnusedDependency, UnusedExport, UnusedFile,
    };
    use std::path::PathBuf;

    fn make_results() -> AnalysisResults {
        AnalysisResults {
            unused_files: vec![
                UnusedFile {
                    path: PathBuf::from("src/old.ts"),
                },
                UnusedFile {
                    path: PathBuf::from("src/dead.ts"),
                },
            ],
            unused_exports: vec![UnusedExport {
                path: PathBuf::from("src/utils.ts"),
                export_name: "helperA".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
            }],
            unused_types: vec![UnusedExport {
                path: PathBuf::from("src/types.ts"),
                export_name: "OldType".to_string(),
                is_type_only: true,
                line: 10,
                col: 0,
                span_start: 100,
                is_re_export: false,
            }],
            unused_dependencies: vec![UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }],
            unused_dev_dependencies: vec![UnusedDependency {
                package_name: "jest".to_string(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }],
            ..Default::default()
        }
    }

    // ── BaselineData round-trip ──────────────────────────────────

    #[test]
    fn baseline_from_results_captures_all_fields() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        assert_eq!(baseline.unused_files.len(), 2);
        assert!(baseline.unused_files.contains(&"src/old.ts".to_string()));
        assert!(baseline.unused_files.contains(&"src/dead.ts".to_string()));
        assert_eq!(baseline.unused_exports, vec!["src/utils.ts:helperA"]);
        assert_eq!(baseline.unused_types, vec!["src/types.ts:OldType"]);
        assert_eq!(baseline.unused_dependencies, vec!["package.json:lodash"]);
        assert_eq!(baseline.unused_dev_dependencies, vec!["package.json:jest"]);
    }

    #[test]
    fn dependency_baseline_keys_include_package_json_path() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![
                UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-a/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
                UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-b/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ],
            ..Default::default()
        };

        let baseline = BaselineData::from_results(&results, root);

        assert_eq!(
            baseline.unused_dependencies,
            vec![
                "packages/app-a/package.json:lodash-es",
                "packages/app-b/package.json:lodash-es"
            ]
        );
    }

    #[test]
    fn dependency_baseline_filter_matches_path_before_package_name() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![
                UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-a/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
                UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-b/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ],
            ..Default::default()
        };
        let baseline = BaselineData {
            unused_dependencies: vec!["packages/app-a/package.json:lodash-es".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), root)
        };

        let filtered = filter_new_issues(results, &baseline, root);

        assert_eq!(filtered.unused_dependencies.len(), 1);
        assert_eq!(
            filtered.unused_dependencies[0].path,
            PathBuf::from("/repo/packages/app-b/package.json")
        );
    }

    #[test]
    fn dependency_baseline_filter_supports_legacy_package_only_keys() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }],
            ..Default::default()
        };
        let baseline = BaselineData {
            unused_dependencies: vec!["lodash-es".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), root)
        };

        let filtered = filter_new_issues(results, &baseline, root);

        assert!(filtered.unused_dependencies.is_empty());
    }

    #[test]
    fn baseline_serialization_roundtrip() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: BaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.unused_files, baseline.unused_files);
        assert_eq!(deserialized.unused_exports, baseline.unused_exports);
        assert_eq!(deserialized.unused_types, baseline.unused_types);
        assert_eq!(
            deserialized.unused_dependencies,
            baseline.unused_dependencies
        );
        assert_eq!(
            deserialized.unused_dev_dependencies,
            baseline.unused_dev_dependencies
        );
    }

    // ── filter_new_issues ────────────────────────────────────────

    #[test]
    fn filter_removes_baseline_issues() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert!(
            filtered.unused_files.is_empty(),
            "all files were in baseline"
        );
        assert!(
            filtered.unused_exports.is_empty(),
            "all exports were in baseline"
        );
        assert!(
            filtered.unused_types.is_empty(),
            "all types were in baseline"
        );
        assert!(
            filtered.unused_dependencies.is_empty(),
            "all deps were in baseline"
        );
        assert!(
            filtered.unused_dev_dependencies.is_empty(),
            "all dev deps were in baseline"
        );
    }

    #[test]
    fn filter_keeps_new_issues_not_in_baseline() {
        let baseline = BaselineData {
            unused_files: vec!["src/old.ts".to_string()],
            unused_exports: vec![],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
        };
        let results = AnalysisResults {
            unused_files: vec![
                UnusedFile {
                    path: PathBuf::from("src/old.ts"),
                },
                UnusedFile {
                    path: PathBuf::from("src/new-dead.ts"),
                },
            ],
            ..Default::default()
        };
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_files.len(), 1);
        assert_eq!(
            filtered.unused_files[0].path,
            PathBuf::from("src/new-dead.ts")
        );
    }

    #[test]
    fn filter_with_empty_baseline_keeps_all() {
        let baseline = BaselineData {
            unused_files: vec![],
            unused_exports: vec![],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
        };
        let results = make_results();
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_files.len(), 2);
        assert_eq!(filtered.unused_exports.len(), 1);
    }

    #[test]
    fn filter_new_exports_by_file_and_name() {
        let baseline = BaselineData {
            unused_files: vec![],
            unused_exports: vec!["src/utils.ts:helperA".to_string()],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
        };
        let results = AnalysisResults {
            unused_exports: vec![
                UnusedExport {
                    path: PathBuf::from("src/utils.ts"),
                    export_name: "helperA".to_string(),
                    is_type_only: false,
                    line: 5,
                    col: 0,
                    span_start: 40,
                    is_re_export: false,
                },
                UnusedExport {
                    path: PathBuf::from("src/utils.ts"),
                    export_name: "helperB".to_string(),
                    is_type_only: false,
                    line: 10,
                    col: 0,
                    span_start: 80,
                    is_re_export: false,
                },
            ],
            ..Default::default()
        };
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_exports.len(), 1);
        assert_eq!(filtered.unused_exports[0].export_name, "helperB");
    }

    // ── DuplicationBaselineData ──────────────────────────────────

    fn make_clone_group(instances: Vec<(&str, usize, usize)>) -> CloneGroup {
        CloneGroup {
            instances: instances
                .into_iter()
                .map(|(file, start, end)| CloneInstance {
                    file: PathBuf::from(file),
                    start_line: start,
                    end_line: end,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                })
                .collect(),
            token_count: 50,
            line_count: 10,
        }
    }

    fn make_duplication_report(groups: Vec<CloneGroup>) -> DuplicationReport {
        DuplicationReport {
            clone_groups: groups,
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 10,
                files_with_clones: 2,
                total_lines: 1000,
                duplicated_lines: 100,
                total_tokens: 5000,
                duplicated_tokens: 500,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 10.0,
            },
        }
    }

    #[test]
    fn clone_group_key_is_deterministic() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let key1 = clone_group_key(&group, root);
        let key2 = clone_group_key(&group, root);
        assert_eq!(key1, key2);
    }

    #[test]
    fn clone_group_key_is_sorted() {
        let root = Path::new("/project");
        // Order of instances in group shouldn't matter for the key
        let group_ab = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let group_ba = make_clone_group(vec![
            ("/project/src/b.ts", 5, 15),
            ("/project/src/a.ts", 1, 10),
        ]);
        assert_eq!(
            clone_group_key(&group_ab, root),
            clone_group_key(&group_ba, root),
            "key should be stable regardless of instance order"
        );
    }

    #[test]
    fn duplication_baseline_roundtrip() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: DuplicationBaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.clone_groups, baseline.clone_groups);
    }

    #[test]
    fn filter_new_clone_groups_removes_baseline() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert!(
            filtered.clone_groups.is_empty(),
            "baseline group should be filtered out"
        );
    }

    #[test]
    fn filter_new_clone_groups_keeps_new_groups() {
        let root = Path::new("/project");
        let baseline_group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let new_group = make_clone_group(vec![
            ("/project/src/c.ts", 20, 30),
            ("/project/src/d.ts", 25, 35),
        ]);
        let baseline_report = make_duplication_report(vec![baseline_group]);
        let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

        let report = make_duplication_report(vec![
            make_clone_group(vec![
                ("/project/src/a.ts", 1, 10),
                ("/project/src/b.ts", 5, 15),
            ]),
            new_group,
        ]);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert_eq!(
            filtered.clone_groups.len(),
            1,
            "only the new group should remain"
        );
    }

    #[test]
    fn recompute_stats_after_filtering() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert_eq!(filtered.stats.clone_groups, 0);
        assert_eq!(filtered.stats.clone_instances, 0);
        assert_eq!(filtered.stats.duplicated_lines, 0);
    }

    #[test]
    fn recompute_stats_zero_total_lines() {
        let report = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 0,
                files_with_clones: 0,
                total_lines: 0,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
            },
        };
        let stats = super::recompute_stats(&report);
        assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }

    // ── HealthBaselineData ──────────────────────────────────────────

    fn make_health_finding(
        root: &Path,
        name: &str,
        line: u32,
    ) -> crate::health_types::HealthFinding {
        make_health_finding_with(
            root,
            name,
            line,
            crate::health_types::ExceededThreshold::Both,
            crate::health_types::FindingSeverity::High,
        )
    }

    fn make_health_finding_with(
        root: &Path,
        name: &str,
        line: u32,
        exceeded: crate::health_types::ExceededThreshold,
        severity: crate::health_types::FindingSeverity,
    ) -> crate::health_types::HealthFinding {
        crate::health_types::HealthFinding {
            path: root.join("src/utils.ts"),
            name: name.to_string(),
            line,
            col: 0,
            cyclomatic: 25,
            cognitive: 30,
            line_count: 80,
            param_count: 0,
            exceeded,
            severity,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
        }
    }

    #[test]
    fn health_baseline_roundtrip() {
        let root = PathBuf::from("/project");
        let findings = vec![make_health_finding(&root, "parseExpression", 42)];
        let baseline = HealthBaselineData::from_findings(&findings, &[], &[], &root);
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: HealthBaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.findings, baseline.findings);
        assert_eq!(baseline.findings, Vec::<String>::new());
        assert_eq!(
            deserialized.finding_counts["src/utils.ts"]["complexity_high"].count,
            1
        );
        assert!(!json.contains("parseExpression"));
    }

    #[test]
    fn health_baseline_filters_known_findings() {
        let root = PathBuf::from("/project");
        let mut findings = vec![
            make_health_finding(&root, "parseExpression", 42),
            make_health_finding(&root, "newFunction", 100),
        ];
        findings[1].path = root.join("src/other.ts");
        let baseline = HealthBaselineData::from_findings(&findings[..1], &[], &[], &root);
        let filtered = filter_new_health_findings(findings, &baseline, &root);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "newFunction");
    }

    #[test]
    fn health_baseline_filters_shifted_lines_with_same_category_count() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding(&root, "parseExpression", 43)],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_reports_full_category_when_count_increases() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![
                make_health_finding(&root, "parseExpression", 43),
                make_health_finding(&root, "newFunction", 100),
            ],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn health_baseline_legacy_findings_still_load() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData {
            findings: vec!["src/utils.ts:parseExpression:42".to_owned()],
            finding_counts: BTreeMap::new(),
            target_keys: vec![],
            runtime_coverage_findings: vec![],
        };
        let filtered = filter_new_health_findings(
            vec![make_health_finding(&root, "parseExpression", 42)],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_keeps_crap_categories_separate_from_complexity() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                crate::health_types::ExceededThreshold::Crap,
                crate::health_types::FindingSeverity::High,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![
                make_health_finding_with(
                    &root,
                    "parseExpression",
                    43,
                    crate::health_types::ExceededThreshold::Crap,
                    crate::health_types::FindingSeverity::High,
                ),
                make_health_finding(&root, "newComplexityOnlyFunction", 100),
            ],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "newComplexityOnlyFunction");
    }

    #[test]
    fn health_baseline_suppresses_findings_that_only_improve_in_severity() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                crate::health_types::ExceededThreshold::Both,
                crate::health_types::FindingSeverity::Critical,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding_with(
                &root,
                "parseExpression",
                42,
                crate::health_types::ExceededThreshold::Both,
                crate::health_types::FindingSeverity::High,
            )],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_still_reports_worse_current_severity_as_new() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                crate::health_types::ExceededThreshold::Both,
                crate::health_types::FindingSeverity::High,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding_with(
                &root,
                "parseExpression",
                42,
                crate::health_types::ExceededThreshold::Both,
                crate::health_types::FindingSeverity::Critical,
            )],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "parseExpression");
        assert!(matches!(
            filtered[0].severity,
            crate::health_types::FindingSeverity::Critical
        ));
    }

    #[test]
    fn health_baseline_overlap_counts_partial_category_overflow() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let overlap = baseline.overlap_entry_count(
            &[
                make_health_finding(&root, "parseExpression", 42),
                make_health_finding(&root, "newFunction", 100),
            ],
            &root,
        );
        assert_eq!(overlap, 1);
    }

    #[test]
    fn health_baseline_empty_keeps_all() {
        let root = PathBuf::from("/project");
        let findings = vec![make_health_finding(&root, "parseExpression", 42)];
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: BTreeMap::new(),
            target_keys: vec![],
            runtime_coverage_findings: vec![],
        };
        let filtered = filter_new_health_findings(findings, &baseline, &root);
        assert_eq!(filtered.len(), 1);
    }

    // ── circular_dep_key sort stability ─────────────────────────

    #[test]
    fn circular_dep_key_is_order_independent() {
        use fallow_core::results::CircularDependency;

        let dep_ab = CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        let dep_ba = CircularDependency {
            files: vec![PathBuf::from("src/b.ts"), PathBuf::from("src/a.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        assert_eq!(
            super::circular_dep_key(&dep_ab, Path::new("")),
            super::circular_dep_key(&dep_ba, Path::new("")),
            "same files in different order should produce identical keys"
        );
    }

    #[test]
    fn circular_dep_key_different_files_different_keys() {
        use fallow_core::results::CircularDependency;

        let dep1 = CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        let dep2 = CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/c.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        assert_ne!(
            super::circular_dep_key(&dep1, Path::new("")),
            super::circular_dep_key(&dep2, Path::new("")),
        );
    }

    #[test]
    fn circular_dep_key_three_files_order_independent() {
        use fallow_core::results::CircularDependency;

        let dep_abc = CircularDependency {
            files: vec![
                PathBuf::from("src/a.ts"),
                PathBuf::from("src/b.ts"),
                PathBuf::from("src/c.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        let dep_cab = CircularDependency {
            files: vec![
                PathBuf::from("src/c.ts"),
                PathBuf::from("src/a.ts"),
                PathBuf::from("src/b.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        assert_eq!(
            super::circular_dep_key(&dep_abc, Path::new("")),
            super::circular_dep_key(&dep_cab, Path::new("")),
        );
    }

    // ── filter_new_issues: extended issue types ────────────────

    fn make_full_results() -> AnalysisResults {
        use fallow_core::extract::MemberKind;
        use fallow_core::results::*;

        let mut r = make_results();
        r.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        r.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 15,
            used_in_workspaces: Vec::new(),
        });
        r.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 0,
        });
        r.unused_class_members.push(UnusedMember {
            path: PathBuf::from("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacy".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 0,
        });
        r.unresolved_imports
            .push(fallow_core::results::UnresolvedImport {
                path: PathBuf::from("src/app.ts"),
                specifier: "./missing".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            });
        r.unlisted_dependencies
            .push(fallow_core::results::UnlistedDependency {
                package_name: "chalk".to_string(),
                imported_from: vec![],
            });
        r.duplicate_exports
            .push(fallow_core::results::DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    fallow_core::results::DuplicateLocation {
                        path: PathBuf::from("src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    fallow_core::results::DuplicateLocation {
                        path: PathBuf::from("src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            });
        r.type_only_dependencies
            .push(fallow_core::results::TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: PathBuf::from("package.json"),
                line: 8,
            });
        r.test_only_dependencies
            .push(fallow_core::results::TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: PathBuf::from("package.json"),
                line: 10,
            });
        r.boundary_violations
            .push(fallow_core::results::BoundaryViolation {
                from_path: PathBuf::from("src/ui/btn.ts"),
                to_path: PathBuf::from("src/db/query.ts"),
                from_zone: "ui".to_string(),
                to_zone: "db".to_string(),
                import_specifier: "../db/query".to_string(),
                line: 1,
                col: 0,
            });
        r
    }

    #[test]
    fn baseline_from_results_captures_all_extended_fields() {
        let results = make_full_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        assert_eq!(baseline.circular_dependencies.len(), 1);
        assert_eq!(
            baseline.unused_optional_dependencies,
            vec!["package.json:fsevents"]
        );
        assert_eq!(baseline.unused_enum_members.len(), 1);
        assert!(baseline.unused_enum_members[0].contains("Status.Deprecated"));
        assert_eq!(baseline.unused_class_members.len(), 1);
        assert!(baseline.unused_class_members[0].contains("UserService.legacy"));
        assert_eq!(baseline.unresolved_imports.len(), 1);
        assert!(baseline.unresolved_imports[0].contains("./missing"));
        assert_eq!(baseline.unlisted_dependencies, vec!["chalk"]);
        assert_eq!(baseline.duplicate_exports.len(), 1);
        assert!(baseline.duplicate_exports[0].starts_with("Config|"));
        assert_eq!(baseline.type_only_dependencies, vec!["package.json:zod"]);
        assert_eq!(baseline.test_only_dependencies, vec!["package.json:vitest"]);
        assert_eq!(baseline.boundary_violations.len(), 1);
        assert!(baseline.boundary_violations[0].contains("->"));
    }

    #[test]
    fn filter_removes_all_extended_baseline_issues() {
        let results = make_full_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert!(filtered.circular_dependencies.is_empty());
        assert!(filtered.unused_optional_dependencies.is_empty());
        assert!(filtered.unused_enum_members.is_empty());
        assert!(filtered.unused_class_members.is_empty());
        assert!(filtered.unresolved_imports.is_empty());
        assert!(filtered.unlisted_dependencies.is_empty());
        assert!(filtered.duplicate_exports.is_empty());
        assert!(filtered.type_only_dependencies.is_empty());
        assert!(filtered.test_only_dependencies.is_empty());
        assert!(filtered.boundary_violations.is_empty());
    }

    #[test]
    fn filter_keeps_new_circular_deps() {
        use fallow_core::results::CircularDependency;
        let baseline = BaselineData {
            circular_dependencies: vec!["src/a.ts->src/b.ts".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), Path::new(""))
        };
        let mut results = AnalysisResults::default();
        // One in baseline, one new
        results.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        results.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("src/x.ts"), PathBuf::from("src/y.ts")],
            length: 2,
            line: 5,
            col: 0,
            is_cross_package: false,
        });
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_keeps_new_boundary_violations() {
        use fallow_core::results::BoundaryViolation;
        let baseline = BaselineData {
            boundary_violations: vec!["src/a.ts->src/b.ts".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), Path::new(""))
        };
        let mut results = AnalysisResults::default();
        results.boundary_violations.push(BoundaryViolation {
            from_path: PathBuf::from("src/a.ts"),
            to_path: PathBuf::from("src/b.ts"),
            from_zone: "a".to_string(),
            to_zone: "b".to_string(),
            import_specifier: "../b".to_string(),
            line: 1,
            col: 0,
        });
        results.boundary_violations.push(BoundaryViolation {
            from_path: PathBuf::from("src/new.ts"),
            to_path: PathBuf::from("src/secret.ts"),
            from_zone: "new".to_string(),
            to_zone: "secret".to_string(),
            import_specifier: "../secret".to_string(),
            line: 1,
            col: 0,
        });
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.boundary_violations.len(), 1);
    }

    // ── filter_new_health_targets ──────────────────────────────

    #[test]
    fn health_targets_baseline_filters_known() {
        let root = PathBuf::from("/project");
        let targets = vec![
            crate::health_types::RefactoringTarget {
                path: root.join("src/complex.ts"),
                priority: 80.0,
                efficiency: 40.0,
                recommendation: "Split file".to_string(),
                category: crate::health_types::RecommendationCategory::SplitHighImpact,
                effort: crate::health_types::EffortEstimate::Medium,
                confidence: crate::health_types::Confidence::Medium,
                factors: vec![],
                evidence: None,
            },
            crate::health_types::RefactoringTarget {
                path: root.join("src/new-issue.ts"),
                priority: 60.0,
                efficiency: 30.0,
                recommendation: "Extract function".to_string(),
                category: crate::health_types::RecommendationCategory::ExtractComplexFunctions,
                effort: crate::health_types::EffortEstimate::Low,
                confidence: crate::health_types::Confidence::High,
                factors: vec![],
                evidence: None,
            },
        ];
        let baseline = HealthBaselineData::from_findings(&[], &[], &targets[..1], &root);
        let filtered = filter_new_health_targets(targets, &baseline, &root);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, root.join("src/new-issue.ts"));
    }

    // ── duplicate_export_key ───────────────────────────────────

    #[test]
    fn duplicate_export_key_is_sorted() {
        use fallow_core::results::{DuplicateExport, DuplicateLocation};
        let dup_ab = DuplicateExport {
            export_name: "foo".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        };
        let dup_ba = DuplicateExport {
            export_name: "foo".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 5,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 1,
                    col: 0,
                },
            ],
        };
        assert_eq!(
            super::duplicate_export_key(&dup_ab, Path::new("")),
            super::duplicate_export_key(&dup_ba, Path::new("")),
        );
    }

    // ── boundary_violation_key ─────────────────────────────────

    #[test]
    fn boundary_violation_key_format() {
        use fallow_core::results::BoundaryViolation;
        let v = BoundaryViolation {
            from_path: PathBuf::from("src/ui/btn.ts"),
            to_path: PathBuf::from("src/db/query.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/query".to_string(),
            line: 1,
            col: 0,
        };
        let key = super::boundary_violation_key(&v, Path::new(""));
        assert_eq!(key, "src/ui/btn.ts->src/db/query.ts");
    }

    // ── cross-machine baseline portability (#87) ──────────────

    /// Build results with absolute paths rooted at the given prefix.
    fn make_absolute_results(root: &str) -> AnalysisResults {
        use fallow_core::extract::MemberKind;
        use fallow_core::results::*;

        let p = |rel: &str| PathBuf::from(format!("{root}/{rel}"));

        AnalysisResults {
            unused_files: vec![UnusedFile {
                path: p("src/old.ts"),
            }],
            unused_exports: vec![UnusedExport {
                path: p("src/utils.ts"),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
            }],
            unused_dependencies: vec![UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: p("packages/app/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }],
            circular_dependencies: vec![CircularDependency {
                files: vec![p("src/a.ts"), p("src/b.ts")],
                length: 2,
                line: 1,
                col: 0,
                is_cross_package: false,
            }],
            unused_enum_members: vec![UnusedMember {
                path: p("src/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 0,
            }],
            unused_class_members: vec![UnusedMember {
                path: p("src/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "legacy".to_string(),
                kind: MemberKind::ClassMethod,
                line: 42,
                col: 0,
            }],
            unresolved_imports: vec![UnresolvedImport {
                path: p("src/app.ts"),
                specifier: "./missing".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            }],
            duplicate_exports: vec![DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: p("src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: p("src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            }],
            boundary_violations: vec![BoundaryViolation {
                from_path: p("src/ui/btn.ts"),
                to_path: p("src/db/query.ts"),
                from_zone: "ui".to_string(),
                to_zone: "db".to_string(),
                import_specifier: "../db/query".to_string(),
                line: 1,
                col: 0,
            }],
            ..Default::default()
        }
    }

    /// Regression test: baseline saved on one machine (different absolute root)
    /// must match issues found on another machine across all path-based types.
    #[test]
    fn baseline_keys_are_relative_to_root() {
        let local_root = Path::new("/Users/dev/project");
        let results = make_absolute_results("/Users/dev/project");
        let baseline = BaselineData::from_results(&results, local_root);

        // Keys should be relative
        assert_eq!(baseline.unused_files, vec!["src/old.ts"]);
        assert_eq!(baseline.unused_exports, vec!["src/utils.ts:helper"]);
        assert_eq!(
            baseline.unused_dependencies,
            vec!["packages/app/package.json:lodash-es"]
        );
        assert_eq!(
            baseline.boundary_violations,
            vec!["src/ui/btn.ts->src/db/query.ts"]
        );
        assert_eq!(baseline.circular_dependencies, vec!["src/a.ts->src/b.ts"]);
        assert_eq!(
            baseline.unused_enum_members,
            vec!["src/enums.ts:Status.Deprecated"]
        );
        assert_eq!(
            baseline.unused_class_members,
            vec!["src/service.ts:UserService.legacy"]
        );
        assert_eq!(baseline.unresolved_imports, vec!["src/app.ts:./missing"]);
        assert_eq!(baseline.duplicate_exports, vec!["Config|src/a.ts|src/b.ts"]);

        // Simulate loading baseline on CI (different absolute root, same relative structure)
        let ci_root = Path::new("/home/runner/work/project/project");
        let ci_results = make_absolute_results("/home/runner/work/project/project");

        let filtered = filter_new_issues(ci_results, &baseline, ci_root);
        assert!(filtered.unused_files.is_empty(), "unused files");
        assert!(filtered.unused_exports.is_empty(), "unused exports");
        assert!(filtered.unused_dependencies.is_empty(), "unused deps");
        assert!(
            filtered.boundary_violations.is_empty(),
            "boundary violations"
        );
        assert!(filtered.circular_dependencies.is_empty(), "circular deps");
        assert!(filtered.unused_enum_members.is_empty(), "enum members");
        assert!(filtered.unused_class_members.is_empty(), "class members");
        assert!(filtered.unresolved_imports.is_empty(), "unresolved imports");
        assert!(filtered.duplicate_exports.is_empty(), "duplicate exports");
    }
}
